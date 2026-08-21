use crate::client::create_client;
use crate::gpio::handshake;
use crate::pattern::PATTERN;
use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::state::{
    CURRENT_CHUNK_START_ROW, GLOBAL_ROW, PATTERN_END, PATTERN_START, ROW, ROWS_IN_CURRENT_CHUNK, US_PER_MS,
};
use crate::{
    gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast},
    isr::{install_isrs, uninstall_isrs},
    logger::{log, pop_log},
    state::{self},
};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_sys::esp_http_client;
use log::info;
use std::sync::atomic::Ordering;
use std::ffi::c_void;

pub extern "C" fn engine_task(_: *mut c_void) {
    info!("Engine task started");

    loop {
        let mut processed = 0usize;

        while processed < 32 {
            match QUEUE.recv_front() {
                Some(event) => {
                    let _timestamp_us = event.timestamp_us;
                    if event.kind == EVT_CCP {
                        on_ccp_tick_fast(event.seq);
                    } else if event.kind == EVT_ND1 {
                        on_nd1_falling_fast();
                    } else if event.kind == EVT_KSL {
                        on_ksl_change(event.seq);
                    } else if event.kind == EVT_HOK {
                        on_hok_change_fast(event.level);
                    }
                    processed += 1;
                }
                None => break,
            }
        }

        if state::CHUNK_SWAP_PENDING.swap(false, Ordering::AcqRel) {
            let _ = crate::pattern::swap_to_next_chunk();
        }

        if processed == 0 {
            FreeRtos::delay_ms(1);
        } else {
            delay_us(100);
        }
    }
}

pub extern "C" fn client_task(_: *mut c_void) {
    info!("Client started - streaming pattern loader");
    let mut hits_delay = 0;
    let client = create_client();
    start_knitting(client);
    loop {
        // ✅ Проверяем: если вязание началось и начальный чанк еще не запрошен
        if state::KNITTING.load(Ordering::Relaxed)
            && !state::INITIAL_CHUNK_REQUESTED.load(Ordering::Relaxed)
        {
            // Запрашиваем первый чанк
            crate::client::request_initial_chunk(client);
            state::INITIAL_CHUNK_REQUESTED.store(true, Ordering::Relaxed);
        }

        // Проверяем, не пора ли запросить новый чанк
        crate::client::check_and_request_chunk(client);

        // Обрабатываем полученные данные
        if let Some(data) = crate::client::receive_data() {
            if !crate::client::process_chunk_data(data) {
                log("ERROR", "Failed to process chunk data");
            }
        }

        // ✅ Отправляем информацию о ряде на сервер если pending
        if state::ROW_INFO_PENDING.load(Ordering::Acquire) {
            let row = state::ROW_INFO_ROW.load(Ordering::Relaxed);
            let dir = state::ROW_INFO_DIR.load(Ordering::Relaxed);
            if crate::client::send_queued_row_info(client) {
                let msg = format!(
                        "Row info sent: row={}, dir={}",
                        row,
                        if dir { "RIGHT" } else { "LEFT" }
                    );
                log(
                    "DEBUG",
                    &msg
                );
                drop(msg);
            } else {
                log("WARN", "Failed to send row info");
            }
            state::ROW_INFO_PENDING.store(false, Ordering::Release);
        }
        hits_delay+=1;
        if hits_delay>=100{
            hits_delay=0;
            // ✅ Отправляем hits на сервер
            let _ = crate::client::send_solenoid_hits(client);
        }


        // Используем FreeRTOS delay: это корректно сбрасывает watchdog на ESP-IDF.
        FreeRtos::delay_ms(10);
    }
}

pub extern "C" fn logger_task(_: *mut c_void) {
    info!("Logger task started");
    loop {
        let mut had_logs = false;

        while let Some(entry) = pop_log() {
            had_logs = true;

            // UART
            println!("[{}] {}: {}", entry.timestamp, entry.level, entry.message);

            // кладём в веб буфер
            //push_web_log(entry);
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

    // ✅ Сброс защиты от подёргивания при старте
    state::HOK_DIR_CHANGE_DEBOUNCE_US.store(0, Ordering::Relaxed);

    // ✅ Сначала проверяем restart флаг от сервера
    crate::client::check_if_restart(client);

    // ✅ Если нет сохранённого прогресса (или был restart) — начинаем с нуля
    if crate::knit_state::restore_progress().is_none() {
        PATTERN_START.store(0, Ordering::Relaxed);
        PATTERN_END.store(
            *&PATTERN.lock().unwrap().width.saturating_sub(1) as i32,
            Ordering::Relaxed,
        );
        ROW.store(0, Ordering::Relaxed);
        GLOBAL_ROW.store(0, Ordering::Relaxed);
        CURRENT_CHUNK_START_ROW.store(0, Ordering::Relaxed);
        ROWS_IN_CURRENT_CHUNK.store(0, Ordering::Relaxed);
    }

    // ✅ Сбрасываем флаг чтобы client_task запросил первый чанк
    state::INITIAL_CHUNK_REQUESTED.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub fn stop_knitting() {
    log("INFO", "Stopping knitting...");
    uninstall_isrs();
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Сбросить прогресс вязания (начать заново)
/// Вызывается при команде сервера или когда все ряды провязаны
pub fn reset_knitting() {
    log("INFO", "Resetting knitting progress...");
    crate::knit_state::reset_progress();
    state::ROW.store(0, Ordering::Relaxed);
    state::GLOBAL_ROW.store(0, Ordering::Relaxed);
    state::CURRENT_CHUNK_START_ROW.store(0, Ordering::Relaxed);
    state::ROWS_IN_CURRENT_CHUNK.store(0, Ordering::Relaxed);
    state::INITIAL_CHUNK_REQUESTED.store(false, Ordering::Relaxed);
}
pub fn delay_us(us: u32) {
    FreeRtos::delay_ms(us.saturating_add(US_PER_MS - 1) / US_PER_MS);
}
