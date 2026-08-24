use std::sync::{LazyLock, Mutex};

use crate::logger::log;

pub struct KnitPattern {
    pub rows: Vec<Vec<bool>>,
    pub width: usize,
    pub height: usize,
}

// Глобальный изменяемый паттерн
pub static PATTERN: LazyLock<Mutex<KnitPattern>> = LazyLock::new(|| {
    Mutex::new(KnitPattern {
        rows: Vec::new(),
        width: 0,
        height: 0,
    })
});

// Быстрый доступ для чтения (вызывается из engine_task, не из ISR)
pub fn pattern_get(row: i32, needle: i32) -> bool {
    let pattern = PATTERN.lock().unwrap();
    if row < 0 || needle < 0 {
        return false;
    }
    if row >= pattern.height as i32 || needle >= pattern.width as i32 {
        return false;
    }
    pattern.rows[row as usize][needle as usize]
}

pub fn replace_pattern(rows: Vec<Vec<bool>>, width: usize, height: usize) {
    if width == 0 || height == 0 {
        log("ERROR", "Invalid data!");
        return;
    }
    if width > 200 {
        crate::logger::log("ERROR", "Pattern is wider than 200 needles, rejected");
        return;
    }

    let mut pattern = PATTERN.lock().unwrap();
    pattern.width = width;
    pattern.height = height;
    pattern.rows = rows;
}