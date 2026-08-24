use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::state::{
    CCP, CCP_LAST_STATE, HOK, HOK_LAST_DEBOUNCE_US, HOK_LAST_STATE, 
    KSL, KSL_HOK_DEBOUNCE_US, KSL_LAST_DEBOUNCE_US, KSL_LAST_STATE, ND1
};
use std::sync::atomic::Ordering;

extern "C" fn ccp_isr(_: *mut core::ffi::c_void) {
    let current_level = unsafe { (esp_idf_sys::GPIO.in_ >> CCP) & 0x1 != 0 };
    CCP_LAST_STATE.store(current_level, Ordering::Relaxed);
    let _ = QUEUE.send_back(EVT_CCP, 1u32);
}

extern "C" fn nd1_isr(_: *mut core::ffi::c_void) {
    let _ = QUEUE.send_back(EVT_ND1, 1u32);
}

extern "C" fn ksl_isr(_: *mut core::ffi::c_void) {
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let ksl_state = unsafe { (esp_idf_sys::GPIO.in_ >> KSL) & 0x1 != 0 };
    let last_state = KSL_LAST_STATE.load(Ordering::Relaxed);
    let last_debounce = KSL_LAST_DEBOUNCE_US.load(Ordering::Relaxed);

    if now_us.saturating_sub(last_debounce) < KSL_HOK_DEBOUNCE_US {
        return;
    }

    if ksl_state != last_state {
        KSL_LAST_STATE.store(ksl_state, Ordering::Relaxed);
        KSL_LAST_DEBOUNCE_US.store(now_us, Ordering::Relaxed);
        let _ = QUEUE.send_back(EVT_KSL, 1u32);
    }
}

extern "C" fn hok_isr(_: *mut core::ffi::c_void) {
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let hok_state = unsafe { (esp_idf_sys::GPIO.in_ >> HOK) & 0x1 != 0 };
    let last_state = HOK_LAST_STATE.load(Ordering::Relaxed);
    let last_debounce = HOK_LAST_DEBOUNCE_US.load(Ordering::Relaxed);

    if now_us.saturating_sub(last_debounce) < KSL_HOK_DEBOUNCE_US {
        return;
    }

    if hok_state != last_state {
        HOK_LAST_STATE.store(hok_state, Ordering::Relaxed);
        HOK_LAST_DEBOUNCE_US.store(now_us, Ordering::Relaxed);
        
        let _ = QUEUE.send_back(EVT_HOK, 1u32);
    }
}

pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_set_intr_type(CCP, esp_idf_sys::gpio_int_type_t_GPIO_INTR_POSEDGE); // Только восходящий
        esp_idf_sys::gpio_set_intr_type(KSL, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(HOK, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(ND1, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);

        esp_idf_sys::gpio_isr_handler_add(CCP, Some(ccp_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(ND1, Some(nd1_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(KSL, Some(ksl_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(HOK, Some(hok_isr), core::ptr::null_mut());
    }
}

