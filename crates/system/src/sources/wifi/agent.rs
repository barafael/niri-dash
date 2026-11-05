//! A NetworkManager secret agent (nmrs serves and registers it): when NetworkManager needs a
//! Wi-Fi password it has no (working) copy of, it asks its agents, and this one shows the
//! widget's password prompt.
//!
//! It only asks while the dashboard is shown, or for a network joined from the widget; otherwise
//! it says it has no secrets, so another agent (a desktop's own) can answer instead.

use super::{AgentReply, Error, Found, Inner, PromptKind, nm};
use futures_util::StreamExt;
use nmrs::agent::{SecretAgent, SecretAgentCapabilities, SecretRequest};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use zbus::zvariant::{OwnedValue, Value};

const IDENTIFIER: &str = "io.github.niri_dash";
/// nmrs gives NetworkManager an answer by itself after 120 s; close the prompt just before.
const WAIT: Duration = Duration::from_secs(115);

/// Register the agent, and again whenever NetworkManager restarts.
pub async fn serve_forever(actor: UnboundedSender<Inner>) {
    loop {
        if let Err(e) = serve(&actor).await {
            log::debug!("wifi agent: {e}");
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

async fn serve(actor: &UnboundedSender<Inner>) -> Result<(), Error> {
    let (mut handle, mut requests) = SecretAgent::builder()
        .with_identifier(IDENTIFIER)
        // Only Wi-Fi: no VPN hints wanted.
        .with_capabilities(SecretAgentCapabilities::empty())
        .register()
        .await?;
    log::debug!("wifi agent registered");
    // NetworkManager forgets its agents when it restarts.
    let conn = zbus::Connection::system().await?;
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, nm::NM)])
        .await?;
    loop {
        tokio::select! {
            request = requests.next() => match request {
                Some(r) => {
                    let _ = actor.send(Inner::Agent(Box::new(r)));
                }
                None => return Ok(()),
            },
            Some(cancel) = handle.cancellations().next() => {
                let gone = Found::AgentGone(cancel.connection_path.to_string());
                let _ = actor.send(gone.into());
            }
            change = owners.next() => match change {
                Some(c) if c.args().is_ok_and(|a| a.new_owner().is_some()) => {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    handle.reregister().await?;
                    log::debug!("wifi agent registered again");
                }
                Some(_) => {}
                None => return Ok(()),
            },
        }
    }
}

/// Tell NetworkManager what became of a request: nothing here, or what the prompt got.
pub async fn respond(
    req: SecretRequest,
    reply: AgentReply,
    path: String,
    actor: UnboundedSender<Inner>,
) {
    let (kind, answer) = match reply {
        AgentReply::NoSecrets => {
            let _ = req.responder.no_secrets().await;
            return;
        }
        AgentReply::Wait(kind, answer) => (kind, answer),
    };
    let replied = match tokio::time::timeout(WAIT, answer).await {
        Ok(Ok(Some(a))) => match kind {
            PromptKind::Password => req.responder.wifi_psk(a.password).await,
            PromptKind::Wep => {
                let key =
                    OwnedValue::try_from(Value::from(a.password)).expect("a string has no fds");
                req.responder
                    .raw(
                        "802-11-wireless-security",
                        HashMap::from([("wep-key0".to_string(), key)]),
                    )
                    .await
            }
            PromptKind::Enterprise => {
                let identity = Some(a.identity).filter(|i| !i.is_empty());
                req.responder.wifi_eap(identity, a.password).await
            }
        },
        Ok(_) => req.responder.cancel().await,
        Err(_) => {
            let _ = actor.send(Found::AgentGone(path).into());
            req.responder.cancel().await
        }
    };
    if let Err(e) = replied {
        log::debug!("wifi agent: answering: {e}");
    }
}
