use std::{
    borrow::Cow,
    ffi::{CStr, CString},
    ptr,
    sync::LazyLock,
};

use crate::{
    log_fmt, logger::log, pattern::update_pattern_chunk, state::{
        BUFFER_SIZE, CHUNK_LOADING, CHUNK_RETRY_DEADLINE_US, CHUNK_RETRY_INTERVAL_US, CHUNK_RETRY_PENDING, CHUNK_RETRY_ROW, CHUNK_SIZE, CURRENT_CHUNK_START_ROW, GLOBAL_ROW, MAX_HTTP_OUTPUT_BUFFER, REQUEST_NEW_CHUNK, ROWS_IN_CURRENT_CHUNK, WIDTH,
    },
};
use esp_idf_sys::{
    ESP_FAIL, ESP_OK, TICKS_PER_US_ROM, esp_err_t, esp_http_client, esp_http_client_cleanup, esp_http_client_close, esp_http_client_config_t, esp_http_client_event_t, esp_http_client_fetch_headers, esp_http_client_init, esp_http_client_is_chunked_response, esp_http_client_method_t_HTTP_METHOD_GET, esp_http_client_method_t_HTTP_METHOD_POST, esp_http_client_open, esp_http_client_read_response, esp_http_client_set_header, esp_http_client_set_method, esp_http_client_set_redirection, esp_http_client_set_url, esp_http_client_transport_t_HTTP_TRANSPORT_OVER_TCP, esp_http_client_write, esp_tls_error_handle_t, esp_tls_get_and_clear_last_error,
};
use heapless::mpmc;

// Очередь для полученных данных от сервера
static DATA_QUEUE: LazyLock<mpmc::Queue<Vec<u8>, 8>> = LazyLock::new(|| mpmc::Queue::default());

// IP сервера - настраивается при инициализации
static mut SERVER_IP: [u8; 16] = [0u8; 16];

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpEventId {
    Error = 0,
    OnConnected = 1,
    HeadersSent = 2,
    OnHeader = 3,
    OnHeadersComplete = 4,
    OnStatusCode = 5,
    OnData = 6,
    OnFinish = 7,
    Disconnected = 8,
    Redirect = 9,
}

impl From<u32> for HttpEventId {
    fn from(value: u32) -> Self {
        match value {
            0 => HttpEventId::Error,
            1 => HttpEventId::OnConnected,
            2 => HttpEventId::HeadersSent,
            3 => HttpEventId::OnHeader,
            4 => HttpEventId::OnHeadersComplete,
            5 => HttpEventId::OnStatusCode,
            6 => HttpEventId::OnData,
            7 => HttpEventId::OnFinish,
            8 => HttpEventId::Disconnected,
            9 => HttpEventId::Redirect,
            _ => HttpEventId::Error, // default
        }
    }
}

