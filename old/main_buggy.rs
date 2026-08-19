use dotenvy_macro::dotenv;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::gpio::{Gpio4, IOPin, Input, InterruptType, Output, PinDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::task::queue::Queue;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::AnyIOPin;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use esp_idf_sys::portTICK_TYPE_IS_ATOMIC;
use log::{error, info};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

const SSID: &str = dotenv!("WIFI_SSID");
const PASS: &str = dotenv!("WIFI_PASS");
const INDEX_HTML: &str = include_str!("index.html"); // Убедитесь, что эти файлы существуют
const CSS: &str = include_str!("style.css");
// Паттерн вязания (ваш пример)
const KNITTING_PATTERN: &str = include_str!("pat.txt"); // Убедитесь, что этот файл существует и содержит ваш паттерн

const DOB_PIN_NUM: i32 = 4;
const CCP_PIN_NUM: i32 = 18;
const HOK_PIN_NUM: i32 = 19;
const KSL_PIN_NUM: i32 = 21;
const ND1_PIN_NUM: i32 = 22;

// Константы протокола
const FREQUENCY_SILVER_REED_US: u32 = 110; // ~9090 Hz = 110 мкс
const CCP_QUEUE_TIMEOUT_MS: u32 = 1000; // Таймаут ожидания импульса CCP (мс)

// Статус для веб-интерфейса
struct KnittingStatus {
    current_row: usize,
    current_column: usize,
    total_rows: usize,
    total_columns: usize,
    is_knitting: bool,
    last_error: Option<String>,
}

static KNITTING_STATUS: Mutex<RefCell<KnittingStatus>> = Mutex::new(RefCell::new(KnittingStatus {
    current_row: 0,
    current_column: 0,
    total_rows: 0,
    total_columns: 0,
    is_knitting: false,
    last_error: None,
}));

static DOB_LOGICAL_STATE: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));
static KNIT: AtomicBool = AtomicBool::new(false);
// Структура для хранения паттерна
#[derive(Clone)] // Добавлен Clone, чтобы передавать в эмулятор
pub struct KnitPattern {
    rows: Vec<Vec<bool>>,
    width: usize,
    height: usize,
}
// --- НОВОЕ: Структура для лога ---
#[derive(Debug, Clone)]
struct LogEntry {
    timestamp: String,
    level: String,
    message: String,
}

// --- НОВОЕ: Глобальный буфер логов ---
static LOG_BUFFER: LazyLock<Mutex<VecDeque<LogEntry>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(100))); // Хранит последние 100 записей


    // --- НОВАЯ ВЕРСИЯ: Функция для добавления лога в буфер ---
fn add_log_to_buffer(level: &str, message: &str) {
    // Используем SystemTime для получения времени (альтернатива chrono)
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    // Форматируем как секунды.миллисекунды
    let timestamp = format!("{}.{}", now.as_secs(), now.subsec_millis());

    let entry = LogEntry {
        timestamp,
        level: level.to_string(),
        message: message.to_string(),
    };

    let mut buffer = LOG_BUFFER.lock().unwrap();
    buffer.push_back(entry);

    // Удаляем старые записи, если буфер переполнен
    if buffer.len() > 100 {
        buffer.pop_front();
    }

    // --- НОВОЕ: Выводим лог в консоль ESP-IDF сразу при добавлении ---
    // info!() также будет видно в monitor
    info!("[{}] {}", level, message);
    // или println!("LOG [{}] {}", level, message); -- но info() предпочтительнее для ESP-IDF
    // -----------------------------------------------
}

// Конвертируем текстовый паттерн в булев массив
fn parse_pattern(pattern: &str) -> KnitPattern {
    let rows: Vec<Vec<bool>> = pattern
        .lines()
        .map(|line| {
            line.chars()
                .map(|c| c == '#' || c == '@' || c == 'X' || c == 'x')
                .collect()
        })
        .collect();

    let height = rows.len();
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(0);

    KnitPattern {
        rows,
        width,
        height,
    }
}

// Инициализация глобального паттерна
static PATTERN: LazyLock<KnitPattern> = LazyLock::new(|| parse_pattern(KNITTING_PATTERN));

// --- НОВОЕ: Статическая переменная для хранения эмулятора ---
static EMULATOR_INSTANCE: LazyLock<Mutex<Option<SilverLinkEmulator>>> =
    LazyLock::new(|| Mutex::new(None));

