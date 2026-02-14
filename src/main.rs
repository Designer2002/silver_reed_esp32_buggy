use esp_idf_hal::task::thread::{ ThreadSpawnConfiguration};
use esp_idf_svc::http::server::EspHttpServer;

use crate::{isr::install_isrs, pattern::PATTERN, state::{HEIGHT, WIDTH}, tasks::engine_task};
use core::sync::atomic::Ordering;
use std::ptr::null_mut;

mod gpio;
mod isr;
mod pattern;
mod state;
mod tasks;
mod logger;
mod web;

fn main() -> anyhow::Result<()> {
    WIDTH.store(PATTERN.width, Ordering::Relaxed);
    HEIGHT.store(PATTERN.height, Ordering::Relaxed);
    install_isrs();
    ThreadSpawnConfiguration {
        name: Some("knit_thread".as_bytes()),
        stack_size: 4096,
        priority: 10,
        ..Default::default()
    }
    .set()
    .unwrap();

    let knit_thread = std::thread::Builder::new()
        .spawn(move || {
            engine_task(null_mut());
        })
        .unwrap();

    knit_thread.join().unwrap();
    let server = EspHttpServer::new(&Default::default()).unwrap();
    web::init_server(server)?;
    
    Ok(())
}
