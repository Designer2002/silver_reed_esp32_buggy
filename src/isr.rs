use esp_idf_sys::{BIT0, BIT1, BIT2, BIT3, EventGroupHandle_t, xEventGroupSetBits};
use core::ptr::null_mut;
use std::ffi::c_void;

pub const EVENTS: EventGroupHandle_t = null_mut();
pub const BIT_CCP: u32 = BIT0;
pub const BIT_HOK: u32 = BIT1;
pub const BIT_KSL: u32 = BIT2;
pub const BIT_ND1: u32 = BIT3;


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
        xEventGroupSetBits(EVENTS, BIT_CCP);
    }
}


