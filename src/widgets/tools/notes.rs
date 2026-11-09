//! Sticky notes: each a Markdown file in the data directory, shared by the widgets with the same
//! note name (`NotesWidget` and `NoteEdit` in `legacy/ui/widgets/tools.slint`,
//! `legacy/src/app/notes.rs`).
//!
//! The text lives here, by note name; each widget on show has a [`NoteView`], a small view of its
//! own that holds the text area (GPUI makes a text area in a window, which a widget's draw
//! function has no access to) and tells the text back as it is typed. The views belong to the
//! window: when the dashboard comes up in a new one, they make their text area again.

use crate::dash::{Dash, NavMode};
use crate::kit::Tk;
use crate::layout::Kind;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, EntityInputHandler as _, Focusable as _, KeyDownEvent, Render,
    Subscription, WeakEntity, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, MoveToEnd, Textarea, TextareaState};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct State {
    /// Note texts by note name, and the names with unsaved changes.
    texts: HashMap<String, String>,
    dirty: HashSet<String>,
    /// The notes widgets on show and their note names, by widget id.
    widgets: Vec<(u32, String)>,
    /// Typed since the last tick: saves once typing pauses.
    edited: bool,
    /// The text areas, by widget id (made when first drawn).
    views: RefCell<HashMap<u32, Entity<NoteView>>>,
    /// The widget just entered with the keyboard: its text area takes the keys when drawn.
    focus: Cell<Option<u32>>,
}

impl State {
    fn name_of(&self, id: u32) -> Option<&str> {
        self.widgets
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| n.as_str())
    }

    /// The text of notes widget `id`.
    pub fn text_of(&self, id: u32) -> String {
        self.name_of(id)
            .and_then(|n| self.texts.get(n))
            .cloned()
            .unwrap_or_default()
    }

    /// What the widget is called: its note's name.
    fn title_of(&self, id: u32) -> String {
        self.name_of(id)
            .map_or_else(|| "Notes".to_string(), title_case)
    }
}

/// A note name as a file name: letters, digits, '-' and '_'; empty is "notes".
fn note_name(raw: &str) -> String {
    let name: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        "notes".into()
    } else {
        name
    }
}

fn note_path(name: &str) -> std::path::PathBuf {
    crate::files::data_home()
        .join("niri-dash/notes")
        .join(format!("{name}.md"))
}

fn title_case(name: &str) -> String {
    let mut c = name.chars();
    c.next()
        .map(|f| {
            f.to_uppercase()
                .chain(c)
                .collect::<String>()
                .replace('-', " ")
        })
        .unwrap_or_default()
}

/// The notes widgets and their note names.
fn note_widgets(d: &Dash) -> Vec<(u32, String)> {
    use crate::widget_settings::{Opt, get_str};
    super::placed_of(d, Kind::Notes)
        .into_iter()
        .map(|p| {
            (
                p.id,
                note_name(&get_str(&p.settings, Opt::Note, d.locale.us)),
            )
        })
        .collect()
}

/// Find the notes widgets and read their notes; `reload` reads unchanged notes from disk again
/// (they may have been edited elsewhere).
pub fn sync(d: &mut Dash, reload: bool) {
    let widgets = note_widgets(d);
    let n = &mut d.widgets.tools.notes;
    for (_, name) in &widgets {
        if n.dirty.contains(name) {
            continue;
        }
        if reload || !n.texts.contains_key(name) {
            let text = crate::files::read(&note_path(name)).unwrap_or_default();
            n.texts.insert(name.clone(), text);
        }
    }
    n.views
        .borrow_mut()
        .retain(|id, _| widgets.iter().any(|(i, _)| i == id));
    n.widgets = widgets;
}

/// Note `id` was typed in.
fn edited(d: &mut Dash, id: u32, text: String) {
    let n = &mut d.widgets.tools.notes;
    let Some(name) = n.name_of(id).map(str::to_string) else {
        return;
    };
    // Other widgets showing the same note follow when they are drawn; the one being typed in
    // keeps its text.
    n.texts.insert(name.clone(), text);
    n.dirty.insert(name);
    n.edited = true;
}

fn save(d: &mut Dash) {
    let n = &mut d.widgets.tools.notes;
    let dirty: Vec<String> = n.dirty.drain().collect();
    let mut failed = Vec::new();
    for name in dirty {
        let path = note_path(&name);
        let text = n.texts.get(&name).map_or("", String::as_str);
        if let Err(e) = crate::files::write_atomic(&path, text) {
            log::warn!("saving {}: {e}", path.display());
            failed.push(format!("Saving note {name}: {e}"));
        }
    }
    n.edited = false;
    for f in failed {
        d.toast(&f);
    }
}

/// The dashboard came up or went away: read the notes again on show (they may have been edited
/// elsewhere), save right away on hide.
pub fn awake(d: &mut Dash, on: bool) {
    if on {
        sync(d, true);
    } else if !d.widgets.tools.notes.dirty.is_empty() {
        save(d);
    }
}

/// Saves once typing pauses: a tick without typing since the one before.
pub fn tick(d: &mut Dash) {
    let n = &mut d.widgets.tools.notes;
    if n.dirty.is_empty() {
        n.edited = false;
    } else if std::mem::take(&mut n.edited) {
        // Still typing.
    } else {
        save(d);
    }
}

/// Entered with the keyboard: straight into the text. Escape leaves it again.
pub fn entered(d: &mut Dash, id: u32) {
    d.widgets.tools.notes.focus.set(Some(id));
}

