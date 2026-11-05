//! A minimal org.freedesktop.Notifications server that keeps a history for the dashboard. It only
//! claims the bus name if nobody else has it, so a running mako/dunst/swaync wins.
//!
//! On Plasma it doesn't try: Plasma shows the notifications and keeps their history. Do not
//! disturb then pauses Plasma's pop-ups through the server's `Inhibit`, and our own messages (a
//! timer going off) are sent to Plasma as notifications.

use super::icons::IconThread;
use super::osd::{self, Burst};
use super::{Ctx, IconSrc, Mailbox, prop, unescape_entities};
use crate::live::Stateful;
pub use crate::notifications::{Dnd, Notification, State};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, watch};
use zbus::fdo::RequestNameFlags;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::OwnedValue;

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";
const MAX: usize = 100;
/// How often to look whether a timed do not disturb has run out.
const EXPIRY_CHECK: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Store {
    server: bool,
    can_pause: bool,
    next_id: u32,
    items: Vec<Notification>,
    dnd: Dnd,
    missed: u32,
}

impl Store {
    /// Turn an expired "until" off; returns whether anything changed.
    fn expire(&mut self, now: DateTime<Local>) -> bool {
        if let Dnd::Until(_) = self.dnd
            && !self.dnd.active(now)
        {
            self.dnd = Dnd::Off;
            self.missed = 0;
            return true;
        }
        false
    }

    fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    /// Put `n` in place of the notification with its id, or on top of the history.
    fn push(&mut self, n: Notification) {
        match self.items.iter_mut().find(|x| x.id == n.id) {
            Some(slot) => *slot = n,
            None => self.items.insert(0, n),
        }
        self.items.truncate(MAX);
    }

    /// A notification came in: its id, and what to pop up.
    fn notify(&mut self, n: Incoming, now: DateTime<Local>) -> (u32, Option<String>) {
        self.expire(now);
        let replacing = n.replaces_id != 0 && self.items.iter().any(|x| x.id == n.replaces_id);
        let id = if replacing {
            n.replaces_id
        } else {
            self.new_id()
        };
        // Updates of a notification (progress and the like) don't pop up again.
        let popup = if replacing {
            None
        } else if n.urgent || !self.dnd.active(now) {
            Some(if n.app.is_empty() {
                n.summary.clone()
            } else {
                format!("{}: {}", n.app, n.summary)
            })
        } else {
            self.missed += 1;
            None
        };
        self.push(Notification {
            id,
            app: n.app,
            summary: n.summary,
            body: n.body,
            icon: n.icon,
            actions: n.actions,
            urgent: n.urgent,
            time: now,
        });
        (id, popup)
    }

    fn set_dnd(&mut self, dnd: Dnd, now: DateTime<Local>) -> bool {
        let active = dnd.active(now);
        self.dnd = dnd;
        if !active {
            self.dnd = Dnd::Off;
            self.missed = 0;
        }
        active
    }
}

/// A notification as it came in over the bus.
pub struct Incoming {
    app: String,
    replaces_id: u32,
    summary: String,
    /// Without its markup.
    body: String,
    icon: IconSrc,
    actions: Vec<(String, String)>,
    urgent: bool,
    popup_icon: String,
}

/// What reaches the actor from its tasks and from its server on the bus.
pub enum Inner {
    Notify(Box<Incoming>, oneshot::Sender<u32>),
    Close(u32, oneshot::Sender<()>),
    /// We tried to be the server: whether we got the name, and the connection to emit signals on.
    Served {
        owned: bool,
        conn: zbus::Connection,
    },
    /// On Plasma: whether its server can pause its pop-ups.
    PlasmaReady {
        can_pause: bool,
    },
    /// Time to look whether a timed do not disturb has run out.
    CheckExpiry,
    /// A pop-up burst's time is up.
    BurstOver,
    /// The action's signal is out: the notification goes now.
    Invoked(u32),
}

