use esp_idf_hal::gpio::{Gpio4, Gpio18, Gpio19, Gpio21, Gpio22, Input, InterruptType, Output, PinDriver};
use std::sync::{LazyLock, Mutex};

use crate::event_bus::{Event, push_event};

pub static GPIO: LazyLock<Mutex<Option<GpioBundle>>> = LazyLock::new(|| Mutex::new(None));

pub struct GpioBundle {
    pub nd1: PinDriver<'static, Gpio22, Input>,
    pub ksl: PinDriver<'static, Gpio21, Input>,
    pub ccp: PinDriver<'static, Gpio18, Input>,
    pub hok: PinDriver<'static, Gpio19, Input>,
    pub dob: PinDriver<'static, Gpio4, Output>,
}

pub fn dob_fire() {
    if let Some(ref mut gpio) = *GPIO.lock().unwrap() {
        let _ = gpio.dob.set_low();
        esp_idf_hal::delay::Ets::delay_us(100);
        let _ = gpio.dob.set_high();
    }
}

pub fn read_inputs() -> Option<(bool,bool,bool,bool)> {
    let guard = GPIO.lock().unwrap();
    let gpio = guard.as_ref()?;

    Some((
        gpio.ccp.is_high(),
        gpio.hok.is_high(),
        gpio.ksl.is_high(),
        gpio.nd1.is_high(),
    ))
}

pub fn install_ccp_interrupt() {
    let mut guard = GPIO.lock().unwrap();
    let gpio = guard.as_mut().unwrap();

    gpio.ccp.set_interrupt_type(InterruptType::PosEdge).unwrap();

    unsafe {
        gpio.ccp.subscribe(|| {
            push_event(Event::CCP);
        }).unwrap();
    }

    gpio.ccp.enable_interrupt().unwrap();
}

pub fn get_pin_state_json() -> String
{
    let mut guard = GPIO.lock().unwrap();
    let gpio = guard.as_mut().unwrap();
    let json = format!(
            r#"{{"ccp":{},"ksl":{},"nd1":{},"hok":{}}}"#,
            gpio.ccp.is_high(),
            gpio.ksl.is_high(),
            gpio.nd1.is_high(),
            gpio.hok.is_high()
        );
    json
}