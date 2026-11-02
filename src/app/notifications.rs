//! The notifications widget and do not disturb.

use super::clock::{clock_text, when_text};
use super::{App, model};
use crate::sources::{
    self,
    notifications::{Dnd, next_hour},
};
use crate::{Dash, Notif, Notifications as UiNotifications};
use slint::ComponentHandle;
use std::rc::Rc;

/// "Until tomorrow" ends at this hour.
const DND_MORNING: u32 = 7;

impl App {
    pub(super) fn show_notifications(&self, s: sources::notifications::State) {
        let ui = self.ui();
        let n = ui.global::<UiNotifications>();
        n.set_server(s.server);
        self.sync_dnd(s.dnd, s.missed);
        n.set_items(model(
            s.items
                .iter()
                .map(|x| {
                    let icon = self.image(&x.icon);
                    Notif {
                        id: x.id as i32,
                        app: x.app.as_str().into(),
                        summary: x.summary.as_str().into(),
                        body: x.body.as_str().into(),
                        time: when_text(x.time, self.locale.us).into(),
                        has_icon: icon.is_some(),
                        icon: icon.unwrap_or_default(),
                        has_default: x.has_default(),
                        urgent: x.urgent,
                    }
                })
                .collect(),
        ));
    }

    /// The notification server's do-not-disturb state: show it, and remember it for restarts.
    fn sync_dnd(&self, dnd: Dnd, missed: u32) {
        let ui = self.ui();
        let n = ui.global::<crate::Notifications>();
        let now = chrono::Local::now();
        let us = self.locale.us;
        n.set_dnd(dnd.active(now));
        n.set_missed(missed as i32);
        n.set_dnd_text(
            match dnd {
                Dnd::Off => String::new(),
                Dnd::Forever => "Pop-ups paused".into(),
                Dnd::Until(t) if t.date_naive() == now.date_naive() => {
                    format!("Pop-ups paused until {}", clock_text(t, us))
                }
                Dnd::Until(t) => format!("Pop-ups paused until tomorrow {}", clock_text(t, us)),
            }
            .into(),
        );
        n.set_morning(clock_text(next_hour(now, DND_MORNING), us).into());
        let saved = dnd.to_saved();
        if self.state.borrow().dnd_until != saved {
            self.update_state(|s| s.dnd_until = saved);
        }
    }

    pub(super) fn toggle_dnd(&self) {
        let on = Dnd::from_saved(self.state.borrow().dnd_until).active(chrono::Local::now());
        self.notifications
            .set_dnd(if on { Dnd::Off } else { Dnd::Forever });
        self.osd(
            if on {
                "Notifications back on"
            } else {
                "Do not disturb"
            },
            if on {
                "preferences-system-notifications"
            } else {
                "notifications-disabled"
            },
        );
    }

    pub(super) fn osd(&self, text: &str, icon: &str) {
        self.notifications.osd().show(&self.ctx, text, icon);
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let n = ui.global::<crate::Notifications>();
    n.set_morning(clock_text(next_hour(chrono::Local::now(), DND_MORNING), app.locale.us).into());
    let a = app.clone();
    n.on_set_dnd(move |choice| {
        let now = chrono::Local::now();
        let dnd = match choice {
            1 => Dnd::Until(now + chrono::Duration::hours(1)),
            2 => Dnd::Until(next_hour(now, DND_MORNING)),
            3 => Dnd::Forever,
            _ => Dnd::Off,
        };
        a.notifications.set_dnd(dnd);
    });
    // Notifications.
    let n = ui.global::<UiNotifications>();
    let a = app.clone();
    n.on_dismiss(move |id| a.notifications.dismiss(id as u32));
    let a = app.clone();
    n.on_clear(move || a.notifications.clear());
    let a = app.clone();
    n.on_invoke(move |id| a.notifications.invoke(id as u32));
    // Do not disturb survives a restart.
    let saved = Dnd::from_saved(app.state.borrow().dnd_until);
    if saved != Dnd::Off {
        app.notifications.set_dnd(saved);
    }
}
