use std::{ffi::c_void, thread, time::Duration};

use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::{
    gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast},
    isr::{install_isrs, uninstall_isrs},
    logger::{log, pop_log, push_web_log},
    state::{self, HOK, KSL},
};
use log::info;

pub extern "C" fn engine_task(_: *mut c_void) {
    info!("Engine task started");

    let mut row = 0;

    loop {
        let evt = QUEUE.recv_front(1u32);

        match evt {
            Some(signal) => match signal.0 {
                EVT_CCP => {
                    on_ccp_tick_fast();
                }

                // EVT_ND1 => {
                //     on_nd1_falling_fast();
                // }

                EVT_KSL => {
                    let level = unsafe { esp_idf_sys::gpio_get_level(KSL) };
                    on_ksl_change(level == 1);

                    row += 1;
                    let msg = format!("KSL change detected, row updated to {}", row);
                    log("DEBUG", Box::leak(msg.into_boxed_str()));
                }

                EVT_HOK => {
                    let level = unsafe { esp_idf_sys::gpio_get_level(HOK) };
                    on_hok_change_fast(level == 1);
                }

                _ => {}
            },

            None => {}
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
}

pub fn stop_knitting() {
    log("INFO", "Stopping knitting...");
    uninstall_isrs();
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}
