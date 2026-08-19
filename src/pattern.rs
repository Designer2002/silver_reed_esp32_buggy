use std::sync::{LazyLock, Mutex};
use crate::state::CHUNK_SIZE;

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

// Обновление только части паттерна (чанка) - для потоковой загрузки
// new_rows: массив из 4 рядов, start_row: номер ряда, с которого начинается чанк (ГЛОБАЛЬНЫЙ)
// но внутри паттерна ряды сохраняются как local_index = global_index % CHUNK_SIZE
pub fn update_pattern_chunk(new_rows: Vec<Vec<bool>>, start_row: usize, width: usize) {
    let mut pattern = PATTERN.lock().unwrap();
    let chunk_height = new_rows.len();

    // Убеждаемся, что паттерн имеет достаточный размер для CHUNK_SIZE рядов
    if pattern.rows.is_empty() || pattern.width != width {
        pattern.width = width;
        pattern.height = CHUNK_SIZE; // всегда держим CHUNK_SIZE рядов в памяти
        pattern.rows = vec![vec![false; width]; CHUNK_SIZE];
    }

    // Обновляем ряды по ЛОКАЛЬНЫМ индексам (0-3)
    for (i, row) in new_rows.into_iter().enumerate() {
        let local_row_idx = i; // 0, 1, 2, 3 — локальный индекс внутри чанка
        if local_row_idx < pattern.rows.len() {
            pattern.rows[local_row_idx] = row;
        }
    }

    pattern.height = CHUNK_SIZE;
}

// Получить высоту полного паттерна (для определения когда запрашивать следующий чанк)
pub fn get_pattern_height() -> usize {
    let pattern = PATTERN.lock().unwrap();
    pattern.height
}
