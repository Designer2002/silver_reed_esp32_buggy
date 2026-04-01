
use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::state::{
    CCP, CCP_LAST_TICK_US, CCP_MIN_INTERVAL_US, EVENT_SEQUENCE, HOK, KSL, KSL_DEBOUNCE_MS,
    KSL_FALL_DEBOUNCE_UNTIL, KSL_LAST_STATE, KSL_RISE_DEBOUNCE_UNTIL, ND1,
};
use std::sync::atomic::Ordering;

// ✅ Глобальное состояние для debounce CCP (аппаратный фильтр)
static CCP_LAST_LEVEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

extern "C" fn ccp_isr(_: *mut core::ffi::c_void) {
    // ✅ Читаем текущий уровень CCP
    let current_level = unsafe { (esp_idf_sys::GPIO.in_ >> CCP) & 0x1 != 0 };
    let last_level = CCP_LAST_LEVEL.load(Ordering::Relaxed);
    
    // ✅ Реагируем ТОЛЬКО на восходящий фронт (rising edge)
    if !current_level || last_level {
        CCP_LAST_LEVEL.store(current_level, Ordering::Relaxed);
        return; // Не rising edge — игнорируем
    }
    
    CCP_LAST_LEVEL.store(current_level, Ordering::Relaxed);
    
    // ✅ CCP Debounce: проверяем что прошло достаточно времени
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let last_tick = CCP_LAST_TICK_US.load(Ordering::Relaxed);
    
    if now_us - last_tick < CCP_MIN_INTERVAL_US {
        return; // 🚫 Слишком быстро — помеха
    }
    
    CCP_LAST_TICK_US.store(now_us, Ordering::Relaxed);
    
    // ✅ Захватываем sequence number в момент прерывания
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let _ = QUEUE.send_front((EVT_CCP, seq), 1u32);
}

extern "C" fn nd1_isr(_: *mut core::ffi::c_void) {
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let _ = QUEUE.send_front((EVT_ND1, seq), 1u32);
}

extern "C" fn ksl_isr(_: *mut core::ffi::c_void) {
    // ✅ Читаем текущее состояние KSL
    let ksl_state = unsafe { (esp_idf_sys::GPIO.in_ >> KSL) & 0x1 != 0 };
    let last_state = crate::state::KSL_LAST_STATE.load(Ordering::Relaxed);
    
    // ✅ Определяем направление изменения
    let is_rise = ksl_state && !last_state;  // false → true
    let is_fall = !ksl_state && last_state;  // true → false
    
    // ✅ Сохраняем состояние для следующего раза
    crate::state::KSL_LAST_STATE.store(ksl_state, Ordering::Relaxed);
    
    // ✅ Debounce: проверяем, не прошло ли слишком мало времени
    let now_ms = unsafe { esp_idf_sys::esp_timer_get_time() } as u32 / 1000;
    
    let debounce_until = if is_rise {
        KSL_RISE_DEBOUNCE_UNTIL.load(Ordering::Relaxed)
    } else if is_fall {
        KSL_FALL_DEBOUNCE_UNTIL.load(Ordering::Relaxed)
    } else {
        0; // Нет изменения — не должно случиться
        return;
    };
    
    if now_ms < debounce_until {
        return; // 🚫 Дребезг, игнорируем
    }
    
    // ✅ Устанавливаем следующее допустимое время срабатывания
    if is_rise {
        KSL_RISE_DEBOUNCE_UNTIL.store(now_ms + KSL_DEBOUNCE_MS, Ordering::Relaxed);
    } else if is_fall {
        KSL_FALL_DEBOUNCE_UNTIL.store(now_ms + KSL_DEBOUNCE_MS, Ordering::Relaxed);
    }
    
    // ✅ Захватываем sequence number и сохраняем как последний KSL
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    crate::state::LAST_KSL_SEQUENCE.store(seq, Ordering::SeqCst);
    let _ = QUEUE.send_front((EVT_KSL, seq), 1u32);
}

extern "C" fn hok_isr(_: *mut core::ffi::c_void) {
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let _ = QUEUE.send_front((EVT_HOK, seq), 1u32);
}

pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_set_intr_type(CCP, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(ND1, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(KSL, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
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