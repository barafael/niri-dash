//! Text fields for the Wi-Fi and Bluetooth forms: the password prompt, the hidden-network form,
//! the pairing code, the new name of a device. Ports of `KeyField` (`slint/ui/widgets/net.slint`)
//! and `SilentTextField` (`slint/ui/common.slint`) onto `gpui_kit::component::input`.
//!
//! A [`Pool`] holds a widget's `InputState`s. They are made in a window (that is how GPUI makes
//! a text input), so a pool appears a moment after the form that needs it: through [`later`],
//! which does something to the dashboard with the window at hand. The text of a field is what its
//! `InputState` holds, read when it is needed (a join, an answer), and never copied anywhere else.
//!
//! Keys: while a field has the keyboard, what it types is its own, and so are the keys it
//! edits with. Only these reach the dashboard through the field's wrapper (`key_field`'s
//! `on_key`): Tab (the next field), Escape (the keyboard goes back to the widget's cursor), and
//! Enter (an `InputEvent`, "accepted").
//!
//! For screen readers a password is a password field, which reads out no value, and so is a pairing
//! code; a field's label is what it is for.

use crate::dash::Dash;
use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, Focusable as _, KeyDownEvent, SharedString, Window, div, px,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, Size};
use std::rc::Rc;

/// What to make a field of.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub name: &'static str,
    pub placeholder: &'static str,
    /// What is typed shows as dots.
    pub masked: bool,
}

/// What a field reports about itself: it got the keyboard, lost it, was accepted with Enter.
pub type OnEvent =
    Rc<dyn Fn(&mut Dash, &'static str, &InputEvent, &mut Window, &mut Context<Dash>)>;

/// A widget's text fields, by name.
#[derive(Clone)]
pub struct Pool {
    items: Vec<(Spec, Entity<InputState>)>,
}

impl Pool {
    /// Make the fields in `window`; `on` hears what each does.
    pub fn new(specs: &[Spec], window: &mut Window, cx: &mut Context<Dash>, on: OnEvent) -> Self {
        let items = specs
            .iter()
            .map(|&spec| {
                let state = cx.new(|cx| {
                    let state = InputState::new(window, cx).placeholder(spec.placeholder);
                    if spec.masked {
                        state.masked(true)
                    } else {
                        state
                    }
                });
                let on = on.clone();
                cx.subscribe_in(
                    &state,
                    window,
                    move |d, _, event: &InputEvent, window, cx| on(d, spec.name, event, window, cx),
                )
                .detach();
                (spec, state)
            })
            .collect();
        Self { items }
    }

    pub fn state(&self, name: &str) -> Option<&Entity<InputState>> {
        self.items
            .iter()
            .find(|(s, _)| s.name == name)
            .map(|(_, e)| e)
    }

    /// What was typed into the field `name` (nothing if there is no such field).
    pub fn value(&self, name: &str, cx: &App) -> String {
        self.state(name)
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// Put `text` in the field `name`.
    pub fn set(&self, name: &str, text: &str, window: &mut Window, cx: &mut App) {
        if let Some(s) = self.state(name) {
            s.update(cx, |s, cx| s.set_value(text.to_string(), window, cx));
        }
    }

    /// Give the keyboard to the field `name`.
    pub fn focus(&self, name: &str, window: &mut Window, cx: &mut App) {
        if let Some(s) = self.state(name) {
            s.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// Whether the field `name` has the keyboard in `window`.
    pub fn is_focused(&self, name: &str, window: &Window, cx: &App) -> bool {
        self.state(name)
            .is_some_and(|s| s.read(cx).focus_handle(cx).is_focused(window))
    }
}

/// Do something to the dashboard with the window at hand, once what is going on is done (a text
/// input is made, and given the keyboard, in a window).
pub fn later(
    cx: &mut Context<Dash>,
    f: impl FnOnce(&mut Dash, &mut Window, &mut Context<Dash>) + 'static,
) {
    let me = cx.entity();
    // The dashboard has one window at a time; its type says which (its root can't be read from
    // here, while the window is being drawn).
    let Some(window) = cx
        .windows()
        .into_iter()
        .find(|w| w.downcast::<Dash>().is_some())
    else {
        return;
    };
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            me.update(cx, |d, cx| f(d, window, cx));
        });
    });
}

