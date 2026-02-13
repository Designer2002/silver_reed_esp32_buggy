use dotenvy_macro::dotenv;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::gpio::{AnyIOPin, Gpio4, IOPin, Input, InterruptType, Output, PinDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::task::queue::Queue;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::info;
use serde::Serialize;
use std::collections::{vec_deque, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const SSID: &str = dotenv!("WIFI_SSID");
const PASS: &str = dotenv!("WIFI_PASS");
const INDEX_HTML: &str = include_str!("index.html");
const CSS: &str = include_str!("style.css");
const PAT_TXT: &str = include_str!("pat.txt"); // Убедись, что файл существует

const DOB_PIN_NUM: i32 = 4;
const CCP_PIN_NUM: i32 = 18;
const HOK_PIN_NUM: i32 = 19;
const KSL_PIN_NUM: i32 = 21;
const ND1_PIN_NUM: i32 = 22;

const FREQUENCY_SILVER_REED_US: u32 = 110; // ~9090 Hz
const CCP_QUEUE_TIMEOUT_MS: u32 = 1000;

// --- ГЛОБАЛЬНЫЕ СТАТУСЫ ---
static GLOBAL_KNITTING_ACTIVE: AtomicBool = AtomicBool::new(false);
static CURRENT_ROW: AtomicUsize = AtomicUsize::new(0);
static CURRENT_COLUMN: AtomicUsize = AtomicUsize::new(0);
static TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);
static TOTAL_COLUMNS: AtomicUsize = AtomicUsize::new(0);
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

// --- ЛОГИ ---
#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    timestamp: String,
    level: String,
    message: String,
}

//LOGGER
static LOG_BUFFER: LazyLock<Arc<Mutex<Option<VecDeque<LogEntry>>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

fn add_log_to_buffer(level: &str, message: &str) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let timestamp = format!("{}.{}", now.as_secs(), now.subsec_millis() / 10);

    let entry = LogEntry {
        timestamp,
        level: level.to_string(),
        message: message.to_string(),
    };

    let mut buffer_guard = LOG_BUFFER.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(ref mut buffer) = *buffer_guard {
        // <- Получаем ссылку на эмулятор
        buffer.push_back(entry);
        if buffer.len() > 100 {
            buffer.pop_front();
        }
    } else {
        log::error!("Global buffer not initialized before calling this func!");
    }

    // Выводим в монитор
    info!("[{}] {}", level, message);
}

// --- ПАТТЕРН ---
#[derive(Clone)]
struct KnitPattern {
    rows: Vec<Vec<bool>>,
    width: usize,
    height: usize,
}

fn parse_pattern(pattern_text: &str) -> KnitPattern {
    let rows: Vec<Vec<bool>> = pattern_text
        .lines()
        .map(|line| {
            line.chars()
                .map(|c| c == '#' || c == '@' || c == 'X' || c == 'x')
                .collect()
        })
        .collect();

    let height = rows.len();
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);

    KnitPattern {
        rows,
        width,
        height,
    }
}

static PATTERN: std::sync::LazyLock<KnitPattern> =
    std::sync::LazyLock::new(|| parse_pattern(PAT_TXT));

// --- ISR и ОЧЕРЕДЬ ---
static CCP_QUEUE: std::sync::LazyLock<Queue<u8>> = std::sync::LazyLock::new(|| Queue::new(32));

extern "C" fn ccp_isr_handler(_arg: *mut core::ffi::c_void) {
    let _ = CCP_QUEUE.send_back(1u8, 100); // Не критично, если не отправилось
}

// --- ЭМУЛЯТОР ---
static EMULATOR_INSTANCE: LazyLock<Arc<Mutex<Option<SilverLinkEmulator>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