pub unsafe extern "C" fn http_event_handler(evt: *mut esp_http_client_event_t) -> esp_err_t {
    if evt.is_null() {
        return ESP_FAIL;
    }

    let event = &*evt;
    let event_id: HttpEventId = event.event_id.into();

    match event_id {
        HttpEventId::Error => {
            log("ERROR", "HTTP_EVENT_ERROR");
        }

        HttpEventId::OnConnected => {
            // log("DEBUG", "HTTP_EVENT_ON_CONNECTED"); // слишком шумно
        }

        HttpEventId::HeadersSent => {
            // log("DEBUG", "HTTP_EVENT_HEADER_SENT");
        }

        HttpEventId::OnHeader => {
            // log("DEBUG", "HTTP_EVENT_ON_HEADER"); // слишком шумно
            // let _key = if !event.header_key.is_null() {
            //     CStr::from_ptr(event.header_key).to_string_lossy()
            // } else {
            //     Cow::from("")
            // };
            // let value = if !event.header_value.is_null() {
            //     CStr::from_ptr(event.header_value).to_string_lossy()
            // } else {
            //     Cow::from("")
            // };
            // // log_fmt!(
            // //     "DEBUG",
            // //     "HTTP_EVENT_ON_HEADER, key={}, value={}",
            // //     _key,
            // //     value
            // // );
        }

        HttpEventId::OnHeadersComplete => {
            // log("DEBUG", "HTTP_EVENT_ON_HEADERS_COMPLETE");
        }

        HttpEventId::OnData => {
            // log("DEBUG", "HTTP_EVENT_ON_DATA, len={}", event.data_len); // слишком шумно

            let is_chunked = esp_http_client_is_chunked_response(event.client);

            if !is_chunked && !event.user_data.is_null() {
                let user_data = &mut *(event.user_data as *mut HttpEventContext);

                let copy_len = core::cmp::min(
                    event.data_len as usize,
                    MAX_HTTP_OUTPUT_BUFFER - user_data.output_len,
                );

                if copy_len > 0 && !event.data.is_null() {
                    let data_slice = core::slice::from_raw_parts(event.data as *const u8, copy_len);
                    user_data.output_buffer.extend_from_slice(data_slice);
                    user_data.output_len += copy_len;
                }
            }
        }

        HttpEventId::OnFinish => {
            log("DEBUG", "HTTP_EVENT_ON_FINISH");
            if !event.user_data.is_null() {
                let user_data = &mut *(event.user_data as *mut HttpEventContext);
                if !user_data.output_buffer.is_empty() {
                    log_fmt!("INFO", "Response received: {} bytes", user_data.output_len);
                    log_fmt!(
                        "DEBUG",
                        "Response data: {:?}",
                        &user_data.output_buffer[..user_data.output_len]
                    );
                    user_data.output_buffer.clear();
                }
                user_data.output_len = 0;
            }
        }

        HttpEventId::Disconnected => {
            log("INFO", "HTTP_EVENT_DISCONNECTED");

            let mut mbedtls_err: i32 = 0;
            let err = esp_tls_get_and_clear_last_error(
                event.data as esp_tls_error_handle_t,
                &mut mbedtls_err,
                ptr::null_mut(),
            );

            if err != 0 {
                log_fmt!("INFO", "Last esp error code: 0x{:x}", err);
                log_fmt!("INFO", "Last mbedtls failure: 0x{:x}", mbedtls_err);
            }

            if !event.user_data.is_null() {
                let user_data = &mut *(event.user_data as *mut HttpEventContext);
                user_data.output_buffer.clear();
                user_data.output_len = 0;
            }
        }

        HttpEventId::Redirect => {
            log("DEBUG", "HTTP_EVENT_REDIRECT");
            let from = CString::new("user@example.com").unwrap();
            let accept = CString::new("text/html").unwrap();
            esp_http_client_set_header(event.client, from.as_ptr(), accept.as_ptr());
            esp_http_client_set_redirection(event.client);
        }

        HttpEventId::OnStatusCode => {
            log("DEBUG", "HTTP_EVENT_ON_STATUS_CODE");
            // Можно получить status code если нужно
        }
    }

    ESP_OK
}

/// Инициализация IP адреса сервера
pub fn init_server_ip(ip: &str) {
    unsafe {
        let bytes = ip.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if i >= 15 {
                break;
            }
            SERVER_IP[i] = b;
        }
        SERVER_IP[15] = 0; // null terminator
    }
}

/// Получить IP сервера как строку
pub fn get_server_ip() -> String {
    unsafe {
        core::ffi::CStr::from_bytes_until_nul(&SERVER_IP)
            .unwrap_or(core::ffi::CStr::from_bytes_until_nul(b"192.168.1.100\0").unwrap())
            .to_string_lossy()
            .into_owned()
    }
}

pub fn cleanup_client(client: *mut esp_http_client) {
    unsafe {
        esp_http_client_cleanup(client);
    }
}

