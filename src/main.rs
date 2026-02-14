use crate::{pattern::PATTERN, state::WIDTH, state::HEIGHT};
use core::sync::atomic::Ordering;

mod tasks;
mod isr;
mod state;
mod pattern;
mod gpio;

fn main() -> anyhow::Result<()> {
    state::KNITTING.store(true, std::sync::atomic::Ordering::Relaxed);
    WIDTH.store(PATTERN.width, Ordering::Relaxed);
    HEIGHT.store(PATTERN.height, Ordering::Relaxed);
    Ok(())
}