struct SilverLinkEmulator {
    pattern: KnitPattern,
    ccp: PinDriver<'static, AnyIOPin, Input>,
    hok: PinDriver<'static, AnyIOPin, Input>,
    ksl: PinDriver<'static, AnyIOPin, Input>,
    nd1: PinDriver<'static, AnyIOPin, Input>,
    dob: Arc<Mutex<PinDriver<'static, Gpio4, Output>>>,
}

impl SilverLinkEmulator {
    pub fn new(
        mut ccp: PinDriver<'static, AnyIOPin, Input>,
        hok: PinDriver<'static, AnyIOPin, Input>,
        ksl: PinDriver<'static, AnyIOPin, Input>,
        nd1: PinDriver<'static, AnyIOPin, Input>,
        dob: PinDriver<'static, Gpio4, Output>,
    ) -> Result<Self, esp_idf_hal::gpio::GpioError> {
        unsafe {
            ccp.set_interrupt_type(InterruptType::AnyEdge)?;
            ccp.subscribe(|| ccp_isr_handler(core::ptr::null_mut()))?;
            ccp.enable_interrupt()?;
        }

        let dob_arc = Arc::new(Mutex::new(dob));

        Ok(Self {
            pattern: PATTERN.clone(), // Используем статический паттерн
            ccp,
            hok,
            ksl,
            nd1,
            dob: dob_arc,
        })
    }

    pub fn start_knitting(&self) {
        add_log_to_buffer("INFO", "Start knitting thread initiated.");

        // Установим статус
        GLOBAL_KNITTING_ACTIVE.store(true, Ordering::Relaxed);
        TOTAL_ROWS.store(self.pattern.height, Ordering::Relaxed);
        TOTAL_COLUMNS.store(self.pattern.width, Ordering::Relaxed);
        CURRENT_ROW.store(0, Ordering::Relaxed);
        CURRENT_COLUMN.store(0, Ordering::Relaxed);

        let dob_arc = self.dob.clone();

        // Основной цикл
        for row_idx in 0..self.pattern.height {
            if !GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                add_log_to_buffer("INFO", "Knitting stopped externally during row loop.");
                break;
            }

            CURRENT_ROW.store(row_idx, Ordering::Relaxed);
            CURRENT_COLUMN.store(0, Ordering::Relaxed);

            add_log_to_buffer("INFO", &format!("Processing row {}", row_idx));

            // Ждём ND1 LOW (начало строки)
            while self.nd1.is_high() && GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                FreeRtos::delay_ms(10);
            }
            if !GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                break;
            }
            // Ждём ND1 HIGH (конец строки)
            while self.nd1.is_low() && GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                FreeRtos::delay_ms(10);
            }
            if !GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                break;
            }

            // Ждём KSL HIGH (в диапазоне)
            add_log_to_buffer("DEBUG", "Waiting for KSL HIGH...");
            while !self.ksl.is_high() && GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                FreeRtos::delay_ms(10);
            }
            if !GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                break;
            }

            let is_right_to_left = self.hok.is_high();
            add_log_to_buffer(
                "DEBUG",
                &format!(
                    "Direction: {}",
                    if is_right_to_left {
                        "Right-to-Left"
                    } else {
                        "Left-to-Right"
                    }
                ),
            );

            // Цикл по столбцам
            let mut col_idx = 0;
            while self.ksl.is_high()
                && GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed)
                && col_idx < self.pattern.width
            {
                // Ждём импульс CCP
                if CCP_QUEUE.recv_front(CCP_QUEUE_TIMEOUT_MS).is_some() {
                    CURRENT_COLUMN.store(col_idx, Ordering::Relaxed);

                    let should_activate_dob = if is_right_to_left {
                        if col_idx < self.pattern.width {
                            self.pattern.rows[row_idx][self.pattern.width - 1 - col_idx]
                        } else {
                            false
                        }
                    } else {
                        if col_idx < self.pattern.width {
                            self.pattern.rows[row_idx][col_idx]
                        } else {
                            false
                        }
                    };

                    if should_activate_dob {
                        add_log_to_buffer(
                            "DEBUG",
                            &format!("Activating DOB for Row {}, Col {}", row_idx, col_idx),
                        );
                        {
                            let mut dob = dob_arc.lock().unwrap();
                            dob.set_low().unwrap();
                            Ets::delay_us(100); // Кратковременно
                            dob.set_high().unwrap();
                        }
                    }

                    col_idx += 1;
                } else {
                    // Таймаут CCP - строка может закончиться
                    if !self.ksl.is_high() {
                        // KSL стал LOW, строка закончена
                        break;
                    }
                    // Проверяем статус в таймауте CCP
                    if !GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                        break;
                    }
                    FreeRtos::delay_ms(1); // Дышим
                }
            }

            // Ждём KSL LOW (конец строки)
            while self.ksl.is_high() && GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                FreeRtos::delay_ms(10);
            }
        }

        // Завершение
        GLOBAL_KNITTING_ACTIVE.store(false, Ordering::Relaxed);
        add_log_to_buffer("INFO", "Knitting completed or stopped.");
    }

    pub fn get_signal_states(&self) -> (bool, bool, bool, bool, bool) {
        (
            self.ccp.is_high(),
            self.hok.is_high(),
            self.ksl.is_high(),
            self.nd1.is_high(),
            self.dob.lock().unwrap().is_set_high(),
        )
    }
}

