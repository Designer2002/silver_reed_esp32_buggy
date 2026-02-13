use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};


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

// --- Глобальный буфер ---
static LOGS: LazyLock<Mutex<VecDeque<LogEntry>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(300)));

// --- Функция для добавления лога ---
pub fn log(level: &'static str, msg: &'static str) { // <- Параметры теперь 'static str
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let timestamp = format!("{}.{}", now.as_secs(), now.subsec_millis());

    // --- НОВОЕ: Преобразуем динамическую строку timestamp в 'static ---
    // Это НЕБЕЗОПАСНО: строка будет жить до конца программы!
    let timestamp_static = Box::leak(timestamp.into_boxed_str());

    // Создаём LogEntry с 'static str
    let entry = LogEntry::new(timestamp_static, level, msg);

    // Захватываем мьютекс, добавляем в буфер
    let mut buf = LOGS.lock().unwrap();
    buf.push_back(entry);

    // Ограничиваем размер
    if buf.len() > 300 {
        buf.pop_front();
    }
}

// --- Функция для получения всех логов ---
pub fn get_logs() -> Vec<LogEntry> {
    let buf = LOGS.lock().unwrap();
    buf.iter().cloned().collect() // Cloned работает, потому что &'static str реализует Copy
}