use std::sync::{LazyLock, Mutex, atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering}};
use esp_idf_svc::nvs::{EspNvs, NvsDefault};

pub static ROW: AtomicI32 = AtomicI32::new(0);
pub static NEEDLE: AtomicI32 = AtomicI32::new(0);
pub static DIR_RIGHT: AtomicBool = AtomicBool::new(true);
pub static INSIDE_PATTERN: AtomicBool = AtomicBool::new(true);
pub static KNITTING: AtomicBool = AtomicBool::new(false);
pub static DOB_LAST_STATE: AtomicBool = AtomicBool::new(true);
pub static CCP_LAST_STATE: AtomicBool = AtomicBool::new(true);
pub static PATTERN_START: AtomicI32 = AtomicI32::new(0);
pub static PATTERN_END: AtomicI32 = AtomicI32::new(0);

pub static ROW_START_NEEDLE: AtomicI32 = AtomicI32::new(0);
pub static ROW_END_NEEDLE: AtomicI32 = AtomicI32::new(0);

pub static CCP_AVG_INTERVAL_US: AtomicU32 = AtomicU32::new(0);
pub static CCP_INTERVAL_SUM: AtomicU32 = AtomicU32::new(0);
pub static CCP_INTERVAL_COUNT: AtomicU32 = AtomicU32::new(0);
pub static CCP_FILTER_RESET: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn ccp_filter_reset_on_ksl_rise() {
    CCP_AVG_INTERVAL_US.store(0, Ordering::Relaxed);
    CCP_INTERVAL_SUM.store(0, Ordering::Relaxed);
    CCP_INTERVAL_COUNT.store(0, Ordering::Relaxed);
    CCP_FILTER_RESET.store(true, Ordering::Relaxed);
}

pub static HOK_LAST_STATE: AtomicBool = AtomicBool::new(false);
pub static HOK_LAST_DEBOUNCE_US: AtomicU32 = AtomicU32::new(0);
pub static HOK_DIR_CHANGE_DEBOUNCE_US: AtomicU32 = AtomicU32::new(0);
pub const MIN_DIR_CHANGE_INTERVAL_US: u32 = 300_000;

pub static KSL_LAST_STATE: AtomicBool = AtomicBool::new(true);
pub static KSL_LAST_DEBOUNCE_US: AtomicU32 = AtomicU32::new(0);
pub const KSL_HOK_DEBOUNCE_US: u32 = 20_000;

pub const FREQUENCY_SILVER_REED: u32 = 110011;
pub const US_PER_MS: u32 = 1_000;
pub const DOB: i32 = 23;
pub const CCP: i32 = 18;
pub const HOK: i32 = 19;
pub const KSL: i32 = 21;
pub const ND1: i32 = 22;

pub const BUFFER_SIZE: i32 = 8192; // Увеличен с запасом для полного паттерна

// ✅ ЕДИНЫЙ источник истины для текущего ряда
pub static GLOBAL_ROW: AtomicI32 = AtomicI32::new(0);
pub static PATTERN_LOADED: AtomicBool = AtomicBool::new(false);

// ✅ Для отправки информации о ряде на сервер
pub static ROW_INFO_PENDING: AtomicBool = AtomicBool::new(false);
pub static ROW_INFO_ROW: AtomicI32 = AtomicI32::new(0);
pub static ROW_INFO_DIR: AtomicBool = AtomicBool::new(true);

// ✅ NVS keys (только глобальный ряд)
pub const KNIT_NAMESPACE: &str = "knit";
pub const KEY_GLOBAL_ROW: &str = "global_row";
pub static KNIT_NVS: LazyLock<Mutex<Option<EspNvs<NvsDefault>>>> = LazyLock::new(|| Mutex::new(None));