use crate::event_bus::{pop_event, Event};
use crate::logger::log;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{PinDriver, Output, Gpio4};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub fn start_dob_task(
    dob: Arc<Mutex<PinDriver<'static, Gpio4, Output>>>
) {

    thread::spawn(move || {

        log("INFO","DOB TASK STARTED");

        loop {
            if let Some(evt) = pop_event() {

                if let Event::DobFire = evt {
                    fire(&dob);
                }
            }

            thread::sleep(Duration::from_millis(1));
        }
    });
}

fn fire(dob: &Arc<Mutex<PinDriver<'static, Gpio4, Output>>>) {

    let mut pin = dob.lock().unwrap();

    pin.set_low().unwrap();
    Ets::delay_us(110);
    pin.set_high().unwrap();

    log("DEBUG","DOB FIRE");
}
