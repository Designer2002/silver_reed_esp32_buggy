use std::{ffi::CString, ptr, sync::LazyLock};

use crate::{
    log_fmt,
    logger::log,
    state::{
        BUFFER_SIZE, CHUNK_LOADING, CHUNK_RETRY_DEADLINE_US, CHUNK_RETRY_INTERVAL_US,
        CHUNK_RETRY_PENDING, CHUNK_RETRY_ROW, CHUNK_SIZE, CURRENT_CHUNK_START_ROW, GLOBAL_ROW,
        MAX_HTTP_OUTPUT_BUFFER, REQUEST_NEW_CHUNK, ROWS_IN_CURRENT_CHUNK,
    },
};
use esp_idf_sys::{
    esp_err_t, esp_http_client, esp_http_client_cleanup, esp_http_client_close,
    esp_http_client_config_t, esp_http_client_event_t, esp_http_client_fetch_headers,
    esp_http_client_init, esp_http_client_is_chunked_response,
    esp_http_client_method_t_HTTP_METHOD_GET, esp_http_client_method_t_HTTP_METHOD_POST,
    esp_http_client_open, esp_http_client_read_response, esp_http_client_set_header,
    esp_http_client_set_method, esp_http_client_set_redirection, esp_http_client_set_url,
    esp_http_client_transport_t_HTTP_TRANSPORT_OVER_TCP, esp_http_client_write,
    esp_tls_error_handle_t, esp_tls_get_and_clear_last_error, ESP_FAIL, ESP_OK, TICKS_PER_US_ROM,
};
use heapless::mpmc;

// Очередь для полученных данных от сервера.
// heapless::mpmc::Queue требует размер > 1, поэтому оставляем 2, чтобы не накапливать heap.
static DATA_QUEUE: LazyLock<mpmc::Queue<Vec<u8>, 2>> = LazyLock::new(|| mpmc::Queue::default());

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

pub fn trim_memory() {
    for _ in 0..2 {
        let _ = DATA_QUEUE.dequeue();
    }

    crate::logger::trim_logs();

    let mut next = crate::state::NEXT_CHUNK_ROWS.lock().unwrap();
    if next.as_ref().is_some() {
        let rows = next.as_ref().unwrap().len();
        if rows > 2 {
            *next = None;
        }
    }
}

fn close_client(client: *mut esp_http_client) {
    unsafe {
        esp_http_client_close(client);
        esp_http_client_cleanup(client);
    }
}

/// Проверить, нужно ли начать с нуля (restart флаг от сервера)
/// Возвращает true если сервер требует сбросить прогресс
fn check_server_restart_flag() -> bool {
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

        let temp_client = create_client(c_url.as_ptr());
        if temp_client.is_null() {
            log(
                "ERROR",
                "Failed to create temporary client for restart check",
            );
            return false;
        }

        esp_http_client_set_url(temp_client, c_url.as_ptr());
        esp_http_client_set_method(temp_client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(temp_client, 0);
        if err != ESP_OK {
            log("WARN", "Failed to open check_restart connection");
            close_client(temp_client);
            return false;
        }

        esp_http_client_fetch_headers(temp_client);

        let mut output_buffer = [0u8; 256];
        let data_read = esp_http_client_read_response(temp_client, output_buffer.as_mut_ptr(), 256);
        close_client(temp_client);

        if data_read > 0 {
            let payload = &output_buffer[..data_read as usize];
            if let Ok(response) = serde_json::from_slice::<RestartResponse>(payload) {
                return response.restart.unwrap_or(false);
            }
        }

        false
    }
}

