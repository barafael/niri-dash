//! A minimal org.freedesktop.Notifications server that keeps a history for the dashboard. It only
//! claims the bus name if nobody else has it, so a running mako/dunst/swaync wins.
//!
//! On Plasma it doesn't try: Plasma shows the notifications. Do not disturb then pauses Plasma's
//! pop-ups through the server's `Inhibit`, and our own messages (a timer going off) are sent to
//! Plasma as notifications. The history is a copy kept by listening in on the session bus (a bus
//! monitor), from the time we start: Plasma doesn't give its own out.

use super::{Ctx, IconSrc, icons, osd::Osd, prop, unescape_entities};
pub use crate::notifications::{Dnd, Notification, State};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use zbus::fdo::RequestNameFlags;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::OwnedValue;

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";
const MAX: usize = 100;

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
    fn expire(&mut self) -> bool {
        if let Dnd::Until(_) = self.dnd
            && !self.dnd.active(chrono::Local::now())
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
}

struct Server {
    store: Arc<Mutex<Store>>,
    ctx: Ctx,
    osd: Osd,
}

fn publish(ctx: &Ctx, store: &Mutex<Store>) {
    let state = store
        .lock()
        .map(|s| State {
            server: s.server,
            can_pause: s.can_pause,
            items: s.items.clone(),
            dnd: s.dnd,
            missed: s.missed,
        })
        .unwrap_or_default();
    ctx.send(crate::notifications::Event::State(state));
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

fn resolve_icon(app_icon: &str, hints: &HashMap<String, OwnedValue>) -> IconSrc {
    for key in ["image-data", "image_data", "icon_data"] {
        if let Some(img) = hints.get(key).and_then(image_data) {
            return img;
        }
    }
    for path in [
        prop::<&str>(hints, "image-path"),
        prop::<&str>(hints, "image_path"),
        Some(app_icon),
    ]
    .into_iter()
    .flatten()
    {
        let path = path.strip_prefix("file://").unwrap_or(path);
        if let Some(p) = icons::lookup(path, 48) {
            return IconSrc::Path(p);
        }
    }
    if let Some(p) = prop::<&str>(hints, "desktop-entry").and_then(icons::app_icon) {
        return IconSrc::Path(p);
    }
    IconSrc::None
}

/// The `["key", "label", …]` list of a notification's actions as (key, label) pairs.
fn pairs(actions: &[String]) -> Vec<(String, String)> {
    actions
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].clone(), c[1].clone()))
        .collect()
}

