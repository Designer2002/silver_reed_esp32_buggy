use std::{ffi::c_void, thread, time::Duration};

use crate::{
    gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast},
    isr::{BIT_CCP, BIT_HOK, BIT_KSL, BIT_ND1, get_handle, install_isrs, uninstall_isrs},
    logger::{log, pop_log, push_web_log},
    state,
};
use esp_idf_sys::xEventGroupWaitBits;
use log::info;
pub extern "C" fn engine_task(_: *mut c_void) {
    info!("Engine task started");
    let mut row = 0;
    let mut test = 0;
    loop {
        let bits = unsafe {
            xEventGroupWaitBits(
                get_handle(),
                BIT_CCP | BIT_ND1 | BIT_KSL | BIT_HOK,
                true as i32,
                false as i32,
                u32::MAX,
            )
        };

        if bits & BIT_HOK != 0 {
            on_hok_change_fast(true);
        }

        if bits & BIT_ND1 != 0 {
            on_nd1_falling_fast();
        }

        if bits & BIT_KSL != 0 {
            on_ksl_change(true);
            row += 1;
            let msg = format!("KSL change detected, row updated to {}", row);
            log("DEBUG", Box::leak(msg.into_boxed_str()));
        }

        if bits & BIT_CCP != 0 {
            on_ccp_tick_fast(); // САМЫЙ ВАЖНЫЙ
        }
        test += 1;
        if test == 5 {
            log("DEBUG", "Test log entry from engine task");
            test = 0;
        }
        std::thread::sleep(std::time::Duration::from_nanos(1));
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