/// The server on the bus. Its methods run at the same time, so they only prepare what came in and
/// ask the actor, which owns the notifications.
struct Server {
    actor: UnboundedSender<Inner>,
    icons: IconThread,
}

/// An icon name for the pop-up: SwayOSD takes names from the icon theme, not paths.
fn osd_icon(app_icon: &str, hints: &HashMap<String, OwnedValue>, urgent: bool) -> String {
    let name = |s: &str| (!s.is_empty() && !s.contains('/')).then(|| s.to_string());
    name(app_icon)
        .or_else(|| prop::<&str>(hints, "desktop-entry").and_then(name))
        .unwrap_or_else(|| {
            if urgent {
                "dialog-warning".into()
            } else {
                "dialog-information".into()
            }
        })
}

/// Body markup is a small HTML subset; strip tags and decode the common entities.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    unescape_entities(&out)
}

fn image_data(v: &OwnedValue) -> Option<IconSrc> {
    let (w, h, stride, alpha, _bps, channels, data): (i32, i32, i32, bool, i32, i32, Vec<u8>) =
        v.try_clone().ok()?.try_into().ok()?;
    if w <= 0 || h <= 0 || channels < 3 {
        return None;
    }
    let (w, h, stride, ch) = (w as usize, h as usize, stride as usize, channels as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = data.get(y * stride..y * stride + w * ch)?;
        for px in row.chunks_exact(ch) {
            rgba.extend_from_slice(&[
                px[0],
                px[1],
                px[2],
                if alpha && ch >= 4 { px[3] } else { 255 },
            ]);
        }
    }
    Some(IconSrc::Rgba {
        width: w as u32,
        height: h as u32,
        data: Arc::new(rgba),
    })
}

async fn resolve_icon(
    icons: &IconThread,
    app_icon: &str,
    hints: &HashMap<String, OwnedValue>,
) -> IconSrc {
    for key in ["image-data", "image_data", "icon_data"] {
        if let Some(img) = hints.get(key).and_then(image_data) {
            return img;
        }
    }
    // Owned: the hints don't go across a wait.
    let paths: Vec<String> = [
        prop::<&str>(hints, "image-path"),
        prop::<&str>(hints, "image_path"),
        Some(app_icon),
    ]
    .into_iter()
    .flatten()
    .map(|p| p.strip_prefix("file://").unwrap_or(p).to_string())
    .collect();
    let entry = prop::<&str>(hints, "desktop-entry").map(str::to_string);
    for path in paths {
        if let Some(p) = icons.lookup(&path, 48).await {
            return IconSrc::Path(p);
        }
    }
    if let Some(entry) = entry
        && let Some(p) = icons.app_icon(&entry).await
    {
        return IconSrc::Path(p);
    }
    IconSrc::None
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        let urgent = prop::<u8>(&hints, "urgency") == Some(2);
        let icon = resolve_icon(&self.icons, &app_icon, &hints).await;
        let popup_icon = osd_icon(&app_icon, &hints, urgent);
        let actions = actions
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (c[0].clone(), c[1].clone()))
            .collect();
        let (reply, id) = oneshot::channel();
        let incoming = Incoming {
            app: app_name,
            replaces_id,
            summary,
            body: strip_markup(&body),
            icon,
            actions,
            urgent,
            popup_icon,
        };
        let _ = self.actor.send(Inner::Notify(Box::new(incoming), reply));
        id.await.unwrap_or(0)
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let (reply, done) = oneshot::channel();
        let _ = self.actor.send(Inner::Close(id, reply));
        let _ = done.await;
        Self::notification_closed(&emitter, id, 3).await?;
        Ok(())
    }

    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".into(),
            "actions".into(),
            "icon-static".into(),
            "persistence".into(),
        ]
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "niri-dash".into(),
            "niri-dash".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

