//! A minimal org.freedesktop.Notifications server that keeps a history for the dashboard. It only
//! claims the bus name if nobody else has it, so a running mako/dunst/swaync wins.

use super::{Ctx, IconSrc, Msg, icons, osd::Osd, prop, unescape_entities};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::fdo::RequestNameFlags;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::OwnedValue;

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";
const MAX: usize = 100;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: IconSrc,
    pub actions: Vec<(String, String)>,
    pub urgent: bool,
    pub time: chrono::DateTime<chrono::Local>,
}

impl Notification {
    pub fn has_default(&self) -> bool {
        self.actions.iter().any(|(k, _)| k == "default")
    }
}

/// Do not disturb: new notifications go into the history without a pop-up (critical ones still
/// pop up).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dnd {
    #[default]
    Off,
    Until(chrono::DateTime<chrono::Local>),
    Forever,
}

impl Dnd {
    pub fn active(self, now: chrono::DateTime<chrono::Local>) -> bool {
        match self {
            Dnd::Off => false,
            Dnd::Until(t) => now < t,
            Dnd::Forever => true,
        }
    }

    /// As stored in state.json: Unix seconds, `i64::MAX` for forever.
    pub fn to_saved(self) -> Option<i64> {
        match self {
            Dnd::Off => None,
            Dnd::Until(t) => Some(t.timestamp()),
            Dnd::Forever => Some(i64::MAX),
        }
    }

    pub fn from_saved(v: Option<i64>) -> Dnd {
        use chrono::TimeZone;
        match v {
            None => Dnd::Off,
            Some(i64::MAX) => Dnd::Forever,
            Some(t) => chrono::Local
                .timestamp_opt(t, 0)
                .single()
                .map_or(Dnd::Off, Dnd::Until),
        }
    }
}

/// The next `hour`:00 after `now` (tomorrow if that hour has passed today).
pub fn next_hour(
    now: chrono::DateTime<chrono::Local>,
    hour: u32,
) -> chrono::DateTime<chrono::Local> {
    use chrono::{Days, TimeZone};
    let mut day = now.date_naive();
    loop {
        if let Some(t) = day
            .and_hms_opt(hour, 0, 0)
            .and_then(|t| chrono::Local.from_local_datetime(&t).earliest())
            && t > now
        {
            return t;
        }
        day = day + Days::new(1);
    }
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub server: bool,
    pub items: Vec<Notification>,
    pub dnd: Dnd,
    /// Notifications that came in without a pop-up during do not disturb.
    pub missed: u32,
}

#[derive(Default)]
struct Store {
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
            server: true,
            items: s.items.clone(),
            dnd: s.dnd,
            missed: s.missed,
        })
        .unwrap_or_default();
    ctx.send(Msg::Notifications(state));
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
        let icon = resolve_icon(&app_icon, &hints);
        let popup_icon = osd_icon(&app_icon, &hints, urgent);
        let actions = actions
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (c[0].clone(), c[1].clone()))
            .collect();
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
            store.push(Notification {
                id,
                app: app_name,
                summary,
                body: strip_markup(&body),
                icon,
                actions,
                urgent,
                time: chrono::Local::now(),
            });
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
}

impl Notifications {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            osd: Osd::default(),
            store: Default::default(),
            conn: Default::default(),
        };
        let m = me.clone();
        ctx.spawn(async move {
            match m.serve().await {
                Ok(true) => {
                    log::info!("serving {NAME}");
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
                    m.ctx.send(Msg::Notifications(State {
                        server: false,
                        ..Default::default()
                    }));
                }
                Err(e) => log::warn!("notification server: {e}"),
            }
        });
        me
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
        if let Ok(mut s) = self.store.lock() {
            s.dnd = dnd;
            if !dnd.active(chrono::Local::now()) {
                s.dnd = Dnd::Off;
                s.missed = 0;
            }
        }
        publish(&self.ctx, &self.store);
    }

    /// Put one of our own messages (a timer going off) into the history.
    pub fn add_local(&self, app: &str, summary: &str, body: &str) {
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

#[cfg(test)]
mod tests {
    use super::{Dnd, next_hour, strip_markup};
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
