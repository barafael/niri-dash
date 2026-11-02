//! The widgets that show what their source reports and hand clicks back to it: system monitor,
//! Claude, windows, keyboard layout, Wi-Fi, Bluetooth, brightness, tray, weather and media.

use super::enums::{toggle_to_ui, weather_icon_to_ui};
use super::{App, held, model};
use crate::sources::{self, IconSrc, niri, tray};
use crate::{
    Bluetooth as UiBluetooth, BtDevice, Claude as UiClaude, ClaudeSession, Control, Dash,
    Media as UiMedia, MenuEntry, Perf, Tray as UiTray, TrayItem, Weather as UiWeather, WeatherDay,
    Wifi as UiWifi, WifiNet, WinItem, Windows as UiWindows,
};
use slint::ComponentHandle;
use std::rc::Rc;
use std::time::Instant;

impl App {
    pub(super) fn show_perf(&self, s: Box<sources::perf::Snapshot>) {
        let ui = self.ui();
        let s = *s;
        let p = ui.global::<Perf>();
        p.set_cpu(s.cpu);
        p.set_mem(s.mem);
        p.set_cpu_history(model(s.cpu_history));
        p.set_mem_history(model(s.mem_history));
        p.set_cpu_text(s.cpu_text.into());
        p.set_mem_text(s.mem_text.into());
        p.set_temp_text(s.temp_text.into());
        p.set_net_text(s.net_text.into());
        p.set_load_text(s.load_text.into());
        p.set_cores(model(s.cores));
        p.set_mem_used_text(s.mem_used_text.into());
        p.set_mem_avail_text(s.mem_avail_text.into());
        p.set_swap(s.swap);
        p.set_swap_text(s.swap_text.into());
        p.set_top_mem(model(
            s.top_mem
                .into_iter()
                .map(|(n, v)| crate::NameValue {
                    name: n.into(),
                    value: v.into(),
                })
                .collect(),
        ));
        p.set_net_rx_history(model(s.net_rx_history));
        p.set_net_tx_history(model(s.net_tx_history));
        p.set_net_rx_text(s.net_rx_text.into());
        p.set_net_tx_text(s.net_tx_text.into());
        p.set_net_peak_text(s.net_peak_text.into());
        p.set_net_iface(s.net_iface.into());
        p.set_net_addr(s.net_addr.into());
        p.set_net_rx_frac(s.net_rx_frac);
        p.set_net_tx_frac(s.net_tx_frac);
        let row = |d: &sources::perf::DiskRow| crate::DiskRow {
            mount: d.mount.as_str().into(),
            used: d.used,
            text: d.text.as_str().into(),
        };
        p.set_user_disks(model(
            s.disks.iter().filter(|d| !d.system).map(row).collect(),
        ));
        p.set_disks(model(s.disks.iter().map(row).collect()));
        p.set_disk_history(model(s.disk_history));
        p.set_disk_read_text(s.disk_read_text.into());
        p.set_disk_write_text(s.disk_write_text.into());
        p.set_root_used(s.root_used);
        p.set_has_gpu(s.gpu.is_some());
        if let Some(g) = s.gpu {
            p.set_gpu_name(g.name.into());
            p.set_gpu_busy(g.busy);
            p.set_gpu_history(model(g.history));
            p.set_gpu_vram(g.vram);
            p.set_gpu_vram_text(g.vram_text.into());
            p.set_gpu_temp_text(g.temp_text.into());
            p.set_gpu_power_text(g.power_text.into());
        }
    }

    pub(super) fn show_claude_usage(&self, u: sources::claude_usage::State) {
        let ui = self.ui();
        let c = ui.global::<crate::ClaudeUsage>();
        c.set_plan(u.plan.into());
        c.set_window_active(u.window_active);
        c.set_window_tokens(u.window_tokens.into());
        c.set_window_count(u.window_count.into());
        c.set_window_cached(u.window_cached.into());
        c.set_window_messages(u.window_messages as i32);
        c.set_window_left(u.window_left.into());
        c.set_window_progress(u.window_progress);
        c.set_window_resets(u.window_resets.into());
        c.set_burn(u.burn.into());
        c.set_today(u.today.into());
        c.set_week(u.week.into());
        c.set_hourly(model(u.hourly));
        c.set_models(model(
            u.models
                .into_iter()
                .map(|(n, sh)| crate::ModelShare {
                    name: n.into(),
                    share: sh,
                })
                .collect(),
        ));
    }

    pub(super) fn show_claude(&self, st: sources::claude::State) {
        let ui = self.ui();
        let c = ui.global::<UiClaude>();
        let mut wins = self.claude_windows.borrow_mut();
        wins.clear();
        c.set_busy(st.busy as i32);
        c.set_sessions(model(
            st.sessions
                .iter()
                .map(|s| {
                    if let Some(w) = s.window {
                        wins.insert(s.pid, w);
                    }
                    ClaudeSession {
                        pid: s.pid,
                        title: s.title.as_str().into(),
                        place: s.place.as_str().into(),
                        status: s.status_text.as_str().into(),
                        busy: s.status == sources::claude::Status::Busy,
                        has_window: s.window.is_some(),
                    }
                })
                .collect(),
        ));
    }