// --- НОВОЕ: Очередь для CCP ---
static CCP_QUEUE: LazyLock<Queue<u8>> = LazyLock::new(|| Queue::new(32));

extern "C" fn ccp_isr_handler(_arg: *mut core::ffi::c_void) {
    let value: u8 = 1;
    // Преобразуем таймаут из мс в тики FreeRTOS
    let ticks = CCP_QUEUE_TIMEOUT_MS * 1000 / (portTICK_TYPE_IS_ATOMIC as u32 * 1000);
    CCP_QUEUE
        .send_back(value, ticks)
        .inspect_err(|e| {
            error!("Failed to send to CCP_QUEUE in ISR: {:?}", e);
        })
        .ok();
}

pub struct SilverLinkEmulator<'a> {
    pattern: KnitPattern, // Хранить паттерн локально или использовать статический - решать вам. Здесь локально.
    // GPIO - ссылки на пины, полученные из main
    ccp: PinDriver<'a, AnyIOPin, Input>,
    hok: PinDriver<'a, AnyIOPin, Input>,
    ksl: PinDriver<'a, AnyIOPin, Input>,
    nd1: PinDriver<'a, AnyIOPin, Input>,
    // DOB - теперь ссылка на пин из статической переменной GPIO_STATE
}

// --- НОВОЕ: Статическая переменная для GPIO пина DOB ---
static GPIO_DOB_STATE: LazyLock<Mutex<RefCell<Option<PinDriver<'static, Gpio4, Output>>>>> =
    LazyLock::new(|| Mutex::new(RefCell::new(None)));

impl<'a> SilverLinkEmulator<'a> {
    pub fn new(
        pattern: KnitPattern, // Передаём склонированный паттерн
        ccp: PinDriver<'a, AnyIOPin, Input>,
        hok: PinDriver<'a, AnyIOPin, Input>,
        ksl: PinDriver<'a, AnyIOPin, Input>,
        nd1: PinDriver<'a, AnyIOPin, Input>,
        // dob: PinDriver<'a, AnyIOPin, OutputOpenDrain>, // Больше не передаём dob сюда
    ) -> Result<Self, esp_idf_hal::gpio::GpioError> {
        let mut emulator = Self {
            ccp,
            pattern,
            hok,
            ksl,
            nd1,
            // dob, // Больше не храним dob внутри структуры
        };

        // Подписываемся на CCP по фронту
        unsafe {
            let _ = emulator.ccp.set_interrupt_type(InterruptType::AnyEdge);
            emulator
                .ccp
                .subscribe(|| ccp_isr_handler(ptr::null_mut()))?;
        }

        emulator.ccp.enable_interrupt().unwrap();

        Ok(emulator)
    }