pub struct Notifications {
    ctx: Ctx,
    /// On-screen messages keep working where the rest of this stays dormant: a standalone app
    /// without a notifications widget still announces what it does.
    osd_ctx: Ctx,
    mailbox: Mailbox<Inner>,
    store: Store,
    /// The current burst of pop-ups.
    burst: Burst,
    /// The connection we serve on, to emit signals on; None unless we tried to serve.
    conn: Option<zbus::Connection>,
    /// Plasma shows the notifications: we leave the bus name to it.
    plasma: bool,
    /// Whether do not disturb wants the other server's pop-ups paused.
    pause: watch::Sender<bool>,
}

impl Notifications {
    pub fn start(ctx: Ctx, osd_ctx: Ctx) -> Self {
        let mut me = Self {
            ctx: ctx.clone(),
            osd_ctx,
            mailbox: Default::default(),
            store: Default::default(),
            burst: Default::default(),
            conn: None,
            plasma: ctx.is_kwin(),
            pause: watch::Sender::new(false),
        };
        let found = me.mailbox.sender();
        if me.plasma {
            let (c, want) = (ctx.clone(), me.pause.subscribe());
            me.mailbox
                .spawn(&ctx, async move { pause_plasma(c, want, found).await });
            return me;
        }
        me.mailbox.spawn(&ctx, async move {
            match serve(found.clone()).await {
                Ok((owned, conn)) => {
                    let _ = found.send(Inner::Served { owned, conn });
                }
                Err(e) => log::warn!("notification server: {e}"),
            }
        });
        me
    }

    fn publish(&self) {
        let s = &self.store;
        self.ctx.send(crate::notifications::Event::State(State {
            server: s.server,
            can_pause: s.can_pause,
            items: s.items.clone(),
            dnd: s.dnd,
            missed: s.missed,
        }));
    }

    /// End a timed do not disturb on time, even without new notifications.
    fn check_expiry_from_now_on(&mut self) {
        let found = self.mailbox.sender();
        self.mailbox.spawn(&self.ctx, async move {
            loop {
                tokio::time::sleep(EXPIRY_CHECK).await;
                let _ = found.send(Inner::CheckExpiry);
            }
        });
    }

    /// Show `text` with an icon name from the icon theme. A burst of messages shows the first one
    /// and then a single summary when it settles.
    fn pop_up(&mut self, text: &str, icon: &str) {
        if !self.burst.shown(Instant::now()) {
            return;
        }
        osd::send(&self.osd_ctx, text.to_string(), icon.to_string());
        let found = self.mailbox.sender();
        self.mailbox.spawn(&self.osd_ctx, async move {
            tokio::time::sleep(osd::BURST).await;
            let _ = found.send(Inner::BurstOver);
        });
    }

    fn set_dnd(&mut self, dnd: Dnd) {
        let active = self.store.set_dnd(dnd, Local::now());
        self.publish();
        self.pause.send_replace(active);
    }

    /// Put one of our own messages (a timer going off) into the history, or on Plasma, send it
    /// to Plasma as a notification.
    fn add_local(&mut self, app: String, summary: String, body: String) {
        if self.plasma {
            let c = self.ctx.clone();
            self.ctx.spawn(async move {
                let sent = async {
                    c.bus(false)
                        .await?
                        .call_method(
                            Some(NAME),
                            PATH,
                            Some(NAME),
                            "Notify",
                            &(
                                app.as_str(),
                                0u32,
                                "alarm-symbolic",
                                summary.as_str(),
                                body.as_str(),
                                Vec::<&str>::new(),
                                HashMap::<&str, zbus::zvariant::Value>::new(),
                                -1i32,
                            ),
                        )
                        .await
                };
                if let Err(e) = sent.await {
                    log::warn!("notify: {e}");
                }
            });
            return;
        }
        let id = self.store.new_id();
        self.store.push(Notification {
            id,
            app,
            summary,
            body,
            icon: IconSrc::None,
            actions: Vec::new(),
            urgent: false,
            time: Local::now(),
        });
        self.publish();
    }

