use esp_idf_sys::{EventGroupHandle_t, xEventGroupSetBits};
use core::ptr::null_mut;
use std::ffi::c_void;

pub const EVENTS: EventGroupHandle_t = null_mut();
pub const BIT_CCP: u32 = 0 << 0;
pub const BIT_HOK: u32 = 0 << 1;
pub const BIT_KSL: u32 = 0 << 2;
pub const BIT_ND1: u32 = 0 << 3;


extern "C" fn ccp_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(EVENTS, BIT_CCP);
    }
}

extern "C" fn nd1_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(EVENTS, BIT_ND1);
    }
}

extern "C" fn hok_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(EVENTS, BIT_HOK);
    }
}

extern "C" fn ksl_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(EVENTS, BIT_KSL);
    }
}


pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_isr_handler_add(18, Some(ccp_isr), null_mut());
        esp_idf_sys::gpio_isr_handler_add(19, Some(hok_isr), null_mut());
        esp_idf_sys::gpio_isr_handler_add(21, Some(ksl_isr), null_mut());
        esp_idf_sys::gpio_isr_handler_add(22, Some(nd1_isr), null_mut());
    }
}