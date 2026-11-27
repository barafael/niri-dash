//! Screen readers: the dashboard's accessibility tree, published over AT-SPI.
//!
//! BUI builds each window's tree (its widgets and the page, named as a browser names them), but
//! serves it only on Windows. Here `accesskit_unix` serves it: its handlers run on a thread of
//! their own and only hand the news to the UI thread; there the window's tree is read with
//! `App::accessibility_tree`, turned into AccessKit nodes, and sent, the whole tree when a reader
//! arrives and then only what changed, at every tick while one listens. What a reader asks of a
//! node goes back to BUI as `App::accessibility_action`, which does it as the input it stands for.

use crate::app::Shell;
use accesskit::{
    Action as A, ActionRequest, Node, NodeId, Rect, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use blenderui::a11y::{AccessNode, AccessTree, Action, Role, flags};
use blenderui::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// What reaches the UI thread from `accesskit_unix`.
enum Request {
    Activated,
    Deactivated,
    Action(ActionRequest),
}

/// The handlers `accesskit_unix` calls on its thread.
#[derive(Clone)]
struct Handlers(UiSender);

impl Handlers {
    fn tell(&self, r: Request) {
        // A request is a value of the accesskit thread's; it crosses in a Mutex to be `Send`.
        let r = std::sync::Mutex::new(Some(r));
        self.0.send(move |app| {
            if let Some(r) = r.lock().ok().and_then(|mut r| r.take()) {
                handle(app, r);
            }
        });
    }
}

impl accesskit::ActivationHandler for Handlers {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Built on the UI thread, and sent from there.
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

/// The bridge: the adapter, whether a reader listens, and what it was sent.
pub struct Bridge {
    adapter: accesskit_unix::Adapter,
    active: bool,
    /// A hash of each node as last sent, by id.
    sent: HashMap<u64, u64>,
}

impl Bridge {
    /// Start serving (nothing is built until a reader asks).
    pub fn start(ui: UiSender) -> Self {
        let h = Handlers(ui);
        Self {
            adapter: accesskit_unix::Adapter::new(h.clone(), h.clone(), h),
            active: false,
            sent: HashMap::new(),
        }
    }
}

fn handle(app: &mut App, r: Request) {
    match r {
        Request::Activated => {
            with_bridge(app, |b| {
                b.active = true;
                b.sent.clear();
            });
            update(app);
        }
        Request::Deactivated => with_bridge(app, |b| b.active = false),
        Request::Action(req) => {
            let Some(win) = app.global::<Shell>().window else {
                return;
            };
            let action = match req.action {
                A::Click => Action::Click,
                A::Focus => Action::Focus,
                A::SetValue => Action::SetValue,
                A::Increment => Action::Increment,
                A::Decrement => Action::Decrement,
                A::Expand => Action::Expand,
                A::Collapse => Action::Collapse,
                A::ScrollIntoView => Action::ScrollIntoView,
                A::ShowContextMenu => Action::ContextMenu,
                _ => return,
            };
            let (text, num) = match &req.data {
                Some(accesskit::ActionData::Value(v)) => (Some(v.to_string()), 0.0),
                Some(accesskit::ActionData::NumericValue(n)) => (None, *n),
                _ => (None, 0.0),
            };
            app.accessibility_action(win, req.target_node.0, action, text.as_deref(), num);
            app.notify_all();
        }
    }
}

fn with_bridge(app: &mut App, f: impl FnOnce(&mut Bridge)) {
    if app.try_global::<Shell>().is_some()
        && let Some(b) = app.global_mut::<Shell>().a11y.as_mut()
    {
        f(b);
    }
}

/// Send the reader what changed in the window, if one listens and the window is up.
pub fn update(app: &mut App) {
    let listening = app
        .try_global::<Shell>()
        .is_some_and(|s| s.a11y.as_ref().is_some_and(|b| b.active));
    if !listening {
        return;
    }
    let Some(win) = app.global::<Shell>().window else {
        return;
    };
    let tree = app.accessibility_tree(win);
    let name = match app.global::<Shell>().solo {
        Some(k) => format!("{} (niri-dash)", k.info().title),
        None => "niri-dash".into(),
    };
    with_bridge(app, |b| {
        let update = diff(&mut b.sent, &tree, &name);
        if !update.nodes.is_empty() || update.tree.is_some() {
            b.adapter.update_if_active(|| update);
        }
    });
}

/// The window's focus, which a reader follows only while the window has it.
pub fn window_focus(app: &mut App, focused: bool) {
    with_bridge(app, |b| b.adapter.update_window_focus_state(focused));
}

/// The update that brings a reader from what it was sent (`sent`, updated) to `tree`.
pub fn diff(sent: &mut HashMap<u64, u64>, tree: &AccessTree, name: &str) -> TreeUpdate {
    let full = sent.is_empty();
    let mut kids: Vec<Vec<NodeId>> = vec![Vec::new(); tree.nodes.len()];
    for n in &tree.nodes {
        if let Some(p) = n.parent {
            kids[p].push(NodeId(n.id));
        }
    }
    let mut next = HashMap::with_capacity(tree.nodes.len());
    let mut nodes = Vec::new();
    for (i, n) in tree.nodes.iter().enumerate() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{n:?}").hash(&mut h);
        kids[i].hash(&mut h);
        let hash = h.finish();
        next.insert(n.id, hash);
        if full || sent.get(&n.id) != Some(&hash) {
            let mut node = convert(n, &kids[i]);
            // The window: a layer surface has no title for BUI to name it after.
            if i == 0 && n.name.is_empty() {
                node.set_label(name);
            }
            nodes.push((NodeId(n.id), node));
        }
    }
    *sent = next;
    let root = NodeId(tree.nodes.first().map_or(0, |n| n.id));
    let focus = if sent.contains_key(&tree.focus) {
        NodeId(tree.focus)
    } else {
        root
    };
    TreeUpdate {
        nodes,
        tree: full.then(|| TreeInfo {
            root,
            toolkit_name: Some("BUI".into()),
            toolkit_version: Some("0.6".into()),
        }),
        tree_id: TreeId::ROOT,
        focus,
    }
}

/// One node of BUI's tree as AccessKit's, as BUI's own (Windows) adapter makes it.
fn convert(n: &AccessNode, kids: &[NodeId]) -> Node {
    let has = |f: u32| n.flags & f != 0;
    let mut node = Node::new(if n.role == Role::Button && has(flags::DEFAULT) {
        accesskit::Role::DefaultButton
    } else {
        n.role.accesskit()
    });
    let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
    if n.role == Role::Label {
        if let Some(s) = some(&n.name).or(some(&n.value)) {
            node.set_value(s);
        }
    } else {
        if let Some(s) = some(&n.name) {
            node.set_label(s);
        }
        if let Some(s) = some(&n.value).or_else(|| has(flags::EDITABLE).then(String::new)) {
            node.set_value(s);
        }
    }
    if let Some(s) = some(&n.description) {
        node.set_description(s);
    }
    let [x, y, w, h] = n.bounds;
    if w > 0.0 || h > 0.0 {
        node.set_bounds(Rect {
            x0: f64::from(x),
            y0: f64::from(y),
            x1: f64::from(x + w),
            y1: f64::from(y + h),
        });
        node.add_action(A::ScrollIntoView);
    }
    if !kids.is_empty() {
        node.set_children(kids);
    }
    if n.level > 0 {
        node.set_level(usize::from(n.level));
    }
    let live = !has(flags::DISABLED);
    if !live {
        node.set_disabled();
    }
    if has(flags::CHECKABLE) {
        node.set_toggled(if has(flags::MIXED) {
            Toggled::Mixed
        } else {
            Toggled::from(has(flags::CHECKED))
        });
    }
    if has(flags::EXPANDABLE) {
        node.set_expanded(has(flags::EXPANDED));
    }
    if has(flags::SELECTABLE) {
        node.set_selected(has(flags::SELECTED));
    }
    if has(flags::READONLY) {
        node.set_read_only();
    }
    if let Some((v, min, max)) = n.numeric {
        node.set_numeric_value(v);
        if min.is_finite() {
            node.set_min_numeric_value(min);
        }
        if max.is_finite() {
            node.set_max_numeric_value(max);
        }
    }
    let writable = live && !has(flags::READONLY);
    if has(flags::FOCUSABLE) && live {
        node.add_action(A::Focus);
    }
    if has(flags::CLICKABLE) && live {
        node.add_action(A::Click);
    }
    if writable && (has(flags::EDITABLE) || has(flags::NUMERIC)) {
        node.add_action(A::SetValue);
    }
    if writable && has(flags::NUMERIC) {
        node.add_action(A::Increment);
        node.add_action(A::Decrement);
    }
    if has(flags::EXPANDABLE) && live {
        node.add_action(if has(flags::EXPANDED) {
            A::Collapse
        } else {
            A::Expand
        });
    }
    node
}
