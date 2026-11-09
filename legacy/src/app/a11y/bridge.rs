//! The screen reader's side of the dashboard: what it asks for, and how the tree reaches it.
//!
//! `accesskit_unix` runs on a thread of its own and calls its handlers there, but the dashboard's
//! elements may only be touched on the UI thread. So the handlers do nothing but put a
//! [`Request`] on the dashboard's input channel, and the UI thread's loop answers it: it builds
//! the tree ([`super::tree`]) and hands it to a [`Sink`].
//!
//! The sink is the seam for tests. The real one is [`Unix`], which publishes over AT-SPI; a test
//! hands in one that only records, so everything up to the bus runs without a bus. Nothing
//! creates the real one but the daemon's and the apps' event loop (`wayland.rs`), so tests and
//! the stress run never reach the session's accessibility bus.

use super::tree;
use crate::app::{App, Input};
use crate::{Nav, NavMode};
use accesskit::{Action, ActionRequest, ActivationHandler, TreeUpdate};
use i_slint_core::item_tree::{ItemRc, ParentItemTraversalMode};
use i_slint_core::items::FocusReason;
use i_slint_core::window::WindowInner;
use niri_dash_system::Sink as Wake;
use slint::ComponentHandle;
use std::time::{Duration, Instant};

/// The id of the widget that holds `item` (a tile says `widget-<id>`), if one does.
fn widget_around(item: &ItemRc) -> Option<i32> {
    let mut at = Some(item.clone());
    while let Some(i) = at {
        if i.is_accessible()
            && let Some(id) = super::prop(&i, super::Prop::Id).strip_prefix("widget-")
        {
            return id.parse().ok();
        }
        at = i.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    None
}

/// What reaches the dashboard from a screen reader.
#[derive(Debug)]
pub(in crate::app) enum Request {
    /// A screen reader is here: it wants the whole tree, and changes to it from now on.
    Activated,
    /// The last screen reader left.
    Deactivated,
    /// It asked an element to do something.
    Action(ActionRequest),
}

/// Where the tree goes.
pub(in crate::app) trait TreeSink {
    fn update(&mut self, update: TreeUpdate);
    /// Whether the window has the keyboard: a screen reader reads out the focus only while it does.
    fn window_focus(&mut self, focused: bool);
}

/// The sink that publishes on the accessibility bus.
pub(in crate::app) struct Unix(accesskit_unix::Adapter);

impl TreeSink for Unix {
    fn update(&mut self, update: TreeUpdate) {
        self.0.update_if_active(|| update);
    }

    fn window_focus(&mut self, focused: bool) {
        self.0.update_window_focus_state(focused);
    }
}

/// What `accesskit_unix` calls, on its own thread: it only passes the news on.
#[derive(Clone)]
struct Handlers(Wake<Input>);

impl Handlers {
    fn tell(&self, request: Request) {
        let _ = self.0.send(Input::Access(request));
    }
}

impl ActivationHandler for Handlers {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // The UI thread builds it and sends it as soon as it gets to the request.
        self.tell(Request::Activated);
        None
    }
}

impl accesskit::ActionHandler for Handlers {
    fn do_action(&mut self, request: ActionRequest) {
        self.tell(Request::Action(request));
    }
}

impl accesskit::DeactivationHandler for Handlers {
    fn deactivate_accessibility(&mut self) {
        self.tell(Request::Deactivated);
    }
}

/// A sink on the session's accessibility bus. It costs a thread that sleeps until a screen
/// reader turns up; nothing is built before that.
pub(in crate::app) fn unix(input: Wake<Input>) -> Box<dyn TreeSink> {
    let h = Handlers(input);
    Box::new(Unix(accesskit_unix::Adapter::new(h.clone(), h.clone(), h)))
}

/// The least time between two trees, so a burst of changes (a list scrolling, an animation) is
/// one update.
const EVERY: Duration = Duration::from_millis(40);

/// What the dashboard knows of the screen reader.
#[derive(Default)]
pub(in crate::app) struct Access {
    sink: Option<Box<dyn TreeSink>>,
    /// A screen reader is listening.
    active: bool,
    /// What it was last given, so that an unchanged tree isn't sent again.
    last: Option<TreeUpdate>,
    /// Whether it was last told the window has the keyboard.
    focused: bool,
    sent_at: Option<Instant>,
    /// A change had to wait for [`EVERY`].
    waiting: bool,
}

impl App {
    /// Hand over where the tree goes. Nothing is built until a screen reader asks.
    pub(in crate::app) fn access_attach(&self, sink: Box<dyn TreeSink>) {
        let mut a = self.access.borrow_mut();
        *a = Access {
            sink: Some(sink),
            ..Access::default()
        };
    }

    /// What a screen reader asked of the dashboard.
    pub(in crate::app) fn access_request(&self, request: Request) {
        match request {
            Request::Activated => {
                {
                    let mut a = self.access.borrow_mut();
                    a.active = true;
                    a.last = None;
                }
                self.access_sync(true);
            }
            Request::Deactivated => {
                let mut a = self.access.borrow_mut();
                a.active = false;
                a.last = None;
                a.waiting = false;
            }
            Request::Action(request) => {
                self.access_act(&request);
                self.access_sync(true);
            }
        }
    }

    fn access_act(&self, request: &ActionRequest) {
        let Some(item) = self.access_ids.borrow().item(request.target_node) else {
            return;
        };
        match request.action {
            Action::Focus => {
                let ui = self.ui();
                match widget_around(&item) {
                    // The keyboard of this dashboard is its own: focusing something in a
                    // widget is entering the widget, as typing its letters is.
                    Some(id) if !ui.get_editing() => {
                        let nav = ui.global::<Nav>();
                        nav.set_expanded_id(0);
                        nav.set_active_id(id);
                        nav.set_mode(NavMode::Widget);
                        nav.set_tick(nav.get_tick() + 1);
                    }
                    _ => WindowInner::from_pub(ui.window()).set_focus_item(
                        &item,
                        true,
                        FocusReason::Programmatic,
                    ),
                }
            }
            _ => {
                if let Some(action) = tree::slint_action(request) {
                    item.accessible_action(&action);
                }
            }
        }
    }

    /// Give the screen reader what changed since it last looked. Called once a turn of the
    /// loop; free while no screen reader is there.
    pub(in crate::app) fn access_sync(&self, force: bool) {
        {
            let mut a = self.access.borrow_mut();
            if !a.active || a.sink.is_none() {
                return;
            }
            if !force && a.sent_at.is_some_and(|t| t.elapsed() < EVERY) {
                a.waiting = true;
                return;
            }
        }
        let update = self.access_tree();
        let visible = self.visible.get();
        let mut a = self.access.borrow_mut();
        let a = &mut *a;
        let Some(sink) = a.sink.as_mut() else { return };
        if a.last.as_ref() != Some(&update) {
            sink.update(update.clone());
            a.last = Some(update);
        }
        if a.focused != visible || a.sent_at.is_none() {
            sink.window_focus(visible);
            a.focused = visible;
        }
        a.sent_at = Some(Instant::now());
        a.waiting = false;
    }

    /// How long until the loop has to come back to send a change that had to wait.
    pub(in crate::app) fn access_wait(&self) -> Option<Duration> {
        let a = self.access.borrow();
        let waited = a.sent_at?.elapsed();
        a.waiting.then(|| EVERY.saturating_sub(waited))
    }
}
