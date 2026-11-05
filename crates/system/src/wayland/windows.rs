//! The window list on KWin, through org_kde_plasma_window_management: which windows there are,
//! which is active, and focusing and closing them. KWin hands this interface only to clients whose
//! desktop file asks for it (`X-KDE-Wayland-Interfaces=org_kde_plasma_window_management`, which
//! `niri-dash desktop-entries` writes), so without one the global is missing and the list is
//! unavailable.

use super::Services;
use crate::windows::{Command, Event, Row, State};
use crate::{IconSrc, sources::icons, text::strip_spinner};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle, globals::GlobalList,
};
use std::collections::HashMap;
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window::{self, OrgKdePlasmaWindow},
    org_kde_plasma_window_management::{self, OrgKdePlasmaWindowManagement},
};

// The bits of org_kde_plasma_window_management.state used here.
const ACTIVE: u32 = 0x1;
const MINIMIZED: u32 = 0x2;
const DEMANDS_ATTENTION: u32 = 0x80;
/// Left out of taskbars (docks, panels, the dashboard itself): left out of the list too.
const SKIP_TASKBAR: u32 = 0x1000;

/// Every listed window by its process: (window id, title). Kept for the Claude sessions, which
/// find the terminal a session runs in by its process.
static BY_PID: std::sync::Mutex<Option<HashMap<i32, (u64, String)>>> = std::sync::Mutex::new(None);

/// The windows KWin listed last, by process id. Empty if it lists none (or not to us).
pub fn by_pid() -> HashMap<i32, (u64, String)> {
    BY_PID
        .lock()
        .ok()
        .and_then(|m| m.clone())
        .unwrap_or_default()
}

struct Window {
    id: u64,
    proxy: OrgKdePlasmaWindow,
    title: String,
    app_id: String,
    pid: u32,
    state: u32,
    /// Everything it starts with has arrived (`initial_state`).
    ready: bool,
}

#[derive(Default)]
pub struct Windows {
    manager: Option<OrgKdePlasmaWindowManagement>,
    /// In the order KWin announced them.
    list: Vec<Window>,
    next_id: u64,
    sent: Option<State>,
}

impl Windows {
    /// Bind the window management global, if KWin offers it to us. `kwin`: this is KWin at all
    /// (elsewhere it is not worth asking).
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Services>, kwin: bool) -> Self {
        if !kwin {
            return Self::default();
        }
        // 13 is the first version that names windows by uuid.
        let manager = globals
            .bind::<OrgKdePlasmaWindowManagement, _, _>(qh, 13..=18, ())
            .ok();
        if manager.is_none() {
            log::warn!(
                "KWin doesn't offer the window list to this program; run `niri-dash \
                 desktop-entries` and restart niri-dash"
            );
        }
        Self {
            manager,
            ..Default::default()
        }
    }

    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    fn find(&mut self, id: u64) -> Option<&mut Window> {
        self.list.iter_mut().find(|w| w.id == id)
    }

    fn rows(&self) -> State {
        let mut rows = Vec::new();
        let mut focused_icon = None;
        for w in self
            .list
            .iter()
            .filter(|w| w.ready && w.state & SKIP_TASKBAR == 0)
        {
            let icon = icons::app_icon(&w.app_id)
                .map(IconSrc::Path)
                .unwrap_or(IconSrc::None);
            let active = w.state & ACTIVE != 0;
            if active {
                focused_icon = Some(icon.clone());
            }
            let title = strip_spinner(&w.title);
            rows.push(Row {
                id: w.id,
                header: false,
                title: if title.is_empty() { &w.app_id } else { title }.to_string(),
                app: w.app_id.rsplit('.').next().unwrap_or(&w.app_id).to_string(),
                icon,
                focused: active,
                urgent: w.state & DEMANDS_ATTENTION != 0,
            });
        }
        State {
            connected: true,
            count: rows.len(),
            rows,
            focused_icon,
        }
    }

    /// Tell the dashboard, if the list changed.
    fn publish(&mut self, events: &crate::EventSender) {
        if self.manager.is_none() {
            return;
        }
        let state = self.rows();
        if let Ok(mut by_pid) = BY_PID.lock() {
            *by_pid = Some(
                self.list
                    .iter()
                    .filter(|w| w.ready && w.pid != 0)
                    .map(|w| (w.pid as i32, (w.id, w.title.clone())))
                    .collect(),
            );
        }
        if self.sent.as_ref() != Some(&state) {
            self.sent = Some(state.clone());
            let _ = events.send(Event::State(state).into());
        }
    }

    fn activate(&self, w: &Window) {
        // Active, and no longer minimized.
        w.proxy.set_state(ACTIVE | MINIMIZED, ACTIVE);
    }

    pub fn command(&mut self, cmd: Command, events: &crate::EventSender) {
        match cmd {
            Command::Focus(id) => {
                if let Some(w) = self.list.iter().find(|w| w.id == id) {
                    self.activate(w);
                }
            }
            Command::FocusPid(pid) => {
                if let Some(w) = self
                    .list
                    .iter()
                    .find(|w| w.ready && i64::from(w.pid) == i64::from(pid))
                {
                    self.activate(w);
                }
            }
            Command::Close(id) => {
                if let Some(w) = self.list.iter().find(|w| w.id == id) {
                    w.proxy.close();
                }
            }
        }
        self.publish(events);
    }

    fn event(&mut self, id: u64, event: org_kde_plasma_window::Event, events: &crate::EventSender) {
        use org_kde_plasma_window::Event as E;
        let Some(w) = self.find(id) else { return };
        match event {
            E::TitleChanged { title } => w.title = title,
            E::AppIdChanged { app_id } => w.app_id = app_id,
            E::StateChanged { flags } => w.state = flags,
            E::PidChanged { pid } => w.pid = pid,
            E::InitialState => w.ready = true,
            E::Unmapped => {
                if let Some(at) = self.list.iter().position(|w| w.id == id) {
                    let gone = self.list.remove(at);
                    gone.proxy.destroy();
                }
            }
            _ => return,
        }
        self.publish(events);
    }
}

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for Services {
    fn event(
        me: &mut Self,
        manager: &OrgKdePlasmaWindowManagement,
        event: org_kde_plasma_window_management::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let org_kde_plasma_window_management::Event::WindowWithUuid { uuid, .. } = event {
            let id = me.windows.next_id;
            me.windows.next_id += 1;
            let proxy = manager.get_window_by_uuid(uuid, qh, id);
            me.windows.list.push(Window {
                id,
                proxy,
                title: String::new(),
                app_id: String::new(),
                pid: 0,
                state: 0,
                ready: false,
            });
        }
    }
}

impl Dispatch<OrgKdePlasmaWindow, u64> for Services {
    fn event(
        me: &mut Self,
        _: &OrgKdePlasmaWindow,
        event: org_kde_plasma_window::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let events = me.events.clone();
        me.windows.event(*id, event, &events);
    }
}
