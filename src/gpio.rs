use esp_idf_hal::delay::Ets;
use esp_idf_sys::esp_timer_get_time;

use crate::logger::log;
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
        // esp_idf_sys::gpio_pullup_en(KSL);
        //esp_idf_sys::gpio_pullup_en(ND1);
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
pub fn on_ccp_tick_fast() {
    if !KNITTING.load(Ordering::Relaxed) {
        log("ERROR", "CCP tick ignored because knitting is not active");
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
    let now = unsafe { esp_timer_get_time() } as u64;

    unsafe {
        if now - LAST_HOK < 2000 {
            return; // игнорируем дребезг
        }
        LAST_HOK = now;
    }
    //инверсия так как оптопара 6n137 инвертирует выход
    DIR_RIGHT.store(!level, Ordering::Relaxed);
    let lvl_static: &'static str = if level { "HIGH" } else { "LOW" };
    log("DEBUG", Box::leak(format!("HOK change detected, direction updated to {}", lvl_static).into_boxed_str()));
}

static mut LAST_ND1: u64 = 0;
static mut LAST_KSL: u64 = 0;
static mut LAST_HOK: u64 = 0;

pub fn on_nd1_falling_fast() {
    let now = unsafe { esp_timer_get_time() } as u64;

    unsafe {
        if now - LAST_ND1 < 2000 {
            return; // игнорируем дребезг
        }
        LAST_ND1 = now;
    }

    if DIR_RIGHT.load(Ordering::Relaxed) && NEEDLE.load(Ordering::Relaxed) != -1 {
        NEEDLE.store(-1, Ordering::Relaxed);
        log("DEBUG", "ND1 falling edge detected, needle reset");
    }

    
}

pub fn on_ksl_change(level: bool) {
    let now = unsafe { esp_timer_get_time() } as u64;
    unsafe {
        if now - LAST_KSL < 2000 {
            return; // игнорируем дребезг
        }
        LAST_KSL = now;
    }
    let dir = DIR_RIGHT.load(Ordering::Relaxed);

    //инверсия так как оптопара 6n137 инвертирует выход
    if !level {
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
