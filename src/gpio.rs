use esp_idf_hal::delay::Ets;
use esp_idf_sys::{esp_timer_create, esp_timer_create_args_t, esp_timer_dispatch_t_ESP_TIMER_TASK, esp_timer_get_time, esp_timer_handle_t, gpio_get_level, gpio_mode_t_GPIO_MODE_INPUT, gpio_num_t, gpio_pull_mode_t_GPIO_FLOATING, gpio_pull_mode_t_GPIO_PULLDOWN_ONLY, gpio_pull_mode_t_GPIO_PULLUP_ONLY, gpio_reset_pin, gpio_set_direction, gpio_set_pull_mode};

use crate::logger::log;
use crate::queue::{EVT_ND1, QUEUE};
use std::ffi::CString;
use std::ptr;
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
        // esp_idf_sys::gpio_pullup_en(CCP);
        // esp_idf_sys::gpio_pullup_en(HOK);
        // esp_idf_sys::gpio_pullup_en(KSL);
        // esp_idf_sys::gpio_pullup_en(ND1);
    }
}

// pub unsafe fn init_gpio(
//     gpio: gpio_num_t,
//     active_level: bool,
//     int_pull_enabled: bool,
// ) {
//     gpio_reset_pin(gpio);
//     gpio_set_direction(gpio, gpio_mode_t_GPIO_MODE_INPUT);

//     if int_pull_enabled {
//         if active_level {
//             gpio_set_pull_mode(gpio, gpio_pull_mode_t_GPIO_PULLDOWN_ONLY);
//         } else {
//             gpio_set_pull_mode(gpio, gpio_pull_mode_t_GPIO_PULLUP_ONLY);
//         }
//     } else {
//         gpio_set_pull_mode(gpio, gpio_pull_mode_t_GPIO_FLOATING);
//     }
// }


// pub static mut TIMER: esp_timer_handle_t = ptr::null_mut();
// pub unsafe fn create_debounce_timer(arg: *mut core::ffi::c_void) {
//     if TIMER.is_null() {
//         let name = CString::new("debounce").unwrap();

//         let mut cfg = esp_timer_create_args_t {
//             callback: Some(debounce_timeout),
//             arg,
//             dispatch_method: esp_timer_dispatch_t_ESP_TIMER_TASK,
//             name: name.as_ptr(),
//             skip_unhandled_events: false,
//         };

//         esp_timer_create(&mut cfg, &mut TIMER);
//     }
// }

// pub extern "C" fn debounce_timeout(arg: *mut core::ffi::c_void){
//     let _ = QUEUE.send_front(EVT_ND1, 1u32);
// }

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

pub fn read_pin_strong_high(pin: i32) -> bool {
    let mut count = 0;

    for _ in 0..5 {
        let level = unsafe { esp_idf_sys::gpio_get_level(pin) };
        if level == 1 {
            count += 1;
        }
        esp_idf_hal::delay::Ets::delay_us(2);
    }

    count >= 4
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
    log("DEBUG", "CCP TICK!");
    if !KNITTING.load(Ordering::Relaxed) {
        log("ERROR", "CCP tick ignored because knitting is not active");
        return;
    }

    // считаем иглы
    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.fetch_add(1, Ordering::Relaxed);
    } else {
        NEEDLE.fetch_sub(1, Ordering::Relaxed);
    }

    // паттерн работает только внутри зоны
    if INSIDE_PATTERN.load(Ordering::Relaxed) {
        let row = ROW.load(Ordering::Relaxed);
        let needle = NEEDLE.load(Ordering::Relaxed);

        if pattern_get(row, needle) {
            dob_fire_fast();
        }
    }
}

pub fn on_hok_change_fast(level: bool) {
    //инверсия так как оптопара 6n137 инвертирует выход
    DIR_RIGHT.store(!level, Ordering::Relaxed);
    if !level{
        log("DEBUG", "Direction changed to RIGHT!");
    }
    else {
         log("DEBUG", "Direction changed to LEFT!");
    }
}

pub fn on_nd1_falling_fast() {

    if unsafe { gpio_get_level(KSL) == 1} {
        log("ERROR", "ND1 shouldn't be high, why?")
    }
}

pub fn on_ksl_change(level: bool) {
    let inside = !level; // оптопара

    let was_inside = WAS_INSIDE.load(Ordering::Relaxed);

    if inside && !was_inside {
        log("DEBUG", "Entered pattern zone");
    }

    if !inside && was_inside {
        log("DEBUG", "Pattern zone ended -> row++");

        ROW.fetch_add(1, Ordering::Relaxed);

        if DIR_RIGHT.load(Ordering::Relaxed) {
            NEEDLE.store(-1, Ordering::Relaxed);
        } else {
            NEEDLE.store(WIDTH.load(Ordering::Relaxed) as i32, Ordering::Relaxed);
        }
    }

    WAS_INSIDE.store(inside, Ordering::Relaxed);
}
