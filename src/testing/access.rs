//! The dashboard as a screen reader gets it: the accessibility tree GPUI builds from the views'
//! `.role(..)`, `.aria_label(..)` and `.aria_*(..)`, read back in a test. GPUI's test platform has
//! no screen reader, so [`Harness::a11y`] connects one (a patch of the vendored GPUI,
//! `patches/gpui-pre`); from then on every frame builds the tree, and what a test sees is what
//! the Linux adapter would hand AT-SPI.
//!
//! What this adds to [`Harness`]:
//!
//! - [`Harness::a11y`], the tree as of now ([`Tree`]): find an element by role and label, read its
//!   value and states, ask what a screen reader says about it ([`Element::says`]), dump it.
//! - [`Harness::cursor`] / [`Harness::spoken`], the element the keyboard is on in the entered
//!   widget (or in edit mode's sidebar), as the stress run and the audit need it. It is the
//!   element the screen reader's focus is on (`kit::AriaCursor` marks it in the views).
//! - [`Harness::a11y_request`], what a screen reader asks of an element (click, increment,
//!   focus), delivered as the adapter delivers it.

use super::Harness;
use crate::dash::NavMode;
use gpui::AnyWindowHandle;
use gpui::accesskit::{self, ActionRequest, NodeId, Role, TreeId, TreeUpdate};
use std::collections::HashMap;

pub use gpui::accesskit::{Action, Toggled};

/// One node of the tree, as a screen reader reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// The node's id, stable while the element keeps its place in the view.
    pub id: u64,
    pub role: Role,
    pub label: String,
    pub value: String,
    pub description: String,
    pub numeric: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub toggled: Option<Toggled>,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub disabled: bool,
    pub hidden: bool,
    pub actions: Vec<Action>,
    /// (left, top, right, bottom) in device pixels (the window's scale factor times logical ones).
    pub bounds: Option<(f32, f32, f32, f32)>,
    pub children: Vec<u64>,
    pub parent: Option<u64>,
}

impl Element {
    /// What it is called: a text says itself (its value), anything else its label.
    pub fn name(&self) -> &str {
        if self.role == Role::Label {
            &self.value
        } else {
            &self.label
        }
    }

    /// Whether it is on (a switch, a toggle button, a check box).
    pub fn on(&self) -> Option<bool> {
        self.toggled.map(|t| t == Toggled::True)
    }

    /// The role as a word, as a screen reader says it ("button", "listitem", "switch").
    pub fn role_name(&self) -> String {
        format!("{:?}", self.role).to_lowercase()
    }

    /// What a screen reader says: the role, the name, the value, whether it is on.
    /// `button "Rescan"`, `slider "Brightness" 70%`, `switch "Wi-Fi" (on)`.
    pub fn says(&self) -> String {
        let mut out = format!("{} {:?}", self.role_name(), self.name());
        if self.role != Role::Label && !self.value.is_empty() {
            out.push(' ');
            out.push_str(&self.value);
        }
        match self.on() {
            Some(true) => out.push_str(" (on)"),
            Some(false) => out.push_str(" (off)"),
            None => {}
        }
        out
    }

    /// Whether a screen reader can do something to it (click, set a value, step it).
    pub fn acts(&self) -> bool {
        [
            Action::Click,
            Action::Increment,
            Action::Decrement,
            Action::SetValue,
        ]
        .iter()
        .any(|a| self.actions.contains(a))
    }

    fn from_node(id: NodeId, node: &accesskit::Node) -> Self {
        let r = node.bounds();
        Self {
            id: id.0,
            role: node.role(),
            label: node.label().unwrap_or_default().to_string(),
            value: node.value().unwrap_or_default().to_string(),
            description: node.description().unwrap_or_default().to_string(),
            numeric: node.numeric_value(),
            min: node.min_numeric_value(),
            max: node.max_numeric_value(),
            step: node.numeric_value_step(),
            toggled: node.toggled(),
            selected: node.is_selected(),
            expanded: node.is_expanded(),
            disabled: node.is_disabled(),
            hidden: node.is_hidden(),
            actions: [
                Action::Click,
                Action::Focus,
                Action::Increment,
                Action::Decrement,
                Action::SetValue,
                Action::Expand,
                Action::Collapse,
                Action::ScrollIntoView,
            ]
            .into_iter()
            .filter(|a| node.supports_action(*a))
            .collect(),
            bounds: r.map(|r| (r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32)),
            children: node.children().iter().map(|c| c.0).collect(),
            parent: None,
        }
    }
}

