use crate::knitter::parse_pattern;
use crate::types::{KnitPattern, LogEntry, SilverLinkEmulator};
use dotenvy_macro::dotenv;
use heapless::spsc::{Consumer, Producer, Queue};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, LazyLock, Mutex};
use critical_section::Mutex as CSMutex;
use std::cell::RefCell;
use esp_idf_hal::task::queue::Queue as TaskQueue;

pub const SSID: &str = dotenv!("WIFI_SSID");
pub const PASS: &str = dotenv!("WIFI_PASS");
pub const INDEX_HTML: &str = include_str!("index.html");
pub const CSS: &str = include_str!("style.css");
pub const PAT_TXT: &str = include_str!("pat.txt"); // Убедись, что файл существует


pub const DOB_PIN_NUM: i32 = 4;
pub const CCP_PIN_NUM: i32 = 18;
pub const HOK_PIN_NUM: i32 = 19;
pub const KSL_PIN_NUM: i32 = 21;
pub const ND1_PIN_NUM: i32 = 22;

pub const FREQUENCY_SILVER_REED_US: u32 = 110; // ~9090 Hz
pub const CCP_QUEUE_TIMEOUT_MS: u32 = 1000;

// --- ГЛОБАЛЬНЫЕ СТАТУСЫ ---
pub static GLOBAL_KNITTING_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static CURRENT_ROW: AtomicUsize = AtomicUsize::new(0);
pub static CURRENT_COLUMN: AtomicUsize = AtomicUsize::new(0);
pub static TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);
pub static TOTAL_COLUMNS: AtomicUsize = AtomicUsize::new(0);
pub static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

// очередь
pub static LOG_QUEUE: CSMutex<RefCell<Option<Queue<LogEntry, 256>>>> =
    CSMutex::new(RefCell::new(None));

pub static LOG_PROD: CSMutex<RefCell<Option<Producer<'static, LogEntry>>>> =
    CSMutex::new(RefCell::new(None));

pub static LOG_CONS: CSMutex<RefCell<Option<Consumer<'static, LogEntry>>>> =
    CSMutex::new(RefCell::new(None));

pub static PATTERN: std::sync::LazyLock<KnitPattern> =
    std::sync::LazyLock::new(|| parse_pattern(PAT_TXT));

// --- ISR и ОЧЕРЕДЬ ---
pub static CCP_QUEUE: std::sync::LazyLock<TaskQueue<u8>> = std::sync::LazyLock::new(|| TaskQueue::new(32));

pub extern "C" fn ccp_isr_handler(_arg: *mut core::ffi::c_void) {
    let _ = CCP_QUEUE.send_back(1u8, 100); // Не критично, если не отправилось
}

// --- ЭМУЛЯТОР ---
pub static EMULATOR_INSTANCE: LazyLock<Arc<Mutex<Option<SilverLinkEmulator>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));


