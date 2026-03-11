use core::ptr::null_mut;
use esp_idf_sys::{EventGroupDef_t, xEventGroupCreate, xEventGroupSetBits};
use std::{
    ffi::c_void,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::state::{CCP, HOK, KSL, ND1};
pub const BIT_CCP: u32 = 1u32 << 0;  // 0x01
pub const BIT_ND1: u32 = 1u32 << 1;  // 0x02
pub const BIT_KSL: u32 = 1u32 << 2;  // 0x04
pub const BIT_HOK: u32 = 1u32 << 3;  // 0x08
static EVENTS: AtomicPtr<EventGroupDef_t> = AtomicPtr::new(core::ptr::null_mut());
pub fn get_handle() -> *mut EventGroupDef_t {
    let handle = EVENTS.load(Ordering::SeqCst);
    if handle.is_null() {
        panic!("Event group not initialized");
    }
    handle
}

pub fn init_event_group() {
    let handle = unsafe { xEventGroupCreate() };
    if handle.is_null() {
        panic!("Failed to create event group (out of memory?)");
    }
    EVENTS.store(handle, Ordering::SeqCst);
}


extern "C" fn ccp_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(get_handle(), BIT_CCP);
    }
}

extern "C" fn nd1_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(get_handle(), BIT_ND1);
    }
}

extern "C" fn hok_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(get_handle(), BIT_HOK);
    }
}

extern "C" fn ksl_isr(_: *mut c_void) {
    unsafe {
        xEventGroupSetBits(get_handle(), BIT_KSL);
    }
}

pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_isr_handler_add(CCP, Some(ccp_isr), null_mut());
        esp_idf_sys::gpio_isr_handler_add(HOK, Some(hok_isr), null_mut());
        esp_idf_sys::gpio_isr_handler_add(KSL, Some(ksl_isr), null_mut());
        esp_idf_sys::gpio_isr_handler_add(ND1, Some(nd1_isr), null_mut());
    }
}


pub fn uninstall_isrs() {
    unsafe {
        esp_idf_sys::gpio_isr_handler_remove(CCP);
        esp_idf_sys::gpio_isr_handler_remove(HOK);
        esp_idf_sys::gpio_isr_handler_remove(KSL);
        esp_idf_sys::gpio_isr_handler_remove(ND1);
    }
}

