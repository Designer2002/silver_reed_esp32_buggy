use std::sync::LazyLock;

use esp_idf_hal::task::queue::Queue;

pub static QUEUE: LazyLock<Queue<(u8, u32)>> = LazyLock::new(|| {
    let q: Queue<(u8, u32)> = Queue::new(4096);
    q
});

pub const EVT_CCP: u8 = 1;
pub const EVT_ND1: u8 = 2;
pub const EVT_KSL: u8 = 3;
pub const EVT_HOK: u8 = 4;