/// Проверить, нужно ли начать с нуля (restart флаг от сервера)
/// Возвращает true если сервер требует сбросить прогресс
fn check_server_restart_flag(client: *mut esp_http_client) -> bool {
    unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/check_restart", ip);
        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => {
                log("ERROR", "Failed to create check_restart URL");
                return false;
            }
        };

        esp_http_client_set_url(client, c_url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(client, 0);
        if err != ESP_OK {
            log("WARN", "Failed to open check_restart connection");
            esp_http_client_close(client);
            return false;
        }

        esp_http_client_fetch_headers(client);

        let mut output_buffer = [0u8; 256];
        let data_read = esp_http_client_read_response(client, output_buffer.as_mut_ptr(), 256);
        esp_http_client_close(client);

        if data_read > 0 {
            let data_str = match String::from_utf8(output_buffer[..data_read as usize].to_vec()) {
                Ok(s) => s,
                Err(_) => return false,
            };

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data_str) {
                if let Some(restart) = json.get("restart").and_then(|v| v.as_bool()) {
                    return restart;
                }
            }
        }

        false
    }
}

/// Проверить, нужно ли начинать с 0
/// Если сервер вернул restart=true - сбрасываем весь прогресс
pub fn check_if_restart(client: *mut esp_http_client) {
    log("INFO", "Checking if restart is required from server...");

    let should_restart = check_server_restart_flag(client);

    if should_restart {
        log("INFO", "Server requires restart - resetting progress to 0");
        GLOBAL_ROW.store(0, core::sync::atomic::Ordering::Relaxed);
        CURRENT_CHUNK_START_ROW.store(0, core::sync::atomic::Ordering::Relaxed);
        ROWS_IN_CURRENT_CHUNK.store(0, core::sync::atomic::Ordering::Relaxed);

        // Сбрасываем NVS
        if let Ok(mut nvs_guard) = crate::state::KNIT_NVS.lock() {
            if let Some(nvs) = nvs_guard.as_mut() {
                let _ = nvs.set_str(crate::state::KEY_GLOBAL_ROW, "0");
                let _ = nvs.set_str(crate::state::KEY_CHUNK_START, "0");
                let _ = nvs.set_str(crate::state::KEY_ROWS_IN_CHUNK, "0");
                log("INFO", "NVS progress cleared");
            }
        }

        crate::knit_state::reset_progress();
    } else {
        log(
            "INFO",
            "No restart required, continuing from saved progress",
        );
    }
}

/// Отправить запрос на сервер для получения новой части узора
/// start_row: номер ряда, с которого начинается чанк
pub fn send_chunk_request(client: *mut esp_http_client, start_row: i32) -> bool {
    unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/chunk?row={}", ip, start_row);
        let c_url = CString::new(url).unwrap();
        esp_http_client_set_url(client, c_url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(client, 0);
        if err != ESP_OK {
            let error_msg = format!("Failed to open HTTP connection: {}", err);
            log("ERROR", &error_msg);
            // освобождаем строку после логирования, чтобы не держать её в памяти дольше нужного
            drop(error_msg);
            esp_http_client_close(client);
            return false;
        }

        let content_length = esp_http_client_fetch_headers(client);
        if content_length < 0 {
            let error_msg = format!("HTTP fetch headers failed: {}", content_length);
            log("ERROR", &error_msg);
            drop(error_msg);
            esp_http_client_close(client);
            return false;
        }

        // Выделяем буфер для ответа
        let mut output_buffer = Box::new([0u8; BUFFER_SIZE as usize]);
        let data_read =
            esp_http_client_read_response(client, output_buffer.as_mut_ptr(), BUFFER_SIZE);

        esp_http_client_close(client);

        if data_read >= 0 {
            let data: Vec<u8> = output_buffer[0..data_read as usize].to_vec();
            // Отправляем данные в очередь для обработки
            if DATA_QUEUE.enqueue(data).is_err() {
                log("WARN", "Data queue is full, dropping chunk");
                return false;
            }
            return true;
        } else {
            let error_msg = format!("Failed to read response: {}", data_read);
            log("ERROR", &error_msg);
            drop(error_msg);
            return false;
        }
    }
}

