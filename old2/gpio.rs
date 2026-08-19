use esp_idf_hal::gpio::{
    Gpio18, Gpio19, Gpio21, Gpio22, Gpio4, Input, InterruptType, Output, PinDriver,
};
use std::sync::{LazyLock, Mutex};

use crate::core::{on_ccp_tick, on_hok_change, on_ksl_change, on_nd1_falling};

pub static GPIO: LazyLock<Mutex<Option<GpioBundle>>> = LazyLock::new(|| Mutex::new(None));

pub struct GpioBundle {
    pub nd1: PinDriver<'static, Gpio22, Input>,
    pub ksl: PinDriver<'static, Gpio21, Input>,
    pub ccp: PinDriver<'static, Gpio18, Input>,
    pub hok: PinDriver<'static, Gpio19, Input>,
    pub dob: PinDriver<'static, Gpio4, Output>,
}

pub fn init(bundle: GpioBundle) {
    *GPIO.lock().unwrap() = Some(bundle);
}

#[inline(always)]
pub fn dob_fire_fast() {
    if let Some(ref mut gpio) = *GPIO.lock().unwrap() {
        let _ = gpio.dob.set_low();
        esp_idf_hal::delay::Ets::delay_us(5);
        let _ = gpio.dob.set_high();
    }
}

pub fn install_ccp_interrupt() {
    let mut guard = GPIO.lock().unwrap();
    let gpio = guard.as_mut().unwrap();

    gpio.ccp.set_interrupt_type(InterruptType::PosEdge).unwrap();

    unsafe {
        gpio.ccp
            .subscribe(|| {
                on_ccp_tick();
            })
            .unwrap();
    }

    gpio.ccp.enable_interrupt().unwrap();
}

pub fn install_hok_interrupt() {
    let mut g = GPIO.lock().unwrap();
    let gpio = g.as_mut().unwrap();
    unsafe {
        gpio.hok
            .subscribe(|| {
                let gpio = GPIO.lock().unwrap();
                if let Some(ref gpio_bundle) = *gpio {
                    let level = gpio_bundle.hok.is_high();
                    on_hok_change(level);
                }
            })
            .unwrap();
    }
    gpio.hok.set_interrupt_type(InterruptType::AnyEdge).unwrap();
    gpio.hok.enable_interrupt().unwrap();
}

pub fn install_nd1_interrupt() {
    let mut g = GPIO.lock().unwrap();
    let gpio = g.as_mut().unwrap();

    gpio.nd1.set_interrupt_type(InterruptType::NegEdge).unwrap();

    unsafe {
        gpio.nd1
            .subscribe(|| {
                on_nd1_falling();
            })
            .unwrap();
    }

    gpio.nd1.enable_interrupt().unwrap();
}

pub fn install_ksl_interrupt() {
    let mut g = GPIO.lock().unwrap();
    let gpio = g.as_mut().unwrap();

    unsafe {
        gpio.ksl
            .subscribe(|| {
                let level = gpio.ksl.is_high();
                on_ksl_change(level);
            })
            .unwrap();
    }
    gpio.ksl.set_interrupt_type(InterruptType::AnyEdge).unwrap();
    gpio.ksl.enable_interrupt().unwrap();
}
