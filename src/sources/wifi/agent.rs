//! A NetworkManager secret agent: when NetworkManager needs a Wi-Fi password it has no (working)
//! copy of, it asks its agents, and this one shows the widget's password prompt.
//!
//! It only asks while the dashboard is shown, or for a network joined from the widget; otherwise
//! it says it has no secrets, so another agent (a desktop's own) can answer instead.

use super::{Answer, Prompt, PromptKind, Wifi, nm};
use std::collections::HashMap;
use std::time::Duration;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
const IDENTIFIER: &str = "io.github.niri_dash";
// NM_SECRET_AGENT_GET_SECRETS_FLAG_*.
const ALLOW_INTERACTION: u32 = 0x1;
const REQUEST_NEW: u32 = 0x2;
/// NetworkManager gives up on an agent after two minutes; answer before that.
const WAIT: Duration = Duration::from_secs(115);

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.NetworkManager.SecretAgent")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NoSecrets(String),
    UserCanceled(String),
}

struct Agent {
    wifi: Wifi,
}

fn str_of<'a>(s: &'a nm::Settings, setting: &str, key: &str) -> Option<&'a str> {
    s.get(setting)
        .and_then(|v| crate::sources::prop::<&str>(v, key))
}

fn owned(v: Value<'_>) -> OwnedValue {
    v.try_to_owned().expect("plain values have no fds")
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.SecretAgent")]
impl Agent {
    async fn get_secrets(
        &self,
        connection: nm::Settings,
        connection_path: OwnedObjectPath,
        setting_name: String,
        _hints: Vec<String>,
        flags: u32,
    ) -> Result<nm::Settings, AgentError> {
        let none = || AgentError::NoSecrets("niri-dash has none stored".into());
        let ssid = connection
            .get("802-11-wireless")
            .and_then(|w| w.get("ssid"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<u8>::try_from(v).ok())
            .map(|b| nm::ssid_text(&b))
            .ok_or_else(none)?;
        let wifi_setting = matches!(setting_name.as_str(), "802-11-wireless-security" | "802-1x");
        if !wifi_setting || flags & ALLOW_INTERACTION == 0 || !self.wifi.may_ask(&ssid) {
            return Err(none());
        }
        let key_mgmt = str_of(&connection, "802-11-wireless-security", "key-mgmt").unwrap_or("");
        let kind = if setting_name == "802-1x" || key_mgmt.starts_with("wpa-eap") {
            PromptKind::Enterprise
        } else if key_mgmt == "none" {
            PromptKind::Wep
        } else {
            PromptKind::Password
        };
        let prompt = Prompt {
            ssid,
            kind,
            retry: flags & REQUEST_NEW != 0,
            identity: str_of(&connection, "802-1x", "identity")
                .unwrap_or_default()
                .into(),
            ..Default::default()
        };
        let path = connection_path.to_string();
        let rx = self.wifi.ask_agent(prompt, path.clone());
        let answer: Answer = match tokio::time::timeout(WAIT, rx).await {
            Ok(Ok(Some(a))) => a,
            Ok(_) => return Err(AgentError::UserCanceled("cancelled".into())),
            Err(_) => {
                self.wifi.drop_agent_request(&path);
                return Err(AgentError::UserCanceled("nobody answered".into()));
            }
        };
        let mut values: HashMap<String, OwnedValue> = HashMap::new();
        match kind {
            PromptKind::Password => {
                values.insert("psk".into(), owned(Value::from(answer.password)));
            }
            PromptKind::Wep => {
                values.insert("wep-key0".into(), owned(Value::from(answer.password)));
            }
            PromptKind::Enterprise => {
                if !answer.identity.is_empty() {
                    values.insert("identity".into(), owned(Value::from(answer.identity)));
                }
                values.insert("password".into(), owned(Value::from(answer.password)));
            }
        }
        Ok(HashMap::from([(setting_name, values)]))
    }

    async fn cancel_get_secrets(&self, connection_path: OwnedObjectPath, _setting_name: String) {
        self.wifi.drop_agent_request(connection_path.as_str());
    }

    /// NetworkManager stores the secrets it got (they are system-owned), so there is nothing
    /// to keep here.
    async fn save_secrets(&self, _connection: nm::Settings, _connection_path: OwnedObjectPath) {}

    async fn delete_secrets(&self, _connection: nm::Settings, _connection_path: OwnedObjectPath) {}
}

/// Serve the agent and register it with NetworkManager, again whenever NetworkManager restarts.
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

async fn register(conn: &zbus::Connection) {
    let r = conn
        .call_method(
            Some(nm::NM),
            nm::AGENT_MANAGER_PATH,
            Some(nm::AGENT_MANAGER),
            "RegisterWithCapabilities",
            &(IDENTIFIER, 0u32),
        )
        .await;
    match r {
        Ok(_) => log::debug!("wifi agent registered"),
        Err(e) => log::debug!("wifi agent: registering: {e}"),
    }
}

async fn serve(wifi: &Wifi) -> zbus::Result<()> {
    use futures_util::StreamExt;
    // A connection of its own: the shared one is dropped and remade when it fails, which
    // would take the agent object with it.
    let conn = zbus::Connection::system().await?;
    conn.object_server()
        .at(PATH, Agent { wifi: wifi.clone() })
        .await?;
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, nm::NM)])
        .await?;
    register(&conn).await;
    while let Some(change) = owners.next().await {
        if change.args().is_ok_and(|a| a.new_owner().is_some()) {
            tokio::time::sleep(Duration::from_secs(1)).await;
            register(&conn).await;
        }
    }
    Ok(())
}
