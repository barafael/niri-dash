//! A BlueZ pairing agent: when pairing needs a PIN, a passkey or a "yes, the numbers match",
//! BlueZ asks the agent of the connection that started the pairing, and this one shows the
//! widget's prompt.
//!
//! It only answers for the device being paired from the widget; anything else (a device
//! pairing or connecting on its own) is rejected. It is not registered as the default agent,
//! so it doesn't take over from a desktop's own.

use super::{Answer, Asked, BLUEZ, Error, Inner, PromptKind};
use crate::sources::connector::Connector;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use zbus::Connection;
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

/// The agent's methods run at the same time, and wait for the user: they only ask the actor, which
/// owns the prompt and knows what is being paired.
struct Agent {
    actor: UnboundedSender<Inner>,
}

fn not_ours() -> AgentError {
    AgentError::Rejected("not a pairing started here".into())
}

impl Agent {
    /// Only the device being paired from the widget gets an answer.
    async fn ours(&self, device: &ObjectPath<'_>) -> Result<(), AgentError> {
        let (reply, ours) = oneshot::channel();
        let _ = self.actor.send(Inner::IsOurs {
            device: device.to_string(),
            reply,
        });
        if ours.await == Ok(true) {
            Ok(())
        } else {
            Err(not_ours())
        }
    }

    /// Show a code to type on the device, if it is ours; there is nothing to wait for.
    async fn show(&self, device: &ObjectPath<'_>, kind: PromptKind) -> Result<(), AgentError> {
        let (reply, ours) = oneshot::channel();
        let _ = self.actor.send(Inner::Show {
            device: device.to_string(),
            kind,
            reply,
        });
        if ours.await == Ok(true) {
            Ok(())
        } else {
            Err(not_ours())
        }
    }

    /// Ask the user about the device, if it is ours, and wait for the answer. In one message, so
    /// what is being paired can't change between the check and the prompt.
    async fn wait(&self, device: &ObjectPath<'_>, kind: PromptKind) -> Result<Answer, AgentError> {
        let (reply, answer) = oneshot::channel();
        let _ = self.actor.send(Inner::Ask {
            device: device.to_string(),
            kind,
            reply,
        });
        match tokio::time::timeout(WAIT, answer).await {
            Ok(Ok(Asked::Answer(answer))) => Ok(answer),
            Ok(Ok(Asked::NotOurs)) => Err(not_ours()),
            Ok(_) => Err(AgentError::Rejected("declined".into())),
            Err(_) => {
                let _ = self.actor.send(Inner::PromptGone);
                Err(AgentError::Canceled("nobody answered".into()))
            }
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
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
        // Shown until the pairing ends; nothing to wait for.
        self.show(&device, PromptKind::ShowPin(pincode)).await
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
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
        self.show(&device, PromptKind::ShowPasskey(passkey, entered))
            .await
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        match self.wait(&device, PromptKind::Confirm(passkey)).await? {
            Answer::Yes => Ok(()),
            Answer::Code(_) => Err(AgentError::Rejected("declined".into())),
        }
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
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
        self.ours(&device).await
    }

    fn cancel(&self) {
        let _ = self.actor.send(Inner::PromptGone);
    }
}

/// Serve the agent and register it, again whenever BlueZ restarts.
pub async fn serve_forever(
    agent_conn: Connector<Connection, zbus::Error>,
    actor: UnboundedSender<Inner>,
) {
    loop {
        if let Err(e) = serve(&agent_conn, &actor).await {
            log::debug!("bluetooth agent: {e}");
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
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

async fn serve(
    agent_conn: &Connector<Connection, zbus::Error>,
    actor: &UnboundedSender<Inner>,
) -> Result<(), Error> {
    let conn = agent_conn.get().await?;
    conn.object_server()
        .at(
            PATH,
            Agent {
                actor: actor.clone(),
            },
        )
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
