//! The privacy widget: screen sharing (from niri) and microphone and camera use (from PipeWire).

use super::{App, model};
use crate::Dash;
use crate::sources::{self, niri};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

/// What the privacy widget shows.
#[derive(Default)]
pub(super) struct Privacy {
    /// Screencasts, from niri.
    casts: RefCell<Vec<niri::ScreenCast>>,
    /// Microphone and camera use, from PipeWire.
    usage: RefCell<sources::privacy::Usage>,
}

impl App {
    pub(super) fn set_casts(&self, casts: Vec<niri::ScreenCast>) {
        let started = casts.iter().any(|c| {
            !self
                .privacy
                .casts
                .borrow()
                .iter()
                .any(|o| o.session_id == c.session_id)
        });
        if started {
            self.osd("Screen sharing started", "video-display");
        }
        *self.privacy.casts.borrow_mut() = casts;
        self.push_privacy();
    }

    pub(super) fn set_privacy_usage(&self, usage: sources::privacy::Usage) {
        *self.privacy.usage.borrow_mut() = usage;
        self.push_privacy();
    }

    fn push_privacy(&self) {
        use crate::{PrivacyItem, PrivacyKind};
        let usage = self.privacy.usage.borrow();
        let casts = self.privacy.casts.borrow();
        let mut items = Vec::new();
        for app in &usage.mic {
            items.push(PrivacyItem {
                kind: PrivacyKind::Mic,
                app: app.as_str().into(),
                detail: "microphone".into(),
                session: 0,
            });
        }
        for app in &usage.camera {
            items.push(PrivacyItem {
                kind: PrivacyKind::Camera,
                app: app.as_str().into(),
                detail: "camera".into(),
                session: 0,
            });
        }
        for c in casts.iter() {
            let app = c
                .pid
                .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
                .map(|s| s.trim().to_string())
                .or_else(|| {
                    let node = c.pw_node?;
                    usage
                        .video_readers
                        .iter()
                        .find(|(n, _)| *n == node)
                        .map(|(_, app)| app.clone())
                })
                .unwrap_or_else(|| "An app".into());
            items.push(PrivacyItem {
                kind: PrivacyKind::Screen,
                app: app.into(),
                detail: format!("{}{}", c.target, if c.active { "" } else { " · paused" }).into(),
                session: c.session_id as i32,
            });
        }
        let ui = self.ui();
        let p = ui.global::<crate::Privacy>();
        p.set_mic(!usage.mic.is_empty());
        p.set_camera(!usage.camera.is_empty());
        p.set_screen(!casts.is_empty());
        p.set_items(model(items));
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let a = app.clone();
    ui.global::<crate::Privacy>()
        .on_stop(move |session| niri::stop_cast(&a.ctx, session as u64));
}
