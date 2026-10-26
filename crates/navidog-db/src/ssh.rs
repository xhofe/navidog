//! SSH local port forwarding: listen on a random 127.0.0.1 port and open one
//! direct-tcpip channel per incoming connection.

use std::sync::Arc;

use russh::client::{self, Handle};
use russh::keys::{PrivateKeyWithHashAlg, load_secret_key};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::driver::{DbError, DbResult};
use crate::params::SshParams;

/// Trust-on-first-use host-key verification. An unknown key is learned into
/// `~/.ssh/known_hosts`; a changed key is refused. `verify` can be turned off
/// per connection (for appliances that rotate keys), but it defaults on.
struct HostKeyVerifier {
    host: String,
    port: u16,
    verify: bool,
}

impl client::Handler for HostKeyVerifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        if !self.verify {
            return Ok(true);
        }
        let public = key.public_key();
        match russh::keys::check_known_hosts(&self.host, self.port, &public) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // `check_known_hosts` returns false both for a new host and for
                // a recorded key with a different algorithm. Only the former may
                // be learned; otherwise a changed RSA key could replace a
                // recorded Ed25519 key.
                match russh::keys::known_hosts::known_host_keys(&self.host, self.port) {
                    Ok(keys) if keys.is_empty() => {
                        match russh::keys::known_hosts::learn_known_hosts(
                            &self.host, self.port, &public,
                        ) {
                            Ok(()) => {
                                tracing::warn!(
                                    host = %self.host,
                                    port = self.port,
                                    "SSH host key was new; added it to known_hosts"
                                );
                                Ok(true)
                            }
                            Err(err) => {
                                tracing::error!(
                                    host = %self.host,
                                    "could not record the SSH host key: {err}"
                                );
                                Ok(false)
                            }
                        }
                    }
                    Ok(_) => {
                        tracing::error!(
                            host = %self.host,
                            port = self.port,
                            "SSH host key does not match known_hosts"
                        );
                        Ok(false)
                    }
                    Err(err) => {
                        tracing::error!(
                            host = %self.host,
                            port = self.port,
                            "could not read known_hosts: {err}"
                        );
                        Ok(false)
                    }
                }
            }
            Err(err) => {
                tracing::error!(
                    host = %self.host,
                    port = self.port,
                    "SSH host key is unknown or changed: {err}"
                );
                Ok(false)
            }
        }
    }
}

/// Tunnel handle; dropping it stops the listener and disconnects SSH.
pub struct Tunnel {
    pub local_port: u16,
    accept: JoinHandle<()>,
    _session: Arc<Handle<HostKeyVerifier>>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

fn ssh_err(e: impl std::fmt::Display) -> DbError {
    DbError::msg(format!("SSH: {e}"))
}

pub async fn open(ssh: &SshParams, target_host: String, target_port: u16) -> DbResult<Tunnel> {
    let config = Arc::new(client::Config::default());
    let verifier = HostKeyVerifier {
        host: ssh.host.clone(),
        port: ssh.port,
        verify: ssh.verify_host_key,
    };
    let mut session = client::connect(config, (ssh.host.as_str(), ssh.port), verifier)
        .await
        .map_err(ssh_err)?;

    let mut authed = false;
    if let Some(key_path) = &ssh.private_key {
        let key = load_secret_key(key_path, ssh.passphrase.as_deref()).map_err(ssh_err)?;
        let hash = session
            .best_supported_rsa_hash()
            .await
            .map_err(ssh_err)?
            .flatten();
        let res = session
            .authenticate_publickey(&ssh.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
            .await
            .map_err(ssh_err)?;
        authed = res.success();
    }
    if !authed && let Some(password) = &ssh.password {
        authed = session
            .authenticate_password(&ssh.user, password)
            .await
            .map_err(ssh_err)?
            .success();
    }
    if !authed {
        return Err(DbError::coded(
            "navidog.ssh_auth",
            "SSH authentication failed.",
        ));
    }

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(ssh_err)?;
    let local_port = listener.local_addr().map_err(ssh_err)?.port();
    let session = Arc::new(session);
    let handle = session.clone();
    let accept = tokio::spawn(async move {
        while let Ok((mut socket, peer)) = listener.accept().await {
            let handle = handle.clone();
            let host = target_host.clone();
            tokio::spawn(async move {
                let channel = match handle
                    .channel_open_direct_tcpip(
                        host,
                        target_port as u32,
                        peer.ip().to_string(),
                        peer.port() as u32,
                    )
                    .await
                {
                    Ok(ch) => ch,
                    Err(err) => {
                        tracing::warn!("opening the SSH channel failed: {err}");
                        return;
                    }
                };
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
            });
        }
    });
    Ok(Tunnel {
        local_port,
        accept,
        _session: session,
    })
}