    fn dismiss(&mut self, id: u32) {
        self.store.items.retain(|n| n.id != id);
        self.publish();
        let conn = self.conn.clone();
        self.ctx.spawn(async move {
            let _ = emit_closed(conn.as_ref(), id, 2).await;
        });
    }

    fn clear(&mut self) {
        let ids: Vec<u32> = self.store.items.drain(..).map(|n| n.id).collect();
        self.publish();
        let conn = self.conn.clone();
        self.ctx.spawn(async move {
            for id in ids {
                let _ = emit_closed(conn.as_ref(), id, 2).await;
            }
        });
    }

    /// Say the action was invoked, and only then that the notification closed.
    fn invoke(&self, id: u32) {
        let (conn, found) = (self.conn.clone(), self.mailbox.sender());
        self.ctx.spawn(async move {
            if let Err(e) = emit_invoked(conn.as_ref(), id, "default").await {
                log::warn!("invoke: {e}");
            }
            let _ = found.send(Inner::Invoked(id));
        });
    }
}

impl Stateful for Notifications {
    type Command = crate::notifications::Command;
    type Inner = Inner;

    fn mailbox(&mut self) -> &mut Mailbox<Inner> {
        &mut self.mailbox
    }

    fn on_command(&mut self, cmd: crate::notifications::Command) {
        use crate::notifications::Command::*;
        match cmd {
            SetDnd(dnd) => self.set_dnd(dnd),
            Dismiss(id) => self.dismiss(id),
            Clear => self.clear(),
            Invoke(id) => self.invoke(id),
            AddLocal { app, summary, body } => self.add_local(app, summary, body),
            Osd { text, icon } => self.pop_up(&text, &icon),
        }
    }

    fn on_inner(&mut self, inner: Inner) {
        match inner {
            Inner::Notify(incoming, reply) => {
                let popup_icon = incoming.popup_icon.clone();
                let (id, popup) = self.store.notify(*incoming, Local::now());
                if let Some(text) = popup {
                    self.pop_up(&text, &popup_icon);
                }
                self.publish();
                let _ = reply.send(id);
            }
            Inner::Close(id, reply) => {
                self.store.items.retain(|n| n.id != id);
                self.publish();
                let _ = reply.send(());
            }
            Inner::Served { owned, conn } => {
                self.conn = Some(conn);
                if owned {
                    log::info!("serving {NAME}");
                    self.store.server = true;
                    self.check_expiry_from_now_on();
                } else {
                    log::info!("{NAME} is owned by another daemon");
                }
                self.publish();
            }
            Inner::PlasmaReady { can_pause } => {
                log::info!("Plasma shows notifications; pausing its pop-ups: {can_pause}");
                self.store.can_pause = can_pause;
                self.publish();
                if can_pause {
                    self.check_expiry_from_now_on();
                }
            }
            Inner::CheckExpiry => {
                if self.store.expire(Local::now()) {
                    self.publish();
                    self.pause.send_replace(false);
                }
            }
            Inner::BurstOver => {
                if let Some(text) = osd::summary(self.burst.over()) {
                    osd::send(
                        &self.osd_ctx,
                        text,
                        "preferences-system-notifications".into(),
                    );
                }
            }
            Inner::Invoked(id) => self.dismiss(id),
        }
    }
}

/// Try to be the notification server: whether we got the name, and the connection.
async fn serve(actor: UnboundedSender<Inner>) -> zbus::Result<(bool, zbus::Connection)> {
    let conn = zbus::Connection::session().await?;
    conn.object_server()
        .at(
            PATH,
            Server {
                actor,
                icons: IconThread::start(),
            },
        )
        .await?;
    let reply = conn
        .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
        .await;
    let owned = matches!(
        reply,
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner)
    );
    Ok((owned, conn))
}

