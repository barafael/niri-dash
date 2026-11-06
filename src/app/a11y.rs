//! What a screen reader would say about where the keyboard is, read from the accessible properties
//! in the `.slint` files. The stress run and the tests check it; nothing else needs it yet.
//!
//! The rule (see `NavScope` in `ui/common.slint`): the keyboard is on the element that has Slint's
//! focus or, when that is a `NavScope`, on the one visible element in its widget's tile that says
//! `accessible-item-selected` (a scope's controls are often beside it rather than inside). A widget
//! without controls has no focus of its own while entered, so its tile answers for it.

// The screen reader's side is the desktop's: AT-SPI, over D-Bus. The web build has no use for it.
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod bridge;
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod tree;

use super::App;
use crate::{Nav, NavMode};
use i_slint_core::accessibility::{AccessibilityAction, AccessibleStringProperty as Prop};
use i_slint_core::item_tree::{ItemRc, ParentItemTraversalMode};
use i_slint_core::items::{AccessibleRole, Flickable};
use i_slint_core::window::WindowInner;
use slint::ComponentHandle;
use std::ops::ControlFlow;

/// An element, as a screen reader would read it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spoken {
    pub role: String,
    pub label: String,
    pub value: String,
    pub checked: Option<bool>,
}

impl std::fmt::Display for Spoken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.role, self.label)?;
        if !self.value.is_empty() {
            write!(f, " {}", self.value)?;
        }
        match self.checked {
            Some(true) => write!(f, " (on)"),
            Some(false) => write!(f, " (off)"),
            None => Ok(()),
        }
    }
}

