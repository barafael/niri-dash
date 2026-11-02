//! Sticky notes: each a Markdown file in the data directory, shared by widgets with the same note
//! name.

use super::{App, set_if_changed};
use crate::Dash;
use crate::layout::Kind;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
pub(super) struct Notes {
    /// Note texts by note name, and the names with unsaved changes.
    texts_by_name: RefCell<HashMap<String, String>>,
    dirty: RefCell<HashSet<String>>,
    /// `Notes.texts` and `Notes.names`, by widget id.
    texts: Rc<VecModel<SharedString>>,
    names: Rc<VecModel<SharedString>>,
    /// Saves once typing pauses.
    save: Timer,
}

impl App {
    /// The dashboard came up or went away: read the notes again on show (they may have been
    /// edited elsewhere), save right away on hide.
    pub(super) fn notes_awake(&self, visible: bool) {
        if visible {
            self.sync_notes(true);
        } else if !self.notes.dirty.borrow().is_empty() {
            self.notes.save.stop();
            self.save_notes();
        }
    }

    /// The notes widgets and their note names, by widget id.
    fn note_widgets(&self) -> Vec<(u32, String)> {
        use crate::widget_settings::{Opt, get_str};
        let us = self.locale.us;
        self.placed_of(Kind::Notes)
            .into_iter()
            .map(|p| (p.id, note_name(&get_str(&p.settings, Opt::Note, us))))
            .collect()
    }

    /// Fill `Notes.texts` for the notes widgets; `reload` reads unchanged notes from disk again
    /// (they may have been edited elsewhere).
    pub(super) fn sync_notes(&self, reload: bool) {
        let widgets = self.note_widgets();
        if widgets.is_empty() && self.notes.texts.row_count() == 0 {
            return;
        }
        {
            let mut notes = self.notes.texts_by_name.borrow_mut();
            let dirty = self.notes.dirty.borrow();
            for (_, name) in &widgets {
                if dirty.contains(name) {
                    continue;
                }
                if reload || !notes.contains_key(name) {
                    let text = std::fs::read_to_string(note_path(name)).unwrap_or_default();
                    notes.insert(name.clone(), text);
                }
            }
        }
        let len = widgets
            .iter()
            .map(|(id, _)| *id as usize + 1)
            .max()
            .unwrap_or(0);
        let notes = self.notes.texts_by_name.borrow();
        let mut texts = vec![slint::SharedString::new(); len];
        let mut names = vec![slint::SharedString::new(); len];
        for (id, name) in widgets {
            texts[id as usize] = notes.get(&name).cloned().unwrap_or_default().into();
            names[id as usize] = title_case(&name).into();
        }
        set_if_changed(&self.notes.texts, texts, |a, b| a == b);
        set_if_changed(&self.notes.names, names, |a, b| a == b);
    }

    fn note_edited(&self, id: u32, text: String) {
        let Some(name) = self
            .note_widgets()
            .into_iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| n)
        else {
            return;
        };
        self.notes
            .texts_by_name
            .borrow_mut()
            .insert(name.clone(), text.clone());
        self.notes.dirty.borrow_mut().insert(name.clone());
        // Other widgets showing the same note follow; the one being typed in keeps its text.
        for (other, n) in self.note_widgets() {
            if other != id && n == name {
                self.notes
                    .texts
                    .set_row_data(other as usize, text.as_str().into());
            }
        }
        // Single-shot, restarted by every keystroke: saves once typing pauses.
        self.notes.save.restart();
    }

    pub(super) fn save_notes(&self) {
        let dirty: Vec<String> = self.notes.dirty.borrow_mut().drain().collect();
        let notes = self.notes.texts_by_name.borrow();
        for name in dirty {
            let path = note_path(&name);
            let text = notes.get(&name).map(String::as_str).unwrap_or("");
            if let Err(e) = crate::files::write_atomic(&path, text) {
                log::warn!("saving {}: {e}", path.display());
                self.toast(&format!("Saving note {name}: {e}"));
            }
        }
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

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let nt = ui.global::<crate::Notes>();
    nt.set_texts(ModelRc::from(app.notes.texts.clone()));
    nt.set_names(ModelRc::from(app.notes.names.clone()));
    let a = app.clone();
    nt.on_edited(move |id, text| a.note_edited(id as u32, text.into()));
    let w = Rc::downgrade(app);
    app.notes
        .save
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = w.upgrade() {
                a.save_notes();
            }
        });
    app.notes.save.stop();
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