fn connect_wifi(wifi: &mut BlockingWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let wifi_configuration = Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: PASS.try_into().unwrap(),
        channel: None,
        ..Default::default()
    });
    wifi.set_configuration(&wifi_configuration)?;

    wifi.start()?;
    info!("WiFi started");

    wifi.connect()?;
    info!("WiFi connected");

    wifi.wait_netif_up()?;
    info!("WiFi netif up");

    Ok(())
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();

    //logs
    EspLogger::initialize_default();
    log::set_max_level(log::LevelFilter::Debug);
    let log_vec = VecDeque::with_capacity(100);
    {
        let mut guard = LOG_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(log_vec);
    }

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

    server.fn_handler("/logs", Method::Get, |_req| -> anyhow::Result<()> {
        let mut logs_vec: Vec<&LogEntry> = VecDeque::with_capacity(100).into();
        let mut buffer_guard = LOG_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut buffer) = *buffer_guard {
            for log in buffer {
                logs_vec.push(log);
                info!("NEW LOGS: {:?}", log);
            }
        } else {
            log::error!("Global buffer not initialized before calling this func!");
        }

        if !logs_vec.is_empty() {
            info!("LOGS:{:?}", logs_vec);
        }

        let json_string = serde_json::to_string(&logs_vec).unwrap_or_else(|_| "[]".to_string());

        let headers = [
            ("Content-Type", "application/json"),
            ("Cache-Control", "max-age=86400"),
        ];
        let mut resp = _req.into_response(200, Some("OK"), &headers)?;
        resp.write_all(json_string.as_bytes())?;
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
                add_log_to_buffer("WARN", "Knitting already in progress.");
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
            add_log_to_buffer("INFO", "Stop knitting command received.");

            let mut last_err = LAST_ERROR.lock().unwrap();
            *last_err = "Knitting stopped by user.".to_string();
            drop(last_err);

            let mut resp = _req.into_ok_response()?;
            resp.write_all(b"OK")?;
            Ok(())
        },
    )?;

    server.fn_handler(
        "/test_log",
        Method::Post,
        |_req| -> anyhow::Result<()> {
            add_log_to_buffer("INFO", "Лог захуячен ёпта :)");
            let mut resp = _req.into_ok_response()?;
            resp.write_all(b"OK")?;
            Ok(())
        },
    )?;

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
    add_log_to_buffer(
        "INFO",
        "Сервер запущен успешно! Ожидание начала процесса...",
    );

    core::mem::forget(wifi);
    core::mem::forget(server);

    Ok(())
}
