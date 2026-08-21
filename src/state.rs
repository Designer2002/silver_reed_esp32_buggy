use core::cell::UnsafeCell;
use std::sync::{LazyLock, Mutex, atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering}};
use esp_idf_svc::nvs::{EspNvs, NvsDefault};

pub static ROW: AtomicI32 = AtomicI32::new(0);
pub static NEEDLE: AtomicI32 = AtomicI32::new(0);
pub static DIR_RIGHT: AtomicBool = AtomicBool::new(true);
pub static INSIDE_PATTERN: AtomicBool = AtomicBool::new(true);
pub static KNITTING: AtomicBool = AtomicBool::new(false);
pub static WIDTH: AtomicUsize = AtomicUsize::new(0);
pub static DOB_LAST_STATE: AtomicBool = AtomicBool::new(true); // по умолчанию HIGH
pub static CCP_LAST_STATE: AtomicBool = AtomicBool::new(true); // по умолчанию HIGH
pub static PATTERN_START: AtomicI32 = AtomicI32::new(0);
pub static PATTERN_END: AtomicI32 = AtomicI32::new(0);

// ✅ Sequence counter для синхронизации событий в единой FIFO очереди
pub static EVENT_SEQUENCE: AtomicU32 = AtomicU32::new(0);

// ✅ Tracking для отладки прошлых рядов
// На какой игле начался ряд (KSL rise - вход в паттерн)
pub static ROW_START_NEEDLE: AtomicI32 = AtomicI32::new(0);
// На какой игле закончился ряд (KSL rise - выход из паттерна)
pub static ROW_END_NEEDLE: AtomicI32 = AtomicI32::new(0);

// ✅ CCP debounce - минимальное время между тиками (защита от помех)
pub static CCP_LAST_TICK_US: AtomicU32 = AtomicU32::new(0);
// ✅ Адаптивный debounce: среднее время между тиками (простое среднее)
pub static CCP_AVG_INTERVAL_US: AtomicU32 = AtomicU32::new(0);
pub static CCP_INTERVAL_SUM: AtomicU32 = AtomicU32::new(0);
pub static CCP_INTERVAL_COUNT: AtomicU32 = AtomicU32::new(0);
pub static CCP_FILTER_RESET: AtomicBool = AtomicBool::new(false);
pub const CCP_AUTO_PASS_US: u32 = 1000; // > 1000μs = точно реальный тик
pub const CCP_AUTO_REJECT_US: u32 = 250; // < 250μs = точно помеха
pub const CCP_MIN_RATIO: u32 = 3;
pub const CCP_MAX_RATIO: u32 = 3;

// ✅ Сбросить CCP avg при входе в паттерн (после простоя скорость другая)
#[inline(always)]
pub fn ccp_filter_reset_on_ksl_rise() {
    CCP_AVG_INTERVAL_US.store(0, Ordering::Relaxed);
    CCP_INTERVAL_SUM.store(0, Ordering::Relaxed);
    CCP_INTERVAL_COUNT.store(0, Ordering::Relaxed);
    CCP_FILTER_RESET.store(true, Ordering::Relaxed);
}
pub static HOK_LAST_STATE: AtomicBool = AtomicBool::new(false);
pub static HOK_LAST_DEBOUNCE_US: AtomicU32 = AtomicU32::new(0);

// ✅ Защита от подёргивания каретки: debounce направления
// При реальном развороте проходит > 300ms между сменами направления
// При подёргивании — < 100ms
pub static HOK_DIR_CHANGE_DEBOUNCE_US: AtomicU32 = AtomicU32::new(0);
pub const MIN_DIR_CHANGE_INTERVAL_US: u32 = 300_000; // 300ms между сменами направления

// ✅ KSL debounce (вход/выход из паттерна)
pub static KSL_LAST_STATE: AtomicBool = AtomicBool::new(true); // по умолчанию HIGH (вне паттерна)
pub static KSL_LAST_DEBOUNCE_US: AtomicU32 = AtomicU32::new(0);
pub const KSL_HOK_DEBOUNCE_US: u32 = 20_000; // 20ms debounce для KSL/HOK

pub const FREQUENCY_SILVER_REED: u32 = 110011; // 9090 Hz in microseconds
pub const US_PER_MS: u32 = 1_000;
pub const DOB: i32 = 23;
pub const CCP: i32 = 18;
pub const HOK: i32 = 19;
pub const KSL: i32 = 21;
pub const ND1: i32 = 22;

pub const BUFFER_SIZE: i32 = 2048; // Увеличен для поддержки широких узоров
pub const MAX_HTTP_OUTPUT_BUFFER: usize = 8192;
pub const CHUNK_SIZE: usize = 4; // рядов в одном чанке
pub const MAX_ROW_HITS: usize = 512; // фиксированный лимит hits в одном ряде, чтобы не расти в heap
pub const MAX_ROW_JSON_BYTES: usize = 4096; // фиксированный JSON на один ряд

