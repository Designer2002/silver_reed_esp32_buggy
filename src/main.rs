use esp_idf_hal::{ task::thread::ThreadSpawnConfiguration};
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use esp_idf_hal::prelude::Peripherals;
use esp_idf_sys::{gpio_install_isr_service, link_patches};
use log::info;

use crate::{
    gpio::init_pins, tasks::{client_task, engine_task, init_knitter, logger_task}
};
use std::ptr::null_mut;

mod client;
mod gpio;
mod knit_state;
mod logger;
mod pattern;
mod state;
mod tasks;
mod queue;
mod web;
mod isr;

fn main() -> anyhow::Result<()> {
    link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    log::set_max_level(log::LevelFilter::Debug);
    

    //wifi
    let peripherals = Peripherals::take().unwrap();
    let sysloop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;
    let nvs = esp_idf_svc::nvs::EspDefaultNvsPartition::take().unwrap();
    let nvs_knit = nvs.clone(); // ✅ Клонируем для knit_state
    let mut wifi = BlockingWifi::wrap(
            EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
            sysloop,
        )?;
    web::connect_wifi(&mut wifi)?;
    init_pins();

    // ✅ Инициализация NVS для сохранения прогресса вязания
    knit_state::init_knit_nvs(nvs_knit);

    // ✅ Проверяем есть ли сохранённый прогресс
    if let Some(global_row) = knit_state::restore_progress() {
        // Восстанавливаем состояние
        crate::state::ROW.store(global_row, std::sync::atomic::Ordering::Relaxed);
        crate::state::GLOBAL_ROW.store(global_row, std::sync::atomic::Ordering::Relaxed);
        info!("Resuming from saved progress: row={}", global_row);
    } else {
        info!("Starting fresh - no saved progress");
    }

    unsafe {
        gpio_install_isr_service(0);
    }
    init_knitter();

    info!("Starting knitting machine with streaming pattern...");

    // Thread 1: engine (вязание)
    ThreadSpawnConfiguration {
        name: Some(b"engine\0"),
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

    // Thread 2: logger
    ThreadSpawnConfiguration {
        name: Some(b"logger\0"),
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

    // Thread 3: client (загрузка паттерна)
    ThreadSpawnConfiguration {
        name: Some(b"client\0"),
        stack_size: 32768,
        priority: 10,
        ..Default::default()
    }
    .set()
    .unwrap();

    let client_thread = std::thread::Builder::new()
        .spawn(move || {
            client_task(null_mut());
        })
        .unwrap();
    
    std::thread::park();
    Ok(())
}
