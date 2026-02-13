extern "C" fn ccp_isr(_: *mut core::ffi::c_void) {
    push_event(Event::CCP);
}

mod event_bus;
mod engine;
mod logger;

use engine::start_engine;
use logger::log;

use crate::event_bus::{Event, push_event};

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log("INFO","BOOT");

    start_engine();

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
