//! The system tray (StatusNotifierItem).

use crate::IconSrc;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub address: String,
    pub title: String,
    pub icon: IconSrc,
    pub has_menu: bool,
    pub item_is_menu: bool,
}

/// Whether a menu entry shows a checkmark or radio state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    None,
    Check,
    Radio,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i32,
    pub label: String,
    pub depth: i32,
    pub separator: bool,
    pub enabled: bool,
    pub toggle: Toggle,
    pub checked: bool,
    pub header: bool,
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub address: String,
    pub title: String,
    pub entries: Vec<Entry>,
    pub x: f32,
    pub y: f32,
}

/// Items are named by their address.
#[derive(Debug)]
pub enum Command {
    Activate { address: String, x: i32, y: i32 },
    OpenMenu { address: String, x: f32, y: f32 },
    MenuItem { address: String, id: i32 },
}

#[derive(Debug)]
pub enum Event {
    Items(Vec<Item>),
    Menu(Menu),
}
