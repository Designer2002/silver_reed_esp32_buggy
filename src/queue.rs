use std::sync::LazyLock;

use esp_idf_hal::task::queue::Queue;

pub static QUEUE: LazyLock<Queue<u8>> = LazyLock::new(|| {
    let q: Queue<u8> = Queue::new(4096);
    q
});

pub const EVENT: u8 = 1;