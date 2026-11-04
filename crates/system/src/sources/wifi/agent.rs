//! A NetworkManager secret agent (nmrs serves and registers it): when NetworkManager needs a
//! Wi-Fi password it has no (working) copy of, it asks its agents, and this one shows the
//! widget's password prompt.
//!
//! It only asks while the dashboard is shown, or for a network joined from the widget; otherwise
//! it says it has no secrets, so another agent (a desktop's own) can answer instead.

use super::{Error, Prompt, PromptKind, Wifi, nm};
use futures_util::StreamExt;
use nmrs::agent::{
    SecretAgent, SecretAgentCapabilities, SecretAgentFlags, SecretRequest, SecretSetting,
};
use std::collections::HashMap;
use std::time::Duration;
use zbus::zvariant::{OwnedValue, Value};

const IDENTIFIER: &str = "io.github.niri_dash";
/// nmrs gives NetworkManager an answer by itself after 120 s; close the prompt just before.
const WAIT: Duration = Duration::from_secs(115);

/// Register the agent, and again whenever NetworkManager restarts.
pub fn start(wifi: Wifi) {
    let ctx = wifi.ctx.clone();
    ctx.spawn(async move {
        loop {
            if let Err(e) = serve(&wifi).await {
                log::debug!("wifi agent: {e}");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

async fn serve(wifi: &Wifi) -> Result<(), Error> {
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
                Some(r) => answer(wifi, r),
                None => return Ok(()),
            },
            Some(cancel) = handle.cancellations().next() => {
                wifi.drop_agent_request(cancel.connection_path.as_str());
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

/// Answer one request: ask in the widget, or say there are no secrets here.
fn answer(wifi: &Wifi, req: SecretRequest) {
    let wifi = wifi.clone();
    wifi.ctx.clone().spawn(async move {
        let path = req.connection_path.to_string();
        let profile = wifi.profile_at(&path);
        let ssid_of_profile = || {
            profile
                .as_ref()
                .map(|p| p.ssid.clone())
                .unwrap_or_else(|| req.connection_id.clone())
        };
        let (ssid, kind, identity) = match &req.setting {
            SecretSetting::WifiPsk { ssid } => {
                let wep = profile.as_ref().is_some_and(|p| p.key_mgmt == "none");
                let ssid = if ssid.is_empty() {
                    ssid_of_profile()
                } else {
                    ssid.clone()
                };
                let kind = if wep {
                    PromptKind::Wep
                } else {
                    PromptKind::Password
                };
                (ssid, kind, String::new())
            }
            SecretSetting::WifiEap { identity, .. } => (
                ssid_of_profile(),
                PromptKind::Enterprise,
                identity.clone().unwrap_or_default(),
            ),
            _ => {
                let _ = req.responder.no_secrets().await;
                return;
            }
        };
        if !req.flags.contains(SecretAgentFlags::ALLOW_INTERACTION) || !wifi.may_ask(&ssid) {
            let _ = req.responder.no_secrets().await;
            return;
        }
        let prompt = Prompt {
            ssid,
            kind,
            retry: req.flags.contains(SecretAgentFlags::REQUEST_NEW),
            identity,
            ..Default::default()
        };
        let rx = wifi.ask_agent(prompt, path.clone());
        let replied = match tokio::time::timeout(WAIT, rx).await {
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
                wifi.drop_agent_request(&path);
                req.responder.cancel().await
            }
        };
        if let Err(e) = replied {
            log::debug!("wifi agent: answering: {e}");
        }
    });
}
