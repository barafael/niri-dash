//! CPU, memory, disks, network and GPU.

pub use crate::sources::perf::{DiskRow, Gpu, Snapshot, human_bytes};

#[derive(Debug)]
pub enum Event {
    Snapshot(Box<Snapshot>),
}
