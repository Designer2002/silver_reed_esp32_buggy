use std::sync::LazyLock;

use esp_idf_hal::task::queue::Queue;

#[derive(Clone, Copy, Debug)]
pub struct EngineEvent {
    pub kind: u8,
    pub seq: u32,
    pub timestamp_us: u64,
    pub level: bool,
}

pub static QUEUE: LazyLock<Queue<EngineEvent>> = LazyLock::new(|| {
    let q: Queue<EngineEvent> = Queue::new(4096);
    q
});

pub const EVT_CCP: u8 = 1;
pub const EVT_ND1: u8 = 2;
pub const EVT_KSL: u8 = 3;
pub const EVT_HOK: u8 = 4;