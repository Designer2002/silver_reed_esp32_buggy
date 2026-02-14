use std::{ffi::c_void, thread, time::Duration};

use esp_idf_sys::xEventGroupWaitBits;
use crate::{gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast}, isr::{BIT_CCP, BIT_HOK, BIT_KSL, BIT_ND1, EVENTS}, logger::{log, pop_log, push_web_log}, state};
pub extern "C" fn engine_task(_: *mut c_void) {
    let mut row = 0;
    loop {
        let bits = unsafe {
            xEventGroupWaitBits(
                EVENTS,
                BIT_CCP | BIT_ND1 | BIT_KSL | BIT_HOK,
                true as i32,
                false as i32,
                u32::MAX,
            )
        };

        if bits & BIT_HOK != 0 {
            on_hok_change_fast(true);
            log("DEBUG", "HOK change detected, direction updated");
        }

        if bits & BIT_ND1 != 0 {
            on_nd1_falling_fast();
            log("DEBUG", "ND1 falling edge detected, needle reset");
        }

        if bits & BIT_KSL != 0 {
            on_ksl_change(true);
            row += 1;
            let msg = format!("KSL change detected, row updated to {}", row);
            log("DEBUG", Box::leak(msg.into_boxed_str()));
        }

        if bits & BIT_CCP != 0 {
            on_ccp_tick_fast();   // САМЫЙ ВАЖНЫЙ
        }
    }
}

pub extern "C" fn logger_task(_: *mut c_void) {
    loop {
        let mut had_logs = false;

        while let Some(entry) = pop_log() {
            had_logs = true;

            // UART
            println!(
                "[{}] {}: {}",
                entry.timestamp,
                entry.level,
                entry.message
            );

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

pub fn init_knitter(){
    log("INFO","Initializing knitter...");
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub fn start_knitting(){
    log("INFO","Starting knitting...");
    state::KNITTING.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn stop_knitting(){
    log("INFO","Stopping knitting...");
    state::KNITTING.store(false, std::sync::atomic::Ordering::Relaxed);
}