pub fn check(d: &Dash) -> Result<(), String> {
    let n = &d.widgets.tools.notes;
    for name in &n.dirty {
        if !n.texts.contains_key(name) {
            return Err(format!("note {name} has unsaved changes but no text"));
        }
    }
    for (id, name) in &n.widgets {
        if !n.texts.contains_key(name) {
            return Err(format!("notes widget {id}: note {name} was never read"));
        }
    }
    Ok(())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let d = ctx.d;
    let tk = ctx.tk;
    let id = ctx.tile.id();
    let n = &d.widgets.tools.notes;
    let view = n
        .views
        .borrow_mut()
        .entry(id)
        .or_insert_with(|| {
            let dash = cx.entity().downgrade();
            cx.new(|_| NoteView {
                id,
                dash,
                state: None,
                window: None,
                _sub: None,
                entered: false,
            })
        })
        .clone();
    let tall = ctx.h > 150.0;
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .px(px(tk.pad()))
        .pb(px(tk.pad()))
        .pt(px(if tall { tk.pad() } else { tk.pad_tight() }))
        .when(tall, |col| {
            col.child(super::ui::header("note", n.title_of(id), tk))
        })
        .child(div().flex_1().min_h_0().w_full().child(view))
        .into_any_element()
}

/// A note's text area in the theme's colors, one per notes widget on show.
struct NoteView {
    id: u32,
    dash: WeakEntity<Dash>,
    /// Made when first drawn: it needs the window, and is made again for another one (the
    /// dashboard's window goes when it is hidden).
    state: Option<Entity<TextareaState>>,
    window: Option<gpui::WindowId>,
    _sub: Option<Subscription>,
    /// Whether the widget was entered with the keyboard when last drawn.
    entered: bool,
}

impl NoteView {
    fn state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        let here = window.window_handle().window_id();
        if self.window != Some(here) {
            self.window = Some(here);
            self.state = None;
            self._sub = None;
            self.entered = false;
        }
        if let Some(state) = &self.state {
            return state.clone();
        }
        let state = cx.new(|cx| TextareaState::new(window, cx).placeholder("Write something…"));
        self._sub = Some(cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let text = state.read(cx).value().to_string();
            let id = this.id;
            if let Some(dash) = this.dash.upgrade() {
                dash.update(cx, |d, cx| {
                    edited(d, id, text);
                    cx.notify();
                });
            }
        }));
        self.state = Some(state.clone());
        state
    }
}

impl Render for NoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(dash) = self.dash.upgrade() else {
            return div().into_any_element();
        };
        let (text, label, entered, requested, tk, dash_focus) = {
            let d = dash.read(cx);
            let n = &d.widgets.tools.notes;
            let requested = n.focus.get() == Some(self.id);
            if requested {
                n.focus.set(None);
            }
            (
                n.text_of(self.id),
                n.title_of(self.id),
                d.nav.mode == NavMode::Widget && d.nav.active_id == self.id,
                requested,
                Tk::new(&d.look),
                d.focus.clone(),
            )
        };
        let state = self.state(window, cx);
        // What the note says elsewhere (another widget of the same note, the file read again)
        // reaches the text area; what is typed here has come the other way already.
        let handle = state.read(cx).focus_handle(cx);
        if state.read(cx).value() != text.as_str() {
            // A note at rest shows its beginning, as in Slint.
            state.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        if entered && (!self.entered || requested) && !handle.is_focused(window) {
            state.update(cx, |s, cx| s.focus(window, cx));
            // Entered with the keyboard, the cursor goes to the end, where typing goes on.
            let at = handle.clone();
            window.defer(cx, move |window, cx| {
                at.dispatch_action(&MoveToEnd, window, cx)
            });
        } else if !entered && self.entered && handle.is_focused(window) {
            // Left with Escape: the dashboard has the keys again.
            window.focus(&dash_focus, cx);
        }
        self.entered = entered;
        let focused = handle.is_focused(window);
        div()
            .id(("note", self.id))
            .size_full()
            .rounded(px(tk.radius_small()))
            .bg(tk.subtle())
            .border(px(1.0))
            .border_color(if focused { tk.accent() } else { tk.border() })
            // The text area has 10 px (sideways) and about 1 px (up and down) inside already,
            // and the border is inside the box: the text sits `pad_tight` from the edge, as it
            // does in Slint's `NoteEdit`.
            .px(px((tk.pad_tight() - 10.0).max(0.0)))
            .py(px((tk.pad_tight() - 2.0).max(0.0)))
            .debug_selector({
                let label = label.clone();
                move || label.clone()
            })
            // Typing is the text area's: letters never reach the dashboard's own keys (f for the
            // hints, s, e, H/J/K/L ...). What is bound to an action (arrows, Enter, Backspace,
            // Ctrl+C) is done before this sees it.
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let k = &ev.keystroke;
                let m = &k.modifiers;
                if m.control || m.alt || m.platform || m.function {
                    return;
                }
                let Some(typed) = k
                    .key_char
                    .as_deref()
                    .filter(|t| !t.chars().any(char::is_control))
                else {
                    return;
                };
                let Some(state) = this.state.clone() else {
                    return;
                };
                if state.read(cx).focus_handle(cx).is_focused(window) {
                    state.update(cx, |s, cx| s.replace_text_in_range(None, typed, window, cx));
                    cx.stop_propagation();
                }
            }))
            .child(
                Textarea::new(&state)
                    .appearance(false)
                    .bordered(false)
                    .aria_label(label)
                    .size_full(),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name(""), "notes");
        assert_eq!(note_name(" Work Stuff! "), "work-stuff");
        assert_eq!(note_name("../etc/passwd"), "etc-passwd");
        assert_eq!(title_case("work-stuff"), "Work stuff");
    }
}
