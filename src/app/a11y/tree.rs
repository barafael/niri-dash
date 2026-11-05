//! The dashboard as an AccessKit tree: what a screen reader browses, and where its focus is.
//!
//! The tree is built from the same accessible properties of the `.slint` files that `a11y.rs`
//! reads for the tests, so what the tests say a screen reader hears is what it is given. It is
//! plain data ([`TreeUpdate`]), made without any platform, so a test can build it and read it
//! back with `accesskit_consumer`, as a screen reader's adapter does.
//!
//! The mapping follows Slint's own AccessKit adapter (`i-slint-backend-winit`), which can't be
//! used here: it belongs to the winit window. Two differences:
//!
//! - **The cursor is not "selected".** An element marks where the keyboard is with
//!   `accessible-item-selected` (see `NavScope`); a screen reader would announce that as a
//!   selection. Here it only becomes the tree's focus, and `selected` is said only for elements
//!   that say they are selectable.
//! - **Focus is the cursor,** found by [`super::App::cursor`], not Slint's own focus item, which
//!   for a widget with many stops is one `FocusScope` holding the keys for all of them.

use accesskit::{
    Action, ActionData, ActionRequest, Live, Node, NodeId, Orientation, Rect as AkRect, Role,
    Toggled, Tree, TreeId, TreeUpdate,
};
use i_slint_core::SharedString;
use i_slint_core::accessibility::{
    AccessibilityAction, AccessibleStringProperty as Prop, SupportedAccessibilityAction as Can,
    accessible_descendents, find_text_input, nearest_accessible_item,
};
use i_slint_core::item_tree::{ItemTreeRc, ItemTreeRef, ItemTreeWeak};
use i_slint_core::items::{AccessibleRole as Slint, ItemRc, WindowItem};
use i_slint_core::window::WindowInner;
use std::collections::HashMap;

/// Item index bits of a node id; the rest is the component's number.
const INDEX_BITS: u32 = 16;

/// Which node is which element, kept between builds so that an element keeps its id and a
/// screen reader sees "this changed" rather than "this is new".
#[derive(Default)]
pub(in crate::app) struct Ids {
    next: u32,
    free: Vec<u32>,
    by_component: HashMap<usize, u32>,
    components: HashMap<u32, ItemTreeWeak>,
}

impl Ids {
    fn component(&mut self, item: &ItemRc) -> u32 {
        let tree = item.item_tree();
        let key = ItemTreeRef::as_ptr(ItemTreeRc::borrow(tree)).as_ptr() as usize;
        if let Some(&n) = self.by_component.get(&key) {
            return n;
        }
        let n = self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next
        });
        self.by_component.insert(key, n);
        self.components.insert(n, ItemTreeRc::downgrade(tree));
        n
    }

    fn id(&mut self, item: &ItemRc) -> NodeId {
        let c = self.component(item);
        NodeId(u64::from(c) << INDEX_BITS | u64::from(item.index()) & ((1 << INDEX_BITS) - 1))
    }

    /// The element a node stands for, if it is still there.
    pub fn item(&self, id: NodeId) -> Option<ItemRc> {
        let component = ((id.0 >> INDEX_BITS) & 0xFFFF_FFFF) as u32;
        let index = (id.0 & ((1 << INDEX_BITS) - 1)) as u32;
        Some(ItemRc::new(
            self.components.get(&component)?.upgrade()?,
            index,
        ))
    }

    /// Forget the components that are gone (a list's rows, a closed page), so their numbers can
    /// go to new ones.
    fn forget_dead(&mut self) {
        let dead: Vec<u32> = self
            .components
            .iter()
            .filter(|(_, w)| w.upgrade().is_none())
            .map(|(n, _)| *n)
            .collect();
        for n in dead {
            self.components.remove(&n);
            self.by_component.retain(|_, v| *v != n);
            self.free.push(n);
        }
    }
}

/// The whole tree of `window`, with focus on the element `focus` stands for (the nearest
/// element that is accessible, or the window itself if it is none).
pub(in crate::app) fn build(
    window: &slint::Window,
    ids: &mut Ids,
    focus: Option<ItemRc>,
) -> TreeUpdate {
    let inner = WindowInner::from_pub(window);
    inner.ensure_tree_instantiated();
    let Some(component) = inner.try_component() else {
        return empty();
    };
    ids.forget_dead();
    let root = ItemRc::new_root(component);
    let scale = window.scale_factor();
    let mut nodes = Vec::new();
    let root_id = node_and_children(&root, ids, scale, &mut nodes);
    let focus = focus
        .map(nearest_accessible_item)
        .map_or(root_id, |item| ids.id(&item));
    let mut tree = Tree::new(root_id);
    tree.toolkit_name = Some("Slint".into());
    TreeUpdate {
        nodes,
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus,
    }
}

