//! The system tray (StatusNotifierItem).

pub use crate::sources::tray::{Item, Menu, Toggle};

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