#[inline(always)]
pub fn row_hit_capacity() -> usize {
    let width = WIDTH.load(Ordering::Relaxed);
    let row_width = width.max(1) as usize;
    row_width.min(MAX_ROW_HITS)
}

// ✅ Для потоковой загрузки паттерна
// Счетчик рядов с момента последней загрузки чанка
pub static ROWS_IN_CURRENT_CHUNK: AtomicI32 = AtomicI32::new(0);
// Флаг: пора запрашивать новую часть узора (когда вошли в 3-й ряд текущего чанка)
pub static REQUEST_NEW_CHUNK: AtomicBool = AtomicBool::new(false);
// Флаг: данные загружаются, нельзя менять буфер
pub static CHUNK_LOADING: AtomicBool = AtomicBool::new(false);
// Общий номер текущего ряда (для отправки на сервер)
pub static GLOBAL_ROW: AtomicI32 = AtomicI32::new(0);
// Номер ряда, с которого начался текущий чанк
pub static CURRENT_CHUNK_START_ROW: AtomicI32 = AtomicI32::new(0);
// Буфер для следующего чанка (4 ряда)
pub static NEXT_CHUNK_ROWS: std::sync::LazyLock<std::sync::Mutex<Option<Vec<Vec<bool>>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
// Флаг: начальный чанк был запрошен при старте вязания
pub static INITIAL_CHUNK_REQUESTED: AtomicBool = AtomicBool::new(false);
// Временный следующий чанк, который будет активирован только на KSL FALL
pub static NEXT_CHUNK_START_ROW: AtomicI32 = AtomicI32::new(0);
pub static CHUNK_SWAP_PENDING: AtomicBool = AtomicBool::new(false);

// ✅ Для отправки информации о ряде на сервер
pub static ROW_INFO_PENDING: AtomicBool = AtomicBool::new(false);
pub static ROW_INFO_ROW: AtomicI32 = AtomicI32::new(0);
pub static ROW_INFO_DIR: AtomicBool = AtomicBool::new(true);

// ✅ Retry для chunk запросов (каждые 5 секунд)
pub static CHUNK_RETRY_PENDING: AtomicBool = AtomicBool::new(false);
pub static CHUNK_RETRY_ROW: AtomicI32 = AtomicI32::new(0);
pub static CHUNK_RETRY_DEADLINE_US: AtomicU32 = AtomicU32::new(0);
pub const CHUNK_RETRY_INTERVAL_US: u32 = 5_000_000; // 5 секунд между retry

// ✅ Общая высота паттерна (из ответа сервера)
pub static PATTERN_HEIGHT: AtomicI32 = AtomicI32::new(0);

//nvs - write state

pub const KNIT_NAMESPACE: &str = "knit";
pub const KEY_GLOBAL_ROW: &str = "global_row";
pub const KEY_CHUNK_START: &str = "chunk_start";
pub const KEY_ROWS_IN_CHUNK: &str = "rows_in_chunk";

pub static KNIT_NVS: LazyLock<Mutex<Option<EspNvs<NvsDefault>>>> = LazyLock::new(|| Mutex::new(None));


/// Одно срабатывание соленоида
#[derive(Debug, Clone, Copy)]
pub struct SolenoidHit {
    pub row: i32,        // глобальный номер ряда
    pub needle: i32,     // индекс иглы (0..width-1)
    pub actual_fire: bool, // факт: DOB сработал (true) или нет (false)
    pub direction: bool, // true = RIGHT
}

// Фиксированный буфер hits только для текущего ряда.
// Не держим весь массив в heap и не растём динамически по мере работы.
pub static CURRENT_ROW_HITS: LazyLock<Mutex<heapless::Vec<SolenoidHit, MAX_ROW_HITS>>> =
    LazyLock::new(|| Mutex::new(heapless::Vec::new()));

// Single-producer / single-consumer ring buffer for hot-path solenoid hits.
// No requeueing for mismatched rows. The producer is the ISR and the consumer is the task.
pub struct SolenoidHitRing {
    storage: UnsafeCell<[core::mem::MaybeUninit<SolenoidHit>; 256]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for SolenoidHitRing {}

impl SolenoidHitRing {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            storage: UnsafeCell::new(core::array::from_fn(|_| core::mem::MaybeUninit::uninit())),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    pub fn send_back(&self, value: SolenoidHit) -> Result<(), SolenoidHit> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if ((head + 1) % 256) == tail {
            return Err(value);
        }

        unsafe {
            let slot = &mut (*self.storage.get())[head];
            slot.write(value);
        }

        self.head.store((head + 1) % 256, Ordering::Release);
        Ok(())
    }

    #[inline(always)]
    pub fn recv_front(&self) -> Option<SolenoidHit> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail == head {
            return None;
        }

        let value = unsafe { (*self.storage.get())[tail].assume_init_read() };
        self.tail.store((tail + 1) % 256, Ordering::Release);
        Some(value)
    }
}

pub static SOLENOID_HITS: LazyLock<SolenoidHitRing> = LazyLock::new(SolenoidHitRing::new);