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
    let response_json = format!(
        r#"{{"ccp": "{}", "hok": "{}", "ksl": "{}", "nd1": "{}", "dob": "{}"}}"#,
        if ccp_state == 1 { "HIGH" } else { "LOW" },
        if hok_state == 1 { "HIGH" } else { "LOW" },
        if ksl_state == 1 { "HIGH" } else { "LOW" },
        if nd1_state == 1 { "HIGH" } else { "LOW" },
        if dob_state == 1 { "HIGH" } else { "LOW" }
    );
    response_json
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

        // // Включаем подтяжку для входов, чтобы избежать "плавающего" состояния
        esp_idf_sys::gpio_pullup_en(CCP);
        esp_idf_sys::gpio_pullup_en(HOK);
        esp_idf_sys::gpio_pullup_en(KSL);
        esp_idf_sys::gpio_pullup_en(ND1);
    }
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
    log("DEBUG", "DOB changed!");
    gpio_set_low(DOB);
    Ets::delay_us(3);
    gpio_set_high(DOB);
}
#[inline(always)]
pub fn on_ksl_change(level: bool) {
    if level {
        INSIDE_PATTERN.store(true, Ordering::Relaxed);
        log("DEBUG", "Entered pattern zone");

        let dir = DIR_RIGHT.load(Ordering::Relaxed);
        let width = WIDTH.load(Ordering::Relaxed);
        let half = (width / 2) as i32;

        if dir {
            // Движение вправо: вход с левой границы = игла +half
            NEEDLE.store(half, Ordering::Relaxed);
            log("DEBUG", <String as Clone>::clone(&(&format!("Needle reset to +{} (RIGHT)", half))).leak());
        } else {
            // Движение влево: вход с правой границы = игла -half
            NEEDLE.store(-half, Ordering::Relaxed);
            log("DEBUG", <String as Clone>::clone(&(&format!("Needle reset to {} (LEFT)", -half))).leak());
        }
    } else {
        INSIDE_PATTERN.store(false, Ordering::Relaxed);
        log("DEBUG", "Exited pattern zone");
    }
}

#[inline(always)]
pub fn on_ccp_tick_fast() {
    if !KNITTING.load(Ordering::Relaxed) || !INSIDE_PATTERN.load(Ordering::Relaxed) {
        return;
    }

    let needle = NEEDLE.load(Ordering::Relaxed);
    let row = ROW.load(Ordering::Relaxed);
    let width = WIDTH.load(Ordering::Relaxed);
    let half = (width / 2) as i32;

    // Конвертация с учётом отсутствия иглы "0"
    let pattern_index = if needle > 0 {
        half - needle
    } else {
        half - needle - 1  // прыжок через отсутствующий ноль
    };

    // Проверка границ и триггер
    if pattern_index >= 0 && pattern_index < width as i32 {
        if pattern_get(row, pattern_index as i32) {
            dob_fire_fast();
        }
    }

    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.fetch_sub(1, Ordering::Relaxed);  // уменьшаем
    } else {
        NEEDLE.fetch_add(1, Ordering::Relaxed);   // увеличиваем
    }
}

#[inline(always)]
pub fn on_hok_change_fast(level: bool) {
    DIR_RIGHT.store(level, Ordering::Relaxed);
    if level {
        log("DEBUG", "Direction updated to RIGHT");
        ROW.fetch_add(1, Ordering::Relaxed);
    } else {
        log("DEBUG", "Direction updated to LEFT");
        ROW.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn on_nd1_falling_fast() {
    if DIR_RIGHT.load(Ordering::Relaxed) && NEEDLE.load(Ordering::Relaxed) != -1 {
        NEEDLE.store(-1, Ordering::Relaxed);
        log("DEBUG", "ND1 falling edge detected, needle reset");
    }
}
