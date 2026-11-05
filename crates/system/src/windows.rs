//! Open windows by workspace (niri only).

use crate::IconSrc;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: u64,
    pub header: bool,
    pub title: String,
    pub app: String,
    pub icon: IconSrc,
    pub focused: bool,
    pub urgent: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub connected: bool,
    pub rows: Vec<Row>,
    pub count: usize,
    pub focused_icon: Option<IconSrc>,
}

#[derive(Debug)]
pub enum Command {
    Focus(u64),
    /// Focus the window of this process, if it has one.
    FocusPid(i32),
    /// Bring up the terminal tab this process runs in, where the terminal can be asked (Konsole).
    ShowTab(i32),
    Close(u64),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}