/// The tree a screen reader is given, at one frame.
#[derive(Debug, Clone)]
pub struct Tree {
    /// Every node, parents before their children, in the order of the tree.
    pub elements: Vec<Element>,
    /// The window.
    pub root: u64,
    /// The node the screen reader's focus is on.
    pub focus: u64,
    /// The ids as the update listed them, to see a node listed twice.
    listed: Vec<u64>,
    index: HashMap<u64, usize>,
}

impl Tree {
    fn build(update: &TreeUpdate) -> Self {
        let root = update.tree.as_ref().map_or(0, |t| t.root.0);
        let mut by_id: HashMap<u64, Element> = update
            .nodes
            .iter()
            .map(|(id, n)| (id.0, Element::from_node(*id, n)))
            .collect();
        let listed: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        let links: Vec<(u64, Vec<u64>)> = by_id
            .iter()
            .map(|(id, e)| (*id, e.children.clone()))
            .collect();
        for (parent, kids) in links {
            for kid in kids {
                if let Some(k) = by_id.get_mut(&kid) {
                    k.parent = Some(parent);
                }
            }
        }
        // In tree order, from the root; what no one owns comes last (a problem, reported by
        // `problems`).
        let mut elements = Vec::with_capacity(by_id.len());
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some(e) = by_id.remove(&id) {
                stack.extend(e.children.iter().rev().copied());
                elements.push(e);
            }
        }
        let mut orphans: Vec<Element> = by_id.into_values().collect();
        orphans.sort_by_key(|e| e.id);
        elements.extend(orphans);
        let index = elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        Self {
            elements,
            root,
            focus: update.focus.0,
            listed,
            index,
        }
    }

    pub fn get(&self, id: u64) -> Option<&Element> {
        self.index.get(&id).map(|i| &self.elements[*i])
    }

    /// The window.
    pub fn window(&self) -> Option<&Element> {
        self.get(self.root)
    }

    /// The element the screen reader's focus is on.
    pub fn focused(&self) -> Option<&Element> {
        self.get(self.focus)
    }

    pub fn children(&self, e: &Element) -> Vec<&Element> {
        e.children.iter().filter_map(|c| self.get(*c)).collect()
    }

    pub fn parent(&self, e: &Element) -> Option<&Element> {
        e.parent.and_then(|p| self.get(p))
    }

    /// Everything under `e`, itself first.
    pub fn below<'a>(&'a self, e: &'a Element) -> Vec<&'a Element> {
        let mut out = Vec::new();
        let mut stack = vec![e];
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.children(n).into_iter().rev());
        }
        out
    }

    /// The first element with this role and name ("switch", "Wi-Fi").
    pub fn find(&self, role: Role, name: &str) -> Option<&Element> {
        self.elements
            .iter()
            .find(|e| e.role == role && e.name() == name)
    }

    pub fn find_all(&self, role: Role, name: &str) -> Vec<&Element> {
        self.elements
            .iter()
            .filter(|e| e.role == role && e.name() == name)
            .collect()
    }

    /// The first element with this name, whatever it is.
    pub fn named(&self, name: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.name() == name)
    }

    /// Whether anything is called this.
    pub fn has(&self, name: &str) -> bool {
        self.named(name).is_some()
    }

    /// Everything a screen reader could read out: labels, values, descriptions.
    pub fn readable(&self) -> Vec<String> {
        let mut all = Vec::new();
        for e in &self.elements {
            for text in [&e.label, &e.value, &e.description] {
                if !text.is_empty() {
                    all.push(text.clone());
                }
            }
        }
        all
    }

    /// The tree as indented text, for a failure message or a look.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let mut seen = std::collections::HashSet::new();
        self.dump_from(self.root, 0, &mut out, &mut seen);
        for e in self.elements.iter().filter(|e| !seen.contains(&e.id)) {
            out.push_str(&format!("(unreachable) {}\n", e.describe()));
        }
        out
    }

    fn dump_from(
        &self,
        id: u64,
        depth: usize,
        out: &mut String,
        seen: &mut std::collections::HashSet<u64>,
    ) {
        let Some(e) = self.get(id) else { return };
        if !seen.insert(id) {
            return;
        }
        let mark = if id == self.focus { "> " } else { "  " };
        out.push_str(&format!("{mark}{}{}\n", "  ".repeat(depth), e.describe()));
        for c in &e.children {
            self.dump_from(*c, depth + 1, out, seen);
        }
    }

    /// What is wrong with the tree itself: a node listed twice or that no one owns, a root that
    /// isn't a window, something to act on that has no name.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut ids = self.listed.clone();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() != self.listed.len() {
            out.push(format!(
                "{} nodes with a duplicate id",
                self.listed.len() - ids.len()
            ));
        }
        let reachable = self.below_root().len();
        if reachable != self.elements.len() {
            out.push(format!(
                "{} nodes in the update, {reachable} reachable from the root",
                self.elements.len()
            ));
        }
        match self.window() {
            Some(w) if w.role == Role::Window => {}
            Some(w) => out.push(format!("the root is a {:?}, not a window", w.role)),
            None => out.push("no root".to_string()),
        }
        if self.focused().is_none() {
            out.push("the focus is on a node that isn't in the tree".to_string());
        }
        // What can be acted on has a name, or a screen reader can only say "button".
        for e in &self.elements {
            if e.acts()
                && !e.hidden
                && e.role != Role::Window
                && e.name().trim().is_empty()
                && e.description.is_empty()
            {
                out.push(format!("a {:?} with no name (id {})", e.role, e.id));
            }
        }
        out
    }

    fn below_root(&self) -> Vec<&Element> {
        self.window().map(|w| self.below(w)).unwrap_or_default()
    }

    /// Whether the screen reader's focus is on the dashboard's own root (or the window): the
    /// keyboard is on no element in particular.
    pub fn focus_is_root(&self) -> bool {
        match self.focused() {
            None => true,
            Some(f) => {
                f.id == self.root
                    || (f.role == Role::Group && f.label.is_empty() && f.parent == Some(self.root))
            }
        }
    }

    /// The elements that say they are under the keyboard cursor: the one the screen reader's
    /// focus is on (`kit::AriaCursor`), and any that still say `selected`, which a view that
    /// hasn't been given the cursor's marker yet does (and which `Harness::cursor` then reports
    /// as a cursor the screen reader's focus doesn't follow).
    pub fn cursors(&self) -> Vec<&Element> {
        let mut out: Vec<&Element> = self
            .elements
            .iter()
            // A tab that is selected is the tab shown, not where the keyboard is.
            .filter(|e| e.selected == Some(true) && e.role_name() != "tab")
            .collect();
        if let Some(f) = self.focused().filter(|_| !self.focus_is_root())
            && !out.iter().any(|e| e.id == f.id)
        {
            out.push(f);
        }
        out
    }

    /// How much of `e` shows inside its ancestors, 0 to 1 (the least over them): a list that
    /// didn't scroll to the cursor clips it away.
    pub fn shown(&self, e: &Element) -> f32 {
        let Some((l, t, r, b)) = e.bounds else {
            return 1.0;
        };
        let (w, h) = (r - l, b - t);
        if w <= 0.0 || h <= 0.0 {
            return 0.0;
        }
        let mut least: f32 = 1.0;
        let mut at = self.parent(e);
        while let Some(p) = at {
            if let Some((pl, pt, pr, pb)) = p.bounds {
                let dx = (r.min(pr) - l.max(pl)).max(0.0);
                let dy = (b.min(pb) - t.max(pt)).max(0.0);
                // The parent may be smaller than the child (a scroll view's content is taller
                // than it): count against the smaller of the two.
                let seen = (dx * dy) / (w * h).min((pr - pl).max(1.0) * (pb - pt).max(1.0));
                least = least.min(seen);
            }
            at = self.parent(p);
        }
        least
    }

    /// Whether `e` is clearly in sight: at least three quarters of it shows.
    pub fn in_view(&self, e: &Element) -> bool {
        !e.hidden && self.shown(e) >= 0.75
    }
}

