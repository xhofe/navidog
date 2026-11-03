//! Generation-scoped bounded transport. No engine values cross this thread-safe bridge.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{FutureExt as _, channel::oneshot};
use navidog_db::{Column, ColumnOrigin, DbError, DbResult, ResultSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::Limits;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct StreamColumn {
    pub name: String,
    #[serde(default)]
    pub type_name: String,
    #[serde(default)]
    pub origin: Option<Origin>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Origin {
    pub schema: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Chunk {
    pub result_set: usize,
    pub sequence: usize,
    #[serde(default)]
    pub columns: Option<Vec<StreamColumn>>,
    #[serde(default)]
    pub rows: Vec<Vec<Option<String>>>,
    #[serde(default)]
    pub end: bool,
    #[serde(default)]
    pub affected_rows: Option<String>,
    #[serde(default)]
    pub notices: Vec<String>,
}

#[derive(Default)]
struct Stream {
    sets: Vec<ResultSet>,
    sequence: usize,
    ended: bool,
    rows: usize,
    bytes: usize,
}
impl Stream {
    fn chunk(&mut self, chunk: Chunk, limits: &Limits) -> DbResult<()> {
        let bytes = serde_json::to_vec(&chunk)
            .map_err(|e| DbError::msg(e.to_string()))?
            .len();
        if chunk.rows.len() > limits.batch_rows
            || bytes > limits.batch_bytes
            || self.rows.saturating_add(chunk.rows.len()) > limits.result_rows
            || self.bytes.saturating_add(bytes) > limits.result_bytes
        {
            return Err(DbError::msg("Plugin result exceeds configured limits."));
        }
        let next = self.sets.is_empty() || self.ended;
        if chunk.result_set
            != if next {
                self.sets.len()
            } else {
                self.sets.len() - 1
            }
            || chunk.sequence != if next { 0 } else { self.sequence }
            || chunk.columns.is_some() != next
        {
            return Err(DbError::msg(
                "Duplicate, out-of-order or invalid result chunk.",
            ));
        }
        if chunk.columns.as_ref().is_some_and(|columns| {
            let mut names = std::collections::BTreeSet::new();
            columns
                .iter()
                .any(|column| column.name.is_empty() || !names.insert(&column.name))
        }) {
            return Err(DbError::msg(
                "Result columns must have non-empty unique names.",
            ));
        }
        let width = if next {
            chunk.columns.as_ref().unwrap().len()
        } else {
            self.sets.last().unwrap().columns.len()
        };
        if chunk.rows.iter().any(|row| row.len() != width) {
            return Err(DbError::msg("Result row does not match column metadata."));
        }
        let affected_rows = chunk
            .affected_rows
            .as_deref()
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| DbError::msg("Affected rows must be an unsigned decimal string."))?;
        if !chunk.end && affected_rows.is_some() {
            return Err(DbError::msg("Affected rows are only valid at result end."));
        }
        if next {
            self.sets.push(ResultSet {
                columns: chunk
                    .columns
                    .unwrap()
                    .into_iter()
                    .map(|c| Column {
                        name: c.name,
                        type_name: c.type_name,
                        origin: c.origin.map(|o| ColumnOrigin {
                            schema: o.schema,
                            table: o.table,
                            column: o.column,
                        }),
                    })
                    .collect(),
                ..Default::default()
            });
        }
        let set = self.sets.last_mut().unwrap();
        self.rows += chunk.rows.len();
        self.bytes += bytes;
        set.rows.extend(chunk.rows);
        set.notices.extend(chunk.notices);
        set.affected_rows = affected_rows;
        self.sequence = chunk.sequence + 1;
        self.ended = chunk.end;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Response {
    value: Value,
    sets: Vec<ResultSet>,
}
impl Response {
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn into_sets(self) -> Vec<ResultSet> {
        self.sets
    }
}
struct Pending {
    arguments: Value,
    stream: Stream,
    reply: oneshot::Sender<DbResult<Response>>,
}
struct State {
    next_id: u64,
    active: bool,
    pending: BTreeMap<String, Pending>,
}
struct Transport {
    generation: u64,
    cancelled: tokio::sync::watch::Sender<bool>,
    limits: Limits,
    state: Mutex<State>,
    sender: async_channel::Sender<Value>,
    receiver: async_channel::Receiver<Value>,
}
#[derive(Clone)]
pub struct Bridge(Arc<Transport>);
impl Bridge {
    pub fn new(generation: u64, limits: Limits) -> DbResult<Self> {
        limits.validate().map_err(DbError::msg)?;
        let (sender, receiver) = async_channel::bounded(limits.events);
        Ok(Self(Arc::new(Transport {
            generation,
            cancelled: tokio::sync::watch::channel(false).0,
            limits,
            state: Mutex::new(State {
                next_id: 0,
                active: true,
                pending: BTreeMap::new(),
            }),
            sender,
            receiver,
        })))
    }
    pub fn limits(&self) -> &Limits {
        &self.0.limits
    }
    pub fn cancellation(&self) -> tokio::sync::watch::Receiver<bool> {
        self.0.cancelled.subscribe()
    }
    pub fn generation(&self) -> u64 {
        self.0.generation
    }
    pub fn is_active(&self) -> bool {
        self.0.state.lock().unwrap().active
    }
    pub async fn next_event(&self) -> DbResult<Value> {
        if !self.is_active() {
            return Err(DbError::msg("Plugin generation has ended."));
        }
        let event = self
            .0
            .receiver
            .recv()
            .await
            .map_err(|_| DbError::msg("Plugin generation has ended."))?;
        if !self.is_active() {
            return Err(DbError::msg("Plugin generation has ended."));
        }
        Ok(event)
    }
    pub fn event(&self, mut event: Value) -> DbResult<()> {
        let state = self.0.state.lock().unwrap();
        if !state.active {
            return Err(DbError::msg("Plugin generation has ended."));
        }
        event["generation"] = self.generation().into();
        self.0
            .sender
            .try_send(event)
            .map_err(|_| DbError::msg("Plugin event queue is full."))
    }
    pub fn request(
        &self,
        kind: &str,
        operation: &str,
        arguments: Value,
        timeout: Option<Duration>,
    ) -> DbResult<futures::future::BoxFuture<'static, DbResult<Response>>> {
        Ok(self.request_with_id(kind, operation, arguments, timeout)?.1)
    }
    pub fn request_with_id(
        &self,
        kind: &str,
        operation: &str,
        arguments: Value,
        timeout: Option<Duration>,
    ) -> DbResult<(
        String,
        futures::future::BoxFuture<'static, DbResult<Response>>,
    )> {
        let mut state = self.0.state.lock().unwrap();
        if !state.active {
            return Err(DbError::msg("Plugin generation has ended."));
        }
        if state.pending.len() >= self.0.limits.requests {
            return Err(DbError::msg("Plugin in-flight request limit reached."));
        }
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or_else(|| DbError::msg("Plugin request identifiers exhausted."))?;
        let id = format!("{}:{}", self.generation(), state.next_id);
        let (reply, receive) = oneshot::channel();
        self.0.sender.try_send(serde_json::json!({ "kind": kind, "op": operation, "request_id": id, "generation": self.generation(), "arguments": arguments.clone() })).map_err(|_| DbError::msg("Plugin event queue is full."))?;
        state.pending.insert(
            id.clone(),
            Pending {
                arguments,
                stream: Stream::default(),
                reply,
            },
        );
        drop(state);
        let bridge = self.clone();
        let timeout =
            timeout.unwrap_or_else(|| Duration::from_secs_f64(self.0.limits.timeout_seconds));
        let guard = RequestGuard {
            bridge: bridge.clone(),
            id: Some(id.clone()),
        };
        let timeout_bridge = bridge.clone();
        let timeout_id = id.clone();
        let operation = navidog_db::runtime::spawn(async move {
            let result = match tokio::time::timeout(timeout, receive).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(DbError::msg("Plugin request was cancelled.")),
                Err(_) => Err(DbError::msg("Plugin request timed out.")),
            };
            if result.is_err() {
                timeout_bridge.cancel(&timeout_id, "Request failed or timed out.");
            }
            result
        });
        let request_id = id.clone();
        Ok((
            request_id,
            async move {
                let mut guard = guard;
                let result = operation.await.map_err(|e| DbError::msg(e.to_string()))?;
                if result.is_err() {
                    bridge.cancel(&id, "Request failed or timed out.");
                }
                guard.id = None;
                drop(guard);
                result
            }
            .boxed(),
        ))
    }
    pub fn input_chunk(&self, id: &str, generation: u64, chunk: Value) -> DbResult<()> {
        let state = self.0.state.lock().unwrap();
        self.validate(&state, generation)?;
        if !state.pending.contains_key(id) {
            return Err(DbError::msg("Unknown or cancelled plugin request."));
        }
        if serde_json::to_vec(&chunk)
            .map_err(|e| DbError::msg(e.to_string()))?
            .len()
            > self.0.limits.batch_bytes
        {
            return Err(DbError::msg("Input batch exceeds plugin limits."));
        }
        self.0.sender.try_send(serde_json::json!({"kind":"export.chunk","request_id":id,"generation":generation,"chunk":chunk})).map_err(|_|DbError::msg("Plugin event queue is full."))
    }
    pub async fn send_input_chunk(&self, id: &str, generation: u64, chunk: Value) -> DbResult<()> {
        {
            let state = self.0.state.lock().unwrap();
            self.validate(&state, generation)?;
            if !state.pending.contains_key(id) {
                return Err(DbError::msg("Unknown or cancelled plugin request."));
            }
        }
        if serde_json::to_vec(&chunk)
            .map_err(|error| DbError::msg(error.to_string()))?
            .len()
            > self.0.limits.batch_bytes
        {
            return Err(DbError::msg("Input batch exceeds plugin limits."));
        }
        let mut cancellation = self.cancellation();
        let send=self.0.sender.send(serde_json::json!({"kind":"export.chunk","request_id":id,"generation":generation,"chunk":chunk}));
        tokio::select! {
            result=send=>result.map_err(|_|DbError::msg("Plugin generation ended.")),
            _=cancellation.changed()=>Err(DbError::msg("Plugin generation ended.")),
            _=tokio::time::sleep(Duration::from_secs_f64(self.0.limits.timeout_seconds))=>{self.cancel(id,"Input batch timed out.");Err(DbError::msg("Input batch timed out."))},
        }
    }
    pub fn chunk(&self, id: &str, generation: u64, chunk: Chunk) -> DbResult<()> {
        let mut state = self.0.state.lock().unwrap();
        self.validate(&state, generation)?;
        let pending = state
            .pending
            .get_mut(id)
            .ok_or_else(|| DbError::msg("Unknown or cancelled plugin request."))?;
        let result = pending.stream.chunk(chunk, &self.0.limits);
        if let Err(error) = &result {
            let pending = state.pending.remove(id).unwrap();
            let _ = pending.reply.send(Err(error.clone()));
            self.send_cancel(id, "Invalid result stream.", pending.arguments);
        }
        result
    }
    pub fn respond(&self, id: &str, generation: u64, result: DbResult<Value>) -> DbResult<()> {
        let mut state = self.0.state.lock().unwrap();
        self.validate(&state, generation)?;
        let pending = state
            .pending
            .remove(id)
            .ok_or_else(|| DbError::msg("Unknown or cancelled plugin request."))?;
        let arguments = pending.arguments.clone();
        let declared_error = result.is_err();
        let result = result.and_then(|value| {
            if !pending.stream.sets.is_empty() && !pending.stream.ended {
                return Err(DbError::msg("Final reply arrived before result end."));
            }
            if serde_json::to_vec(&value)
                .map_err(|e| DbError::msg(e.to_string()))?
                .len()
                > self
                    .0
                    .limits
                    .result_bytes
                    .saturating_sub(pending.stream.bytes)
            {
                return Err(DbError::msg("Final reply exceeds configured limits."));
            }
            Ok(Response {
                value,
                sets: pending.stream.sets,
            })
        });
        let error = result.as_ref().err().cloned();
        if error.is_some() {
            self.send_cancel(id, "Request failed.", arguments);
        }
        let _ = pending.reply.send(result);
        if declared_error {
            Ok(())
        } else {
            error.map_or(Ok(()), Err)
        }
    }
    fn validate(&self, state: &State, generation: u64) -> DbResult<()> {
        if !state.active || generation != self.generation() {
            return Err(DbError::msg("Stale plugin generation."));
        }
        Ok(())
    }
    fn send_cancel(&self, id: &str, reason: &str, arguments: Value) {
        // Cancellation must be delivered even when ordinary events have exhausted the queue.
        if self.0.sender.try_send(serde_json::json!({"kind":"cancel", "request_id":id, "generation": self.generation(), "reason":reason,"arguments":arguments})).is_err() {
            // A plugin that cannot consume cancellation loses its generation instead of continuing.
            self.0.sender.close();
        }
    }
    pub fn cancel(&self, id: &str, reason: &str) {
        let mut state = self.0.state.lock().unwrap();
        if let Some(pending) = state.pending.remove(id) {
            let _ = pending.reply.send(Err(DbError::msg(reason)));
            self.send_cancel(id, reason, pending.arguments);
        }
    }
    pub fn stop(&self, reason: &str) {
        let mut state = self.0.state.lock().unwrap();
        state.active = false;
        self.0.cancelled.send_replace(true);
        for (id, pending) in std::mem::take(&mut state.pending) {
            self.send_cancel(&id, reason, pending.arguments);
            let _ = pending
                .reply
                .send(Err(DbError::msg(reason).with_session_loss(true)));
        }
        self.0.sender.close();
    }
}
struct RequestGuard {
    bridge: Bridge,
    id: Option<String>,
}
impl Drop for RequestGuard {
    fn drop(&mut self) {
        if let Some(id) = &self.id {
            self.bridge.cancel(id, "Caller cancelled the request.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(set: usize, sequence: usize, columns: bool, end: bool) -> Chunk {
        Chunk {
            result_set: set,
            sequence,
            columns: columns.then(|| {
                vec![StreamColumn {
                    name: "n".into(),
                    type_name: "bigint".into(),
                    origin: None,
                }]
            }),
            rows: vec![vec![Some("9007199254740993".into())]],
            end,
            affected_rows: None,
            notices: vec![],
        }
    }
    #[test]
    fn streams_multiple_results_and_rejects_order_and_limits() {
        let mut stream = Stream::default();
        let limits = Limits::default();
        stream.chunk(chunk(0, 0, true, false), &limits).unwrap();
        assert!(stream.chunk(chunk(0, 0, false, false), &limits).is_err());
        stream.chunk(chunk(0, 1, false, true), &limits).unwrap();
        stream.chunk(chunk(1, 0, true, true), &limits).unwrap();
        assert_eq!(stream.sets.len(), 2);
        assert_eq!(
            stream.sets[0].rows[0][0].as_deref(),
            Some("9007199254740993")
        );
        let mut limits = limits;
        limits.batch_rows = 1;
        let mut oversized = chunk(2, 0, true, true);
        oversized.rows.push(vec![None]);
        assert!(stream.chunk(oversized, &limits).is_err());
    }
    #[test]
    fn cancellation_revokes_requests_and_generation() {
        let bridge = Bridge::new(7, Limits::default()).unwrap();
        let request = bridge
            .request("driver.request", "connect", Value::Null, None)
            .unwrap();
        let event = futures::executor::block_on(bridge.next_event()).unwrap();
        let id = event["request_id"].as_str().unwrap();
        assert!(bridge.respond(id, 6, Ok(Value::Null)).is_err());
        bridge.cancel(id, "Cancelled");
        assert!(bridge.respond(id, 7, Ok(Value::Null)).is_err());
        drop(request);
        bridge.stop("Stopped");
        assert!(bridge.event(serde_json::json!({"kind":"command"})).is_err());
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    fn dropped_unpolled_requests_cancel_and_timeouts_reject_late_responses() {
        let bridge = Bridge::new(1, Limits::default()).unwrap();
        let request = bridge
            .request(
                "driver.request",
                "execute",
                Value::Null,
                Some(Duration::from_millis(5)),
            )
            .unwrap();
        let event = futures::executor::block_on(bridge.next_event()).unwrap();
        let id = event["request_id"].as_str().unwrap().to_string();
        drop(request);
        assert_eq!(
            futures::executor::block_on(bridge.next_event()).unwrap()["kind"],
            "cancel"
        );
        assert!(bridge.respond(&id, 1, Ok(Value::Null)).is_err());
        let request = bridge
            .request(
                "driver.request",
                "execute",
                Value::Null,
                Some(Duration::from_millis(5)),
            )
            .unwrap();
        let event = futures::executor::block_on(bridge.next_event()).unwrap();
        let id = event["request_id"].as_str().unwrap();
        assert!(
            navidog_db::runtime::runtime()
                .block_on(request)
                .unwrap_err()
                .message
                .contains("timed out")
        );
        assert!(bridge.respond(id, 1, Ok(Value::Null)).is_err());
    }
}