/// The keyboard goes back to the dashboard (and the widget's cursor).
pub fn give_back(d: &Dash, window: &mut Window, cx: &mut App) {
    window.focus(&d.focus, cx);
}

/// A text field as the widget shows it: the input in a frame, and under it a line that shows the
/// keyboard cursor being on the field (the stop). `label` says what it is for; with `secret`, what
/// is typed isn't read out. Without its `pool` (a moment after the form appeared) an empty frame
/// stands in.
pub struct Field<'a> {
    pub pool: Option<&'a Pool>,
    pub name: &'static str,
    pub label: &'a str,
    pub placeholder: &'a str,
    pub secret: bool,
    /// The keyboard cursor is on this field.
    pub kfocus: bool,
    /// This field has the keyboard.
    pub typing: bool,
}

impl Field<'_> {
    /// `on_key` hears the keys the field passes on (see the module's doc).
    pub fn view(
        self,
        tk: &Tk,
        cx: &mut Context<Dash>,
        on_key: impl Fn(&mut Dash, &KeyDownEvent, &mut Window, &mut Context<Dash>) + 'static,
    ) -> AnyElement {
        let label = self.label.to_string();
        let selector = label.clone();
        let frame = div()
            .id(SharedString::from(format!("field-{}", self.name)))
            .flex_none()
            .w_full()
            .h(px(32.0))
            // The input has padding of its own (8), and the border is inside the frame (1): the
            // text sits `pad_tight` from the edge, as in Slint's `SilentTextField`.
            .px(px((tk.pad_tight() - 9.0).max(0.0)))
            .rounded(px(tk.radius_small()))
            .bg(tk.subtle())
            .border_1()
            .border_color(if self.typing {
                tk.accent()
            } else {
                tk.border()
            })
            .flex()
            .items_center()
            .overflow_hidden()
            .text_size(px(tk.font_body()))
            .text_color(tk.text())
            .debug_selector(move || selector)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(
                cx.listener(move |d, ev: &KeyDownEvent, window, cx| on_key(d, ev, window, cx)),
            );
        let frame = match self.pool.and_then(|p| p.state(self.name)) {
            Some(state) => frame.child(
                Input::new(state)
                    .id(SharedString::from(format!("input-{}", self.name)))
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    // Text as big as the look says (the input scales its size by 0.875).
                    .with_size(Size::Size(px(tk.font_body() / 0.875)))
                    .aria_label(label.clone())
                    .when(self.secret, |i| {
                        i.content_type(gpui_kit::component::input::InputContentType::Password)
                    })
                    .w_full(),
            ),
            None => frame.child(
                div()
                    .text_color(tk.faint())
                    .child(self.placeholder.to_string()),
            ),
        };
        div()
            .flex_none()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(frame)
            .child(
                div()
                    .h(px(2.0))
                    .w_full()
                    .rounded(px(1.0))
                    .bg(if self.kfocus {
                        tk.accent()
                    } else {
                        gpui::transparent_black()
                    }),
            )
            .into_any_element()
    }
}

/// What the key that reached a field's wrapper is, for the widget to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    /// Tab (or Shift+Tab): on to the next field (or the one before).
    Tab { back: bool },
    /// Escape: the keyboard goes back to the widget's cursor.
    Escape,
    /// What is typed: put it in the field, and keep it from the dashboard's own keys.
    Text,
    /// Anything else: the field's, or nobody's.
    Other,
}

impl Pressed {
    pub fn of(ev: &KeyDownEvent) -> Self {
        let k = &ev.keystroke;
        match k.key.as_str() {
            "escape" => Self::Escape,
            "tab" => Self::Tab {
                back: k.modifiers.shift,
            },
            _ if k.key_char.is_some()
                && !k.modifiers.control
                && !k.modifiers.alt
                && !k.modifiers.platform =>
            {
                Self::Text
            }
            _ => Self::Other,
        }
    }
}

/// The text a typed key makes, in `ev`.
pub fn typed(ev: &KeyDownEvent) -> Option<String> {
    ev.keystroke.key_char.clone()
}

/// Insert `text` where the cursor of field `name` is (replacing what is selected).
pub fn insert(pool: &Pool, name: &str, text: &str, window: &mut Window, cx: &mut App) {
    if let Some(s) = pool.state(name) {
        s.update(cx, |s, cx| s.replace(text.to_string(), window, cx));
    }
}
