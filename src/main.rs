use esp_idf_hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::server::EspHttpServer,
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};
use esp_idf_sys::{esp_wifi_set_ps, gpio_install_isr_service, link_patches, wifi_ps_type_t_WIFI_PS_NONE};
use log::info;

use crate::{
    gpio::init_pins, pattern::PATTERN, state::{HEIGHT, WIDTH}, tasks::{engine_task, init_knitter, logger_task}, web::connect_wifi
};
use core::sync::atomic::Ordering;
use std::ptr::null_mut;

mod gpio;
mod logger;
mod pattern;
mod state;
mod tasks;
mod web;
mod queue;
mod isr;

fn main() -> anyhow::Result<()> {
    link_patches();
    EspLogger::initialize_default();
    log::set_max_level(log::LevelFilter::Debug);
    WIDTH.store(PATTERN.width, Ordering::Relaxed);
    HEIGHT.store(PATTERN.height, Ordering::Relaxed);
    init_pins();
    unsafe {
        gpio_install_isr_service(0);
    }
    init_knitter();

    // Thread name must be a valid C string (null-terminated, no embedded nulls)
    ThreadSpawnConfiguration {
        name: Some(b"thread1\0"), // for knit thread
        stack_size: 4096,
        priority: 24,
        ..Default::default()
    }
    .set()
    .unwrap();

    let knit_thread = std::thread::Builder::new()
        .spawn(move || {
            engine_task(null_mut());
        })
        .unwrap();

    ThreadSpawnConfiguration {
        name: Some(b"thread2\0"), // for log thread
        stack_size: 4096,
        priority: 10,
        ..Default::default()
    }
    .set()
    .unwrap();

    let logger_thread = std::thread::Builder::new()
        .spawn(move || {
            logger_task(null_mut());
        })
        .unwrap();

    let peripherals = esp_idf_hal::peripherals::Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;
    unsafe { esp_wifi_set_ps(wifi_ps_type_t_WIFI_PS_NONE) };
    connect_wifi(&mut wifi)?;
    let mut server = EspHttpServer::new(&Default::default()).unwrap();
    web::init_server(&mut server)?;
    info!("Server started");
    core::mem::forget(wifi);
    core::mem::forget(server);
    std::thread::park();
    Ok(())
}
