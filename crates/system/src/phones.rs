//! Phones paired through KDE Connect.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Phone {
    pub id: String,
    pub name: String,
    /// "phone", "tablet", "desktop", "laptop" or "tv".
    pub kind: String,
    pub reachable: bool,
    /// Battery in percent, when the phone reports it.
    pub charge: Option<i32>,
    pub charging: bool,
    pub can_ring: bool,
    pub can_share: bool,
}

impl Phone {
    /// "82% · charging", "connected", "not connected".
    pub fn status(&self) -> String {
        if !self.reachable {
            return "not connected".into();
        }
        match self.charge {
            Some(c) if self.charging => format!("{c}% · charging"),
            Some(c) => format!("{c}%"),
            None => "connected".into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub available: bool,
    /// Paired devices: reachable first, phones and tablets before computers.
    pub phones: Vec<Phone>,
}

/// Phones are named by their KDE Connect id.
#[derive(Debug)]
pub enum Command {
    Ring(String),
    /// Pick files with the desktop's file chooser and send them.
    SendFiles(String),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}
