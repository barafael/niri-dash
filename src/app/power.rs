//! The battery widget (charge, power profiles, peripherals, sleep blockers) and the session buttons.

use super::enums::{device_kind, profile_from_ui, profile_to_ui};
use super::{App, model};
use crate::sources::{self, power, session};
use crate::{Control, Dash, Power as UiPower, Session as UiSession};
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
                    kind: device_kind(d.kind),
                    name: d.name.into(),
                    percent: d.percent,
                })
                .collect(),
        ));
    }

    pub(super) fn show_blockers(&self, list: Vec<sources::inhibit::Blocker>) {
        let ui = self.ui();
        ui.global::<crate::Power>().set_blockers(model(
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
    ui.global::<UiPower>()
        .on_set_profile(move |p| power::set_profile(&a.ctx, profile_from_ui(p)));
    let a = app.clone();
    ui.global::<UiPower>()
        .on_toggle_inhibit(move || a.inhibit.toggle());
    // Session.
    let s = ui.global::<UiSession>();
    let a = app.clone();
    s.on_lock(move || {
        a.pending.borrow_mut().push(Control::Hide);
        session::lock(&a.ctx);
    });
    let a = app.clone();
    s.on_suspend(move || {
        a.pending.borrow_mut().push(Control::Hide);
        session::suspend(&a.ctx);
    });
    let a = app.clone();
    s.on_logout(move || {
        a.pending.borrow_mut().push(Control::Hide);
        session::logout(&a.ctx);
    });
    let a = app.clone();
    s.on_reboot(move || session::reboot(&a.ctx));
    let a = app.clone();
    s.on_poweroff(move || session::poweroff(&a.ctx));
}
