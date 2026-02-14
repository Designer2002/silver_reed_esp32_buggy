use crate::logger::log;
use esp_idf_hal::delay::Ets;
use std::sync::atomic::Ordering;

use crate::pattern::pattern_get;
use crate::state::*;

pub fn get_pin_state_json() -> String {
    let mut ccp_state = 0;
    let mut hok_state = 0;
    let mut ksl_state = 0;
    let mut nd1_state = 0;
    let mut dob_state = 0;
    unsafe {
     ccp_state = esp_idf_sys::gpio_get_level(CCP);
     hok_state = esp_idf_sys::gpio_get_level(HOK);
     ksl_state = esp_idf_sys::gpio_get_level(KSL);
     nd1_state = esp_idf_sys::gpio_get_level(ND1);
     dob_state = esp_idf_sys::gpio_get_level(DOB);
}
        let response_json = format!(r#"{{"ccp": "{}", "hok": "{}", "ksl": "{}", "nd1": "{}", "dob": "{}"}}"#,
        if ccp_state == 1 { "HIGH" } else { "LOW" },
        if hok_state == 1 { "HIGH" } else { "LOW" },
        if ksl_state == 1 { "HIGH" } else { "LOW" },
        if nd1_state == 1 { "HIGH" } else { "LOW" },
        if dob_state == 1 { "HIGH" } else { "LOW" }
);
    response_json
}

pub fn gpio_set_low(pin: i32) {
    unsafe {
        esp_idf_sys::gpio_set_level(pin, 0);
    }
}

pub fn gpio_set_high(pin: i32) {
    unsafe {
        esp_idf_sys::gpio_set_level(pin, 1);
    }
}

#[inline(always)]
pub fn dob_fire_fast() {
    gpio_set_low(DOB);
    Ets::delay_us(3);
    gpio_set_high(DOB);
}

#[inline(always)]
pub fn on_ccp_tick_fast() {
    if !KNITTING.load(Ordering::Relaxed) {
        return;
    }

    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.fetch_add(1, Ordering::Relaxed);
    } else {
        NEEDLE.fetch_sub(1, Ordering::Relaxed);
    }

    if INSIDE_PATTERN.load(Ordering::Relaxed) {
        let row = ROW.load(Ordering::Relaxed);
        let needle = NEEDLE.load(Ordering::Relaxed);

        if pattern_get(row, needle) {
            dob_fire_fast();
        }
    }
}

pub fn on_hok_change_fast(level: bool) {
    DIR_RIGHT.store(level, Ordering::Relaxed);
}

pub fn on_nd1_falling_fast() {
    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.store(-1, Ordering::Relaxed);
    }
    log("DEBUG", "ND1 falling edge detected, needle reset");
}

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