    pub(super) fn show_windows(&self, s: niri::WindowsState) {
        let ui = self.ui();
        let w = ui.global::<UiWindows>();
        let rows: Vec<WinItem> = s
            .rows
            .iter()
            .map(|r| {
                let icon = self.image(&r.icon);
                WinItem {
                    id: r.id as i32,
                    header: r.header,
                    title: r.title.as_str().into(),
                    app: r.app.as_str().into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                    focused: r.focused,
                    urgent: r.urgent,
                }
            })
            .collect();
        w.set_items(model(rows));
        w.set_count(s.count as i32);
        w.set_available(s.connected);
        let icon = s.focused_icon.as_ref().and_then(|i| self.image(i));
        w.set_has_focused_icon(icon.is_some());
        w.set_focused_icon(icon.unwrap_or_default());
    }

    pub(super) fn show_keyboard_layouts(&self, l: Option<niri_ipc::KeyboardLayouts>) {
        let ui = self.ui();
        let k = ui.global::<crate::KeyboardLayouts>();
        k.set_available(l.is_some());
        let l = l.unwrap_or(niri_ipc::KeyboardLayouts {
            names: Vec::new(),
            current_idx: 0,
        });
        k.set_current(l.current_idx as i32);
        k.set_layouts(model(
            l.names
                .iter()
                .map(|n| crate::LayoutItem {
                    short: sources::keyboard::short_label(n).into(),
                    name: n.into(),
                })
                .collect(),
        ));
    }

    pub(super) fn show_wifi(&self, s: sources::wifi::State) {
        let ui = self.ui();
        let w = ui.global::<UiWifi>();
        w.set_available(s.available);
        w.set_enabled(s.enabled);
        w.set_ssid(s.ssid.into());
        w.set_signal(s.signal);
        w.set_status(s.activity.to_string().into());
        w.set_needs_password(match &s.activity {
            sources::wifi::Activity::NeedsPassword(ssid) => ssid.as_str().into(),
            _ => Default::default(),
        });
        w.set_busy(s.activity.busy());
        w.set_networks(model(
            s.networks
                .into_iter()
                .map(|n| WifiNet {
                    ssid: n.ssid.into(),
                    signal: n.signal,
                    secure: n.secure,
                    active: n.active,
                    known: n.profile.is_some(),
                })
                .collect(),
        ));
    }

    pub(super) fn show_bluetooth(&self, s: sources::bluetooth::State) {
        let ui = self.ui();
        let b = ui.global::<UiBluetooth>();
        b.set_available(s.available);
        b.set_powered(s.powered);
        b.set_connected_count(s.devices.iter().filter(|d| d.connected).count() as i32);
        b.set_devices(model(
            s.devices
                .into_iter()
                .map(|d| BtDevice {
                    address: d.address.into(),
                    name: d.name.into(),
                    connected: d.connected,
                    battery: d.battery,
                    busy: d.busy,
                })
                .collect(),
        ));
    }

    pub(super) fn show_brightness(&self, level: Option<f32>) {
        let ui = self.ui();
        let b = ui.global::<crate::Brightness>();
        b.set_available(level.is_some());
        if let Some(l) = level
            && !self.brightness_touched.get().is_some_and(held)
        {
            b.set_level(l);
        }
    }

    pub(super) fn show_tray(&self, items: Vec<tray::Item>) {
        let ui = self.ui();
        let t = ui.global::<UiTray>();
        t.set_items(model(
            items
                .iter()
                .map(|i| {
                    let icon = self.image(&i.icon);
                    TrayItem {
                        id: i.address.as_str().into(),
                        title: i.title.as_str().into(),
                        has_icon: icon.is_some(),
                        icon: icon.unwrap_or_default(),
                        has_menu: i.has_menu,
                    }
                })
                .collect(),
        ));
        *self.tray_items.borrow_mut() = items;
    }

    pub(super) fn show_tray_menu(&self, m: tray::Menu) {
        let ui = self.ui();
        let t = ui.global::<UiTray>();
        if m.entries.is_empty() {
            self.toast("This tray item has an empty menu");
            return;
        }
        *self.menu_address.borrow_mut() = m.address;
        t.set_menu_title(m.title.into());
        t.set_menu_x(m.x);
        t.set_menu_y(m.y);
        t.set_menu(model(
            m.entries
                .into_iter()
                .map(|e| MenuEntry {
                    id: e.id,
                    label: e.label.into(),
                    depth: e.depth,
                    separator: e.separator,
                    enabled: e.enabled,
                    toggle: toggle_to_ui(e.toggle),
                    checked: e.checked,
                    header: e.header,
                })
                .collect(),
        ));
        t.set_menu_open(true);
    }