/// Обработать полученные данные и обновить паттерн
/// Ожидаемый формат JSON: {"rows": [[0,1,0...], [1,0,1...], ...], "start_row": N}
pub fn process_chunk_data(data: Vec<u8>) -> bool {
    let data_str = match String::from_utf8(data) {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!("Failed to parse UTF8: {}", e);
            log("ERROR", &error_msg);
            drop(error_msg);
            return false;
        }
    };

    // Парсим JSON
    let json_value: serde_json::Value = match serde_json::from_str(&data_str) {
        Ok(v) => v,
        Err(e) => {
            let error_msg = format!("Failed to parse JSON: {} - data: {}", e, data_str);
            log("ERROR", &error_msg);
            drop(error_msg);
            return false;
        }
    };

    let rows_array = match json_value.get("rows").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            log("ERROR", "Invalid JSON: missing 'rows' array");
            return false;
        }
    };

    let start_row = json_value
        .get("start_row")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize;

    // ✅ Сохраняем total_rows из ответа сервера
    if let Some(total) = json_value.get("total_rows").and_then(|v| v.as_i64()) {
        crate::state::PATTERN_HEIGHT.store(total as i32, core::sync::atomic::Ordering::Relaxed);
    }

    // ✅ Проверяем flag "complete" — все ряды отправлены
    if let Some(complete) = json_value.get("complete").and_then(|v| v.as_bool()) {
        if complete {
            log("INFO", "All rows sent by server — pattern complete!");
            crate::knit_state::reset_progress();
            crate::tasks::reset_knitting();
        }
    }

    // ✅ Проверяем flag "reset" — команда сервера сбросить прогресс
    if let Some(reset) = json_value.get("reset").and_then(|v| v.as_bool()) {
        if reset {
            log("INFO", "Server sent RESET command — resetting progress!");
            crate::knit_state::reset_progress();
        }
    }

    // Парсим ряды: [[0,1,0...], [1,0,1...], ...] -> Vec<Vec<bool>>
    let parsed_rows: Vec<Vec<bool>> = rows_array
        .iter()
        .filter_map(|row_val| {
            row_val
                .as_array()
                .map(|arr| arr.iter().map(|v| v.as_i64().unwrap_or(0) != 0).collect())
        })
        .collect();

    let width = parsed_rows.first().map(|r| r.len()).unwrap_or(0);

    if width == 0 {
        log("ERROR", "Empty chunk received");
        return false;
    }

    // Обновляем паттерн
    update_pattern_chunk(parsed_rows, start_row, width);

    // ✅ Если это начальный чанк — обновляем PATTERN_START/END
    // Они были установлены в 0 при start_knitting т.к. паттерн ещё не загружен
    if crate::state::PATTERN_END.load(core::sync::atomic::Ordering::Relaxed) == 0 {
        crate::state::PATTERN_START.store(0, core::sync::atomic::Ordering::Relaxed);
        crate::state::PATTERN_END.store(width as i32 - 1, core::sync::atomic::Ordering::Relaxed);
        crate::state::WIDTH.store(width, core::sync::atomic::Ordering::Relaxed);
    }

    let msg = format!(
        "Chunk updated: local rows 0-{} (global start={}), width={}",
        CHUNK_SIZE - 1,
        start_row,
        width
    );
    log("INFO", &msg);
    drop(msg);

    true
}