impl Element {
    /// One line: role, name, value, states and id.
    pub fn describe(&self) -> String {
        let mut out = self.says();
        if !self.description.is_empty() {
            out.push_str(&format!(" [{}]", self.description));
        }
        if let Some(n) = self.numeric {
            out.push_str(&format!(" #{n}"));
        }
        if self.selected == Some(true) {
            out.push_str(" *cursor*");
        }
        if self.disabled {
            out.push_str(" (disabled)");
        }
        if self.hidden {
            out.push_str(" (hidden)");
        }
        out.push_str(&format!(" id={}", self.id));
        out
    }
}

/// What a screen reader's action request is about.
pub fn request(action: Action, target: u64) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: NodeId(target),
        data: None,
    }
}

impl Harness {
    fn handle(&self) -> AnyWindowHandle {
        AnyWindowHandle::from(self.window)
    }

    /// Connect a screen reader, if none is yet: from the next frame on the window builds its
    /// accessibility tree. Idempotent.
    pub fn a11y_connect(&mut self) {
        if self.cx.a11y_tree(self.handle()).is_none() {
            self.cx.a11y_activate(self.handle());
            self.settle();
            // The activation asks for a frame on the executor; one more makes sure it happened.
            self.draw();
        }
        assert!(
            self.cx.a11y_tree(self.handle()).is_some(),
            "the window built no accessibility tree: is GPUI's test window patched \
             (patches/gpui-pre) and the accessibility not disabled?"
        );
    }

