use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize};

pub static ROW: AtomicI32 = AtomicI32::new(0);
pub static NEEDLE: AtomicI32 = AtomicI32::new(0);
pub static DIR_RIGHT: AtomicBool = AtomicBool::new(true);
pub static INSIDE_PATTERN: AtomicBool = AtomicBool::new(false);
pub static KNITTING: AtomicBool = AtomicBool::new(false);
pub static WIDTH: AtomicUsize = AtomicUsize::new(0);
pub static HEIGHT: AtomicUsize = AtomicUsize::new(0);
pub static DOB_LAST_STATE: AtomicBool = AtomicBool::new(true); // по умолчанию HIGH
pub static CCP_LAST_STATE: AtomicBool = AtomicBool::new(true); // по умолчанию HIGH
pub static HOK_LAST_LEVEL: AtomicBool = AtomicBool::new(true);
pub static PATTERN_START: AtomicI32 = AtomicI32::new(0);
pub static PATTERN_END: AtomicI32 = AtomicI32::new(0);

// ✅ Sequence counter для синхронизации KSL и CCP
pub static EVENT_SEQUENCE: AtomicU32 = AtomicU32::new(0);
pub static LAST_KSL_SEQUENCE: AtomicU32 = AtomicU32::new(0);

// ✅ Debounce для KSL — раздельный для rise и fall (20ms окно)
pub static KSL_RISE_DEBOUNCE_UNTIL: AtomicU32 = AtomicU32::new(0);
pub static KSL_FALL_DEBOUNCE_UNTIL: AtomicU32 = AtomicU32::new(0);
pub const KSL_DEBOUNCE_MS: u32 = 25;

// ✅ Последнее состояние KSL для определения направления
pub static KSL_LAST_STATE: AtomicBool = AtomicBool::new(true);

// ✅ CCP debounce - минимальное время между тиками (защита от помех)
// ~9090 Hz = ~110μs между тиками
pub static CCP_LAST_TICK_US: AtomicU32 = AtomicU32::new(0);
pub const CCP_MIN_INTERVAL_US: u32 = 100; // 100 microseconds

pub const FREQUENCY_SILVER_REED: u32 = 110011; // 9090 Hz in microseconds
pub const US_PER_MS: u32 = 1_000;
pub const DOB: i32 = 23;
pub const CCP: i32 = 18;
pub const HOK: i32 = 19;
pub const KSL: i32 = 21;
pub const ND1: i32 = 22;