/// On Plasma: hold an inhibition of Plasma's pop-ups while do not disturb is on. Plasma lets go of
/// it by itself when our bus connection closes.
async fn pause_plasma(ctx: Ctx, mut want: watch::Receiver<bool>, actor: UnboundedSender<Inner>) {
    let conn = match ctx.bus(false).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!("notifications: {e}");
            return;
        }
    };
    let can_pause = supports_inhibit(&conn).await;
    let _ = actor.send(Inner::PlasmaReady { can_pause });
    if !can_pause {
        return;
    }
    let mut cookie: Option<u32> = None;
    loop {
        let wanted = *want.borrow_and_update();
        let result = match (wanted, cookie) {
            (true, None) => conn
                .call_method(
                    Some(NAME),
                    PATH,
                    Some(NAME),
                    "Inhibit",
                    &(
                        "niri-dash",
                        "Do not disturb",
                        HashMap::<&str, zbus::zvariant::Value>::new(),
                    ),
                )
                .await
                .and_then(|r| r.body().deserialize::<u32>())
                .map(|c| cookie = Some(c)),
            (false, Some(c)) => conn
                .call_method(Some(NAME), PATH, Some(NAME), "UnInhibit", &(c,))
                .await
                .map(|_| cookie = None),
            _ => Ok(()),
        };
        match result {
            Ok(()) => log::debug!("Plasma pop-ups paused: {}", cookie.is_some()),
            Err(e) => {
                log::warn!("do not disturb: {e}");
                ctx.toast(format!("Do not disturb: {e}"));
            }
        }
        if want.changed().await.is_err() {
            return;
        }
    }
}

/// Our interface on the bus, to emit signals from; None unless we are serving.
async fn iface(conn: Option<&zbus::Connection>) -> zbus::Result<Option<InterfaceRef<Server>>> {
    let Some(conn) = conn else {
        return Ok(None);
    };
    Ok(Some(conn.object_server().interface(PATH).await?))
}

async fn emit_closed(conn: Option<&zbus::Connection>, id: u32, reason: u32) -> zbus::Result<()> {
    if let Some(iface) = iface(conn).await? {
        Server::notification_closed(iface.signal_emitter(), id, reason).await?;
    }
    Ok(())
}

async fn emit_invoked(conn: Option<&zbus::Connection>, id: u32, action: &str) -> zbus::Result<()> {
    if let Some(iface) = iface(conn).await? {
        Server::action_invoked(iface.signal_emitter(), id, action).await?;
    }
    Ok(())
}