    /// The screen reader leaves: the window stops building the tree.
    pub fn a11y_disconnect(&mut self) {
        self.cx.a11y_deactivate(self.handle());
        self.settle();
    }

    /// Whether a screen reader is connected (a tree has been built).
    pub fn a11y_connected(&self) -> bool {
        self.cx.a11y_tree(self.handle()).is_some()
    }

    /// The tree the window sent its adapter in the last frame (connecting a screen reader first).
    pub fn a11y(&mut self) -> Tree {
        self.a11y_connect();
        let update = self
            .cx
            .a11y_tree(self.handle())
            .expect("a tree after connecting");
        Tree::build(&update)
    }

    /// The tree's first element with this role and name, as it is now.
    pub fn a11y_find(&mut self, role: Role, name: &str) -> Option<Element> {
        self.a11y().find(role, name).cloned()
    }

    /// The tree as text.
    pub fn a11y_dump(&mut self) -> String {
        self.a11y().dump()
    }

    /// Ask for `action` on the element `id`, as a screen reader does, and let the dashboard
    /// answer.
    pub fn a11y_request(&mut self, action: Action, id: u64) {
        self.a11y_connect();
        self.cx.a11y_request(self.handle(), request(action, id));
        self.settle();
        self.draw();
    }

    /// `a11y_request` on what a screen reader finds by role and name.
    pub fn a11y_do(&mut self, action: Action, role: Role, name: &str) {
        let Some(e) = self.a11y_find(role, name) else {
            panic!("no {role:?} called {name:?} in:\n{}", self.a11y_dump());
        };
        self.a11y_request(action, e.id);
    }

    /// The entered widget's frame (or, with none, nothing).
    fn entered_frame(&self, tree: &Tree) -> Option<u64> {
        let (mode, title) = self.read(|d| {
            let title = d
                .tiles
                .iter()
                .find(|t| t.id() == d.nav.active_id)
                .map(|t| t.kind().info().title);
            (d.nav.mode, title)
        });
        if mode != NavMode::Widget {
            return None;
        }
        let title = title?;
        let frames = tree.find_all(Role::Group, title);
        // Two widgets of one kind: the one with the cursor in it.
        let cursors = tree.cursors();
        frames
            .iter()
            .find(|f| {
                tree.below(f)
                    .iter()
                    .any(|e| e.id != f.id && cursors.iter().any(|c| c.id == e.id))
            })
            .or(frames.first())
            .map(|f| f.id)
    }

