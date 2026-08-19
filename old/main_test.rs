use dotenvy_macro::dotenv;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::{Gpio4, Output, PinDriver}; // типы GPIO
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::*;
use std::cell::RefCell;
use std::sync::{LazyLock, Mutex}; // Для потокобезопасности
use std::time::{SystemTime, UNIX_EPOCH};

const SSID: &str = dotenv!("WIFI_SSID");
const PASS: &str = dotenv!("WIFI_PASS");
const INDEX_HTML: &str = include_str!("index.html");

// Константа для номера пина GPIO14
const OPTOCOUPLER_PIN_NUM: i32 = 4;

// Глобальное состояние для GPIO пина
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
    // --- НАЧАЛО: Инициализация GPIO ---
    let mut dob = PinDriver::output(peripherals.pins.gpio4)?; // Инициализируем GPIO4 как выход                                              //Устанавливаем начальное состояние LOW
    dob.set_low()?;
    //Сохраняем ссылку на пин в глобальном состоянии
    *GPIO_STATE.lock().unwrap() = RefCell::new(Some(dob));
    // --- КОНЕЦ: Инициализация GPIO ---

    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;

    connect_wifi(&mut wifi)?;

    info!("wifi ok, starting server");

    let mut server = EspHttpServer::new(&HttpConfig::default())?;
    // Обработчик для главной страницы
    server.fn_handler("/", Method::Get, |req| -> anyhow::Result<()> {
        req.into_ok_response()?.write_all(INDEX_HTML.as_bytes())?;
        Ok(())
    })?;
    // --- НОВЫЙ ОБРАБОТЧИК: Получение статуса GPIO (для обновления интерфейса) ---
    server.fn_handler("/status", Method::Get, |_req| -> anyhow::Result<()> {
        // Получаем доступ к GPIO пину
        let gpio_guard = GPIO_STATE.lock().unwrap();
        let pin_ref = gpio_guard.borrow();

        let status = match pin_ref.as_ref() {
            Some(pin_driver) => {
                // Проверяем текущее состояние пина
                if pin_driver.is_set_high() {
                    "Активна (HIGH)"
                } else {
                    "Неактивна (LOW)"
                }
            }
            None => "Ошибка: GPIO не инициализирован",
        };

        // Возвращаем статус в формате JSON
        let response_json = format!("{{\"status\": \"{}\"}}", status);
        let mut resp = _req.into_ok_response()?;
        resp.write_all(response_json.as_bytes())?;
        Ok(())
    })?;
    // --- КОНЕЦ НОВОГО ОБРАБОТЧИКА ---
    server.fn_handler("/trigger", Method::Post, |req| -> anyhow::Result<()> {
        let uri = req.uri();
        let query = uri.split('?').nth(1).unwrap_or("");
        let duration: u64 = query
            .split('&')
            .find(|s| s.starts_with("duration="))
            .and_then(|s| s.split('=').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(500);

        // --- HIGH ---
        {
            let gpio_guard = GPIO_STATE.lock().unwrap();
            let mut pin_ref = gpio_guard.borrow_mut();

            match pin_ref.as_mut() {
                Some(pin_driver) => {
                    pin_driver.set_high()?;
                    info!("GPIO{} HIGH {}ms", OPTOCOUPLER_PIN_NUM, duration);
                }
                None => {
                    req.into_status_response(500)?.write_all(b"GPIO Not Init")?;
                    return Ok(());
                }
            }
        } // <-- mutex отпустился

        // статистика
        {
            let mut stats = STATS.lock().unwrap();
            stats.total_activations += 1;
        }

        // delay БЕЗ mutex
        esp_idf_hal::delay::Ets::delay_ms(duration as u32);

        // --- LOW ---
        {
            let gpio_guard = GPIO_STATE.lock().unwrap();
            let mut pin_ref = gpio_guard.borrow_mut();

            if let Some(pin_driver) = pin_ref.as_mut() {
                pin_driver.set_low()?;
                info!("GPIO{} LOW", OPTOCOUPLER_PIN_NUM);
            }
        }

        req.into_ok_response()?.write_all(b"OK")?;
        Ok(())
    })?;

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

    // Сохраняем wifi и сервер, чтобы они работали постоянно
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
