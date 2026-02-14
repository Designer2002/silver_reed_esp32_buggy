use heapless::spsc::Queue;
use core::cell::UnsafeCell;

// SAFETY: Only accessed via provided API, SPSC (single producer, single consumer)
pub struct LogQueue {
    queue: UnsafeCell<Queue<LogEntry, 256>>,
}

unsafe impl Sync for LogQueue {}

static LOG_QUEUE: LogQueue = LogQueue {
    queue: UnsafeCell::new(Queue::new()),
};
#[derive(Debug, Clone, Copy)]
pub struct LogEntry {
    pub timestamp: &'static str, // Ссылка на строку, живущую всю программу
    pub level: &'static str,
    pub message: &'static str,
}

impl LogEntry {
    pub fn new(t: &'static str, lvl: &'static str, msg: &'static str) -> Self {
        Self {
            timestamp: t,
            level: lvl,
            message: msg,
        }
    }
}

impl LogQueue {
    /// Push a log entry. If full, drops the oldest log.
    pub fn push(&self, entry: LogEntry) {
        // SAFETY: Only one producer (main or ISR)
        let queue = unsafe { &mut *self.queue.get() };
        if queue.enqueue(entry).is_err() {
            queue.dequeue();
            let _ = queue.enqueue(entry);
        }
    }

    /// Pop a log entry. Returns None if empty.
    pub fn pop(&self) -> Option<LogEntry> {
        // SAFETY: Only one consumer (server thread)
        let queue = unsafe { &mut *self.queue.get() };
        queue.dequeue()
    }

    /// Get the number of logs in the queue.
    pub fn len(&self) -> usize {
        let queue = unsafe { &*self.queue.get() };
        queue.len()
    }
}

/// Log from ISR or main context. Timestamp must be 'static.
pub fn log_from_isr(entry: LogEntry) {
    LOG_QUEUE.push(entry);
}

/// Log from main context, with timestamp generated.
pub fn log(level: &'static str, msg: &'static str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let timestamp = format!("{}.{}", now.as_secs(), now.subsec_millis());
    let timestamp_static = Box::leak(timestamp.into_boxed_str());
    let entry = LogEntry::new(timestamp_static, level, msg);
    log_from_isr(entry);
}

/// Pop a log entry (for server/consumer).
pub fn pop_log() -> Option<LogEntry> {
    LOG_QUEUE.pop()
}