/// Whether the notification server on the bus implements `Inhibit` (Plasma's does).
async fn supports_inhibit(conn: &zbus::Connection) -> bool {
    let xml = async {
        zbus::fdo::IntrospectableProxy::builder(conn)
            .destination(NAME)?
            .path(PATH)?
            .build()
            .await?
            .introspect()
            .await
            .map_err(zbus::Error::from)
    };
    match xml.await {
        Ok(xml) => xml.contains(r#"name="Inhibit""#),
        Err(e) => {
            log::debug!("introspecting {NAME}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Actor;
    use crate::notifications::{Command, next_hour};
    use crate::sources::testing::{ctx, reported};
    use chrono::TimeZone;
    use tokio::sync::mpsc::unbounded_channel;

    fn incoming(summary: &str) -> Incoming {
        Incoming {
            app: "Mail".into(),
            replaces_id: 0,
            summary: summary.into(),
            body: String::new(),
            icon: IconSrc::None,
            actions: Vec::new(),
            urgent: false,
            popup_icon: "dialog-information".into(),
        }
    }

    fn evening() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 2, 21, 0, 0).unwrap()
    }

    #[test]
    fn ids_count_up_and_a_replacement_keeps_its_id() {
        let mut s = Store::default();
        let now = evening();
        assert_eq!(s.notify(incoming("a"), now).0, 1);
        assert_eq!(s.notify(incoming("b"), now).0, 2);
        let replacing = Incoming {
            replaces_id: 1,
            ..incoming("a again")
        };
        assert_eq!(s.notify(replacing, now).0, 1);
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[1].summary, "a again", "stays where it was");
        // An id we don't have is a new notification.
        let unknown = Incoming {
            replaces_id: 99,
            ..incoming("c")
        };
        assert_eq!(s.notify(unknown, now).0, 3);
    }

    #[test]
    fn what_pops_up() {
        let mut s = Store::default();
        let now = evening();
        assert_eq!(s.notify(incoming("hi"), now).1.as_deref(), Some("Mail: hi"));
        let nameless = Incoming {
            app: String::new(),
            ..incoming("hi")
        };
        assert_eq!(s.notify(nameless, now).1.as_deref(), Some("hi"));
        let update = Incoming {
            replaces_id: 1,
            ..incoming("50%")
        };
        assert_eq!(s.notify(update, now).1, None, "updates don't pop up again");
    }

    #[test]
    fn do_not_disturb_counts_what_it_held_back_but_urgent_gets_through() {
        let mut s = Store::default();
        let now = evening();
        assert!(s.set_dnd(Dnd::Forever, now));
        assert_eq!(s.notify(incoming("quiet"), now).1, None);
        assert_eq!(s.missed, 1);
        let urgent = Incoming {
            urgent: true,
            ..incoming("fire")
        };
        assert!(s.notify(urgent, now).1.is_some());
        assert_eq!(s.missed, 1);
        assert_eq!(s.items.len(), 2, "both are kept");
    }

    #[test]
    fn a_timed_do_not_disturb_runs_out() {
        let mut s = Store::default();
        let now = evening();
        let until = next_hour(now, 22);
        assert!(s.set_dnd(Dnd::Until(until), now));
        s.notify(incoming("quiet"), now);
        assert!(!s.expire(now), "not yet");
        assert_eq!(s.missed, 1);
        assert!(s.expire(until));
        assert_eq!(s.dnd, Dnd::Off);
        assert_eq!(s.missed, 0);
        assert!(!s.expire(until), "only once");
    }

    #[test]
    fn a_do_not_disturb_already_over_turns_it_off() {
        let mut s = Store::default();
        let now = evening();
        s.set_dnd(Dnd::Forever, now);
        s.notify(incoming("quiet"), now);
        assert!(!s.set_dnd(Dnd::Until(now), now));
        assert_eq!(s.dnd, Dnd::Off);
        assert_eq!(s.missed, 0);
    }

    #[test]
    fn the_history_is_capped_newest_first() {
        let mut s = Store::default();
        let now = evening();
        for i in 0..MAX + 5 {
            s.notify(incoming(&i.to_string()), now);
        }
        assert_eq!(s.items.len(), MAX);
        assert_eq!(s.items[0].summary, (MAX + 4).to_string());
    }

    #[tokio::test]
    async fn the_server_gets_its_id_and_the_dashboard_the_state() {
        let (ctx, events) = ctx();
        let me = Notifications::start(ctx.clone(), ctx);
        let (reply, id) = oneshot::channel();
        let _ = me
            .mailbox
            .sender()
            .send(Inner::Notify(Box::new(incoming("hi")), reply));
        let (commands, inbox) = unbounded_channel::<Command>();
        drop(commands);
        let me = me.event_loop(inbox).await;
        assert_eq!(id.await, Ok(1));
        assert_eq!(me.store.items.len(), 1);
        assert_eq!(reported(events).len(), 1);
    }

    #[tokio::test]
    async fn closing_goes_before_the_answer() {
        let (ctx, _events) = ctx();
        let mut me = Notifications::start(ctx.clone(), ctx);
        me.store.notify(incoming("hi"), evening());
        let (reply, done) = oneshot::channel();
        me.on_inner(Inner::Close(1, reply));
        assert!(me.store.items.is_empty());
        assert_eq!(done.await, Ok(()));
    }

    #[tokio::test]
    async fn dismiss_and_clear_empty_the_list() {
        let (ctx, events) = ctx();
        let me = Notifications::start(ctx.clone(), ctx);
        let (commands, inbox) = unbounded_channel();
        let _ = commands.send(Command::AddLocal {
            app: "Timer".into(),
            summary: "one".into(),
            body: String::new(),
        });
        let _ = commands.send(Command::AddLocal {
            app: "Timer".into(),
            summary: "two".into(),
            body: String::new(),
        });
        let _ = commands.send(Command::Dismiss(1));
        drop(commands);
        let mut me = me.event_loop(inbox).await;
        assert_eq!(me.store.items.len(), 1);
        assert_eq!(me.store.items[0].summary, "two");
        me.on_command(Command::Clear);
        assert!(me.store.items.is_empty());
        assert_eq!(reported(events).len(), 4);
    }

    /// Burst counting as messages come in and the time runs out.
    #[tokio::test]
    async fn a_burst_of_pop_ups_is_counted_until_it_is_over() {
        let (ctx, _events) = ctx();
        let mut me = Notifications::start(ctx.clone(), ctx);
        for summary in ["a", "b", "c"] {
            let (reply, _id) = oneshot::channel();
            me.on_inner(Inner::Notify(Box::new(incoming(summary)), reply));
        }
        assert_eq!(me.burst.over(), 2, "two more after the first");
        assert_eq!(me.burst.over(), 0);
    }

    /// Our own pop-ups count towards the same burst as those from the bus.
    #[tokio::test]
    async fn the_dashboards_own_messages_share_the_burst() {
        let (ctx, _events) = ctx();
        let mut me = Notifications::start(ctx.clone(), ctx);
        me.on_command(Command::Osd {
            text: "Timer done".into(),
            icon: "alarm-symbolic".into(),
        });
        let (reply, _id) = oneshot::channel();
        me.on_inner(Inner::Notify(Box::new(incoming("a")), reply));
        assert_eq!(me.burst.over(), 1);
    }

    #[tokio::test]
    async fn do_not_disturb_is_published_and_asks_for_a_pause() {
        let (ctx, events) = ctx();
        let mut me = Notifications::start(ctx.clone(), ctx);
        me.on_command(Command::SetDnd(Dnd::Forever));
        assert!(*me.pause.borrow());
        me.on_command(Command::SetDnd(Dnd::Off));
        assert!(!*me.pause.borrow());
        assert_eq!(reported(events).len(), 2);
    }

    /// A timed do not disturb running out lets go of the pause too.
    #[tokio::test]
    async fn running_out_lets_go_of_the_pause() {
        let (ctx, events) = ctx();
        let mut me = Notifications::start(ctx.clone(), ctx);
        me.pause.send_replace(true);
        me.store.dnd = Dnd::Until(evening());
        me.on_inner(Inner::CheckExpiry);
        assert!(!*me.pause.borrow());
        assert_eq!(me.store.dnd, Dnd::Off);
        assert_eq!(reported(events).len(), 1);
    }

    #[test]
    fn dnd_times() {
        let evening = chrono::Local
            .with_ymd_and_hms(2026, 11, 2, 21, 0, 0)
            .unwrap();
        let morning = chrono::Local
            .with_ymd_and_hms(2026, 11, 2, 5, 0, 0)
            .unwrap();
        assert_eq!(
            next_hour(evening, 7),
            chrono::Local
                .with_ymd_and_hms(2026, 11, 3, 7, 0, 0)
                .unwrap()
        );
        assert_eq!(
            next_hour(morning, 7),
            chrono::Local
                .with_ymd_and_hms(2026, 11, 2, 7, 0, 0)
                .unwrap()
        );
        let until = Dnd::Until(next_hour(evening, 7));
        assert!(until.active(evening));
        assert!(!until.active(next_hour(evening, 8)));
        assert_eq!(Dnd::from_saved(until.to_saved()), until);
        assert_eq!(Dnd::from_saved(Dnd::Forever.to_saved()), Dnd::Forever);
        assert_eq!(Dnd::from_saved(None), Dnd::Off);
    }

    #[test]
    fn markup() {
        assert_eq!(
            strip_markup("<b>Hi</b> &amp; <a href=\"x\">bye</a>"),
            "Hi & bye"
        );
    }
}
