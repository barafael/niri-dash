//! A BlueZ pairing agent: when pairing needs a PIN, a passkey or a "yes, the numbers match",
//! BlueZ asks the agent of the connection that started the pairing, and this one shows the
//! widget's prompt.
//!
//! It only answers for the device being paired from the widget; anything else (a device
//! pairing or connecting on its own) is rejected. It is not registered as the default agent,
//! so it doesn't take over from a desktop's own.

use super::{Answer, BLUEZ, Bluetooth, Error, PromptKind};
use futures_util::StreamExt;
use std::time::Duration;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

const PATH: &str = "/io/github/niri_dash/bluetooth_agent";
/// BlueZ gives an agent a minute; answer just before.
const WAIT: Duration = Duration::from_secs(55);

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

struct Agent {
    bt: Bluetooth,
}

impl Agent {
    /// Only the device being paired from the widget gets an answer.
    fn ours(&self, device: &ObjectPath<'_>) -> Result<(), AgentError> {
        if self.bt.pairing().as_deref() == Some(device.as_str()) {
            Ok(())
        } else {
            Err(AgentError::Rejected("not a pairing started here".into()))
        }
    }

    async fn wait(&self, device: &ObjectPath<'_>, kind: PromptKind) -> Result<Answer, AgentError> {
        let rx = self.bt.ask(device.as_str(), kind);
        match tokio::time::timeout(WAIT, rx).await {
            Ok(Ok(Some(answer))) => Ok(answer),
            Ok(_) => Err(AgentError::Rejected("declined".into())),
            Err(_) => {
                self.bt.drop_prompt();
                Err(AgentError::Canceled("nobody answered".into()))
            }
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        self.ours(&device)?;
        match self.wait(&device, PromptKind::EnterPin).await? {
            // A legacy PIN: 1 to 16 characters.
            Answer::Code(pin) if (1..=16).contains(&pin.len()) => Ok(pin),
            _ => Err(AgentError::Rejected("no PIN".into())),
        }
    }

    async fn display_pin_code(
        &self,
        device: OwnedObjectPath,
        pincode: String,
    ) -> Result<(), AgentError> {
        self.ours(&device)?;
        // Shown until the pairing ends; nothing to wait for.
        self.bt.show(device.as_str(), PromptKind::ShowPin(pincode));
        Ok(())
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        self.ours(&device)?;
        match self.wait(&device, PromptKind::EnterPasskey).await? {
            Answer::Code(code) => code
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|&k| k <= 999_999)
                .ok_or_else(|| AgentError::Rejected("not a passkey".into())),
            Answer::Yes => Err(AgentError::Rejected("no passkey".into())),
        }
    }

    async fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        entered: u16,
    ) -> Result<(), AgentError> {
        self.ours(&device)?;
        self.bt
            .show(device.as_str(), PromptKind::ShowPasskey(passkey, entered));
        Ok(())
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        self.ours(&device)?;
        match self.wait(&device, PromptKind::Confirm(passkey)).await? {
            Answer::Yes => Ok(()),
            Answer::Code(_) => Err(AgentError::Rejected("declined".into())),
        }
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        self.ours(&device)?;
        match self.wait(&device, PromptKind::Authorize).await? {
            Answer::Yes => Ok(()),
            Answer::Code(_) => Err(AgentError::Rejected("declined".into())),
        }
    }

    /// A service of the device being paired (part of setting it up) is fine; nothing else.
    async fn authorize_service(
        &self,
        device: OwnedObjectPath,
        _uuid: String,
    ) -> Result<(), AgentError> {
        self.ours(&device)
    }

    fn cancel(&self) {
        self.bt.drop_prompt();
    }
}

/// Serve the agent and register it, again whenever BlueZ restarts.
pub fn start(bt: Bluetooth) {
    let ctx = bt.ctx.clone();
    ctx.spawn(async move {
        loop {
            if let Err(e) = serve(&bt).await {
                log::debug!("bluetooth agent: {e}");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

async fn register(conn: &zbus::Connection) {
    let r = conn
        .call_method(
            Some(BLUEZ),
            "/org/bluez",
            Some("org.bluez.AgentManager1"),
            "RegisterAgent",
            &(
                ObjectPath::from_static_str_unchecked(PATH),
                "KeyboardDisplay",
            ),
        )
        .await;
    match r {
        Ok(_) => log::debug!("bluetooth agent registered"),
        Err(e) => log::debug!("bluetooth agent: registering: {e}"),
    }
}

async fn serve(bt: &Bluetooth) -> Result<(), Error> {
    let conn = bt.agent_conn().await?.clone();
    conn.object_server()
        .at(PATH, Agent { bt: bt.clone() })
        .await?;
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, BLUEZ)])
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
