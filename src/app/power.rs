//! The battery widget (charge, power profiles, peripherals, sleep blockers) and the session buttons.

use super::enums::{device_kind_to_ui, profile_from_ui, profile_to_ui};
use super::{App, model};
use crate::{Control, Dash, Power as UiPower, Session as UiSession};
use niri_dash_system::{Tell, power, session};
use slint::ComponentHandle;
use std::rc::Rc;

impl App {
    pub(super) fn show_power(&self, s: power::State) {
        let ui = self.ui();
        let p = ui.global::<UiPower>();
        p.set_has_battery(s.has_battery);
        p.set_percent(s.percent);
        p.set_charging(s.charging);
        p.set_status_text(s.status_text.into());
        p.set_power_text(s.power_text.into());
        p.set_has_profile(s.profile.is_some());
        p.set_profile(
            s.profile
                .map(profile_to_ui)
                .unwrap_or(crate::PowerProfile::Balanced),
        );
        p.set_profiles(model(s.profiles.into_iter().map(profile_to_ui).collect()));
        p.set_peripherals(model(
            s.peripherals
                .into_iter()
                .map(|d| crate::Peripheral {
                    kind: device_kind_to_ui(d.kind),
                    name: d.name.into(),
                    percent: d.percent,
                })
                .collect(),
        ));
    }

    pub(super) fn show_blockers(&self, list: Vec<power::Blocker>) {
        let ui = self.ui();
        ui.global::<UiPower>().set_blockers(model(
            list.into_iter()
                .map(|b| crate::Blocker {
                    who: b.who.into(),
                    why: b.why.into(),
                })
                .collect(),
        ));
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    // Power profiles and inhibiting sleep.
    let a = app.clone();
    ui.global::<UiPower>().on_set_profile(move |p| {
        a.tx.power
            .tell(power::Command::SetProfile(profile_from_ui(p)))
    });
    let a = app.clone();
    ui.global::<UiPower>()
        .on_toggle_inhibit(move || a.tx.power.tell(power::Command::ToggleInhibit));
    // Session.
    let s = ui.global::<UiSession>();
    let a = app.clone();
    s.on_lock(move || {
        a.pending.borrow_mut().push(Control::Hide);
        a.tx.session.tell(session::Command::Lock);
    });
    let a = app.clone();
    s.on_suspend(move || {
        a.pending.borrow_mut().push(Control::Hide);
        a.tx.session.tell(session::Command::Suspend);
    });
    let a = app.clone();
    s.on_logout(move || {
        a.pending.borrow_mut().push(Control::Hide);
        a.tx.session.tell(session::Command::Logout);
    });
    let a = app.clone();
    s.on_reboot(move || a.tx.session.tell(session::Command::Reboot));
    let a = app.clone();
    s.on_poweroff(move || a.tx.session.tell(session::Command::Poweroff));
}
