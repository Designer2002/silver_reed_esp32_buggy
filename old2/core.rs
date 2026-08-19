use core::sync::atomic::Ordering;
use crate::state::*;
use crate::pattern::PATTERN;
use crate::gpio::dob_fire_fast;

#[inline(always)]
pub fn on_ccp_tick() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    if !INSIDE_PATTERN.load(Ordering::Relaxed) {
        return;
    }

    let dir = DIR_RIGHT.load(Ordering::Relaxed);

    let needle = if dir {
        NEEDLE.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        NEEDLE.fetch_sub(1, Ordering::Relaxed) - 1
    };

    let width = WIDTH.load(Ordering::Relaxed) as i32;
    if needle < 0 || needle >= width {
        return;
    }

    let row = ROW.load(Ordering::Relaxed);
    let col = if dir {
        needle as usize
    } else {
        (width as usize - 1) - needle as usize
    };

    if PATTERN.rows[row][col] {
        dob_fire_fast();
    }
}

#[inline(always)]
pub fn on_hok_change(level: bool) {
    DIR_RIGHT.store(level, Ordering::Relaxed);
}
pub fn on_nd1_falling() {
    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.store(-1, Ordering::Relaxed);
    }
}

#[inline(always)]
pub fn on_ksl_change(level: bool) {
    let dir = DIR_RIGHT.load(Ordering::Relaxed);

    if level {
        // вошли в узор
        INSIDE_PATTERN.store(true, Ordering::Relaxed);

        let width = WIDTH.load(Ordering::Relaxed) as i32;

        if dir {
            NEEDLE.store(-1, Ordering::Relaxed);
        } else {
            NEEDLE.store(width, Ordering::Relaxed);
        }

    } else {
        // вышли из узора = новая строка
        INSIDE_PATTERN.store(false, Ordering::Relaxed);
        ROW.fetch_add(1, Ordering::Relaxed);
    }

    
}