    /// The element the keyboard is on, which is also where a screen reader's focus belongs:
    /// `Ok(None)` with no widget entered (and no cursor in edit mode's sidebar); the entered
    /// widget's frame if it has no cursor of its own; an error says what doesn't add up: the
    /// cursor out of sight, or on two things at once.
    pub fn cursor(&mut self) -> Result<Option<Element>, String> {
        let tree = self.a11y();
        self.cursor_in(&tree)
    }

    /// [`Harness::cursor`] in a tree already taken.
    pub fn cursor_in(&self, tree: &Tree) -> Result<Option<Element>, String> {
        let (editing, side_keys, mode) = self.read(|d| (d.editing, d.edit.side_keys, d.nav.mode));
        let scope: Vec<&Element> = if editing && side_keys && mode != NavMode::Hints {
            // Edit mode's sidebar has a cursor of its own; nothing else has one then.
            tree.cursors()
        } else if mode == NavMode::Widget {
            // An open menu over the dashboard has the keys, and its own cursor.
            if let Some(menu) = tree.find(Role::List, "Tray menu") {
                let cursors = tree.cursors();
                let inside: Vec<&Element> = tree
                    .below(menu)
                    .into_iter()
                    .filter(|e| cursors.iter().any(|c| c.id == e.id))
                    .collect();
                return Ok(Some(inside.first().map_or(menu, |e| *e).clone()));
            }
            match self.entered_frame(tree) {
                Some(frame) => {
                    let frame = tree.get(frame).expect("a frame in the tree");
                    let cursors = tree.cursors();
                    tree.below(frame)
                        .into_iter()
                        .filter(|e| e.id != frame.id && cursors.iter().any(|c| c.id == e.id))
                        .collect()
                }
                None => return Err("the entered widget isn't on the dashboard".into()),
            }
        } else {
            return Ok(None);
        };
        let seen: Vec<&Element> = scope.iter().copied().filter(|e| tree.in_view(e)).collect();
        match seen.as_slice() {
            [one] => Ok(Some((*one).clone())),
            [] if scope.is_empty() && !(editing && side_keys) => {
                // A widget without controls: its frame answers.
                Ok(self.entered_frame(tree).and_then(|f| tree.get(f)).cloned())
            }
            [] if scope.is_empty() => Err("the sidebar's cursor is on nothing".into()),
            [] => Err(format!(
                "the cursor is out of sight, on {}",
                scope
                    .iter()
                    .map(|e| e.says())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            many => Err(format!(
                "the cursor is on {} things at once: {}",
                many.len(),
                many.iter().map(|e| e.says()).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// What a screen reader would say about where the keyboard is; `Ok(None)` with no widget
    /// entered. An error says what doesn't add up: the cursor in another widget, on nothing or
    /// two things, or on something with no name.
    pub fn spoken(&mut self) -> Result<Option<String>, String> {
        let tree = self.a11y();
        self.spoken_in(&tree)
    }

    /// [`Harness::spoken`] in a tree already taken.
    pub fn spoken_in(&self, tree: &Tree) -> Result<Option<String>, String> {
        match self.cursor_in(tree)? {
            Some(e) if e.name().trim().is_empty() => Err(format!(
                "under the keyboard: a {} with no name",
                e.role_name()
            )),
            Some(e) => Ok(Some(e.says())),
            None => Ok(None),
        }
    }

    /// What is under the keyboard cursor anywhere in the window, in sight or not, as one line. A
    /// key moved the cursor if this differs from before it. (The role and the name, not the
    /// value: a key that changes a value hasn't moved the cursor.)
    pub fn cursor_signature(&mut self) -> String {
        self.a11y()
            .cursors()
            .iter()
            .map(|e| format!("{} {}", e.role_name(), e.name()))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}