/// What there is to show before the window has anything.
fn empty() -> TreeUpdate {
    let root = NodeId(0);
    TreeUpdate {
        nodes: vec![(root, Node::new(Role::Window))],
        tree: Some(Tree::new(root)),
        tree_id: TreeId::ROOT,
        focus: root,
    }
}

fn node_and_children(
    item: &ItemRc,
    ids: &mut Ids,
    scale: f32,
    nodes: &mut Vec<(NodeId, Node)>,
) -> NodeId {
    let id = ids.id(item);
    let children: Vec<NodeId> = accessible_descendents(item)
        .map(|child| node_and_children(&child, ids, scale, nodes))
        .collect();
    let mut node = node(item, scale);
    node.set_children(children);
    nodes.push((id, node));
    id
}

fn prop(item: &ItemRc, what: Prop) -> Option<SharedString> {
    item.accessible_string_property(what)
}

fn is_true(item: &ItemRc, what: Prop) -> bool {
    prop(item, what).is_some_and(|v| v == "true")
}

fn parse<T: std::str::FromStr>(item: &ItemRc, what: Prop) -> Option<T> {
    prop(item, what)?.parse().ok()
}

fn is_text_input(role: Role) -> bool {
    matches!(
        role,
        Role::TextInput
            | Role::MultilineTextInput
            | Role::NumberInput
            | Role::PasswordInput
            | Role::SearchInput
    )
}

fn role_of(item: &ItemRc) -> (Role, Option<String>) {
    if let Some(window) = item.downcast::<WindowItem>() {
        return (Role::Window, Some(window.as_pin_ref().title().to_string()));
    }
    let role = match item.accessible_role() {
        Slint::Button => Role::Button,
        Slint::Checkbox => Role::CheckBox,
        Slint::Combobox => Role::ComboBox,
        Slint::Groupbox => Role::Group,
        Slint::List => Role::ListBox,
        Slint::Slider => Role::Slider,
        Slint::Spinbox => Role::SpinButton,
        Slint::Tab => Role::Tab,
        Slint::TabList => Role::TabList,
        Slint::TabPanel => Role::TabPanel,
        Slint::Text => Role::Label,
        Slint::Table => Role::Table,
        Slint::Tree => Role::Tree,
        Slint::TextInput => match find_text_input(item) {
            Some(input) if !input.single_line.get_internal() => Role::MultilineTextInput,
            Some(input) => match input.input_type.get_internal() {
                i_slint_core::items::InputType::Decimal
                | i_slint_core::items::InputType::Number => Role::NumberInput,
                i_slint_core::items::InputType::Password => Role::PasswordInput,
                i_slint_core::items::InputType::Search => Role::SearchInput,
                _ => Role::TextInput,
            },
            None => Role::TextInput,
        },
        Slint::ProgressIndicator => Role::ProgressIndicator,
        Slint::Switch => Role::Switch,
        Slint::ListItem => Role::ListBoxOption,
        Slint::Image => Role::Image,
        Slint::RadioButton => Role::RadioButton,
        Slint::RadioGroup => Role::RadioGroup,
        Slint::WindowTitleBar => Role::TitleBar,
        Slint::Banner => Role::Banner,
        Slint::Complementary => Role::Complementary,
        Slint::ContentInfo => Role::ContentInfo,
        Slint::Form => Role::Form,
        Slint::Main => Role::Main,
        Slint::Navigation => Role::Navigation,
        Slint::Region => Role::Region,
        Slint::Search => Role::Search,
        _ => Role::Unknown,
    };
    (role, prop(item, Prop::Label).map(|l| l.to_string()))
}

