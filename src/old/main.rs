mod types;
mod constants;
mod knitter;
mod server;
mod logger;

use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::gpio::{AnyIOPin, Gpio4, IOPin, Input, InterruptType, Output, PinDriver};
use esp_idf_hal::peripherals::Peripherals;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use heapless::spsc::{Consumer, Producer};
use log::info;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use std::time::UNIX_EPOCH;
use std::time::{Duration, SystemTime};











fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();

    //logs
    EspLogger::initialize_default();
    log::set_max_level(log::LevelFilter::Debug);
    critical_section::with(|cs| {
        let mut q = Queue::<LogEntry, 256>::new();
        let (p, c) = q.split();

        *LOG_QUEUE.borrow_ref_mut(cs) = Some(q);
        *LOG_PROD.borrow_ref_mut(cs) = Some(p);
        *LOG_CONS.borrow_ref_mut(cs) = Some(c);
    });

    info!("Booting...");

    let peripherals = Peripherals::take()?;

    // GPIO
    let mut dob_pin = PinDriver::output(peripherals.pins.gpio4)?;
    dob_pin.set_high().unwrap(); // Покой
    let dob_for_emulator = dob_pin;

    let ccp_pin = PinDriver::input(peripherals.pins.gpio18.downgrade())?;
    let hok_pin = PinDriver::input(peripherals.pins.gpio19.downgrade())?;
    let ksl_pin = PinDriver::input(peripherals.pins.gpio21.downgrade())?;
    let nd1_pin = PinDriver::input(peripherals.pins.gpio22.downgrade())?;

    let emulator = SilverLinkEmulator::new(ccp_pin, hok_pin, ksl_pin, nd1_pin, dob_for_emulator)?;
    {
        let mut guard = EMULATOR_INSTANCE.lock().unwrap();
        *guard = Some(emulator);
    }
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;

    connect_wifi(&mut wifi)?;
    info!("WiFi OK, starting server");

    let mut server = EspHttpServer::new(&HttpConfig::default())?;

    // Handlers
    server.fn_handler("/pat.txt", Method::Get, |req| -> anyhow::Result<()> {
        let mut resp = req.into_ok_response()?;
        resp.write_all(PAT_TXT.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler("/style.css", Method::Get, |req| -> anyhow::Result<()> {
        let headers = [
            ("Content-Type", "text/css"),
            ("Cache-Control", "max-age=86400"),
        ];
        let mut resp = req.into_response(200, Some("OK"), &headers)?;
        resp.write_all(CSS.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler("/", Method::Get, |req| -> anyhow::Result<()> {
        let mut resp = req.into_ok_response()?;
        resp.write_all(INDEX_HTML.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler("/logs", Method::Get, |req| -> anyhow::Result<()> {
        let mut json = String::from("[");

        critical_section::with(|cs| {
            if let Some(q) = LOG_QUEUE.borrow_ref_mut(cs).as_mut() {
                while let Some(log) = q.dequeue() {
                    let line = format!(
                        r#"{{"t":{},"l":"{}","m":"{}"}},"#,
                        log.timestamp, log.level, log.msg
                    );
                    json.push_str(&line);
                }
            }
        });

        json.push(']');

        let mut resp = req.into_ok_response()?;
        resp.write_all(json.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler("/knitting_status", Method::Get, |_req| -> anyhow::Result<()> {
        let is_active = GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed);
        let current_row = CURRENT_ROW.load(Ordering::Relaxed);
        let current_col = CURRENT_COLUMN.load(Ordering::Relaxed);
        let total_r = TOTAL_ROWS.load(Ordering::Relaxed);
        let total_c = TOTAL_COLUMNS.load(Ordering::Relaxed);
        let last_err = LAST_ERROR.lock().unwrap().clone();

        let status_obj = serde_json::json!({
            "currentRow": current_row,
            "currentColumn": current_col,
            "totalRows": total_r,
            "totalColumns": total_c,
            "isKnitting": is_active,
            "lastError": if last_err.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(last_err) }
        });

        let json_string = status_obj.to_string();

        let headers = [
            ("Content-Type", "application/json"),
            ("Cache-Control", "max-age=86400"),
        ];
        let mut resp = _req.into_response(200, Some("OK"), &headers)?;
        resp.write_all(json_string.as_bytes())?;
        Ok(())
    })?;

    let _ = server.fn_handler(
        "/signal_status",
        Method::Get,
        move |req| -> anyhow::Result<()> {
            // остальные сигналы
            let emulator_guard = EMULATOR_INSTANCE.lock().unwrap();
            let (ccp_state, hok_state, ksl_state, nd1_state, dob_state) =
                if let Some(ref emulator) = *emulator_guard {
                    emulator.get_signal_states()
                } else {
                    (false, false, false, false, false) // Возвращаем false, если эмулятор не инициализирован
                };

            let response_json = format!(
                r#"{{"ccp":"{}","hok":"{}","ksl":"{}","nd1":"{}","dob":"{}"}}"#,
                if ccp_state { "HIGH" } else { "LOW" },
                if hok_state { "HIGH" } else { "LOW" },
                if ksl_state { "HIGH" } else { "LOW" },
                if nd1_state { "HIGH" } else { "LOW" },
                if dob_state { "HIGH" } else { "LOW" }
            );

            let mut resp = req.into_ok_response()?;
            resp.write_all(response_json.as_bytes())?;
            Ok(())
        },
    );

    server.fn_handler(
        "/start_knitting",
        Method::Post,
        |_req| -> anyhow::Result<()> {
            if GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                add_log("WARN", "Knitting already in progress.");
                // Не запускаем новый поток, если уже запущен
                return Ok(());
            }
            let emulator_arc = EMULATOR_INSTANCE.clone();
            std::thread::spawn(move || { // <- `move` захватывает emulator_arc в поток
            // --- НОВОЕ: Защита через Mutex ---
            let mut emulator_mutex_guard = emulator_arc.lock().unwrap(); // <- Захватываем мьютекс
            if let Some(ref mut emulator) = *emulator_mutex_guard { // <- Получаем ссылку на эмулятор
                // --- НОВОЕ: Вызов метода ---
                emulator.start_knitting(); // <- Вызываем метод на глобальном экземпляре
            } else {
                log::error!("Global emulator not initialized when start_knitting was called from thread.");
            }
            // <- mutex_guard освобождается при выходе из замыкания
            // <- поток завершается
        });

            let mut last_err = LAST_ERROR.lock().unwrap();
            *last_err = String::new(); // Очистить ошибку при запуске
            drop(last_err);

            let mut resp = _req.into_ok_response()?;
            resp.write_all(b"OK")?;
            Ok(())
        },
    )?;

    server.fn_handler(
        "/stop_knitting",
        Method::Post,
        |_req| -> anyhow::Result<()> {
            GLOBAL_KNITTING_ACTIVE.store(false, Ordering::Relaxed);
            add_log("INFO", "Stop knitting command received.");

            let mut last_err = LAST_ERROR.lock().unwrap();
            *last_err = "Knitting stopped by user.".to_string();
            drop(last_err);

            let mut resp = _req.into_ok_response()?;
            resp.write_all(b"OK")?;
            Ok(())
        },
    )?;

    server.fn_handler("/test_log", Method::Post, |_req| -> anyhow::Result<()> {
        add_log("INFO", "Лог-тест отправлен!");
        let mut resp = _req.into_ok_response()?;
        resp.write_all(b"OK")?;
        Ok(())
    })?;

    server.fn_handler("/status", Method::Get, |_req| -> anyhow::Result<()> {
        let is_active = GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed);
        let status_msg = if is_active {
            "Активна (тянем вниз, линия = 0В)"
        } else {
            "Неактивна (покой, линия = 5В)"
        };

        let status_obj = serde_json::json!({ "status": status_msg });
        let json_string = status_obj.to_string();

        let headers = [
            ("Content-Type", "application/json"),
            ("Cache-Control", "max-age=86400"),
        ];
        let mut resp = _req.into_response(200, Some("OK"), &headers)?;
        resp.write_all(json_string.as_bytes())?;
        Ok(())
    })?;

    info!("HTTP Server created with GPIO control handlers.");
    add_log(
        "INFO",
        "Сервер запущен успешно! Ожидание начала процесса...",
    );

    core::mem::forget(wifi);
    core::mem::forget(server);

    Ok(())
}
