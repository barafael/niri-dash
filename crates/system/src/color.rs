//! Picking a pixel's color anywhere on screen, through the compositor.

#[derive(Debug)]
pub enum Command {
    /// Let the user click a pixel. The answer is a `Picked`.
    Pick,
}

#[derive(Debug)]
pub enum Event {
    /// Red, green and blue from 0 to 1; None if cancelled or it failed.
    Picked(Option<[f64; 3]>),
}