    // Основная логика вязания
    pub fn start_knitting(&mut self) {
        // Используем ссылки из self
        let hok = &self.hok;
        let ksl = &self.ksl;
        let nd1 = &self.nd1;
        // Используем DOB из статической переменной
        let dob_guard = GPIO_DOB_STATE.lock().unwrap();
        let mut dob_ref = dob_guard.borrow_mut();
        let dob_pin: &mut PinDriver<'_, Gpio4, esp_idf_hal::gpio::Output> = dob_ref
            .as_mut()
            .expect("DOB Pin not initialized in static state");

        let status_guard = KNITTING_STATUS.lock().unwrap();
        let mut status = status_guard.borrow_mut();
        status.total_rows = self.pattern.height; // Используем локальный паттерн
        status.total_columns = self.pattern.width;
        status.is_knitting = true;
        status.last_error = None;

        add_log_to_buffer(
            "DEBUG",
            &format!(
                "Начало вязания. Всего строк: {}, столбцов: {}",
                status.total_rows, status.total_columns
            ),
        );

        // Основной цикл вязания
        KNIT.store(true, Ordering::Relaxed);
        for row in 0..self.pattern.height {
            // Используем локальный паттерн
            // Обновляем статус
            status.current_row = row;
            status.current_column = 0;

            add_log_to_buffer("DEBUG", &format!("Ожидание начала строки {}", row));
            // Ждём начала строки (ND1 становится LOW после HIGH)
            // Ждём пока ND1 станет HIGH (конец предыдущей строки / начало новой)
            while nd1.is_low() {
                if !status.is_knitting {
                    add_log_to_buffer("INFO", "Вязание остановлено пользователем (ждём ND1 HIGH).");
                    return;
                }
                FreeRtos::delay_ms(10);
            }
            // Теперь ждём пока ND1 станет LOW (реальное начало строки)
            while nd1.is_high() {
                if !status.is_knitting {
                    add_log_to_buffer("INFO", "Вязание остановлено пользователем (ждём ND1 LOW).");
                    return;
                }
                FreeRtos::delay_ms(10);
            }

            add_log_to_buffer("DEBUG", &format!("Начало строки {}", row));

            // Считываем направление
            let is_right_to_left = hok.is_high();
            add_log_to_buffer(
                "DEBUG",
                &format!(
                    "Направление: {}",
                    if is_right_to_left {
                        "справа-налево"
                    } else {
                        "слева-направо"
                    }
                ),
            );

            // Ждём, пока KSL не станет HIGH (игла в диапазоне)
            add_log_to_buffer("INFO", "Ожидание KSL HIGH (вход в диапазон)");
            while !ksl.is_high() {
                if !status.is_knitting {
                    add_log_to_buffer("INFO", "Вязание остановлено пользователем (ждём KSL HIGH).");
                    return;
                }
                FreeRtos::delay_ms(10);
            }
            add_log_to_buffer("INFO", "KSL HIGH - игла в диапазоне");

            // Цикл обработки импульсов CCP
            let mut column = 0;
            while ksl.is_high() && status.is_knitting && column < self.pattern.width {
                // Добавлено условие column < width
                // Ждём импульс CCP
                if CCP_QUEUE.recv_front(CCP_QUEUE_TIMEOUT_MS).is_some() {
                    // Обновляем статус
                    status.current_column = column;

                    // Проверяем, нужно ли активировать DOB
                    let current_bit = if is_right_to_left {
                        // Для направления справа-налево инвертируем позицию
                        if column < self.pattern.width {
                            // Защита от переполнения
                            self.pattern.rows[row][self.pattern.width - 1 - column]
                        // Используем локальный паттерн
                        } else {
                            false // Если индекс за пределами, не активируем
                        }
                    } else {
                        if column < self.pattern.width {
                            // Защита от переполнения
                            self.pattern.rows[row][column] // Используем локальный паттерн
                        } else {
                            false // Если индекс за пределами, не активируем
                        }
                    };

                    if current_bit {
                        add_log_to_buffer(
                            "DEBUG",
                            &format!("Активация DOB для строки {}, столбца {}", row, column),
                        );
                        // Используем пин из статической переменной
                        DOB_LOGICAL_STATE.store(true, Ordering::Relaxed); // Логически активируем
                        dob_pin.set_low().unwrap();
                        Ets::delay_us(FREQUENCY_SILVER_REED_US); // Используй правильную константу
                        dob_pin.set_high().unwrap();
                        DOB_LOGICAL_STATE.store(false, Ordering::Relaxed); // Логически деактивируем
                    } else {
                        // Убедимся, что DOB в покое (для надёжности)
                        dob_pin.set_high().unwrap();
                        DOB_LOGICAL_STATE.store(false, Ordering::Relaxed); // Логически деактивируем
                    }

                    column += 1;

                    // // Если достигли конца строки (по ширине паттерна)
                    // if column >= self.pattern.width {
                    //     info!("Достигнут конец строки {} по ширине паттерна.", row);
                    //     break;
                    // }
                } else {
                    // Таймаут на ожидание CCP - возможно, строка закончилась раньше
                    add_log_to_buffer(
                        "DEBUG",
                        &format!(
                            "Таймаут ожидания CCP для строки {}, столбца {}. Проверяем KSL.",
                            row, column
                        ),
                    );
                    // Проверяем KSL, если она LOW, выходим из цикла строки
                    if !ksl.is_high() {
                        add_log_to_buffer(
                            "DEBUG",
                            &format!("KSL LOW, завершаем обработку строки {}.", row),
                        );
                        break;
                    }
                    // Иначе продолжаем ждать CCP или проверяем is_knitting
                    if !status.is_knitting {
                        add_log_to_buffer(
                            "INFO",
                            "Вязание остановлено пользователем (таймаут CCP).",
                        );
                        return;
                    }
                    FreeRtos::delay_ms(1);
                }
            }

            // Ждём, пока KSL не станет LOW (конец строки)
            add_log_to_buffer(
                "DEBUG",
                &format!("Ожидание KSL LOW (конец строки {}) для завершения.", row),
            );
            while ksl.is_high() && status.is_knitting {
                FreeRtos::delay_ms(10);
            }

            add_log_to_buffer("DEBUG", &format!("Конец строки {}", row));
        }

        // Завершение вязания
        status.is_knitting = false;
        KNIT.store(false, Ordering::Relaxed);
        add_log_to_buffer("INFO", "Вязание успешно завершено.");
    }