    pub(super) fn show_weather(&self, w: sources::weather::State) {
        let ui = self.ui();
        let u = ui.global::<UiWeather>();
        u.set_error(w.error.as_str().into());
        if !w.error.is_empty() {
            // Keep showing the last good forecast if there is one.
            return;
        }
        u.set_loaded(true);
        u.set_place(w.place.into());
        u.set_temp(w.temp.into());
        u.set_feels(w.feels.into());
        u.set_description(w.description.into());
        u.set_icon(weather_icon_to_ui(w.icon));
        u.set_details(w.details.into());
        u.set_days(model(
            w.days
                .into_iter()
                .map(|d| WeatherDay {
                    name: d.name.into(),
                    icon: weather_icon_to_ui(d.icon),
                    max: d.max.into(),
                    min: d.min.into(),
                })
                .collect(),
        ));
    }

    pub(super) fn show_media(&self, m: Option<sources::media::State>) {
        let ui = self.ui();
        let u = ui.global::<UiMedia>();
        u.set_available(m.is_some());
        let m = m.unwrap_or_default();
        u.set_title(m.title.into());
        u.set_artist(m.artist.into());
        u.set_player(m.player.into());
        u.set_playing(m.playing);
        u.set_progress(m.progress);
        let art = m.art.map(IconSrc::Path).and_then(|a| self.image(&a));
        u.set_has_art(art.is_some());
        u.set_art(art.unwrap_or_default());
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    // Windows.
    let w = ui.global::<UiWindows>();
    let a = app.clone();
    w.on_focus(move |id| niri::focus(&a.ctx, id as u64));
    let a = app.clone();
    w.on_close(move |id| niri::close(&a.ctx, id as u64));
    use niri_ipc::LayoutSwitchTarget as T;
    let kb = ui.global::<crate::KeyboardLayouts>();
    let a = app.clone();
    kb.on_next(move || niri::switch_layout(&a.ctx, T::Next));
    let a = app.clone();
    kb.on_prev(move || niri::switch_layout(&a.ctx, T::Prev));
    let a = app.clone();
    kb.on_pick(move |i| niri::switch_layout(&a.ctx, T::Index(i.clamp(0, 255) as u8)));
    // Wi-Fi.
    let w = ui.global::<UiWifi>();
    let a = app.clone();
    w.on_toggle(move || a.wifi.toggle());
    let a = app.clone();
    w.on_rescan(move || a.wifi.rescan());
    let a = app.clone();
    w.on_connect(move |ssid, pw| a.wifi.connect(ssid.into(), pw.into()));
    let a = app.clone();
    w.on_disconnect(move || a.wifi.disconnect());
    // Bluetooth.
    let b = ui.global::<UiBluetooth>();
    let a = app.clone();
    b.on_toggle(move || a.bluetooth.toggle());
    let a = app.clone();
    b.on_connect(move |addr| a.bluetooth.connect(addr.into()));
    let a = app.clone();
    b.on_disconnect(move |addr| a.bluetooth.disconnect(addr.into()));
    let a = app.clone();
    ui.global::<crate::Brightness>().on_set(move |v| {
        a.brightness_touched.set(Some(Instant::now()));
        a.brightness.set(v);
        if let Some(ui) = a.ui.upgrade() {
            ui.global::<crate::Brightness>().set_level(v);
        }
    });
    // Tray.
    let t = ui.global::<UiTray>();
    let a = app.clone();
    t.on_activate(move |id, x, y| {
        let item = a
            .tray_items
            .borrow()
            .iter()
            .find(|i| i.address == id.as_str())
            .cloned();
        match item {
            Some(i) if i.item_is_menu && i.has_menu => a.tray.open_menu(i.address, x, y),
            Some(i) => {
                a.tray.activate(i.address, x as i32, y as i32);
                a.pending.borrow_mut().push(Control::Hide);
            }
            None => {}
        }
    });
    let a = app.clone();
    t.on_open_menu(move |id, x, y| a.tray.open_menu(id.into(), x, y));
    let a = app.clone();
    t.on_menu_activate(move |id| {
        let address = a.menu_address.borrow().clone();
        a.tray.menu_item(address, id);
        if let Some(ui) = a.ui.upgrade() {
            ui.global::<UiTray>().set_menu_open(false);
        }
        a.pending.borrow_mut().push(Control::Hide);
    });
    let a = app.clone();
    t.on_menu_close(move || {
        if let Some(ui) = a.ui.upgrade() {
            ui.global::<UiTray>().set_menu_open(false);
        }
    });
    // Claude sessions.
    let a = app.clone();
    ui.global::<UiClaude>().on_focus(move |pid| {
        let win = a.claude_windows.borrow().get(&pid).copied();
        if let Some(id) = win {
            niri::focus(&a.ctx, id);
        }
    });
    // Media.
    let m = ui.global::<UiMedia>();
    let a = app.clone();
    m.on_play_pause(move || a.media.play_pause());
    let a = app.clone();
    m.on_next(move || a.media.next());
    let a = app.clone();
    m.on_previous(move || a.media.previous());
}
