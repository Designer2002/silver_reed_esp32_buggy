use dotenvy_macro::dotenv;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::{Gpio4, Output, PinDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::*;
use std::cell::RefCell;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const SSID: &str = dotenv!("WIFI_SSID");
const PASS: &str = dotenv!("WIFI_PASS");
const INDEX_HTML: &str = include_str!("index.html");

const OPTOCOUPLER_PIN_NUM: i32 = 4;

// Константы протокола из main_d.rs
const FREQUENCY_SILVER_REED_US: u32 = 110; // ~9090 Hz = 110 мкс
const CMD_P: u8 = b'P';
const CMD_A: u8 = b'A';
const CMD_L: u8 = b'L';
const CMD_y: u8 = b'y';
const CMD_x: u8 = b'x';
const CMD_e: u8 = b'e';
const CMD_N: u8 = b'N';
const CMD_m: u8 = b'm';
const CMD_R: u8 = b'R';
const CMD_c: u8 = b'c';
const CMD_C: u8 = b'C';
const CMD_RESET_1: u8 = b'!';
const CMD_RESET_2: u8 = b'\r';

static GPIO_STATE: Mutex<RefCell<Option<PinDriver<'static, Gpio4, Output>>>> =
    Mutex::new(RefCell::new(None));

struct Stats {
    total_activations: u32,
    last_activation: Option<String>,
    uptime: String,
}

static STATS: LazyLock<Mutex<Stats>> = LazyLock::new(|| {
    Mutex::new(Stats {
        total_activations: 0,
        last_activation: None,
        uptime: "00:00".to_string(),
    })
});

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("booting");

    let peripherals = Peripherals::take()?;

    // Инициализация GPIO
    let mut dob = PinDriver::output_od(peripherals.pins.gpio4)?;
    dob.set_high()?;
    *GPIO_STATE.lock().unwrap().borrow_mut() = Some(dob);

    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;

    connect_wifi(&mut wifi)?;

    info!("wifi ok, starting server");

    let mut server = EspHttpServer::new(&HttpConfig::default())?;

    // Главная страница
    server.fn_handler("/", Method::Get, |req| -> anyhow::Result<()> {
        req.into_ok_response()?.write_all(INDEX_HTML.as_bytes())?;
        Ok(())
    })?;

    // Статус GPIO
    server.fn_handler("/status", Method::Get, |_req| -> anyhow::Result<()> {
        let gpio_guard = GPIO_STATE.lock().unwrap();
        let pin_ref = gpio_guard.borrow();

        let status = match pin_ref.as_ref() {
            Some(pin_driver) => {
                if pin_driver.is_set_high() {
                    // Пин отпущен → линия = 5В → оптопара ВЫКЛЮЧЕНА
                    "Неактивна (покой, линия = 5В)"
                } else {
                    // Пин тянет вниз → линия = 0В → оптопара ВКЛЮЧЕНА
                    "Активна (тянем вниз, линия = 0В)"
                }
            }
            None => "Ошибка: GPIO не инициализирован",
        };

        let response_json = format!("{{\"status\": \"{}\"}}", status);
        let mut resp = _req.into_ok_response()?;
        resp.write_all(response_json.as_bytes())?;
        Ok(())
    })?;

    // Простая активация (как было)
    server.fn_handler("/trigger", Method::Post, |req| -> anyhow::Result<()> {
        let uri = req.uri();
        let query = uri.split('?').nth(1).unwrap_or("");
        let duration: u64 = query
            .split('&')
            .find(|s| s.starts_with("duration="))
            .and_then(|s| s.split('=').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(500);

        {
            let gpio_guard = GPIO_STATE.lock().unwrap();
            let mut pin_ref = gpio_guard.borrow_mut();

            match pin_ref.as_mut() {
                Some(pin_driver) => {
                    pin_driver.set_low()?;
                    info!("GPIO{} LOW {}ms", OPTOCOUPLER_PIN_NUM, duration);
                }
                None => {
                    req.into_status_response(500)?.write_all(b"GPIO Not Init")?;
                    return Ok(());
                }
            }
        }

        {
            let mut stats = STATS.lock().unwrap();
            stats.total_activations += 1;
            stats.last_activation = Some(get_timestamp());
        }

        FreeRtos::delay_ms(duration as u32);

        {
            let gpio_guard = GPIO_STATE.lock().unwrap();
            let mut pin_ref = gpio_guard.borrow_mut();

            if let Some(pin_driver) = pin_ref.as_mut() {
                pin_driver.set_high()?;
                info!("GPIO{} HIGH", OPTOCOUPLER_PIN_NUM);
            }
        }

        req.into_ok_response()?.write_all(b"OK")?;
        Ok(())
    })?;

    // НОВЫЙ ОБРАБОТЧИК: отправка бита с гарантированным возвратом в покой
    server.fn_handler("/send_bit", Method::Post, |req| -> anyhow::Result<()> {
        let uri = req.uri();
        let query = uri.split('?').nth(1).unwrap_or("");

        let bit_value = query
            .split('&')
            .find(|s| s.starts_with("value="))
            .and_then(|s| s.split('=').nth(1))
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(1)
            != 0;

        let duration_ms: u64 = query
            .split('&')
            .find(|s| s.starts_with("duration="))
            .and_then(|s| s.split('=').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let gpio_guard = GPIO_STATE.lock().unwrap();
        let mut pin_ref = gpio_guard.borrow_mut();

        if let Some(pin_driver) = pin_ref.as_mut() {
            let total_us = duration_ms * 1000;
            let cycles = total_us / 110;

            info!(
                "Sending bit {} for {}ms ({} cycles)",
                if bit_value { "1 (LOW)" } else { "0 (HIGH)" },
                duration_ms,
                cycles
            );

            for _i in 0..cycles {
                // ✅ ИНВЕРСИЯ: бит '1' = LOW, бит '0' = HIGH
                if bit_value {
                    pin_driver.set_low()?; // Бит '1' → тянем вниз
                } else {
                    pin_driver.set_high()?; // Бит '0' → отпускаем
                }
                esp_idf_hal::delay::Ets::delay_us(110);
            }

            // ✅ КРИТИЧНО: всегда возвращаем в ПОКОЙ (отпускаем!)
            pin_driver.set_high()?; // ← НЕ set_low()!
            esp_idf_hal::delay::Ets::delay_us(110);
        }

        {
            let mut stats = STATS.lock().unwrap();
            stats.total_activations += 1;
            stats.last_activation = Some(get_timestamp());
        }

        req.into_ok_response()?.write_all(b"OK")?;
        Ok(())
    })?;

    // НОВЫЙ ОБРАБОТЧИК: отправка байта с правильной синхронизацией
    server.fn_handler("/send_byte", Method::Post, |req| -> anyhow::Result<()> {
        let uri = req.uri();
        let query = uri.split('?').nth(1).unwrap_or("");

        let byte_value = query
            .split('&')
            .find(|s| s.starts_with("value="))
            .and_then(|s| s.split('=').nth(1))
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .unwrap_or(0x50); // По умолчанию 'P'

        let gpio_guard = GPIO_STATE.lock().unwrap();
        let mut pin_ref = gpio_guard.borrow_mut();

        if let Some(pin_driver) = pin_ref.as_mut() {
            info!("Sending byte 0x{:02X}", byte_value);

            // START BIT (LOW) — 110 мкс
            pin_driver.set_low()?;
            esp_idf_hal::delay::Ets::delay_us(110);

            // 8 бит данных (инверсия как в протоколе: 1=LOW, 0=HIGH)
            for i in 0..8 {
                let bit = (byte_value >> i) & 1 != 0;
                if bit {
                    pin_driver.set_low()?; // бит '1' = тянем вниз
                } else {
                    pin_driver.set_high()?; // бит '0' = отпускаем
                }
                esp_idf_hal::delay::Ets::delay_us(110);
            }

            // STOP BIT (HIGH) — 110 мкс
            pin_driver.set_high()?;
            esp_idf_hal::delay::Ets::delay_us(110);

            // ⚠️ Возврат в состояние покоя (для оптопары — LOW)
            pin_driver.set_high()?; // ← ВОЗВРАТ В ПОКОЙ!
            esp_idf_hal::delay::Ets::delay_us(110);
        }

        {
            let mut stats = STATS.lock().unwrap();
            stats.total_activations += 1;
            stats.last_activation = Some(get_timestamp());
        }

        req.into_ok_response()?.write_all(b"OK")?;
        Ok(())
    })?;

    // НОВЫЙ ОБРАБОТЧИК: отправка команды протокола
    server.fn_handler("/send_command", Method::Post, |req| -> anyhow::Result<()> {
        let uri = req.uri();
        let query = uri.split('?').nth(1).unwrap_or("");

        let cmd = query
            .split('&')
            .find(|s| s.starts_with("cmd="))
            .and_then(|s| s.split('=').nth(1))
            .unwrap_or("P");

        let gpio_guard = GPIO_STATE.lock().unwrap();
        let mut pin_ref = gpio_guard.borrow_mut();

        if let Some(pin_driver) = pin_ref.as_mut() {
            let byte_value = match cmd {
                "P" => CMD_P,
                "A" => CMD_A,
                "L" => CMD_L,
                "y" => CMD_y,
                "x" => CMD_x,
                "e" => CMD_e,
                "N" => CMD_N,
                "m" => CMD_m,
                "R" => CMD_R,
                "c" => CMD_c,
                "C" => CMD_C,
                "reset" => {
                    info!("Sending RESET command");
                    // !\r — два байта
                    // Первый байт '!'
                    pin_driver.set_low()?;
                    esp_idf_hal::delay::Ets::delay_us(110); // START
                    for i in 0..8 {
                        let bit = (b'!' >> i) & 1 != 0;
                        if bit {
                            pin_driver.set_low()?;
                        } else {
                            pin_driver.set_high()?;
                        }
                        esp_idf_hal::delay::Ets::delay_us(110);
                    }
                    pin_driver.set_high()?;
                    esp_idf_hal::delay::Ets::delay_us(110); // STOP

                    esp_idf_hal::delay::Ets::delay_us(220); // пауза между байтами

                    // Второй байт '\r'
                    pin_driver.set_low()?;
                    esp_idf_hal::delay::Ets::delay_us(110); // START
                    for i in 0..8 {
                        let bit = (b'\r' >> i) & 1 != 0;
                        if bit {
                            pin_driver.set_low()?;
                        } else {
                            pin_driver.set_high()?;
                        }
                        esp_idf_hal::delay::Ets::delay_us(110);
                    }
                    pin_driver.set_high()?;
                    esp_idf_hal::delay::Ets::delay_us(110); // STOP

                    // Возврат в покой
                    pin_driver.set_high()?;
                    esp_idf_hal::delay::Ets::delay_us(110);

                    {
                        let mut stats = STATS.lock().unwrap();
                        stats.total_activations += 1;
                        stats.last_activation = Some(get_timestamp());
                    }

                    req.into_ok_response()?.write_all(b"OK")?;
                    return Ok(());
                }
                _ => CMD_P,
            };

            info!("Sending command: {} (0x{:02X})", cmd, byte_value);

            // START BIT
            pin_driver.set_low()?;
            esp_idf_hal::delay::Ets::delay_us(110);

            // 8 бит данных
            for i in 0..8 {
                let bit = (byte_value >> i) & 1 != 0;
                if bit {
                    pin_driver.set_low()?; // '1' = LOW
                } else {
                    pin_driver.set_high()?; // '0' = HIGH
                }
                esp_idf_hal::delay::Ets::delay_us(110);
            }

            // STOP BIT
            pin_driver.set_high()?;
            esp_idf_hal::delay::Ets::delay_us(110);

            // ⚠️ ВСЕГДА возвращаем в покой (LOW для оптопары)
            pin_driver.set_high()?; // ← ВОЗВРАТ В ПОКОЙ!
            esp_idf_hal::delay::Ets::delay_us(110);
        }

        {
            let mut stats = STATS.lock().unwrap();
            stats.total_activations += 1;
            stats.last_activation = Some(get_timestamp());
        }

        req.into_ok_response()?.write_all(b"OK")?;
        Ok(())
    })?;

    // Добавь новый обработчик ПЕРЕД основным циклом сервера:
    server.fn_handler("/test_slow", Method::Post, |req| -> anyhow::Result<()> {
        let gpio_guard = GPIO_STATE.lock().unwrap();
        let mut pin_ref = gpio_guard.borrow_mut();

        if let Some(pin_driver) = pin_ref.as_mut() {
            info!("Starting slow blink test (2 Hz)...");
            for _ in 0..6 {
                // 3 полных цикла
                pin_driver.set_low()?; // Диод горит
                info!("LED ON");
                FreeRtos::delay_ms(250);

                pin_driver.set_high()?; // Диод гаснет
                info!("LED OFF");
                FreeRtos::delay_ms(250);
            }
            info!("Slow blink test complete");
        }

        req.into_ok_response()?.write_all(b"OK")?;
        Ok(())
    })?;

    // Получение статистики
    server.fn_handler("/data", Method::Get, |_req| -> anyhow::Result<()> {
        let stats = STATS.lock().unwrap();
        let response_json = format!(
            r#"{{"totalActivations": {}, "lastActivation": {}, "uptime": "{}"}}"#,
            stats.total_activations,
            match &stats.last_activation {
                Some(time) => format!("\"{}\"", time),
                None => "null".to_string(),
            },
            stats.uptime
        );
        let mut resp = _req.into_ok_response()?;
        resp.write_all(response_json.as_bytes())?;
        Ok(())
    })?;

    info!("HTTP Server created with GPIO control handlers.");

    core::mem::forget(wifi);
    core::mem::forget(server);

    Ok(())
}

fn connect_wifi(wifi: &mut BlockingWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: PASS.try_into().unwrap(),
        channel: None,
        ..Default::default()
    });
    wifi.set_configuration(&wifi_configuration)?;

    wifi.start()?;
    info!("Wifi started");

    wifi.connect()?;
    info!("Wifi connected");

    wifi.wait_netif_up()?;
    info!("Wifi netif up");

    Ok(())
}

fn get_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("{}", duration.as_secs()),
        Err(_) => "0".to_string(),
    }
}
