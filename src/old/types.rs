use std::sync::{Mutex, Arc};
use esp_idf_hal::gpio::{PinDriver, AnyIOPin, AnyOutputPin, Output, Input};

#[derive(Clone, Copy)]
pub struct LogEntry {
    pub ts: u32,
    pub level: u8,
    pub msg: [u8; 256],
}

// --- ПАТТЕРН ---
#[derive(Clone)]
pub struct KnitPattern {
    pub rows: Vec<Vec<bool>>,
    pub width: usize,
    pub height: usize,
}

pub struct SilverLinkEmulator {
    pub pattern: KnitPattern,
    pub ccp: PinDriver<'static, AnyIOPin, Input>,
    pub hok: PinDriver<'static, AnyIOPin, Input>,
    pub ksl: PinDriver<'static, AnyIOPin, Input>,
    pub nd1: PinDriver<'static, AnyIOPin, Input>,
    pub dob: Arc<Mutex<PinDriver<'static, AnyOutputPin, Output>>>,
}