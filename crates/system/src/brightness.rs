//! The screen's backlight.

#[derive(Debug)]
pub enum Command {
    /// 0 to 1.
    Set(f32),
}

#[derive(Debug)]
pub enum Event {
    /// None: no backlight.
    Level(Option<f32>),
}
