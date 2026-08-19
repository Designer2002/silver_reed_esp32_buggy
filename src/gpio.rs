use crate::logger::log;
use crate::pattern::pattern_get;
use crate::state::{self, *};
use crate::tasks::delay_us;
use esp_idf_sys::{GPIO, TICKS_PER_US_ROM};
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
            ROW_START_NEEDLE.store(NEEDLE.load(Ordering::Relaxed), Ordering::Relaxed);
            let rows = ROWS_IN_CURRENT_CHUNK.load(Ordering::Relaxed);
            let dir = DIR_RIGHT.load(Ordering::Relaxed);
            let needle = ROW_START_NEEDLE.load(Ordering::Relaxed);
            let msg = format!("KSL RISE: needle={}, rows_in_chunk={}, dir={}", needle, rows, if dir { "RIGHT" } else { "LEFT" });
            log("DEBUG", &msg);
            drop(msg);

            // ✅ Сбрасываем CCP фильтр при входе в паттерн
            ccp_filter_reset_on_ksl_rise();
            
            // ✅ Запрашиваем отправку информации о ряде на сервер (без HTTP!)
            crate::client::queue_row_info(ROW.load(Ordering::Relaxed), dir);
        } else {
            let dir = DIR_RIGHT.load(Ordering::Relaxed);
            ROW_END_NEEDLE.store(NEEDLE.load(Ordering::Relaxed), Ordering::Relaxed);
            // Выход из зоны паттерна (KSL fall: true → false)
            // ✅ НЕ обновляем PATTERN_START/END каждый ряд — они фиксированные!
            // Границы задаются один раз при start_knitting и не меняются
            // Это предотвращает "уплывание" узора из-за механического люфта
            let needle = ROW_END_NEEDLE.load(Ordering::Relaxed);
            let msg = format!("KSL FALL: needle={}, dir={}, PATTERN_START={}, PATTERN_END={}", 
                needle, if dir { "RIGHT" } else { "LEFT" },
                PATTERN_START.load(Ordering::Relaxed),
                PATTERN_END.load(Ordering::Relaxed));
            log("DEBUG", &msg);
            drop(msg);

            // Смена ряда
            let old_row = ROW.fetch_add(1, Ordering::SeqCst);
            let new_row = old_row + 1;
            
            // Обновляем глобальный счетчик рядов
            GLOBAL_ROW.store(new_row, Ordering::Relaxed);
            let msg = format!("В ряду {} игла {} → {}", new_row, ROW_START_NEEDLE.load(Ordering::Relaxed), ROW_END_NEEDLE.load(Ordering::Relaxed));
            log("INFO", &msg);
            drop(msg);
            // Считаем ряды в текущем чанке
            let rows_in_chunk = ROWS_IN_CURRENT_CHUNK.fetch_add(1, Ordering::Relaxed) + 1;

            // Если это 4-й ряд в чанке (rows_in_chunk == 4), ставим флаг запроса
            // Так мы успеем загрузить следующий чанк пока вяжем 4-й ряд
            if rows_in_chunk == 4 {
                REQUEST_NEW_CHUNK.store(true, Ordering::Relaxed);
            }
            
            // Если достигли конца чанка (4 ряда), сбрасываем счетчик
            if rows_in_chunk >= CHUNK_SIZE as i32 {
                ROWS_IN_CURRENT_CHUNK.store(0, Ordering::Relaxed);
            }

            // ✅ Сохраняем прогресс в NVS
            let gr = GLOBAL_ROW.load(Ordering::Relaxed);
            let cs = CURRENT_CHUNK_START_ROW.load(Ordering::Relaxed);
            let ric = ROWS_IN_CURRENT_CHUNK.load(Ordering::Relaxed);
            crate::knit_state::save_progress(gr, cs, ric);

            dob_set_if_changed(true); 
        }
    }
}

#[inline(always)]
pub fn on_ccp_tick_fast(ccp_seq: u32) {
    let inside = INSIDE_PATTERN.load(Ordering::Relaxed);
    let ksl_seq = LAST_KSL_SEQUENCE.load(Ordering::SeqCst);

    if ccp_seq <= ksl_seq {
        dob_set_high_fast();
        return;
    }

    if inside {
        let needle = NEEDLE.load(Ordering::Relaxed);
        if DIR_RIGHT.load(Ordering::Relaxed) {
            NEEDLE.fetch_add(1, Ordering::Relaxed);
        } else {
            NEEDLE.fetch_sub(1, Ordering::Relaxed);
        }

        let rows_in_chunk = ROWS_IN_CURRENT_CHUNK.load(Ordering::Relaxed);
        let local_row = rows_in_chunk % CHUNK_SIZE as i32;
        let should_fire = pattern_get(local_row, needle);
        
        dob_set_if_changed(should_fire);
        
        // ✅ Читаем новое состояние DOB после установки
        let actual_fire = unsafe { (esp_idf_sys::GPIO.out >> DOB) & 0x1 } == 0; // LOW = соленоид ON
        
        // ✅ Записываем в очередь (lock-free, безопасно из ISR)
        let hit = crate::state::SolenoidHit {
            row: ROW.load(Ordering::Relaxed),
            needle,
            actual_fire,
            direction: DIR_RIGHT.load(Ordering::Relaxed),
        };
        let _ = crate::state::SOLENOID_HITS.send_back(hit, TICKS_PER_US_ROM * 1000); // таймаут 1 мс, чтобы не блокировать ISR
    } else {
        dob_set_high_fast();
    }
}

#[inline(always)]
pub fn on_hok_change_fast(level: bool) {
    // ✅ Защита от подёргивания: проверяем что прошло достаточно времени
    // с последней смены направления
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let last_dir_change = state::HOK_DIR_CHANGE_DEBOUNCE_US.load(Ordering::Relaxed);
    if last_dir_change > 0 && now_us - last_dir_change < state::MIN_DIR_CHANGE_INTERVAL_US {
        // 🚫 Подёргивание — игнорируем смену направления
        return;
    }
    state::HOK_DIR_CHANGE_DEBOUNCE_US.store(now_us, Ordering::Relaxed);

    // ✅ Debounce уже сделан в ISR, здесь просто обрабатываем направление
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
        let msg = format!("ND1: reset needle to {} (LEFT)", width - 1);
        log("DEBUG", &msg);
        drop(msg);
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
