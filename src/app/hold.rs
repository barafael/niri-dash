//! Long press: a press held still on a widget opens it on its own for adjusting, as on Plasma.
//!
//! It works anywhere on the widget, controls included, and those only hear of it if it comes to
//! something. The UI can't do this itself: an element sees no press its children took. So the
//! pointer is watched on its way there.

use super::App;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Timer, TimerMode};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// How long a press has to last.
const HOLD: Duration = Duration::from_millis(500);
/// How far it may wander in that time, across and down together.
const SLOP: f32 = 10.0;

#[derive(Default)]
pub(super) struct Hold {
    timer: Timer,
    /// Where the press is going on, until it stops counting.
    at: Cell<Option<LogicalPosition>>,
}

impl Hold {
    pub(super) fn cancel(&self) {
        self.at.set(None);
        self.timer.stop();
    }

    pub(super) fn watch(&self, app: &Rc<App>, event: &WindowEvent) {
        match *event {
            WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            } => {
                self.at.set(Some(position));
                let app: Weak<App> = Rc::downgrade(app);
                self.timer.start(TimerMode::SingleShot, HOLD, move || {
                    if let Some(app) = app.upgrade() {
                        app.held();
                    }
                });
            }
            WindowEvent::PointerMoved { position } => {
                if let Some(at) = self.at.get()
                    && (position.x - at.x).abs() + (position.y - at.y).abs() >= SLOP
                {
                    self.cancel();
                }
            }
            WindowEvent::PointerPressed { .. }
            | WindowEvent::PointerReleased { .. }
            | WindowEvent::PointerExited => self.cancel(),
            _ => {}
        }
    }
}

/// What the shell (or the page) calls with each pointer event.
pub(super) fn watcher(app: &Rc<App>) -> Box<dyn Fn(&WindowEvent)> {
    let app = app.clone();
    Box::new(move |event| app.hold.watch(&app, event))
}

impl App {
    /// The widget under a point in the window.
    fn widget_at(&self, at: LogicalPosition) -> Option<u32> {
        let ui = self.ui();
        let cell = ui.get_cell();
        let col = ((at.x - ui.get_grid_x()) / cell).floor() as i32;
        let row = ((at.y - ui.get_grid_y()) / cell).floor() as i32;
        self.layout
            .borrow()
            .visible()
            .into_iter()
            .find(|p| {
                let r = p.rect();
                (r.col..r.col + r.w).contains(&col) && (r.row..r.row + r.h).contains(&row)
            })
            .map(|p| p.id)
    }

    fn held(&self) {
        let Some(position) = self.hold.at.take() else {
            return;
        };
        let Some(id) = self.widget_at(position) else {
            return;
        };
        let ui = self.ui();
        if !ui.invoke_hold_widget(id as i32) {
            return;
        }
        // Whatever the press was on lets go of it, so letting go won't click it. Pressed again,
        // it lands on the widget's drag handle: the same press goes on to move the widget.
        let window = ui.window();
        window.dispatch_event(WindowEvent::PointerExited);
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
    }
}