/// A notification as the history keeps it.
fn entry(
    id: u32,
    app: String,
    app_icon: &str,
    summary: String,
    body: &str,
    actions: Vec<(String, String)>,
    hints: &HashMap<String, OwnedValue>,
) -> Notification {
    Notification {
        id,
        app,
        summary,
        body: strip_markup(body),
        icon: resolve_icon(app_icon, hints),
        actions,
        urgent: prop::<u8>(hints, "urgency") == Some(2),
        time: chrono::Local::now(),
    }
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
        let popup_icon = osd_icon(&app_icon, &hints, urgent);
        let actions = pairs(&actions);
        let (id, popup) = {
            let mut store = self.store.lock().expect("store poisoned");
            store.expire();
            let replacing = replaces_id != 0 && store.items.iter().any(|n| n.id == replaces_id);
            let id = if replacing {
                replaces_id
            } else {
                store.new_id()
            };
            // Updates of a notification (progress and the like) don't pop up again.
            let popup = if replacing {
                None
            } else if urgent || !store.dnd.active(chrono::Local::now()) {
                Some(if app_name.is_empty() {
                    summary.clone()
                } else {
                    format!("{app_name}: {summary}")
                })
            } else {
                store.missed += 1;
                None
            };
            store.push(entry(
                id, app_name, &app_icon, summary, &body, actions, &hints,
            ));
            (id, popup)
        };
        if let Some(text) = popup {
            self.osd.show(&self.ctx, &text, &popup_icon);
        }
        publish(&self.ctx, &self.store);
        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if let Ok(mut s) = self.store.lock() {
            s.items.retain(|n| n.id != id);
        }
        publish(&self.ctx, &self.store);
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

#[derive(Clone)]
pub struct Notifications {
    ctx: Ctx,
    osd: Osd,
    store: Arc<Mutex<Store>>,
    conn: Arc<tokio::sync::OnceCell<zbus::Connection>>,
    /// Plasma shows the notifications: we leave the bus name to it.
    plasma: bool,
    /// Whether do not disturb wants the other server's pop-ups paused.
    pause: watch::Sender<bool>,
}

impl Notifications {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            osd: Osd::default(),
            store: Default::default(),
            conn: Default::default(),
            plasma: ctx.is_kwin(),
            pause: watch::Sender::new(false),
        };
        let m = me.clone();
        if me.plasma {
            let watcher = me.clone();
            ctx.spawn(async move { m.pause_plasma().await });
            ctx.spawn(async move { watcher.watch_plasma().await });
            return me;
        }
        ctx.spawn(async move {
            match m.serve().await {
                Ok(true) => {
                    log::info!("serving {NAME}");
                    if let Ok(mut s) = m.store.lock() {
                        s.server = true;
                    }
                    publish(&m.ctx, &m.store);
                    // End a timed do not disturb on time, even without new notifications.
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                        let expired = m.store.lock().map(|mut s| s.expire()).unwrap_or(false);
                        if expired {
                            publish(&m.ctx, &m.store);
                        }
                    }
                }
                Ok(false) => {
                    log::info!("{NAME} is owned by another daemon");
                    publish(&m.ctx, &m.store);
                }
                Err(e) => log::warn!("notification server: {e}"),
            }
        });
        me
    }

    /// On Plasma: keep a history by listening in on the notifications other programs send to
    /// Plasma. A bus monitor sees every `Notify` call and its reply (which holds the id Plasma
    /// gave it) and every `NotificationClosed`, without owning the name.
    async fn watch_plasma(&self) {
        match self.monitor().await {
            Ok(()) => log::warn!("notification monitor: the bus closed it"),
            Err(e) => log::warn!("notification history: {e}"),
        }
        if let Ok(mut s) = self.store.lock() {
            s.server = false;
        }
        publish(&self.ctx, &self.store);
    }

    async fn monitor(&self) -> zbus::Result<()> {
        use futures_util::StreamExt;
        use zbus::message::Type;
        // A monitor can't send, so it has a connection of its own.
        let conn = zbus::Connection::session().await?;
        let rules = [
            "type='method_call',interface='org.freedesktop.Notifications',member='Notify'",
            "type='method_return',sender='org.freedesktop.Notifications'",
            "type='signal',interface='org.freedesktop.Notifications',member='NotificationClosed'",
        ]
        .map(zbus::MatchRule::try_from);
        let rules = rules.into_iter().collect::<zbus::Result<Vec<_>>>()?;
        zbus::fdo::MonitoringProxy::new(&conn)
            .await?
            .become_monitor(&rules, 0)
            .await?;
        log::info!("keeping Plasma's notifications in the history");
        if let Ok(mut s) = self.store.lock() {
            s.server = true;
        }
        publish(&self.ctx, &self.store);

        /// A `Notify` call whose reply we are waiting for: (its serial, who called, what).
        type Pending = (u32, String, u32, Notification);
        let mut pending: Vec<Pending> = Vec::new();
        let mut stream = zbus::MessageStream::from(&conn);
        while let Some(msg) = stream.next().await {
            let Ok(msg) = msg else { continue };
            let header = msg.header();
            match header.message_type() {
                Type::MethodCall => {
                    let Ok((app, replaces, app_icon, summary, body, _actions, hints, _)) =
                        msg.body().deserialize::<(
                            String,
                            u32,
                            String,
                            String,
                            String,
                            Vec<String>,
                            HashMap<String, OwnedValue>,
                            i32,
                        )>()
                    else {
                        continue;
                    };
                    // Plasma has no use for actions we can't invoke: they are not offered.
                    let item = entry(0, app, &app_icon, summary, &body, Vec::new(), &hints);
                    let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
                    pending.push((header.primary().serial_num().get(), sender, replaces, item));
                    // A reply that never comes (the call failed) must not pile up.
                    if pending.len() > 32 {
                        pending.remove(0);
                    }
                }
                Type::MethodReturn => {
                    let Some(serial) = header.reply_serial() else {
                        continue;
                    };
                    let dest = header
                        .destination()
                        .map(|d| d.to_string())
                        .unwrap_or_default();
                    let Some(at) = pending
                        .iter()
                        .position(|(s, from, ..)| *s == serial.get() && *from == dest)
                    else {
                        continue;
                    };
                    let (_, _, replaces, mut item) = pending.remove(at);
                    let Ok(id) = msg.body().deserialize::<u32>() else {
                        continue;
                    };
                    item.id = id;
                    log::debug!(
                        "plasma notification {id} from {}: {}",
                        item.app,
                        item.summary
                    );
                    if let Ok(mut store) = self.store.lock() {
                        store.expire();
                        let replacing = replaces != 0 && store.items.iter().any(|n| n.id == id);
                        // Plasma showed no pop-up (do not disturb) for what is new.
                        if !replacing && store.dnd.active(chrono::Local::now()) && !item.urgent {
                            store.missed += 1;
                        }
                        store.push(item);
                    }
                    publish(&self.ctx, &self.store);
                }
                Type::Signal => {
                    let Ok((id, reason)) = msg.body().deserialize::<(u32, u32)>() else {
                        continue;
                    };
                    // Expiring or being cut short leaves it in Plasma's history; dismissing it
                    // or the program withdrawing it takes it out.
                    if matches!(reason, 2 | 3) {
                        let removed = self.store.lock().is_ok_and(|mut s| {
                            let before = s.items.len();
                            s.items.retain(|n| n.id != id);
                            s.items.len() != before
                        });
                        if removed {
                            publish(&self.ctx, &self.store);
                        }
                    }
                }
                Type::Error => {
                    // The call failed: nothing was shown.
                    if let Some(serial) = header.reply_serial() {
                        pending.retain(|(s, ..)| *s != serial.get());
                    }
                }
            }
        }
        Ok(())
    }

    async fn serve(&self) -> zbus::Result<bool> {
        let conn = zbus::Connection::session().await?;
        conn.object_server()
            .at(
                PATH,
                Server {
                    store: self.store.clone(),
                    ctx: self.ctx.clone(),
                    osd: self.osd.clone(),
                },
            )
            .await?;
        let reply = conn
            .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
            .await;
        let owned = matches!(
            reply,
            Ok(zbus::fdo::RequestNameReply::PrimaryOwner
                | zbus::fdo::RequestNameReply::AlreadyOwner)
        );
        let _ = self.conn.set(conn);
        Ok(owned)
    }

    /// On Plasma: hold an inhibition of Plasma's pop-ups while do not disturb is on, and end a
    /// timed one on time. Plasma lets go of it by itself when our bus connection closes.
    async fn pause_plasma(&self) {
        let conn = match self.ctx.bus(false).await {
            Ok(c) => c,
            Err(e) => {
                log::warn!("notifications: {e}");
                return;
            }
        };
        let can_pause = supports_inhibit(&conn).await;
        log::info!("Plasma shows notifications; pausing its pop-ups: {can_pause}");
        if let Ok(mut s) = self.store.lock() {
            s.can_pause = can_pause;
        }
        publish(&self.ctx, &self.store);
        if !can_pause {
            return;
        }
        let mut rx = self.pause.subscribe();
        let mut cookie: Option<u32> = None;
        loop {
            let want = *rx.borrow_and_update();
            let result = match (want, cookie) {
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
                    self.ctx.toast(format!("Do not disturb: {e}"));
                }
            }
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(20)) => {
                    let expired = self.store.lock().map(|mut s| s.expire()).unwrap_or(false);
                    if expired {
                        publish(&self.ctx, &self.store);
                        self.pause.send_replace(false);
                    }
                }
            }
        }
    }

    /// The pop-ups, shared with the dashboard's own messages so a burst of both is folded into
    /// one summary.
    pub fn osd(&self) -> &Osd {
        &self.osd
    }

    /// Our interface on the bus, to emit signals from; None unless we are serving.
    async fn iface(&self) -> zbus::Result<Option<InterfaceRef<Server>>> {
        let Some(conn) = self.conn.get() else {
            return Ok(None);
        };
        Ok(Some(conn.object_server().interface(PATH).await?))
    }

    async fn emit_closed(&self, id: u32, reason: u32) -> zbus::Result<()> {
        if let Some(iface) = self.iface().await? {
            Server::notification_closed(iface.signal_emitter(), id, reason).await?;
        }
        Ok(())
    }

    async fn emit_invoked(&self, id: u32, action: &str) -> zbus::Result<()> {
        if let Some(iface) = self.iface().await? {
            Server::action_invoked(iface.signal_emitter(), id, action).await?;
        }
        Ok(())
    }

    pub fn set_dnd(&self, dnd: Dnd) {
        let active = dnd.active(chrono::Local::now());
        if let Ok(mut s) = self.store.lock() {
            s.dnd = dnd;
            if !active {
                s.dnd = Dnd::Off;
                s.missed = 0;
            }
        }
        publish(&self.ctx, &self.store);
        self.pause.send_replace(active);
    }

    /// Put one of our own messages (a timer going off) into the history, or on Plasma, send it
    /// to Plasma as a notification.
    pub fn add_local(&self, app: &str, summary: &str, body: &str) {
        if self.plasma {
            let (c, app, summary, body) = (
                self.ctx.clone(),
                app.to_string(),
                summary.to_string(),
                body.to_string(),
            );
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
        if let Ok(mut s) = self.store.lock() {
            let id = s.new_id();
            s.push(Notification {
                id,
                app: app.into(),
                summary: summary.into(),
                body: body.into(),
                icon: IconSrc::None,
                actions: Vec::new(),
                urgent: false,
                time: chrono::Local::now(),
            });
        }
        publish(&self.ctx, &self.store);
    }

    pub fn dismiss(&self, id: u32) {
        if let Ok(mut s) = self.store.lock() {
            s.items.retain(|n| n.id != id);
        }
        publish(&self.ctx, &self.store);
        let me = self.clone();
        self.ctx.spawn(async move {
            let _ = me.emit_closed(id, 2).await;
        });
    }

    pub fn clear(&self) {
        let ids: Vec<u32> = self
            .store
            .lock()
            .map(|mut s| s.items.drain(..).map(|n| n.id).collect())
            .unwrap_or_default();
        publish(&self.ctx, &self.store);
        let me = self.clone();
        self.ctx.spawn(async move {
            for id in ids {
                let _ = me.emit_closed(id, 2).await;
            }
        });
    }

    pub fn invoke(&self, id: u32) {
        let me = self.clone();
        self.ctx.spawn(async move {
            if let Err(e) = me.emit_invoked(id, "default").await {
                log::warn!("invoke: {e}");
            }
            me.dismiss(id);
        });
    }
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
    use super::{Dnd, strip_markup};
    use crate::notifications::next_hour;
    use chrono::TimeZone;

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
