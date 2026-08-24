use crate::log_fmt;
use crate::logger::log;
use crate::pattern::pattern_get;
use crate::state::{self, *};
use crate::tasks::delay_us;
use esp_idf_sys::GPIO;
use std::sync::atomic::Ordering;

#[inline(always)]
fn dob_set_if_changed(new_state: bool) {
    let last = DOB_LAST_STATE.load(Ordering::Relaxed);
    if new_state != last {
        if new_state {
            dob_set_high_fast();
        } else {
            dob_set_low_fast();
        }
        DOB_LAST_STATE.store(new_state, Ordering::Relaxed);
    }
}

#[inline(always)]
fn dob_set_low_fast() {
    unsafe {
        // Прямая запись в регистр - быстрее чем gpio_set_level
        (esp_idf_sys::GPIO_OUT_W1TC_REG as *mut u32).write_volatile(1 << DOB);
        // log("INFO", "DOB: SOLENOID ON");
    }
}

#[inline(always)]
fn dob_set_high_fast() {
    unsafe {
        (esp_idf_sys::GPIO_OUT_W1TS_REG as *mut u32).write_volatile(1 << DOB);
    }
}

pub fn init_pins() {
    unsafe {
        esp_idf_sys::gpio_reset_pin(CCP);
        esp_idf_sys::gpio_reset_pin(HOK);
        esp_idf_sys::gpio_reset_pin(KSL);
        esp_idf_sys::gpio_reset_pin(ND1);
        esp_idf_sys::gpio_reset_pin(DOB);

        esp_idf_sys::gpio_set_direction(CCP, esp_idf_sys::gpio_mode_t_GPIO_MODE_INPUT);
        esp_idf_sys::gpio_set_direction(HOK, esp_idf_sys::gpio_mode_t_GPIO_MODE_INPUT);
        esp_idf_sys::gpio_set_direction(KSL, esp_idf_sys::gpio_mode_t_GPIO_MODE_INPUT);
        esp_idf_sys::gpio_set_direction(ND1, esp_idf_sys::gpio_mode_t_GPIO_MODE_INPUT);
        esp_idf_sys::gpio_set_direction(DOB, esp_idf_sys::gpio_mode_t_GPIO_MODE_OUTPUT);

        esp_idf_sys::gpio_set_level(DOB, 1);
        esp_idf_sys::gpio_pullup_en(CCP);
        esp_idf_sys::gpio_pullup_en(HOK);
        esp_idf_sys::gpio_pullup_en(KSL);
        esp_idf_sys::gpio_pullup_en(ND1);
    }
}

#[inline(always)]
pub fn on_ksl_change() {
    let ksl_state = unsafe { (GPIO.in_ >> KSL) & 0x1 } != 0;
    let old_inside = INSIDE_PATTERN.load(Ordering::Relaxed);

    log_fmt!(
        "DEBUG",
        "KSL CHANGE: {} -> {} | row={} global_row={} dir={} needle={}",
        old_inside,
        ksl_state,
        ROW.load(Ordering::Relaxed),
        GLOBAL_ROW.load(Ordering::Relaxed),
        DIR_RIGHT.load(Ordering::Relaxed),
        NEEDLE.load(Ordering::Relaxed)
    );

    if old_inside != ksl_state {
        INSIDE_PATTERN.store(ksl_state, Ordering::Relaxed);

        if ksl_state {
            let dir_right = DIR_RIGHT.load(Ordering::Acquire);
            let pattern_end = PATTERN_END.load(Ordering::Acquire);

            if dir_right {
                NEEDLE.store(0, Ordering::Release);
            } else {
                NEEDLE.store(pattern_end, Ordering::Release);
            }

            log_fmt!(
                "DEBUG",
                "KSL ENTER: row={} needle={} dir={}",
                GLOBAL_ROW.load(Ordering::Relaxed),
                NEEDLE.load(Ordering::Relaxed),
                DIR_RIGHT.load(Ordering::Relaxed)
            );

            ccp_filter_reset_on_ksl_rise();
        } else {
            let dir = DIR_RIGHT.load(Ordering::Relaxed);

            ROW_END_NEEDLE.store(NEEDLE.load(Ordering::Relaxed), Ordering::Relaxed);

            let completed_row = ROW.load(Ordering::Relaxed);

            crate::client::queue_row_info(completed_row, dir);

            let old_row = ROW.fetch_add(1, Ordering::SeqCst);
            let new_row = old_row + 1;

            GLOBAL_ROW.store(new_row, Ordering::Relaxed);

            log_fmt!(
                "INFO",
                "KSL EXIT: completed_row={} -> new_row={} dir={} needle={}",
                completed_row,
                new_row,
                dir,
                NEEDLE.load(Ordering::Relaxed)
            );

            crate::knit_state::save_progress(new_row);

            dob_set_if_changed(true);
        }
    }
}

#[inline(always)]
pub fn on_ccp_tick_fast() {
    let inside = INSIDE_PATTERN.load(Ordering::Relaxed);
    if inside {
        let needle = if DIR_RIGHT.load(Ordering::Relaxed) {
            NEEDLE.fetch_add(1, Ordering::Relaxed)
        } else {
            NEEDLE.fetch_sub(1, Ordering::Relaxed)
        };

        if needle < 0 || needle > PATTERN_END.load(Ordering::Relaxed) {
            dob_set_high_fast();
            return;
        }
        let row = GLOBAL_ROW.load(Ordering::Relaxed);
        let should_fire = pattern_get(row, needle);
        dob_set_if_changed(should_fire);
    } else {
        dob_set_high_fast();
    }
}

#[inline(always)]
pub fn on_hok_change_fast(level: bool) {
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let last_dir_change = state::HOK_DIR_CHANGE_DEBOUNCE_US.load(Ordering::Relaxed);
    if last_dir_change > 0 && now_us - last_dir_change < state::MIN_DIR_CHANGE_INTERVAL_US {
        return;
    }
    state::HOK_DIR_CHANGE_DEBOUNCE_US.store(now_us, Ordering::Relaxed);
    let dir = level;
    DIR_RIGHT.store(dir, Ordering::Relaxed);
    if dir {
        log("DEBUG", "HOK: direction RIGHT");
    } else {
        log("DEBUG", "HOK: direction LEFT");
    }
}

pub fn on_nd1_falling_fast() {
    let width = PATTERN_END.load(Ordering::Relaxed) as i32;
    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.store(0, Ordering::Relaxed);
        log("DEBUG", "ND1: reset needle to 0 (RIGHT)");
    } else {
        NEEDLE.store(width, Ordering::Relaxed);
        log_fmt!("DEBUG", "ND1: reset needle to {} (LEFT)", width - 1);
    }
    dob_set_high_fast();
}

pub fn handshake() {
    send_byte_sync(b'P');
    send_byte_sync(b'A');
    send_byte_sync(0);
    send_byte_sync(0);
}

fn send_byte_sync(byte: u8) {
    dob_set_high_fast();
    for i in 0..8 {
        let bit = (byte >> i) & 1 != 0;
        send_bit(bit);
    }
    dob_set_high_fast();
}

fn send_bit(bit: bool) {
    if !bit {
        dob_set_high_fast();
    } else {
        dob_set_low_fast();
    }
    delay_us(FREQUENCY_SILVER_REED);
}
