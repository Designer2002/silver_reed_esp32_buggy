
use crate::{queue::{EVENT, QUEUE}, state::{CCP, HOK, KSL, ND1}};

extern "C" fn event_isr(_: *mut core::ffi::c_void) {
    let _ = QUEUE.send_front(EVENT, 1u32);
}

pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_set_intr_type(CCP, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(ND1, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(KSL, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(HOK, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);

        esp_idf_sys::gpio_isr_handler_add(CCP, Some(event_isr), core::ptr::null_mut());
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