//! Formats use the same bounded request and result stream as drivers.
use crate::protocol::Bridge;
use navidog_db::{DbError, DbResult, ResultSet};
use serde_json::{Value, json};

pub async fn import_rows(
    bridge: &Bridge,
    format_id: &str,
    context: &str,
    input: String,
) -> DbResult<Vec<ResultSet>> {
    Ok(bridge
        .request(
            "import",
            format_id,
            json!({"context":context,"input":input}),
            None,
        )?
        .await?
        .into_sets())
}
pub async fn export_rows(
    bridge: &Bridge,
    format_id: &str,
    context: &str,
    rows: Value,
) -> DbResult<String> {
    let response = bridge
        .request(
            "export",
            format_id,
            json!({"context":context,"result":rows}),
            None,
        )?
        .await?;
    response
        .value()
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| DbError::msg("Export must return text."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_handlers_stream_import_and_reply_export() {
        navidog_db::runtime::runtime().block_on(async {
            let bridge=Bridge::new(1,Default::default()).unwrap();let worker=bridge.clone();
            let task=tokio::spawn(async move {
                let event=worker.next_event().await.unwrap();let id=event["request_id"].as_str().unwrap();
                let chunk=serde_json::from_value(serde_json::json!({"result_set":0,"sequence":0,"columns":[{"name":"value","type_name":"text"}],"rows":[["a"]],"end":true})).unwrap();
                worker.chunk(id,1,chunk).unwrap();worker.respond(id,1,Ok(Value::Null)).unwrap();
                let event=worker.next_event().await.unwrap();worker.respond(event["request_id"].as_str().unwrap(),1,Ok(Value::String("exported".into()))).unwrap();
            });
            assert_eq!(import_rows(&bridge,"format","context","input".into()).await.unwrap()[0].rows[0][0].as_deref(),Some("a"));
            assert_eq!(export_rows(&bridge,"format","context",Value::Null).await.unwrap(),"exported");task.await.unwrap();
        });
    }
}
