//! The niri-dash info widget: the facts about this build ([`crate::build_info`]), and opening or
//! copying them.

use super::App;
use crate::build_info::{self, Link};
use crate::{Build, BuildFact, Control, Dash};
use niri_dash_system::{Tell, host};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::path::PathBuf;
use std::rc::Rc;

/// Put the build's facts in the UI. `renderer` is what draws the dashboard ("OpenGL ES",
/// "software"); called again once that is known for sure.
pub(super) fn show(ui: &Dash, renderer: &str) {
    let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
    let facts: Vec<BuildFact> = build_info::facts(renderer, home.as_deref())
        .into_iter()
        .map(|f| {
            let (url, dir) = match f.link {
                Link::None => (String::new(), String::new()),
                Link::Url(u) => (u, String::new()),
                Link::Dir(d) => (String::new(), d),
            };
            BuildFact {
                label: f.label.into(),
                value: f.value.into(),
                url: url.into(),
                dir: dir.into(),
            }
        })
        .collect();
    let b = ui.global::<Build>();
    b.set_version(build_info::VERSION.into());
    b.set_toolkit(build_info::toolkit().into());
    b.set_renderer(renderer.into());
    b.set_facts(ModelRc::new(VecModel::from(facts)));
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let b = ui.global::<Build>();
    // What opens shows up behind the dashboard: it gets out of the way.
    let a = app.clone();
    b.on_open_url(move |url| {
        a.tx.host.tell(host::Command::OpenUrl(url.into()));
        a.pending.borrow_mut().push(Control::Hide);
    });
    let a = app.clone();
    b.on_open_dir(move |dir| {
        a.tx.host
            .tell(host::Command::Open(PathBuf::from(dir.as_str())));
        a.pending.borrow_mut().push(Control::Hide);
    });
    let a = app.clone();
    b.on_copy(move |value| a.copy_text(&value));
}