/// Проверить и запросить новый чанк если пора
pub fn check_and_request_chunk(client: *mut esp_http_client) {
    // ✅ Сначала проверяем retry
    if CHUNK_RETRY_PENDING.load(core::sync::atomic::Ordering::Relaxed) {
        let now = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
        let deadline = CHUNK_RETRY_DEADLINE_US.load(core::sync::atomic::Ordering::Relaxed);
        if now >= deadline {
            let row = CHUNK_RETRY_ROW.load(core::sync::atomic::Ordering::Relaxed);
            let msg = format!("Retrying chunk starting at row {}", row);
            log("INFO", &msg);
            drop(msg);

            if send_chunk_request(client, row) {
                CURRENT_CHUNK_START_ROW.store(row, core::sync::atomic::Ordering::Relaxed);
                CHUNK_RETRY_PENDING.store(false, core::sync::atomic::Ordering::Relaxed);
                log("INFO", "Chunk retry succeeded");
            } else {
                // Ещё неудача — следующий retry через 5 секунд
                CHUNK_RETRY_DEADLINE_US.store(
                    now + CHUNK_RETRY_INTERVAL_US,
                    core::sync::atomic::Ordering::Relaxed,
                );
            }
        }
        return;
    }

    // ✅ Обычный запрос нового чанка
    if REQUEST_NEW_CHUNK.load(core::sync::atomic::Ordering::Relaxed) {
        if CHUNK_LOADING.load(core::sync::atomic::Ordering::Relaxed) {
            // Уже загружаем, пропускаем
            return;
        }

        CHUNK_LOADING.store(true, core::sync::atomic::Ordering::Relaxed);

        let current_start = CURRENT_CHUNK_START_ROW.load(core::sync::atomic::Ordering::Relaxed);
        let next_start = current_start + CHUNK_SIZE as i32;

        let msg = format!("Requesting chunk starting at row {}", next_start);
        log("INFO", &msg);
        drop(msg);

        if send_chunk_request(client, next_start) {
            // ✅ Обновляем CURRENT_CHUNK_START_ROW только после успешной загрузки
            CURRENT_CHUNK_START_ROW.store(next_start, core::sync::atomic::Ordering::Relaxed);
            // ⚠️ НЕ сбрасываем ROWS_IN_CURRENT_CHUNK здесь — это делается в gpio.rs при KSL fall
            CHUNK_LOADING.store(false, core::sync::atomic::Ordering::Relaxed);
            REQUEST_NEW_CHUNK.store(false, core::sync::atomic::Ordering::Relaxed);
        } else {
            // ❌ Неудача — ставим retry через 5 секунд
            CHUNK_RETRY_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
            CHUNK_RETRY_ROW.store(next_start, core::sync::atomic::Ordering::Relaxed);
            CHUNK_RETRY_DEADLINE_US.store(
                unsafe { esp_idf_sys::esp_timer_get_time() } as u32 + CHUNK_RETRY_INTERVAL_US,
                core::sync::atomic::Ordering::Relaxed,
            );
            CHUNK_LOADING.store(false, core::sync::atomic::Ordering::Relaxed);
            REQUEST_NEW_CHUNK.store(false, core::sync::atomic::Ordering::Relaxed);
            let msg = "Chunk request failed, will retry in 5s";
            log("WARN", msg);
        }
    }
}

/// Получить данные из очереди
pub fn receive_data() -> Option<Vec<u8>> {
    DATA_QUEUE.dequeue()
}

