use crate::client::{create_client, request_pattern_snapshot};
use crate::gpio::handshake;
use crate::pattern::PATTERN;
use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::state::{
    GLOBAL_ROW, HOK, PATTERN_END, PATTERN_LOADED, PATTERN_START, ROW, US_PER_MS,
};
use crate::{
    gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast},
    isr::install_isrs,
    logger::{log, pop_log},
    state::{self},
};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_sys::esp_http_client;
use log::info;
use std::ffi::c_void;
use std::sync::atomic::Ordering;

pub extern "C" fn engine_task(_: *mut c_void) {
    info!("Engine task started");

    loop {
        let evt: Option<(u8, bool)> = QUEUE.recv_front(1u32);

        if let Some((signal, _)) = evt {
            if signal == EVT_CCP {
                on_ccp_tick_fast();
            } else if signal == EVT_ND1 {
                on_nd1_falling_fast();
            } else if signal == EVT_KSL {
                on_ksl_change();
            } else if signal == EVT_HOK {
                let level = unsafe { esp_idf_sys::gpio_get_level(HOK) };
                on_hok_change_fast(level == 1);
            }
        }
    }
}

pub extern "C" fn client_task(_: *mut c_void) {
    info!("Client started - full pattern loader");
    let client = create_client();
    start_knitting(client);
    
    loop {
        // ✅ Запрашиваем полный паттерн, если вязание началось, но паттерн ещё не загружен
        if state::KNITTING.load(Ordering::Relaxed) && !state::PATTERN_LOADED.load(Ordering::Relaxed) {
            match request_pattern_snapshot(client) {
                Ok(true) => {
                    state::PATTERN_LOADED.store(true, Ordering::Relaxed);
                    log("INFO", "✅ Узор успешно загружен в память ESP32!");
                }
                Ok(false) => {
                    log("WARN", "⚠️ Узор не был загружен");
                }
                Err(e) => {
                    log("ERROR", &format!("❌ Ошибка загрузки паттерна:\n{:#}", e));
                    FreeRtos::delay_ms(1000);
                }
            }
        }

        if state::ROW_INFO_PENDING.load(Ordering::Acquire) {
            let row = state::ROW_INFO_ROW.load(Ordering::Relaxed);
            let dir = state::ROW_INFO_DIR.load(Ordering::Relaxed);
            if crate::client::send_queued_row_info(client) {
                let msg = format!(
                    "Row info sent: row={}, dir={}",
                    row,
                    if dir { "RIGHT" } else { "LEFT" }
                );
                log("DEBUG", &msg);
                drop(msg);
            } else {
                log("WARN", "Failed to send row info");
            }
            state::ROW_INFO_PENDING.store(false, Ordering::Release);
        }
        FreeRtos::delay_ms(10);
    }
}

pub extern "C" fn logger_task(_: *mut c_void) {
    info!("Logger task started");
    loop {
        let mut had_logs = false;
        while let Some(entry) = pop_log() {
            had_logs = true;
            println!("[{}] {}: {}", entry.timestamp, entry.level, entry.message);
        }
        if !had_logs {
            FreeRtos::delay_ms(20);
        } else {
            FreeRtos::delay_ms(2);
        }
    }
}

pub fn init_knitter() {
    log("INFO", "Initializing knitter...");
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub fn start_knitting(client: *mut esp_http_client) {
    log("INFO", "Starting knitting...");
    install_isrs();
    state::KNITTING.store(true, std::sync::atomic::Ordering::Relaxed);
    handshake();
    
    state::HOK_DIR_CHANGE_DEBOUNCE_US.store(0, Ordering::Relaxed);
    
    // ✅ Сначала проверяем restart флаг от сервера
    crate::client::check_if_restart(client);
    
    // ✅ Если нет сохранённого прогресса (или был restart) — начинаем с нуля
    if crate::knit_state::restore_progress().is_none() {
        PATTERN_START.store(0, Ordering::Relaxed);
        PATTERN_END.store(
            PATTERN.lock().unwrap().width.saturating_sub(1) as i32,
            Ordering::Relaxed,
        );
        ROW.store(0, Ordering::Relaxed);
        GLOBAL_ROW.store(0, Ordering::Relaxed);
        
        // ✅ УДАЛЕНО: Сброс CURRENT_CHUNK_START_ROW и ROWS_IN_CURRENT_CHUNK
    }
    
    // ✅ Сбрасываем флаг, чтобы client_task запросил паттерн
    state::PATTERN_LOADED.store(false, std::sync::atomic::Ordering::Relaxed);
}


/// Сбросить прогресс вязания (начать заново)
pub fn reset_knitting() {
    log("INFO", "Resetting knitting progress...");
    crate::knit_state::reset_progress();
    
    ROW.store(0, Ordering::Relaxed);
    GLOBAL_ROW.store(0, Ordering::Relaxed);
    
    // ✅ УДАЛЕНО: Сброс CURRENT_CHUNK_START_ROW и ROWS_IN_CURRENT_CHUNK
    
    state::PATTERN_LOADED.store(false, Ordering::Relaxed);
}

pub fn delay_us(us: u32) {
    FreeRtos::delay_ms(us.saturating_add(US_PER_MS - 1) / US_PER_MS);
}