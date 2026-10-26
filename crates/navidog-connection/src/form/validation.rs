use super::*;

use gpui_kit::{App, SharedString};
use navidog_model::connection::{Connection, HostPreference, Member, SslMode};
use rust_i18n::t;

/// Validate a port field. An empty field means "use the server default" for
/// optional ports; a non-numeric or out-of-range value is an error, not a
/// silent `None`.
pub(super) fn port_value(text: &str, field: &str) -> Result<Option<u16>, SharedString> {
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u16>()
        .map(Some)
        .map_err(|_| t!("conn.bad_port", field = field).to_string().into())
}

pub(super) fn port_or_default(text: &str, default: u16, field: &str) -> Result<u16, SharedString> {
    Ok(port_value(text, field)?.unwrap_or(default))
}

pub(super) fn number_value(text: &str, field: &str) -> Result<Option<u32>, SharedString> {
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u32>()
        .map(Some)
        .map_err(|_| t!("conn.bad_number", field = field).to_string().into())
}

/// Parse the multi-node editor; an invalid line is an error. The previous
/// `filter_map` silently dropped it and saved a different server list.
pub(super) fn member_list(text: &str) -> Result<Vec<Member>, SharedString> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let member =
            Member::parse(line).ok_or_else(|| t!("conn.bad_member", line = line).to_string())?;
        out.push(member);
    }
    Ok(out)
}

impl ConnectionForm {
    fn num(input: &Text, on: bool, field: &str, cx: &App) -> Result<Option<u32>, SharedString> {
        if !on {
            return Ok(None);
        }
        number_value(&Self::value(input, cx), field)
    }

    /// Collects the full connection settings from the form controls.
    pub(crate) fn collect(&self, cx: &App) -> Result<Connection, SharedString> {
        let mut c = self.draft.clone();
        c.name = Self::value(&self.name, cx);
        if c.name.is_empty() {
            return Err(t!("conn.required_name").to_string().into());
        }
        let group_ix = self
            .group
            .read(cx)
            .selected_index(cx)
            .map(|ix| ix.row)
            .unwrap_or(0);
        c.group = self.group_ids.get(group_ix).cloned().flatten();
        let g = &mut c.general;
        g.host = Self::value(&self.host, cx);
        g.database = Self::opt(&self.database, cx);
        g.file = Self::opt(&self.file, cx);
        g.user = Self::value(&self.user, cx);
        if self.multi_node(cx) {
            let text = self.members.read(cx).value().to_string();
            let mut members = member_list(&text)?;
            if members.is_empty() {
                return Err(t!("conn.members_required").to_string().into());
            }
            let first = members.remove(0);
            (g.host, g.port, g.members) = (first.host, first.port, members);
            let ix = Self::selected(&self.host_preference, cx);
            g.host_preference = HostPreference::ALL[ix.min(2)];
        } else {
            g.port = port_value(&Self::value(&self.port, cx), &t!("conn.port"))?;
            g.members.clear();
            g.host_preference = HostPreference::Any;
        }
        let a = &mut c.advanced;
        a.keepalive_secs = Self::num(
            &self.keepalive,
            self.keepalive_on,
            &t!("conn.keepalive"),
            cx,
        )?;
        a.connect_timeout_secs = number_value(
            &Self::value(&self.connect_timeout, cx),
            &t!("conn.connect_timeout"),
        )?;
        a.read_timeout_secs = Self::num(
            &self.read_timeout,
            self.read_timeout_on,
            &t!("conn.read_timeout"),
            cx,
        )?;
        a.write_timeout_secs = Self::num(
            &self.write_timeout,
            self.write_timeout_on,
            &t!("conn.write_timeout"),
            cx,
        )?;
        a.socket = self
            .socket_on
            .then(|| Self::value(&self.socket, cx))
            .filter(|s| !s.is_empty());
        a.session_limit = Self::num(
            &self.session_limit,
            self.session_limit_on,
            &t!("conn.session_limit"),
            cx,
        )?;
        a.params = Self::value(&self.params, cx);
        a.initial_queries = Self::value(&self.initial_queries, cx);
        let ssl_ix = self
            .ssl_mode
            .read(cx)
            .selected_index(cx)
            .map(|ix| ix.row)
            .unwrap_or(1);
        c.ssl.mode = SslMode::ALL[ssl_ix.min(4)];
        c.ssl.client_key = Self::opt(&self.client_key, cx);
        c.ssl.client_cert = Self::opt(&self.client_cert, cx);
        c.ssl.ca_cert = Self::opt(&self.ca_cert, cx);
        c.ssl.cipher = Self::opt(&self.cipher, cx);
        c.ssl.crl = Self::opt(&self.crl, cx);
        c.ssh.host = Self::value(&self.ssh_host, cx);
        if c.ssh.enabled {
            c.ssh.port = port_or_default(&Self::value(&self.ssh_port, cx), 22, &t!("conn.port"))?;
        }
        c.ssh.user = Self::value(&self.ssh_user, cx);
        let auth_ix = self
            .ssh_auth
            .read(cx)
            .selected_index(cx)
            .map(|ix| ix.row)
            .unwrap_or(0);
        c.ssh.auth = SSH_AUTHS[auth_ix.min(2)];
        c.ssh.private_key = Self::opt(&self.ssh_key, cx);
        c.remark = self.remark.read(cx).value().to_string();
        Ok(c)
    }
}