/// One element as a node, without its children.
fn node(item: &ItemRc, scale: f32) -> Node {
    let (role, label) = role_of(item);
    let mut node = Node::new(role);
    if let Some(label) = label {
        // A text says itself; anything else is named by it.
        if role == Role::Label {
            node.set_value(label);
        } else {
            node.set_label(label);
        }
    }
    if prop(item, Prop::Enabled).is_some_and(|v| v != "true") {
        node.set_disabled();
    }
    if !item.is_visible() {
        node.set_hidden();
    }
    if item.borrow().as_ref().clips_children() {
        node.set_clips_children();
    }

    let geometry = item.geometry();
    let origin = item.map_to_window(geometry.origin);
    node.set_bounds(AkRect {
        x0: f64::from(origin.x * scale),
        y0: f64::from(origin.y * scale),
        x1: f64::from((origin.x + geometry.size.width) * scale),
        y1: f64::from((origin.y + geometry.size.height) * scale),
    });

    if is_true(item, Prop::Checkable) {
        node.set_toggled(if is_true(item, Prop::Checked) {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if let Some(d) = prop(item, Prop::Description) {
        node.set_description(d.to_string());
    }
    if let Some(id) = prop(item, Prop::Id) {
        node.set_author_id(id.to_string());
    }
    if is_true(item, Prop::Expandable) {
        node.set_expanded(is_true(item, Prop::Expanded));
    }
    if matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::ComboBox
            | Role::ListBoxOption
            | Role::Slider
            | Role::SpinButton
            | Role::Tab
            | Role::Switch
    ) || is_text_input(role)
    {
        node.add_action(Action::Focus);
    }
    let (min, max): (Option<f64>, Option<f64>) = (
        parse(item, Prop::ValueMinimum),
        parse(item, Prop::ValueMaximum),
    );
    if let Some(v) = min {
        node.set_min_numeric_value(v);
    }
    if let Some(v) = max {
        node.set_max_numeric_value(v);
    }
    if let Some(v) = parse(item, Prop::ValueStep) {
        node.set_numeric_value_step(v);
    }
    if let Some(value) = prop(item, Prop::Value) {
        // Our sliders say their value as a text, "70%": a screen reader needs the number too, to
        // step through it and say how far along it is.
        let percent = value
            .trim()
            .strip_suffix('%')
            .and_then(|n| n.trim().parse::<f64>().ok());
        match value.parse::<f64>() {
            _ if matches!(role, Role::Slider | Role::ProgressIndicator) && percent.is_some() => {
                node.set_numeric_value(percent.unwrap_or_default());
                node.set_value(value.to_string());
                if min.is_none() {
                    node.set_min_numeric_value(0.0);
                }
                if max.is_none() {
                    node.set_max_numeric_value(100.0);
                }
            }
            Ok(n) if role == Role::SpinButton => {
                node.set_numeric_value(n);
                node.set_value(value.to_string());
            }
            Ok(n) if !is_text_input(role) => node.set_numeric_value(n),
            _ => node.set_value(value.to_string()),
        }
    }
    if let Some(p) = prop(item, Prop::PlaceholderText) {
        node.set_placeholder(p.to_string());
    }
    if is_true(item, Prop::ReadOnly) {
        node.set_read_only();
    }
    if let Some(o) = parse::<i_slint_core::items::Orientation>(item, Prop::Orientation) {
        node.set_orientation(match o {
            i_slint_core::items::Orientation::Horizontal => Orientation::Horizontal,
            i_slint_core::items::Orientation::Vertical => Orientation::Vertical,
        });
    }
    if let Some(l) = parse::<i_slint_core::items::AccessibleLiveness>(item, Prop::LiveRegion) {
        node.set_live(match l {
            i_slint_core::items::AccessibleLiveness::Polite => Live::Polite,
            i_slint_core::items::AccessibleLiveness::Assertive => Live::Assertive,
            _ => Live::Off,
        });
    }
    // Only an element that says it can be selected is selected. The cursor marker of a
    // NavScope's stops is `ItemSelected` without this, and is the tree's focus instead.
    if is_true(item, Prop::ItemSelectable) {
        node.set_selected(is_true(item, Prop::ItemSelected));
    }
    if let Some(n) = parse(item, Prop::ItemIndex) {
        node.set_position_in_set(n);
    }
    if let Some(n) = parse(item, Prop::ItemCount) {
        node.set_size_of_set(n);
    }
    let can = item.supported_accessibility_actions();
    for (flag, action) in [
        (Can::Default, Action::Click),
        (Can::Decrement, Action::Decrement),
        (Can::Increment, Action::Increment),
        (Can::SetValue, Action::SetValue),
        (Can::Expand, Action::Expand),
    ] {
        if can.contains(flag) {
            node.add_action(action);
        }
    }
    node
}

/// What a screen reader asked for, as the element would be asked. `None`: nothing here does that
/// (focus is the app's own business, see [`super::App::focus_element`]).
pub(in crate::app) fn slint_action(request: &ActionRequest) -> Option<AccessibilityAction> {
    Some(match request.action {
        Action::Click => AccessibilityAction::Default,
        Action::Decrement => AccessibilityAction::Decrement,
        Action::Increment => AccessibilityAction::Increment,
        Action::Expand => AccessibilityAction::Expand,
        Action::SetValue => match &request.data {
            Some(ActionData::Value(v)) => AccessibilityAction::SetValue(SharedString::from(&**v)),
            Some(ActionData::NumericValue(v)) => {
                AccessibilityAction::SetValue(i_slint_core::format!("{v}"))
            }
            _ => return None,
        },
        _ => return None,
    })
}