/// Проверить, нужно ли начинать с 0
/// Если сервер вернул restart=true - сбрасываем весь прогресс
pub fn check_if_restart() {
    log("INFO", "Checking if restart is required from server...");

    let should_restart = check_server_restart_flag();

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
pub fn send_chunk_request(start_row: i32) -> bool {
    unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/chunk?row={}", ip, start_row);
        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => {
                log("ERROR", "Failed to create check_restart URL");
                return false;
            }
        };

        let temp_client = create_client(c_url.as_ptr());
        if temp_client.is_null() {
            log(
                "ERROR",
                "Failed to create temporary client for chunk request",
            );
            return false;
        }

        esp_http_client_set_url(temp_client, c_url.as_ptr());
        esp_http_client_set_method(temp_client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(temp_client, 0);
        if err != ESP_OK {
            let error_msg = format!("Failed to open HTTP connection: {}", err);
            log("ERROR", &error_msg);
            drop(error_msg);
            close_client(temp_client);
            return false;
        }

        let content_length = esp_http_client_fetch_headers(temp_client);
        if content_length < 0 {
            let error_msg = format!("HTTP fetch headers failed: {}", content_length);
            log("ERROR", &error_msg);
            drop(error_msg);
            close_client(temp_client);
            return false;
        }

        let mut output_buffer = [0u8; BUFFER_SIZE as usize];
        let data_read =
            esp_http_client_read_response(temp_client, output_buffer.as_mut_ptr(), BUFFER_SIZE);

        close_client(temp_client);

        if data_read >= 0 {
            let data: Vec<u8> = output_buffer[..data_read as usize].to_vec();

            for _ in 0..2 {
                let _ = DATA_QUEUE.dequeue();
            }

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

#[derive(serde::Deserialize, Debug)]
struct ChunkResponse {
    rows: Vec<Vec<u8>>,
    #[serde(default)]
    start_row: Option<usize>,
    #[serde(default)]
    total_rows: Option<i64>,
    #[serde(default)]
    complete: Option<bool>,
    #[serde(default)]
    reset: Option<bool>,
}

#[derive(serde::Deserialize, Debug)]
struct RestartResponse {
    #[serde(default)]
    restart: Option<bool>,
}

/// Обработать полученные данные и обновить паттерн.
/// Важно: не строим `serde_json::Value`, потому что на ESP32 это даёт OOM даже на небольших чанках.
pub fn process_chunk_data(data: Vec<u8>) -> bool {
    if data.is_empty() {
        log("WARN", "Empty chunk payload received");
        return false;
    }

    if data.len() > MAX_HTTP_OUTPUT_BUFFER {
        log(
            "WARN",
            "Chunk payload too large for ESP32 heap; dropping it",
        );
        return false;
    }

    let response: ChunkResponse = match serde_json::from_slice(&data) {
        Ok(v) => v,
        Err(e) => {
            let error_msg = format!("Failed to parse chunk JSON: {} (len={})", e, data.len());
            log("ERROR", &error_msg);
            drop(error_msg);
            return false;
        }
    };

    let start_row = response.start_row.unwrap_or(0);

    // ✅ Сохраняем total_rows из ответа сервера.
    if let Some(total) = response.total_rows {
        crate::state::PATTERN_HEIGHT.store(total as i32, core::sync::atomic::Ordering::Relaxed);
    }

    // ✅ Проверяем flag "complete" — все ряды отправлены.
    if response.complete.unwrap_or(false) {
        log("INFO", "All rows sent by server — pattern complete!");
        crate::knit_state::reset_progress();
        crate::tasks::reset_knitting();
    }

    // ✅ Проверяем flag "reset" — команда сервера сбросить прогресс.
    if response.reset.unwrap_or(false) {
        log("INFO", "Server sent RESET command — resetting progress!");
        crate::knit_state::reset_progress();
    }

    let parsed_rows: Vec<Vec<bool>> = response
        .rows
        .into_iter()
        .map(|row| row.into_iter().map(|v| v != 0).collect())
        .collect();

    let width = parsed_rows.first().map(|r| r.len()).unwrap_or(0);
    if width == 0 {
        log("ERROR", "Empty chunk received");
        return false;
    }

    // Сохраняем incoming chunk в NEXT_CHUNK_ROWS — активный PATTERN меняется только после swap на KSL FALL.
    crate::pattern::store_next_chunk(parsed_rows, start_row, width);

    // Для самого первого чанка сразу активируем PATTERN, чтобы не было пустого активного паттерна до первого KSL.
    if crate::state::PATTERN_END.load(core::sync::atomic::Ordering::Relaxed) == 0
        && crate::pattern::PATTERN.lock().unwrap().rows.is_empty()
    {
        let _ = crate::pattern::swap_to_next_chunk();
    }

    // ✅ Если это начальный чанк — обновляем PATTERN_START/END.
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
pub fn check_and_request_chunk() {
    // ✅ Сначала проверяем retry
    if CHUNK_RETRY_PENDING.load(core::sync::atomic::Ordering::Relaxed) {
        let now = unsafe { esp_idf_sys::esp_timer_get_time() } as u32;
        let deadline = CHUNK_RETRY_DEADLINE_US.load(core::sync::atomic::Ordering::Relaxed);
        if now >= deadline {
            let row = CHUNK_RETRY_ROW.load(core::sync::atomic::Ordering::Relaxed);
            let msg = format!("Retrying chunk starting at row {}", row);
            log("INFO", &msg);
            drop(msg);

            if send_chunk_request(row) {
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

        if send_chunk_request(next_start) {
            // Запрос нового чанка не означает, что активный chunk уже поменялся.
            // Переключение происходит только в KSL FALL после завершения текущего ряда.
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
pub fn request_initial_chunk() {
    CHUNK_LOADING.store(true, core::sync::atomic::Ordering::Relaxed);
    CURRENT_CHUNK_START_ROW.store(0, core::sync::atomic::Ordering::Relaxed);
    ROWS_IN_CURRENT_CHUNK.store(0, core::sync::atomic::Ordering::Relaxed);

    log("INFO", "Requesting initial chunk (rows 0-3)");

    // ✅ НЕ меняем CURRENT_CHUNK_START_ROW здесь — он останется 0
    // Ряды 0-3 загружаются, и CURRENT_CHUNK_START_ROW должен оставаться 0
    // пока мы фактически не провяжем эти ряды
    if send_chunk_request(0) {
        log("INFO", "Initial chunk request succeeded");
    } else {
        log("ERROR", "Initial chunk request failed");
        CHUNK_RETRY_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
        CHUNK_RETRY_ROW.store(0, core::sync::atomic::Ordering::Relaxed);
        CHUNK_RETRY_DEADLINE_US.store(
            unsafe { esp_idf_sys::esp_timer_get_time() } as u32 + CHUNK_RETRY_INTERVAL_US,
            core::sync::atomic::Ordering::Relaxed,
        );
    }

    CHUNK_LOADING.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Отправить флаг на сервер (когда входим в 3-й ряд)
/// Можно использовать для уведомления сервера
pub fn send_ready_flag(row: i32) -> bool {
    unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/ready?row={}", ip, row);
        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => {
                log("ERROR", "Failed to create check_restart URL");
                return false;
            }
        };

        let temp_client = create_client(c_url.as_ptr());
        if temp_client.is_null() {
            log(
                "ERROR",
                "Failed to create temporary client for ready flag sending",
            );
            return false;
        }

        esp_http_client_set_url(temp_client, c_url.as_ptr());
        esp_http_client_set_method(temp_client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(temp_client, 0);
        if err != ESP_OK {
            close_client(temp_client);
            return false;
        }

        esp_http_client_fetch_headers(temp_client);
        close_client(temp_client);
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
pub fn send_queued_row_info() -> bool {
    let row = crate::state::ROW_INFO_ROW.load(core::sync::atomic::Ordering::Relaxed);
    let direction = crate::state::ROW_INFO_DIR.load(core::sync::atomic::Ordering::Relaxed);
    unsafe {
        let ip = get_server_ip();
        let dir_str = if direction { "right" } else { "left" };
        let url = format!("http://{}:6666/row_info?row={}&dir={}", ip, row, dir_str);
        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => {
                log("ERROR", "Failed to create check_restart URL");
                return false;
            }
        };

        let temp_client = create_client(c_url.as_ptr());
        if temp_client.is_null() {
            log("ERROR", "Failed to create temporary client for row info");
            return false;
        }

        esp_http_client_set_url(temp_client, c_url.as_ptr());
        esp_http_client_set_method(temp_client, esp_http_client_method_t_HTTP_METHOD_GET);

        let err = esp_http_client_open(temp_client, 0);
        if err != ESP_OK {
            close_client(temp_client);
            return false;
        }

        let mut discard_buf = [0u8; 256];
        let _ = esp_http_client_read_response(temp_client, discard_buf.as_mut_ptr(), 256);

        close_client(temp_client);
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

pub fn create_client(url: *const u8) -> *mut esp_http_client {
    let mut config: esp_http_client_config_t = esp_http_client_config_t::default();
    config.url = url;
    config.transport_type = esp_http_client_transport_t_HTTP_TRANSPORT_OVER_TCP;
    config.event_handler = Some(http_event_handler);

    // Не привязываем host/path к временной строке. Каждый HTTP-запрос задаёт полный URL через set_url,
    // а `esp_http_client_init()` получает только безопасный базовый конфиг.
    unsafe { esp_http_client_init(&config) }
}

/// Отправить один solenoid hit на сервер с частотой ~1 ms.
/// Это избегает большого JSON-пакета и переполнения стека.
pub fn send_solenoid_hits() -> usize {
    use crate::state::SOLENOID_HITS;

    let Some(hit) = SOLENOID_HITS.recv_front(TICKS_PER_US_ROM * 1000) else {
        return 0;
    };

    let h = hit.0;
    let mut json = String::with_capacity(96);
    json.push_str("{\"hits\":[");
    json.push_str(&format!(
        "{{\"r\":{},\"n\":{},\"f\":{},\"d\":{}}}",
        h.row,
        h.needle,
        if h.actual_fire { 1 } else { 0 },
        if h.direction { 1 } else { 0 }
    ));
    json.push_str("]}");

    let sent = unsafe {
        let ip = get_server_ip();
        let url = format!("http://{}:6666/solenoid_hits", ip);
        let c_url = match CString::new(url) {
            Ok(u) => u,
            Err(_) => {
                log(
                    "ERROR",
                    "Failed to create temporary client for solenoid hits info sending",
                );
                return 0;
            }
        };
        let temp_client = create_client(c_url.as_ptr());
        if temp_client.is_null() {
            return 0;
        }
        let body_bytes = json.as_bytes();
        esp_http_client_set_url(temp_client, c_url.as_ptr());
        esp_http_client_set_method(temp_client, esp_http_client_method_t_HTTP_METHOD_POST);
        let hdr_name = CString::new("Content-Type").unwrap();
        let hdr_val = CString::new("application/json").unwrap();
        esp_http_client_set_header(temp_client, hdr_name.as_ptr(), hdr_val.as_ptr());

        let body_ptr = body_bytes.as_ptr() as *const core::ffi::c_void;
        let err = esp_http_client_open(temp_client, body_bytes.len() as i32);
        if err != ESP_OK {
            log("ERROR", "Failed to open esp http client");
            close_client(temp_client);
            return 0;
        }

        let written =
            esp_http_client_write(temp_client, body_ptr as *const u8, body_bytes.len() as i32);
        if written < 0 {
            log("ERROR", "Failed to write solenoid_hits body");
            close_client(temp_client);
            return 0;
        }

        let _ = esp_http_client_fetch_headers(temp_client);
        let mut discard = [0u8; 128];
        let _ = esp_http_client_read_response(temp_client, discard.as_mut_ptr(), 128);
        close_client(temp_client);
        1
    };

    json.clear();
    drop(json);
    sent
}