    // Метод для получения состояния пинов для интерфейса
    pub fn get_signal_states(&self) -> (bool, bool, bool, bool) {
        return (
            self.ccp.is_high(),
            self.hok.is_high(),
            self.ksl.is_high(),
            self.nd1.is_high(),
        );
    }
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

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("booting");

    let peripherals = Peripherals::take()?;

    // --- ИНИЦИАЛИЗАЦИЯ GPIO ---
    // DOB: Output Open Drain
    let mut dob_pin = PinDriver::output(peripherals.pins.gpio4)?;
    dob_pin.set_high().unwrap();
    DOB_LOGICAL_STATE.store(false, Ordering::Relaxed); // Логически деактивируем

    *GPIO_DOB_STATE.lock().unwrap() = RefCell::new(Some(dob_pin)); // Сохраняем в статическую переменную

    // Входы: CCP, HOK, KSL, ND1
    let ccp = PinDriver::input(peripherals.pins.gpio18.downgrade())?;
    let hok = PinDriver::input(peripherals.pins.gpio19.downgrade())?;
    let ksl = PinDriver::input(peripherals.pins.gpio21.downgrade())?;
    let nd1 = PinDriver::input(peripherals.pins.gpio22.downgrade())?;

    // --- СОЗДАНИЕ ЭМУЛЯТОРА ---
    let cloned_pattern = PATTERN.clone(); // Клонируем паттерн
    let emulator = SilverLinkEmulator::new(
        cloned_pattern, // Передаём клон
        ccp,
        hok,
        ksl,
        nd1,
        // dob, // DOB больше не передаём
    )?;

    *EMULATOR_INSTANCE.lock().unwrap() = Some(emulator); // Сохраняем в статическую переменную

    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;

    connect_wifi(&mut wifi)?;

    info!("wifi ok, starting server");

    let mut server = EspHttpServer::new(&HttpConfig::default())?;

    //pat.txt handler
    server.fn_handler("/pat.txt", Method::Get, |req| -> anyhow::Result<()> {
        let mut resp = req.into_ok_response()?;
        resp.write_all(KNITTING_PATTERN.as_bytes())?;
        Ok(())
    })?;
    //css handler
    server.fn_handler("/style.css", Method::Get, |req| -> anyhow::Result<()> {
        let headers = [
            ("Content-Type", "text/css"),
            ("Cache-Control", "max-age=86400"),
        ];

        let mut resp = req.into_response(200, Some("OK"), &headers)?;

        resp.write_all(CSS.as_bytes())?;
        Ok(())
    })?;

    // Главная страница
    server.fn_handler("/", Method::Get, |req| -> anyhow::Result<()> {
        req.into_ok_response()?.write_all(INDEX_HTML.as_bytes())?;
        Ok(())
    })?;

    // Страница с логами
    server.fn_handler("/logs", Method::Get, |_req| -> anyhow::Result<()> {
        let buffer = LOG_BUFFER.lock().unwrap();
        let logs_clone = buffer.clone(); // Клонируем для отправки
        
        // Преобразуем в JSON-совместимую строку (упрощённо)
        let mut json_array = String::from("[");
        let mut first = true;
        for entry in logs_clone {
            if !first {
                json_array.push(',');
            }
            json_array.push_str(&format!(
                r#"{{"timestamp":"{}","level":"{}","message":"{}"}}"#,
                entry.timestamp.replace('"', "&quot;"), // Экранируем кавычки на всякий случай
                entry.level,
                entry.message.replace('"', "&quot;")
            ));
            first = false;
        }
        json_array.push(']');

        let headers = [
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-cache"),
        ];
        let mut resp = _req.into_response(200, Some("OK"), &headers)?;
        resp.write_all(json_array.as_bytes())?;
        Ok(())
    })?;

    // Статус вязания
    server.fn_handler("/knitting_status", Method::Get, |_req| -> anyhow::Result<()> {
        let binding = KNITTING_STATUS.lock().unwrap();
        let status = binding.borrow();
        let response_json = format!(
            r#"{{"currentRow": {}, "currentColumn": {}, "totalRows": {}, "totalColumns": {}, "isKnitting": {}, "lastError": {}}}"#,
            status.current_row,
            status.current_column,
            status.total_rows,
            status.total_columns,
            status.is_knitting,
            match &status.last_error {
                Some(error) => format!("\"{}\"", error),
                None => "null".to_string(),
            }
        );
        let mut resp = _req.into_ok_response()?;
        resp.write_all(response_json.as_bytes())?;
        Ok(())
    })?;

