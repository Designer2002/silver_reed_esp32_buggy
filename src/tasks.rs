use std::sync::atomic::Ordering;
use std::{ffi::c_void, thread, time::Duration};
use crate::gpio::handshake;
use crate::pattern::PATTERN;
use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::state::{PATTERN_END, PATTERN_START, US_PER_MS};
use crate::{
    gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast},
    isr::{install_isrs, uninstall_isrs},
    logger::{log, pop_log, push_web_log},
    state::{self, HOK},
};
use esp_idf_hal::delay::FreeRtos;
use log::info;

pub extern "C" fn engine_task(_: *mut c_void) {
    info!("Engine task started");

    loop {
        let evt: Option<((u8, u32), bool)> = QUEUE.recv_front(1u32);

        if let Some(((signal, seq), _)) = evt {
            if signal == EVT_CCP {
                on_ccp_tick_fast(seq);
            } else if signal == EVT_ND1 {
                on_nd1_falling_fast();
            } else if signal == EVT_KSL {
                on_ksl_change(seq);
            } else if signal == EVT_HOK {
                let level = unsafe { esp_idf_sys::gpio_get_level(HOK) };
                on_hok_change_fast(level == 1);
            }
        }
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
            push_web_log(entry);
        }

        if !had_logs {
            thread::sleep(Duration::from_millis(20));
        } else {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

pub fn init_knitter() {
    log("INFO", "Initializing knitter...");
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub fn start_knitting() {
    log("INFO", "Starting knitting...");
    install_isrs();
    state::KNITTING.store(true, std::sync::atomic::Ordering::Relaxed);
    handshake();
    PATTERN_START.store(0, Ordering::Relaxed);
    PATTERN_END.store(PATTERN.width.saturating_sub(1) as i32, Ordering::Relaxed);
}

pub fn stop_knitting() {
    log("INFO", "Stopping knitting...");
    uninstall_isrs();
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}
pub fn delay_us(us: u32){
    FreeRtos::delay_ms(us.saturating_add(US_PER_MS - 1) / US_PER_MS);
}



