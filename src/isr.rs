
use crate::queue::{EVT_CCP, EVT_HOK, EVT_KSL, EVT_ND1, QUEUE};
use crate::state::{
    CCP, CCP_AUTO_PASS_US, CCP_AUTO_REJECT_US, CCP_AVG_INTERVAL_US, CCP_FILTER_RESET, CCP_INTERVAL_COUNT, CCP_INTERVAL_SUM, CCP_LAST_STATE, CCP_LAST_TICK_US, CCP_MAX_RATIO, CCP_MIN_RATIO, EVENT_SEQUENCE, HOK, HOK_LAST_DEBOUNCE_US, HOK_LAST_STATE, KSL, KSL_HOK_DEBOUNCE_US, KSL_LAST_DEBOUNCE_US, KSL_LAST_STATE, ND1
};
use std::sync::atomic::Ordering;


extern "C" fn ccp_isr(_: *mut core::ffi::c_void) {
    let current_level = unsafe { (esp_idf_sys::GPIO.in_ >> CCP) & 0x1 != 0 };
    let last_level = CCP_LAST_STATE.load(Ordering::Relaxed);

    // ✅ Только rising edge
    if !current_level || last_level {
        CCP_LAST_STATE.store(current_level, Ordering::Relaxed);
        return;
    }
    CCP_LAST_STATE.store(current_level, Ordering::Relaxed);

    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let last_tick = CCP_LAST_TICK_US.load(Ordering::Relaxed);
    let interval = now_us.saturating_sub(last_tick);

    // ✅ Сброс фильтра при входе в зону
    if CCP_FILTER_RESET.load(Ordering::Relaxed) {
        CCP_FILTER_RESET.store(false, Ordering::Relaxed);
        CCP_INTERVAL_SUM.store(0, Ordering::Relaxed);
        CCP_INTERVAL_COUNT.store(0, Ordering::Relaxed);
        CCP_AVG_INTERVAL_US.store(0, Ordering::Relaxed);
        CCP_LAST_TICK_US.store(now_us, Ordering::Relaxed);
        return; // Пропускаем первый тик после сброса
    }

    // ✅ Автоотклонение: < 30μs = точно помеха
    if last_tick > 0 && interval < CCP_AUTO_REJECT_US {
        return;
    }

    // ✅ Автопропуск: > 200μs = точно реальный тик
    if last_tick == 0 || interval > CCP_AUTO_PASS_US {
        CCP_LAST_TICK_US.store(now_us, Ordering::Relaxed);
        let sum = CCP_INTERVAL_SUM.fetch_add(interval, Ordering::Relaxed) + interval;
        let count = CCP_INTERVAL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        CCP_AVG_INTERVAL_US.store(sum / count, Ordering::Relaxed);

        let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let evt = crate::queue::EngineEvent {
            kind: EVT_CCP,
            seq,
            timestamp_us: now_us,
            level: current_level,
        };
        let _ = QUEUE.send_back_isr(evt);
        return;
    }

    let avg = CCP_AVG_INTERVAL_US.load(Ordering::Relaxed);
    if avg > 0 {
        let min_valid = avg / CCP_MIN_RATIO;
        let max_valid = avg * CCP_MAX_RATIO;
        if interval < min_valid || interval > max_valid {
            return;
        }
    }

    CCP_LAST_TICK_US.store(now_us, Ordering::Relaxed);
    let sum = CCP_INTERVAL_SUM.fetch_add(interval, Ordering::Relaxed) + interval;
    let count = CCP_INTERVAL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    CCP_AVG_INTERVAL_US.store(sum / count, Ordering::Relaxed);

    // 🚫 Никогда не делаем format! / String внутри ISR.
    // Это вызывает heap churn и может повреждать память на ESP32.
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let evt = crate::queue::EngineEvent {
        kind: EVT_CCP,
        seq,
        timestamp_us: now_us,
        level: current_level,
    };
    let _ = QUEUE.send_back(evt);
}

extern "C" fn nd1_isr(_: *mut core::ffi::c_void) {
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let evt = crate::queue::EngineEvent {
        kind: EVT_ND1,
        seq,
        timestamp_us: now_us,
        level: false,
    };
    let _ = QUEUE.send_back_isr(evt);
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

        let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let evt = crate::queue::EngineEvent {
            kind: EVT_KSL,
            seq,
            timestamp_us: now_us,
            level: ksl_state,
        };
        let _ = QUEUE.send_back_isr(evt);
    }
}

extern "C" fn hok_isr(_: *mut core::ffi::c_void) {
    let now_us = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
    let hok_state = unsafe { (esp_idf_sys::GPIO.in_ >> HOK) & 0x1 != 0 };
    let last_state = HOK_LAST_STATE.load(Ordering::Relaxed);

    let last_debounce = HOK_LAST_DEBOUNCE_US.load(Ordering::Relaxed);
    if now_us - last_debounce < KSL_HOK_DEBOUNCE_US {
        return;
    }

    if hok_state != last_state {
        HOK_LAST_STATE.store(hok_state, Ordering::Relaxed);
        HOK_LAST_DEBOUNCE_US.store(now_us, Ordering::Relaxed);

        let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let evt = crate::queue::EngineEvent {
            kind: EVT_HOK,
            seq,
            timestamp_us: now_us,
            level: hok_state,
        };
        let _ = QUEUE.send_back_isr(evt);
    }
}

pub fn install_isrs() {
    unsafe {
        esp_idf_sys::gpio_set_intr_type(CCP, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(KSL, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(HOK, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);
        esp_idf_sys::gpio_set_intr_type(ND1, esp_idf_sys::gpio_int_type_t_GPIO_INTR_ANYEDGE);

        esp_idf_sys::gpio_isr_handler_add(CCP, Some(ccp_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(ND1, Some(nd1_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(KSL, Some(ksl_isr), core::ptr::null_mut());
        esp_idf_sys::gpio_isr_handler_add(HOK, Some(hok_isr), core::ptr::null_mut());
    }
}

pub fn uninstall_isrs() {
    unsafe {
        esp_idf_sys::gpio_isr_handler_remove(CCP);
        esp_idf_sys::gpio_isr_handler_remove(ND1);
        esp_idf_sys::gpio_isr_handler_remove(KSL);
        esp_idf_sys::gpio_isr_handler_remove(HOK);
    }
}