use std::{ffi::c_void, thread, time::Duration};
use crate::queue::QUEUE;
use crate::state::ND1;
use crate::{
    gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast},
    isr::{install_isrs, uninstall_isrs},
    logger::{log, pop_log, push_web_log},
    state::{self, HOK, KSL},
};
use esp_idf_sys::gpio_get_level;
use log::info;

static mut LAST_HOK: i32 = 0;
static mut LAST_KSL: i32 = 0;
static mut LAST_ND1: i32 = 0;

pub extern "C" fn engine_task(_: *mut c_void) {
    info!("Engine task started");

    loop {
        let evt = QUEUE.recv_front(1u32);

        match evt {
            Some(signal) => match signal.0 {
                _ => {
                    on_ccp_tick_fast();

                    let hok = unsafe { gpio_get_level(HOK) };
                    let ksl = unsafe { gpio_get_level(KSL) };
                    let nd1 = unsafe { gpio_get_level(ND1) };

                    unsafe {
                        // HOK change
                        if hok != LAST_HOK {
                            on_hok_change_fast(hok == 1);
                            LAST_HOK = hok;
                        }

                        // KSL change
                        if ksl != LAST_KSL {
                            on_ksl_change(ksl == 1);
                            LAST_KSL = ksl;
                        }

                        // ND1 falling edge
                        if LAST_ND1 == 1 && nd1 == 0 {
                            on_nd1_falling_fast();
                        }

                        LAST_ND1 = nd1;
                    }
                }
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
