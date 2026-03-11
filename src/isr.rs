use esp_idf_sys::{esp_timer_get_time, gpio_int_type_t_GPIO_INTR_NEGEDGE, gpio_int_type_t_GPIO_INTR_POSEDGE};

use crate::{queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE}, state::{CCP, HOK, KSL, ND1}};

extern "C" fn ccp_isr(_: *mut core::ffi::c_void) {
    let _ = QUEUE.send_front(EVT_CCP, 1u32);
}
static mut ND1_LOW_START: u64 = 0;

extern "C" fn nd1_isr(_: *mut core::ffi::c_void)  {
    let now = unsafe { esp_timer_get_time() } as u64;
    let level = unsafe { esp_idf_sys::gpio_get_level(ND1) };

    if level == 0 {
        unsafe { ND1_LOW_START = now; }
    } else {
        let duration = now - unsafe { ND1_LOW_START };

        if duration > 200 {
            let _ = QUEUE.send_front(EVT_ND1, 1u32);
        }
    }
}

extern "C" fn ksl_isr(_: *mut core::ffi::c_void) {
    let _ = QUEUE.send_front(EVT_KSL, 1u32);
}

extern "C" fn hok_isr(_: *mut core::ffi::c_void) {
    let _ = QUEUE.send_front(EVT_HOK, 1u32);
}

pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_set_intr_type(CCP, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(ND1, gpio_int_type_t_GPIO_INTR_NEGEDGE);
        esp_idf_sys::gpio_set_intr_type(KSL, gpio_int_type_t_GPIO_INTR_POSEDGE);
        esp_idf_sys::gpio_set_intr_type(HOK, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);

        esp_idf_sys::gpio_isr_handler_add(CCP, Some(ccp_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(ND1, Some(nd1_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(KSL, Some(ksl_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(HOK, Some(hok_isr), core::ptr::null_mut());
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