/// Запросить начальный чанк при старте вязания
/// Запрашиваем ряды начиная с 0
pub fn request_initial_chunk(client: *mut esp_http_client) {
    CHUNK_LOADING.store(true, core::sync::atomic::Ordering::Relaxed);
    CURRENT_CHUNK_START_ROW.store(0, core::sync::atomic::Ordering::Relaxed);
    ROWS_IN_CURRENT_CHUNK.store(0, core::sync::atomic::Ordering::Relaxed);

    log("INFO", "Requesting initial chunk (rows 0-3)");

    // ✅ НЕ меняем CURRENT_CHUNK_START_ROW здесь — он останется 0
    // Ряды 0-3 загружаются, и CURRENT_CHUNK_START_ROW должен оставаться 0
    // пока мы фактически не провяжем эти ряды
    send_chunk_request(client, 0);

    CHUNK_LOADING.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Отправить флаг на сервер (когда входим в 3-й ряд)
/// Можно использовать для уведомления сервера
pub fn send_ready_flag(client: *mut esp_http_client, row: i32) -> bool {
    unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/ready?row={}", ip, row);

        esp_http_client_set_url(client, url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(client, 0);
        if err != ESP_OK {
            esp_http_client_close(client);
            return false;
        }

        esp_http_client_fetch_headers(client);
        esp_http_client_close(client);
        true
    }
}

/// Отправить информацию о текущем ряде на сервер
/// row: глобальный номер ряда, direction: true = RIGHT, false = LEFT
/// НЕ использует HTTP — сохраняет в глобальные переменные, client_task отправляет
pub fn queue_row_info(row: i32, direction: bool) {
    crate::state::ROW_INFO_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
    crate::state::ROW_INFO_ROW.store(row, core::sync::atomic::Ordering::Relaxed);
    crate::state::ROW_INFO_DIR.store(direction, core::sync::atomic::Ordering::Relaxed);
}

/// Реально отправить queued row_info через HTTP (вызывается из client_task)
pub fn send_queued_row_info(client: *mut esp_http_client) -> bool {
    let row = crate::state::ROW_INFO_ROW.load(core::sync::atomic::Ordering::Relaxed);
    let direction = crate::state::ROW_INFO_DIR.load(core::sync::atomic::Ordering::Relaxed);
    unsafe {
        let ip = get_server_ip();
        let dir_str = if direction { "right" } else { "left" };
        let url = format!("http://{}:6666/row_info?row={}&dir={}", ip, row, dir_str);

        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => {
                log("ERROR", "Failed to create URL for row_info");
                return false;
            }
        };

        esp_http_client_set_url(client, c_url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(client, 0);
        if err != ESP_OK {
            esp_http_client_close(client);
            return false;
        }

        // ✅ ВАЖНО: читаем response body чтобы буфер очистился
        let mut discard_buf = [0u8; 256];
        let _ = esp_http_client_read_response(client, discard_buf.as_mut_ptr(), 256);

        esp_http_client_close(client);
        true
    }
}

pub struct HttpEventContext {
    pub output_buffer: Vec<u8>,
    pub output_len: usize,
}

impl Default for HttpEventContext {
    fn default() -> Self {
        Self {
            output_buffer: Vec::with_capacity(MAX_HTTP_OUTPUT_BUFFER),
            output_len: 0,
        }
    }
}

pub fn create_client() -> *mut esp_http_client {
    let mut config: esp_http_client_config_t = esp_http_client_config_t::default();
    config.host = get_server_ip().as_ptr();
    config.path = "/".as_ptr();
    config.transport_type = esp_http_client_transport_t_HTTP_TRANSPORT_OVER_TCP;
    config.event_handler = Some(http_event_handler);
    unsafe {
        let client = esp_http_client_init(&config);
        client
    }
}

/// Отправить один solenoid hit на сервер с частотой ~1 ms.
/// Это избегает большого JSON-пакета и переполнения стека.
pub fn send_solenoid_hits(client: *mut esp_http_client) -> usize {
    use crate::state::SOLENOID_HITS;

    let mut hits = Vec::new();
    while hits.len() < WIDTH.load(std::sync::atomic::Ordering::Relaxed){
        if let Some(hit) = SOLENOID_HITS.recv_front() {
            hits.push(hit);
        } else {
            break;
        }
    }
    let mut json = String::with_capacity(96);
    json.push_str("{\"hits\":[");
    let mut first = true;
    for h in hits {
        if !first {
            json.push(',');
        } else {
            first = false;
        }
        let obj = format!(
            "{{\"r\":{},\"n\":{},\"f\":{},\"d\":{}}}",
            h.row,
            h.needle,
            if h.actual_fire { 1 } else { 0 },
            if h.direction { 1 } else { 0 }
        );
        json.push_str(&obj);
    }
    json.push_str("]}");

    let sent = unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/solenoid_hits", ip);
        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => return 0,
        };
        let body_bytes = json.as_bytes();
        esp_http_client_set_url(client, c_url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_POST);
        let hdr_name = CString::new("Content-Type").unwrap();
        let hdr_val = CString::new("application/json").unwrap();
        esp_http_client_set_header(client, hdr_name.as_ptr(), hdr_val.as_ptr());

        let body_ptr = body_bytes.as_ptr() as *const core::ffi::c_void;
        let err = esp_http_client_open(client, body_bytes.len() as i32);
        if err != ESP_OK {
            log("ERROR", "Failed to open esp http client");
            esp_http_client_close(client);
            return 0;
        }

        let written = esp_http_client_write(client, body_ptr as *const u8, body_bytes.len() as i32);
        if written < 0 {
            log("ERROR", "Failed to write solenoid_hits body");
            esp_http_client_close(client);
            return 0;
        }

        let _ = esp_http_client_fetch_headers(client);
        let mut discard = [0u8; 128];
        let _ = esp_http_client_read_response(client, discard.as_mut_ptr(), 128);
        esp_http_client_close(client);
        1
    };

    json.clear();
    drop(json);
    sent
}