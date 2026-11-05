//! The widgets that show what their source reports and hand clicks back to it: system monitor,
//! Claude, windows, keyboard layout, Wi-Fi, Bluetooth, brightness, tray, weather and media.

use super::enums::{device_kind_to_ui, toggle_to_ui, weather_icon_to_ui};
use super::{App, held, model};
use crate::{
    Bluetooth as UiBluetooth, BtDetail, BtDevice, BtFact, BtPrompt, Claude as UiClaude,
    ClaudeSession, Control, Dash, Media as UiMedia, MenuEntry, Perf, Tray as UiTray, TrayItem,
    Weather as UiWeather, WeatherDay, Wifi as UiWifi, WifiDetail, WifiFact, WifiNet, WifiPrompt,
    WifiSaved, WinItem, Windows as UiWindows,
};
use niri_dash_system::{
    self as sys, Tell, bluetooth, brightness, keyboard, media, tray, wifi, windows,
};
use slint::ComponentHandle;
use std::rc::Rc;
use web_time::Instant;

impl App {
    pub(super) fn show_perf(&self, s: Box<sys::perf::Snapshot>) {
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
        let row = |d: &sys::perf::DiskRow| crate::DiskRow {
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

    pub(super) fn show_claude_usage(&self, u: sys::claude::Usage) {
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

    pub(super) fn show_claude(&self, st: sys::claude::State) {
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
                        busy: s.status == sys::claude::Status::Busy,
                        has_window: s.window.is_some(),
                    }
                })
                .collect(),
        ));
    }

    pub(super) fn show_windows(&self, s: windows::State) {
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

    pub(super) fn show_keyboard_layouts(&self, l: Option<keyboard::Layouts>) {
        let ui = self.ui();
        let k = ui.global::<crate::KeyboardLayouts>();
        k.set_available(l.is_some());
        let l = l.unwrap_or_default();
        k.set_current(l.current as i32);
        k.set_layouts(model(
            l.layouts
                .into_iter()
                .map(|l| crate::LayoutItem {
                    short: l.short.into(),
                    name: l.name.into(),
                })
                .collect(),
        ));
    }

    pub(super) fn show_wifi(&self, s: wifi::State) {
        use wifi::{Activity, PromptKind};
        let ui = self.ui();
        let w = ui.global::<UiWifi>();
        w.set_available(s.available);
        w.set_enabled(s.enabled);
        w.set_blocked(s.blocked);
        w.set_ssid(sys::clean_label(&s.ssid).into());
        w.set_signal(s.signal);
        w.set_connecting(s.connecting_key.as_str().into());
        let status = match &s.activity {
            Activity::Idle if !s.connecting.is_empty() => {
                format!("Connecting to {}…", s.connecting)
            }
            a => a.to_string(),
        };
        w.set_status(status.into());
        w.set_failed(matches!(s.activity, Activity::Failed(_)));
        w.set_scanning(s.scanning);
        w.set_portal(s.link.connectivity == wifi::Connectivity::Portal);
        w.set_prompt(match &s.prompt {
            Some(p) => WifiPrompt {
                serial: p.serial as i32,
                ssid: sys::clean_label(&p.ssid).into(),
                kind: match p.kind {
                    PromptKind::Password => 0,
                    PromptKind::Wep => 1,
                    PromptKind::Enterprise => 2,
                },
                retry: p.retry,
                new: p.new,
                identity: p.identity.as_str().into(),
                message: p.message.as_str().into(),
            },
            None => WifiPrompt::default(),
        });
        w.set_detail(match &s.detail {
            Some(d) => WifiDetail {
                key: d.key.as_str().into(),
                ssid: sys::clean_label(&d.ssid).into(),
                in_range: d.network.is_some(),
                active: d.network.as_ref().is_some_and(|n| n.active),
                known: d.profile.is_some(),
                uuid: d
                    .profile
                    .as_ref()
                    .map(|p| p.uuid.as_str())
                    .unwrap_or_default()
                    .into(),
                autoconnect: d.profile.as_ref().is_some_and(|p| p.autoconnect),
                facts: model(
                    wifi::facts(d, &s.link)
                        .into_iter()
                        .map(|(label, value)| WifiFact {
                            label: label.into(),
                            value: value.into(),
                        })
                        .collect(),
                ),
            },
            None => WifiDetail::default(),
        });
        w.set_revealed(s.secret.is_some());
        w.set_password(
            s.secret
                .as_ref()
                .map(|x| x.password.as_str())
                .unwrap_or_default()
                .into(),
        );
        w.set_qr(
            s.secret
                .as_ref()
                .and_then(|x| self.image(&x.qr))
                .unwrap_or_default(),
        );
        w.set_saved(model(
            s.saved
                .iter()
                .map(|p| {
                    let key = p.key();
                    let in_range = s.networks.iter().any(|n| n.key() == key);
                    WifiSaved {
                        key: key.into(),
                        uuid: p.uuid.as_str().into(),
                        ssid: sys::clean_label(&p.ssid).into(),
                        in_range,
                        detail: if in_range {
                            "in range".into()
                        } else if p.timestamp > 0 {
                            wifi::day(p.timestamp).into()
                        } else {
                            "never used".into()
                        },
                    }
                })
                .collect(),
        ));
        w.set_networks(model(
            s.networks
                .iter()
                .map(|n| {
                    let twins = s.networks.iter().filter(|m| m.ssid == n.ssid).count();
                    WifiNet {
                        key: n.key().into(),
                        ssid: sys::clean_label(&n.ssid).into(),
                        signal: n.signal,
                        secure: n.security.needs_secret(),
                        active: n.active,
                        known: n.known(),
                        tag: if twins > 1 {
                            n.security_label().into()
                        } else {
                            Default::default()
                        },
                    }
                })
                .collect(),
        ));
    }

    pub(super) fn show_bluetooth(&self, s: bluetooth::State) {
        use bluetooth::{Activity, PromptKind};
        let ui = self.ui();
        let b = ui.global::<UiBluetooth>();
        let device = |d: &bluetooth::Device| BtDevice {
            address: d.address.as_str().into(),
            name: d.name.as_str().into(),
            kind: device_kind_to_ui(d.kind),
            connected: d.connected,
            paired: d.paired,
            trusted: d.trusted,
            blocked: d.blocked,
            battery: d.battery,
            signal: d.bars(),
            busy: d.busy,
        };
        b.set_available(s.available);
        b.set_powered(s.powered);
        b.set_blocked(s.blocked);
        b.set_adapter_name(s.adapter_name.as_str().into());
        b.set_discovering(s.discovering);
        b.set_connected_count(s.devices.iter().filter(|d| d.connected).count() as i32);
        b.set_devices(model(s.devices.iter().map(device).collect()));
        b.set_nearby(model(s.nearby.iter().map(device).collect()));
        b.set_unnamed(s.unnamed as i32);
        b.set_status(s.activity.to_string().into());
        b.set_failed(matches!(s.activity, Activity::Failed(_)));
        b.set_prompt(match &s.prompt {
            Some(p) => {
                let (kind, code, entered) = match &p.kind {
                    PromptKind::Confirm(k) => (1, format!("{k:06}"), 0),
                    PromptKind::EnterPin => (2, String::new(), 0),
                    PromptKind::EnterPasskey => (3, String::new(), 0),
                    PromptKind::ShowPin(pin) => (4, pin.clone(), 0),
                    PromptKind::ShowPasskey(k, n) => (5, format!("{k:06}"), i32::from(*n)),
                    PromptKind::Authorize => (6, String::new(), 0),
                };
                BtPrompt {
                    serial: p.serial as i32,
                    name: p.name.as_str().into(),
                    address: p.address.as_str().into(),
                    kind,
                    code: code.into(),
                    entered,
                }
            }
            None => BtPrompt::default(),
        });
        b.set_detail(match &s.detail {
            Some(d) => BtDetail {
                address: d.address.as_str().into(),
                name: d.name.as_str().into(),
                kind: device_kind_to_ui(d.kind),
                connected: d.connected,
                paired: d.paired,
                trusted: d.trusted,
                blocked: d.blocked,
                facts: model(
                    bluetooth::facts(d)
                        .into_iter()
                        .map(|(label, value)| BtFact {
                            label: label.into(),
                            value: value.into(),
                        })
                        .collect(),
                ),
            },
            None => BtDetail::default(),
        });
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

    pub(super) fn show_weather(&self, w: sys::weather::State) {
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

    pub(super) fn show_media(&self, m: Option<media::State>) {
        let ui = self.ui();
        let u = ui.global::<UiMedia>();
        u.set_available(m.is_some());
        let m = m.unwrap_or_default();
        u.set_title(m.title.into());
        u.set_artist(m.artist.into());
        u.set_player(m.player.into());
        u.set_playing(m.playing);
        u.set_progress(m.progress);
        // A cover in pixels is only turned into an image when it changes: the state comes every second.
        let mut last = self.media_art.borrow_mut();
        let art = match (&m.art, last.as_ref()) {
            (Some(new), Some((old, image))) if new == old => image.clone(),
            (Some(new), _) => {
                let image = self.image(new);
                *last = Some((new.clone(), image.clone()));
                image
            }
            (None, _) => {
                *last = None;
                None
            }
        };
        u.set_has_art(art.is_some());
        u.set_art(art.unwrap_or_default());
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    // Windows.
    let w = ui.global::<UiWindows>();
    let a = app.clone();
    w.on_focus(move |id| a.tx.windows.tell(windows::Command::Focus(id as u64)));
    let a = app.clone();
    w.on_close(move |id| a.tx.windows.tell(windows::Command::Close(id as u64)));
    use keyboard::{Command::Switch, Switch as To};
    let kb = ui.global::<crate::KeyboardLayouts>();
    let a = app.clone();
    kb.on_next(move || a.tx.keyboard.tell(Switch(To::Next)));
    let a = app.clone();
    kb.on_prev(move || a.tx.keyboard.tell(Switch(To::Prev)));
    let a = app.clone();
    kb.on_pick(move |i| a.tx.keyboard.tell(Switch(To::Index(i.max(0) as u32))));
    // Wi-Fi.
    let w = ui.global::<UiWifi>();
    let wifi = |a: &Rc<App>, cmd: wifi::Command| a.tx.wifi.tell(cmd);
    let a = app.clone();
    w.on_toggle(move || wifi(&a, wifi::Command::Toggle));
    let a = app.clone();
    w.on_rescan(move || wifi(&a, wifi::Command::Rescan));
    let a = app.clone();
    w.on_connect(move |ssid| wifi(&a, wifi::Command::Connect { key: ssid.into() }));
    let a = app.clone();
    w.on_answer(move |serial, identity, password, eap, domain, insecure| {
        let answer = wifi::Answer {
            identity: identity.into(),
            password: password.into(),
            eap: wifi::Eap::from_index(eap),
            domain: domain.into(),
            insecure,
        };
        wifi(
            &a,
            wifi::Command::Answer {
                serial: serial.max(0) as u64,
                answer,
            },
        )
    });
    let a = app.clone();
    w.on_cancel_prompt(move || wifi(&a, wifi::Command::CancelPrompt));
    let a = app.clone();
    w.on_join_hidden(
        move |ssid, security, identity, password, eap, domain, insecure| {
            let answer = wifi::Answer {
                identity: identity.into(),
                password: password.into(),
                eap: wifi::Eap::from_index(eap),
                domain: domain.into(),
                insecure,
            };
            wifi(
                &a,
                wifi::Command::JoinHidden {
                    ssid: ssid.into(),
                    security,
                    answer,
                },
            )
        },
    );
    let a = app.clone();
    w.on_disconnect(move || wifi(&a, wifi::Command::Disconnect));
    let a = app.clone();
    w.on_forget(move |uuid| wifi(&a, wifi::Command::Forget { uuid: uuid.into() }));
    let a = app.clone();
    w.on_set_autoconnect(move |uuid, on| {
        wifi(
            &a,
            wifi::Command::SetAutoconnect {
                uuid: uuid.into(),
                on,
            },
        )
    });
    let a = app.clone();
    w.on_select(move |ssid| wifi(&a, wifi::Command::Select { key: ssid.into() }));
    let a = app.clone();
    w.on_reveal(move |uuid| wifi(&a, wifi::Command::Reveal { uuid: uuid.into() }));
    let a = app.clone();
    w.on_hide_secret(move || wifi(&a, wifi::Command::HideSecret));
    let a = app.clone();
    w.on_sign_in(move || wifi(&a, wifi::Command::SignIn));
    // Bluetooth.
    use bluetooth::Command as Bt;
    let b = ui.global::<UiBluetooth>();
    let bt = |a: &Rc<App>, cmd: Bt| a.tx.bluetooth.tell(cmd);
    let a = app.clone();
    b.on_toggle(move || bt(&a, Bt::Toggle));
    let a = app.clone();
    b.on_connect(move |addr| bt(&a, Bt::Connect(addr.into())));
    let a = app.clone();
    b.on_disconnect(move |addr| bt(&a, Bt::Disconnect(addr.into())));
    let a = app.clone();
    b.on_scan(move |on| bt(&a, Bt::Scan(on)));
    let a = app.clone();
    b.on_pair(move |addr| bt(&a, Bt::Pair(addr.into())));
    let a = app.clone();
    b.on_cancel_pairing(move || bt(&a, Bt::CancelPairing));
    let a = app.clone();
    b.on_answer(move |serial, yes, code| {
        use bluetooth::Answer;
        let serial = serial.max(0) as u64;
        if !yes {
            bt(&a, Bt::Decline);
        } else if code.is_empty() {
            bt(
                &a,
                Bt::Answer {
                    serial,
                    answer: Answer::Yes,
                },
            );
        } else {
            bt(
                &a,
                Bt::Answer {
                    serial,
                    answer: Answer::Code(code.into()),
                },
            );
        }
    });
    let a = app.clone();
    b.on_select(move |addr| bt(&a, Bt::Select(addr.into())));
    let a = app.clone();
    b.on_set_trusted(move |addr, on| bt(&a, Bt::SetTrusted(addr.into(), on)));
    let a = app.clone();
    b.on_set_blocked(move |addr, on| bt(&a, Bt::SetBlocked(addr.into(), on)));
    let a = app.clone();
    b.on_forget(move |addr| bt(&a, Bt::Forget(addr.into())));
    let a = app.clone();
    b.on_rename(move |addr, name| bt(&a, Bt::Rename(addr.into(), name.into())));
    let a = app.clone();
    ui.global::<crate::Brightness>().on_set(move |v| {
        a.brightness_touched.set(Some(Instant::now()));
        a.tx.brightness.tell(brightness::Command::Set(v));
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
            Some(i) if i.item_is_menu && i.has_menu => a.tx.tray.tell(tray::Command::OpenMenu {
                address: i.address,
                x,
                y,
            }),
            Some(i) => {
                a.tx.tray.tell(tray::Command::Activate {
                    address: i.address,
                    x: x as i32,
                    y: y as i32,
                });
                a.pending.borrow_mut().push(Control::Hide);
            }
            None => {}
        }
    });
    let a = app.clone();
    t.on_open_menu(move |id, x, y| {
        a.tx.tray.tell(tray::Command::OpenMenu {
            address: id.into(),
            x,
            y,
        })
    });
    let a = app.clone();
    t.on_menu_activate(move |id| {
        let address = a.menu_address.borrow().clone();
        a.tx.tray.tell(tray::Command::MenuItem { address, id });
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
            a.tx.windows.tell(windows::Command::ShowTab(pid));
            a.tx.windows.tell(windows::Command::Focus(id));
        }
    });
    // Media.
    let m = ui.global::<UiMedia>();
    let a = app.clone();
    m.on_play_pause(move || a.tx.media.tell(media::Command::PlayPause));
    let a = app.clone();
    m.on_next(move || a.tx.media.tell(media::Command::Next));
    let a = app.clone();
    m.on_previous(move || a.tx.media.tell(media::Command::Previous));
}