fn prop(item: &ItemRc, what: Prop) -> String {
    item.accessible_string_property(what)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn speak(item: &ItemRc) -> Spoken {
    Spoken {
        role: format!("{:?}", item.accessible_role()).to_lowercase(),
        label: prop(item, Prop::Label),
        value: prop(item, Prop::Value),
        checked: (prop(item, Prop::Checkable) == "true")
            .then(|| prop(item, Prop::Checked) == "true"),
    }
}

/// `item`, or the closest element around it that a screen reader can see.
fn nearest_accessible(mut item: ItemRc) -> ItemRc {
    while !item.is_accessible() {
        match item.parent_item(ParentItemTraversalMode::StopAtPopups) {
            Some(parent) => item = parent,
            None => break,
        }
    }
    item
}

/// The element with the accessible id `id` around `item` (or `item` itself).
fn around(item: &ItemRc, id: &str) -> Option<ItemRc> {
    let mut at = Some(item.clone());
    while let Some(i) = at {
        if i.is_accessible() && prop(&i, Prop::Id) == id {
            return Some(i);
        }
        at = i.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    None
}

/// Whether `item` is clearly in sight: no scrolling list around it hides more than a quarter of it
/// (a few pixels cut off at the edge are no matter). Slint's `is_visible` also says yes for what
/// only touches the edge.
fn in_view(item: &ItemRc) -> bool {
    if !item.is_visible() {
        return false;
    }
    // `map_to_window` takes a point in the parent's coordinates, as `geometry` gives it.
    let geometry = item.geometry();
    let top = item.map_to_window(geometry.origin).y;
    let height = geometry.size.height;
    let mut at = item.parent_item(ParentItemTraversalMode::StopAtPopups);
    while let Some(parent) = at {
        if parent.downcast::<Flickable>().is_some() {
            let view = parent.geometry();
            let view_top = parent.map_to_window(view.origin).y;
            let view_height = view.size.height;
            let shown = (top + height).min(view_top + view_height) - top.max(view_top);
            if shown < 0.75 * height.min(view_height) {
                return false;
            }
        }
        at = parent.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    true
}

/// The visible elements in `tile` under the cursor. Two would be two highlights on screen.
fn under_cursor(tile: &ItemRc) -> Vec<ItemRc> {
    let mut found = Vec::new();
    // `visit_descendants` also makes what is still to be made: `first_child` would miss a list
    // or a page that hasn't been laid out yet.
    tile.visit_descendants(|item| {
        if item.is_accessible() && prop(item, Prop::ItemSelected) == "true" && in_view(item) {
            found.push(item.clone());
        }
        ControlFlow::<()>::Continue(())
    });
    found
}

/// What in `tile` is under the cursor but out of sight: clipped away by the widget's edge or a
/// scrolled list that didn't follow the cursor.
fn out_of_sight(tile: &ItemRc) -> Vec<ItemRc> {
    let mut found = Vec::new();
    tile.visit_descendants(|item| {
        if item.is_accessible() && prop(item, Prop::ItemSelected) == "true" && !item.is_visible() {
            found.push(item.clone());
        }
        ControlFlow::<()>::Continue(())
    });
    found
}

/// The element with the accessible id `id`, anywhere in the window.
fn find_id(window: &slint::Window, id: &str) -> Option<ItemRc> {
    let root = ItemRc::new_root(WindowInner::from_pub(window).try_component()?);
    root.visit_descendants(|item| {
        if item.is_accessible() && prop(item, Prop::Id) == id {
            ControlFlow::Break(item.clone())
        } else {
            ControlFlow::Continue(())
        }
    })
}

/// An element as a screen reader finds it, for the tests: what it says, and its actions.
pub struct Element(ItemRc);

impl Element {
    pub fn spoken(&self) -> Spoken {
        speak(&self.0)
    }

    /// Its default action: what a click does.
    pub fn activate(&self) {
        self.0.accessible_action(&AccessibilityAction::Default);
    }

    /// One step up, on a slider.
    pub fn increment(&self) {
        self.0.accessible_action(&AccessibilityAction::Increment);
    }
}

impl App {
    /// The element labelled `label` with this role ("button", "listitem", as [`Spoken`] has it).
    pub(super) fn element(&self, label: &str, role: &str) -> Option<Element> {
        let ui = self.ui();
        let root = ItemRc::new_root(WindowInner::from_pub(ui.window()).try_component()?);
        root.visit_descendants(|item| {
            if item.is_accessible() {
                let spoken = speak(item);
                if spoken.label == label && spoken.role == role {
                    return ControlFlow::Break(Element(item.clone()));
                }
            }
            ControlFlow::Continue(())
        })
    }

    /// Everything in the window a screen reader could read out.
    pub(super) fn readable(&self) -> Vec<String> {
        let ui = self.ui();
        let mut all = Vec::new();
        let Some(component) = WindowInner::from_pub(ui.window()).try_component() else {
            return all;
        };
        ItemRc::new_root(component).visit_descendants(|item| {
            if item.is_accessible() {
                for what in [
                    Prop::Label,
                    Prop::Value,
                    Prop::Description,
                    Prop::PlaceholderText,
                ] {
                    let text = prop(item, what);
                    if !text.is_empty() {
                        all.push(text);
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        });
        all
    }

    /// What has the keyboard, for the log of a stress run.
    pub(super) fn focus_description(&self) -> String {
        let ui = self.ui();
        let focus = WindowInner::from_pub(ui.window())
            .focus_item
            .borrow()
            .upgrade();
        match focus {
            None => "nothing".into(),
            Some(item) => {
                let at = nearest_accessible(item.clone());
                format!("{} {:?}", speak(&at), item.geometry())
            }
        }
    }

    /// Have everything made and laid out, as drawing a frame would. Slint does this lazily: a list
    /// that grew has no new rows until something looks, and nothing moves until then.
    pub(super) fn lay_out(&self) {
        let ui = self.ui();
        if let Some(component) = WindowInner::from_pub(ui.window()).try_component() {
            ItemRc::new_root(component).visit_descendants(|_| ControlFlow::<()>::Continue(()));
        }
    }

    /// Give the keyboard back if what had it is gone. A page or prompt that goes away takes its
    /// text field with it, and with no focus every key is lost, Escape too.
    pub(super) fn keep_keyboard(&self) {
        let ui = self.ui();
        if !self.visible.get()
            || WindowInner::from_pub(ui.window())
                .focus_item
                .borrow()
                .upgrade()
                .is_some()
        {
            return;
        }
        ui.invoke_take_keyboard();
        // An entered widget takes it from there.
        let n = ui.global::<Nav>();
        if n.get_mode() == NavMode::Widget {
            n.set_tick(n.get_tick() + 1);
        }
    }

    /// The dashboard as a screen reader's tree, its focus on the cursor (or, with no widget
    /// entered, on what holds Slint's focus).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn access_tree(&self) -> accesskit::TreeUpdate {
        let ui = self.ui();
        let focus = match self.cursor_now() {
            Ok(Some(cursor)) => Some(cursor),
            _ => WindowInner::from_pub(ui.window())
                .focus_item
                .borrow()
                .upgrade(),
        };
        let mut update = tree::build(ui.window(), &mut self.access_ids.borrow_mut(), focus);
        // A hidden dashboard is there for a screen reader to find, but shows nothing.
        if !self.visible.get() {
            let root = update.tree.as_ref().map(|t| t.root);
            for (id, node) in &mut update.nodes {
                if Some(*id) == root {
                    node.set_hidden();
                }
            }
        }
        update
    }

    /// What a screen reader would say about the entered widget; `None` with no widget entered.
    /// An error says what doesn't add up: the keyboard in another widget, a cursor on nothing or
    /// on two things at once, or a stop that has no name.
    pub(super) fn spoken(&self) -> Result<Option<Spoken>, String> {
        let mut now = self.spoken_now();
        // A list follows the cursor when it is laid out, and a tile settles when it has finished
        // moving: look again a few frames later before calling the cursor lost.
        for _ in 0..4 {
            if !matches!(&now, Err(e) if e.contains("out of sight")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
            slint::platform::update_timers_and_animations();
            self.lay_out();
            now = self.spoken_now();
        }
        now
    }

    fn spoken_now(&self) -> Result<Option<Spoken>, String> {
        match self.cursor_now()? {
            Some(item) => named(speak(&item)).map(Some),
            None => Ok(None),
        }
    }

    /// The element the keyboard is on in the entered widget, which is also where a screen
    /// reader's focus belongs; `None` with no widget entered. An error says what doesn't add up.
    /// It doesn't wait for a moving list to settle (the tree is built on the loop's own turn,
    /// which must not sleep): where it errs, the tree's focus falls back to Slint's.
    pub(super) fn cursor_now(&self) -> Result<Option<ItemRc>, String> {
        let ui = self.ui();
        let n = ui.global::<Nav>();
        // Edit mode's sidebar has a cursor of its own; nothing else has one then.
        if ui.get_editing() && ui.get_side_keys() {
            let root = ItemRc::new_root(
                WindowInner::from_pub(ui.window())
                    .try_component()
                    .ok_or("no window")?,
            );
            return match under_cursor(&root).as_slice() {
                [one] => Ok(Some(one.clone())),
                [] => Err("the sidebar's cursor is on nothing".into()),
                many => Err(format!(
                    "the sidebar's cursor is on {} things at once: {}",
                    many.len(),
                    many.iter()
                        .map(|i| speak(i).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            };
        }
        if n.get_mode() != NavMode::Widget {
            return Ok(None);
        }
        let window = ui.window();
        // An open tray menu has the keys, and its own cursor.
        if ui.global::<crate::Tray>().get_menu_open()
            && let Some(menu) = find_id(window, "tray-menu")
        {
            return match under_cursor(&menu).as_slice() {
                [one] => Ok(Some(one.clone())),
                _ => Ok(Some(menu)),
            };
        }
        let tile = format!("widget-{}", n.get_active_id());
        let focus = WindowInner::from_pub(window).focus_item.borrow().upgrade();
        let Some((item, frame)) = focus.and_then(|f| around(&f, &tile).map(|frame| (f, frame)))
        else {
            // Keyboard not in the tile: it stays with the dashboard for a widget without
            // controls, whose tile answers.
            return find_id(window, &tile)
                .map(Some)
                .ok_or(format!("{tile} isn't on the dashboard"));
        };
        let at = nearest_accessible(item);
        if at.accessible_role() != AccessibleRole::List {
            return Ok(Some(at));
        }
        let stops: usize = prop(&at, Prop::ItemCount).parse().unwrap_or(0);
        let under = under_cursor(&frame);
        match under.as_slice() {
            [one] => Ok(Some(one.clone())),
            [] if stops == 0 => Ok(Some(at)),
            [] => Err(match out_of_sight(&frame).as_slice() {
                [] => format!("the cursor is on nothing in {} ({stops} stops)", speak(&at)),
                hidden => format!(
                    "the cursor is out of sight in {} ({stops} stops), on {}",
                    speak(&at),
                    hidden
                        .iter()
                        .map(|i| speak(i).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
            many => Err(format!(
                "the cursor is on {} things at once: {}",
                many.len(),
                many.iter()
                    .map(|i| speak(i).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// `spoken`, if it says anything.
fn named(spoken: Spoken) -> Result<Spoken, String> {
    if spoken.label.trim().is_empty() {
        Err(format!(
            "under the keyboard: a {} with no name",
            spoken.role
        ))
    } else {
        Ok(spoken)
    }
}
