use std::sync::{LazyLock};
pub const KNITTING_PATTERN: &str = include_str!("pat.txt"); 
pub static PATTERN: LazyLock<KnitPattern> = LazyLock::new(|| parse_pattern(KNITTING_PATTERN));

pub struct KnitPattern {
    pub rows: Vec<Vec<bool>>,
    pub width: usize,
    pub height: usize,
}

pub fn pattern_get(row: i32, needle: i32) -> bool {
    if row < 0 || needle < 0 {
        return false;
    }
    let pattern = &*PATTERN;
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
                .map(|c| c == '#' || c == '@' || c == 'X' || c == 'x')
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