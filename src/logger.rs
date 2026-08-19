use heapless::spsc::Queue;
use core::cell::UnsafeCell;
use std::sync::Mutex;

// SAFETY: Only accessed via provided API, SPSC (single producer, single consumer)
pub struct LogQueue {
    queue: UnsafeCell<Queue<LogEntry, 256>>,
}

unsafe impl Sync for LogQueue {}

static LOG_QUEUE: LogQueue = LogQueue {
    queue: UnsafeCell::new(Queue::new()),
};
static WEB_LOGS: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

impl LogEntry {
    pub fn new(timestamp: String, level: String, message: String) -> Self {
        Self {
            timestamp,
            level,
            message,
        }
    }
}

impl LogQueue {
    /// Push a log entry. If full, drops the oldest log.
    pub fn push(&self, entry: LogEntry) {
        let entry_clone = entry.clone();
        // SAFETY: Only one producer (main or ISR)
        let queue = unsafe { &mut *self.queue.get() };
        if queue.len() >= 128 {
            let _ = queue.dequeue();
        }
        if queue.enqueue(entry).is_err() {
            queue.dequeue();
            let _ = queue.enqueue(entry_clone);
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

/// Log from ISR or main context.
pub fn log_from_isr(entry: LogEntry) {
    LOG_QUEUE.push(entry);
}

/// Логирование с поддержкой разных типов
pub fn log(level: &str, msg: impl AsRef<str>) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let timestamp = format!("{}.{}", now.as_secs(), now.subsec_millis());
    
    let entry = LogEntry::new(
        timestamp,
        level.to_string(),
        msg.as_ref().to_string()
    );
    
    log_from_isr(entry);
}

/// Логирование с форматированием (как println!)
#[macro_export]
macro_rules! log_fmt {
    ($level:expr, $($arg:tt)*) => {
        log($level, &format!($($arg)*))
    };
}

/// Логирование ошибок
pub fn log_error(msg: impl AsRef<str>) {
    log("ERROR", msg);
}

/// Логирование информации
pub fn log_info(msg: impl AsRef<str>) {
    log("INFO", msg);
}

/// Логирование отладки
pub fn log_debug(msg: impl AsRef<str>) {
    log("DEBUG", msg);
}

/// Логирование предупреждений
pub fn log_warn(msg: impl AsRef<str>) {
    log("WARN", msg);
}

/// Pop a log entry (for server/consumer).
pub fn pop_log() -> Option<LogEntry> {
    LOG_QUEUE.pop()
}

pub fn push_web_log(entry: LogEntry) {
    let mut logs = WEB_LOGS.lock().unwrap();

    if logs.len() > 500 {
        logs.remove(0);
    }

    logs.push(entry);
}

pub fn trim_logs() {
    let mut logs = WEB_LOGS.lock().unwrap();
    if logs.len() > 256 {
        let cutoff = logs.len().saturating_sub(256);
        logs.drain(0..cutoff);
    }

    while LOG_QUEUE.len() > 128 {
        let _ = LOG_QUEUE.pop();
    }
}

pub fn get_logs() -> Vec<LogEntry> {
    WEB_LOGS.lock().unwrap().clone()
}