    // Статус сигналов (для отображения на интерфейсе)
    let _ = server.fn_handler(
        "/signal_status",
        Method::Get,
        |_req| -> anyhow::Result<()> {
            let dob_guard = GPIO_DOB_STATE.lock().unwrap();
            let mut dob_ref = dob_guard.borrow_mut();
            let dob_pin: &mut PinDriver<'_, Gpio4, esp_idf_hal::gpio::Output> = dob_ref
                .as_mut()
                .expect("DOB Pin not initialized in static state");
            let dob_state = dob_pin.is_set_high();
            let emulator_guard = EMULATOR_INSTANCE.lock().unwrap();
            let (ccp_state, hok_state, ksl_state, nd1_state) =
                if let Some(ref emulator) = *emulator_guard {
                    emulator.get_signal_states()
                } else {
                    (false, false, false, false) // Возвращаем false, если эмулятор не инициализирован
                };

            let response_json = format!(
                r#"{{"ccp": "{}", "hok": "{}", "ksl": "{}", "nd1": "{}", "dob": "{}"}}"#,
                if ccp_state { "HIGH" } else { "LOW" },
                if hok_state { "HIGH" } else { "LOW" },
                if ksl_state { "HIGH" } else { "LOW" },
                if nd1_state { "HIGH" } else { "LOW" },
                if dob_state { "HIGH" } else { "LOW" }
            );
            let mut resp = _req.into_ok_response()?;
            resp.write_all(response_json.as_bytes())?;
            Ok(())
        },
    );

    // Запуск вязания
    server.fn_handler(
        "/start_knitting",
        Method::Post,
        |req| -> anyhow::Result<()> {
            let status_binding = KNITTING_STATUS.lock().unwrap();
            let mut status = status_binding.borrow_mut();
            status.is_knitting = true;
            status.current_row = 0;
            status.current_column = 0;
            status.last_error = None;

            // Запускаем вязание в отдельном потоке
            std::thread::spawn(|| {
                let mut emulator_guard = EMULATOR_INSTANCE.lock().unwrap();
                if let Some(ref mut emulator) = *emulator_guard {
                    add_log_to_buffer("INFO", "Запуск вязания из HTTP-обработчика...");
                    emulator.start_knitting();
                } else {
                    add_log_to_buffer(
                        "ERROR",
                        "Emulator not initialized when starting knitting thread",
                    );
                    // Обновляем статус ошибки в основном потоке
                    {
                        let err_binding = KNITTING_STATUS.lock().unwrap();
                        let mut err_status = err_binding.borrow_mut();
                        err_status.is_knitting = false;
                        err_status.last_error = Some("Emulator not initialized".to_string());
                    }
                }
            });

            req.into_ok_response()?.write_all(b"OK")?;
            Ok(())
        },
    )?;

    // Остановка вязания
    server.fn_handler(
        "/stop_knitting",
        Method::Post,
        |req| -> anyhow::Result<()> {
            let binding = KNITTING_STATUS.lock().unwrap();
            let mut status = binding.borrow_mut();
            status.is_knitting = false;
            status.last_error = Some("Knitting stopped by user".to_string());
            add_log_to_buffer("ERROR", "Вязание остановлено через HTTP-обработчик");
            req.into_ok_response()?.write_all(b"OK")?;
            Ok(())
        },
    )?;

    // Статус GPIO DOB (для отображения на интерфейсе)
    server.fn_handler("/status", Method::Get, |_req| -> anyhow::Result<()> {
        // Читаем логическое состояние из атомарной переменной
        let is_active = DOB_LOGICAL_STATE.load(Ordering::Relaxed); // Используем Relaxed, т.к. синхронизация не критична для отображения

        let status_message = if is_active {
            "Активна (тянем вниз, линия = 0В)"
        } else {
            "Неактивна (покой, линия = 5В)"
        };

        let response_json = format!("{{\"status\": \"{}\"}}", status_message);
        let mut resp = _req.into_ok_response()?;
        resp.write_all(response_json.as_bytes())?;
        Ok(())
    })?;

    info!("HTTP Server created with GPIO control handlers.");

    core::mem::forget(wifi);
    core::mem::forget(server);

    Ok(())
}
