use esp_idf_hal::task::thread::{self, ThreadSpawnConfiguration};

use crate::{pattern::PATTERN, state::HEIGHT, state::WIDTH};
use core::sync::atomic::Ordering;

mod gpio;
mod isr;
mod pattern;
mod state;
mod tasks;
mod logger;

fn main() -> anyhow::Result<()> {
    state::KNITTING.store(true, std::sync::atomic::Ordering::Relaxed);
    WIDTH.store(PATTERN.width, Ordering::Relaxed);
    HEIGHT.store(PATTERN.height, Ordering::Relaxed);
    ThreadSpawnConfiguration {
        name: Some("knit_thread".as_bytes()),
        stack_size: 4096,
        priority: 10,
        ..Default::default()
    }
    .set()
    .unwrap();

    let knit_thread = std::thread::Builder::new()
        .spawn(move || {
            //do stuff
        })
        .unwrap();

    knit_thread.join().unwrap();
    Ok(())
}
