use std::sync::{LazyLock, Mutex};
use crate::state::{CHUNK_SIZE, CURRENT_CHUNK_START_ROW, NEXT_CHUNK_START_ROW, ROWS_IN_CURRENT_CHUNK};

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

// Быстрый доступ без блокировки для чтения (используем unsafe для производительности в ISR)
// В ISR нельзя использовать Mutex, поэтому pattern_get вызывается только из engine_task
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

pub fn parse_pattern(pattern: &str) -> KnitPattern {
    let rows: Vec<Vec<bool>> = pattern
        .lines()
        .map(|line| {
            line.chars()
                .filter(|&c| c != '\\' && c != '\n' && c != 'n')
                .map(|c| c == '1')
                .collect()
        })
        .collect();

    let height = rows.len();
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(0);

    KnitPattern {
        rows,
        width,
        height,
    }
}

// Сохраняем incoming chunk во временное NEXT_CHUNK_ROWS.
// Активный PATTERN не меняется до явного swap после завершения текущего ряда.
pub fn store_next_chunk(new_rows: Vec<Vec<bool>>, start_row: usize, width: usize) {
    let mut next = crate::state::NEXT_CHUNK_ROWS.lock().unwrap();
    NEXT_CHUNK_START_ROW.store(start_row as i32, std::sync::atomic::Ordering::Release);
    *next = Some(new_rows);
    let _ = width;
}

pub fn swap_to_next_chunk() -> bool {
    let mut next = crate::state::NEXT_CHUNK_ROWS.lock().unwrap();
    let Some(next_rows) = next.take() else {
        return false;
    };

    let width = next_rows.first().map(|row| row.len()).unwrap_or(0);
    if width == 0 {
        return false;
    }

    let start_row = NEXT_CHUNK_START_ROW.load(std::sync::atomic::Ordering::Acquire);
    let mut pattern = PATTERN.lock().unwrap();
    pattern.width = width;
    pattern.height = next_rows.len();
    pattern.rows = next_rows;
    drop(pattern);

    CURRENT_CHUNK_START_ROW.store(start_row, std::sync::atomic::Ordering::Relaxed);
    ROWS_IN_CURRENT_CHUNK.store(0, std::sync::atomic::Ordering::Relaxed);
    return true;
}

// Обновление только части паттерна (чанка) - для потоковой загрузки.
// Оставляем эту функцию как совместимый helper, но основной путь — через NEXT_CHUNK_ROWS + swap_to_next_chunk().
pub fn update_pattern_chunk(new_rows: Vec<Vec<bool>>, _start_row: usize, width: usize) {
    let mut pattern = PATTERN.lock().unwrap();
    if pattern.rows.is_empty() || pattern.width != width {
        pattern.width = width;
        pattern.height = CHUNK_SIZE;
        pattern.rows = vec![vec![false; width]; CHUNK_SIZE];
    }

    for (i, row) in new_rows.into_iter().enumerate() {
        if i < pattern.rows.len() {
            pattern.rows[i] = row;
        }
    }

    pattern.height = CHUNK_SIZE;
}

// Получить высоту полного паттерна (для определения когда запрашивать следующий чанк)
pub fn get_pattern_height() -> usize {
    let pattern = PATTERN.lock().unwrap();
    pattern.height
}
