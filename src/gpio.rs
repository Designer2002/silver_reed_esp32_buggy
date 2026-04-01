use crate::logger::log;
use crate::pattern::pattern_get;
use crate::state::*;
use crate::tasks::delay_us;
use esp_idf_sys::GPIO;
use std::sync::atomic::Ordering;

#[inline(always)]
fn dob_set_if_changed(new_state: bool) {
    let last = DOB_LAST_STATE.load(Ordering::Relaxed);

    // Пишем в регистр ТОЛЬКО если состояние изменилось
    if new_state != last {
        if new_state {
            dob_set_high_fast();
        } else {
            dob_set_low_fast();
        }
        DOB_LAST_STATE.store(new_state, Ordering::Relaxed);
    }
}
// ✅ Для ESP32 можно использовать прямые регистры для максимальной скорости
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

#[inline(always)]
fn is_ccp_before_ksl(ccp_seq: u32, ksl_seq: u32) -> bool {
    ccp_seq < ksl_seq
}

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

        // ✅ DOB по умолчанию HIGH (соленоид выключен)
        esp_idf_sys::gpio_set_level(DOB, 1);

        esp_idf_sys::gpio_pullup_en(CCP);
        esp_idf_sys::gpio_pullup_en(HOK);
        esp_idf_sys::gpio_pullup_en(KSL);
        esp_idf_sys::gpio_pullup_en(ND1);
    }
}

#[inline(always)]
pub fn on_ksl_change(_seq: u32) {
    // ✅ Читаем АКТУАЛЬНОЕ состояние KSL прямо из регистра
    // Это важно потому что ISR мог отфильтровать дребезг
    let ksl_state = unsafe { (GPIO.in_ >> KSL) & 0x1 } != 0;
    
    let old_inside = INSIDE_PATTERN.load(Ordering::Relaxed);
    
    // ✅ Обрабатываем только если состояние действительно изменилось
    if old_inside != ksl_state {
        INSIDE_PATTERN.store(ksl_state, Ordering::Relaxed);
        
        if ksl_state {
            // Вход в зону паттерна (KSL rise: false → true)
            if DIR_RIGHT.load(Ordering::Relaxed) {
                NEEDLE.store(PATTERN_START.load(Ordering::Relaxed), Ordering::Relaxed);
            } else {
                NEEDLE.store(PATTERN_END.load(Ordering::Relaxed), Ordering::Relaxed);
            }
        } else {
            // Выход из зоны паттерна (KSL fall: true → false)
            if DIR_RIGHT.load(Ordering::Relaxed) {
                PATTERN_END.store(
                    NEEDLE.load(Ordering::Relaxed).saturating_sub(1),
                    Ordering::Relaxed,
                );
            } else {
                PATTERN_START.store(
                    0,
                    Ordering::Relaxed,
                );
            }
            
            ROW.fetch_add(1, Ordering::SeqCst);    
            dob_set_high_fast();
        }
    }
}

#[inline(always)]
pub fn on_ccp_tick_fast(ccp_seq: u32) {
    // ✅ Проверяем INSIDE_PATTERN здесь, до обработки
    let inside = INSIDE_PATTERN.load(Ordering::Relaxed);
    let ksl_seq = LAST_KSL_SEQUENCE.load(Ordering::SeqCst);
    
    // ✅ Игнорируем CCP если он был ДО или во время последнего KSL fall
    // Это "хвостовые" тики от предыдущего ряда
    if ccp_seq <= ksl_seq {
        dob_set_high_fast();
        return;
    }
    
    if inside {
        // ✅ CCP тик — это уже rising edge, просто считаем иглу
        if DIR_RIGHT.load(Ordering::Relaxed) {
            NEEDLE.fetch_add(1, Ordering::Relaxed);
        } else {
            NEEDLE.fetch_sub(1, Ordering::Relaxed);
        }
        let row = ROW.load(Ordering::Relaxed);
        let needle = NEEDLE.load(Ordering::Relaxed);
        
        // ✅ Инвертируем полярность DOB для правильного контраста
        // . (точка) = фон, # (решетка) = узор
        let should_fire = pattern_get(row, needle);
        if !should_fire {
            dob_set_low_fast(); // Соленоид включается для узора
        } else {
            dob_set_high_fast(); // Соленоид выключен для фона
        }
    } else {
        dob_set_high_fast();
    }
}

#[inline(always)]
pub fn on_hok_change_fast(level: bool) {
    // Пропускаем, если уровень не изменился (защита от дребезга)
    let last = HOK_LAST_LEVEL.load(Ordering::Relaxed);
    if level == last {
        return;
    }
    HOK_LAST_LEVEL.store(level, Ordering::Relaxed);

    let dir = level;
    DIR_RIGHT.store(dir, Ordering::Relaxed);

    if dir {
        log("DEBUG", "HOK: direction RIGHT");
    } else {
        log("DEBUG", "HOK: direction LEFT");
    }
}

pub fn on_nd1_falling_fast() {
    let width = WIDTH.load(Ordering::Relaxed) as i32;

    if DIR_RIGHT.load(Ordering::Relaxed) {
        NEEDLE.store(0, Ordering::Relaxed);
        log("DEBUG", "ND1: reset needle to 0 (RIGHT)");
    } else {
        NEEDLE.store(width - 1, Ordering::Relaxed);
        let msg = format!("ND1: reset needle to {} (LEFT)", width - 1).leak();
        log("DEBUG", msg);
    }

    // ✅ На ND1 тоже выключаем соленоид для безопасности
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
