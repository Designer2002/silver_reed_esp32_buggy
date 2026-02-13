extern "C" fn ccp_isr(_: *mut core::ffi::c_void) {
    push_event(Event::CCP);
}

mod engine;
mod event_bus;
mod gpio;
mod logger;
mod pattern;
mod web;

use engine::start_engine;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::server::{Configuration as HttpConfig, EspHttpServer},
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};
use logger::log;
use esp_idf_hal::gpio::{Gpio4, Gpio18, Gpio19, Gpio21, Gpio22, Input, InterruptType, Output, PinDriver};
use esp_idf_hal::prelude::Peripherals;

use crate::{
    event_bus::{Event, push_event}, gpio::{GPIO, GpioBundle}, web::{connect_wifi, init_server}
};

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    log("INFO", "BOOT");

    let peripherals = Peripherals::take().unwrap();
    let mut bundle = GpioBundle {
        nd1: PinDriver::input(peripherals.pins.gpio22)?,
        ksl: PinDriver::input(peripherals.pins.gpio21)?,
        ccp: PinDriver::input(peripherals.pins.gpio18)?,
        hok: PinDriver::input(peripherals.pins.gpio19)?,
        dob: PinDriver::output(peripherals.pins.gpio4)?,
    };

    bundle.dob.set_high()?; // DOB idle

    *GPIO.lock().unwrap() = Some(bundle);

    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;

    connect_wifi(&mut wifi)?;

    start_engine();
    let server = EspHttpServer::new(&HttpConfig::default())?;
    let _ = init_server(server);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
