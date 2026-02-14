use std::ffi::c_void;

use esp_idf_sys::xEventGroupWaitBits;
use crate::{gpio::{on_ccp_tick_fast, on_hok_change_fast, on_ksl_change, on_nd1_falling_fast}, isr::{BIT_CCP, BIT_HOK, BIT_KSL, BIT_ND1, EVENTS}};
extern "C" fn engine_task(_: *mut c_void) {
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
        }

        if bits & BIT_ND1 != 0 {
            on_nd1_falling_fast();
        }

        if bits & BIT_KSL != 0 {
            on_ksl_change(true);
        }

        if bits & BIT_CCP != 0 {
            on_ccp_tick_fast();   // САМЫЙ ВАЖНЫЙ
        }
    }
}

