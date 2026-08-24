use std::{
    ffi::{CStr, CString},
    os::raw::c_char,
    ptr,
    sync::LazyLock,
};

use crate::{
    log_fmt,
    logger::log,
    state::{
        BUFFER_SIZE, GLOBAL_ROW,
    },
};

use anyhow::Context;
use esp_idf_sys::{
    ESP_OK, esp_err_to_name, esp_http_client,
    esp_http_client_close, esp_http_client_config_t,
    esp_http_client_fetch_headers, esp_http_client_init,
    esp_http_client_method_t_HTTP_METHOD_GET, esp_http_client_method_t_HTTP_METHOD_POST,
    esp_http_client_open, esp_http_client_read_response, esp_http_client_set_header,
    esp_http_client_set_method, esp_http_client_set_url,
    esp_http_client_transport_t_HTTP_TRANSPORT_OVER_TCP, esp_http_client_write,
};
// ✅ IP сервера: инициализируется ОДИН РАЗ и живёт ВСЮ ЖИЗНЬ программы.
static SERVER_IP: LazyLock<CString> = LazyLock::new(|| CString::new("192.168.1.101").unwrap());

pub fn get_server_ip_ptr() -> *const c_char {
    SERVER_IP.as_ptr()
}

pub fn get_server_ip_str() -> &'static str {
    SERVER_IP.to_str().unwrap()
}

fn check_server_restart_flag(client: *mut esp_http_client) -> bool {
    unsafe {
        let url_str = format!("http://{}:6666/check_restart", get_server_ip_str());
        let c_url = match CString::new(url_str) {
            Ok(u) => u,
            Err(_) => { log("ERROR", "Failed to create check_restart URL"); return false; }
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
            if let Ok(data_str) = String::from_utf8(output_buffer[..data_read as usize].to_vec()) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data_str) {
                    if let Some(restart) = json.get("restart").and_then(|v| v.as_bool()) {
                        return restart;
                    }
                }
            }
        }
        false
    }
}

pub fn check_if_restart(client: *mut esp_http_client) {
    log("INFO", "Checking if restart is required from server...");
    let should_restart = check_server_restart_flag(client);

    if should_restart {
        log("INFO", "Server requires restart - resetting progress to 0");
        GLOBAL_ROW.store(0, core::sync::atomic::Ordering::Relaxed);
        if let Ok(mut nvs_guard) = crate::state::KNIT_NVS.lock() {
            if let Some(nvs) = nvs_guard.as_mut() {
                let _ = nvs.set_str(crate::state::KEY_GLOBAL_ROW, "0");
                log("INFO", "NVS progress cleared");
            }
        }
        crate::knit_state::reset_progress();
    } else {
        log("INFO", "No restart required, continuing from saved progress");
    }
}

// ✅ ПЕРЕИМЕНОВАНО: process_chunk_data -> process_pattern_data
pub fn process_pattern_data(data: &[u8]) -> bool {
    if data.len() < 6 {
        log("ERROR", "Data too short for pattern header");
        return false;
    }
    if &data[0..2] != b"PK" {
        log("ERROR", "Invalid pattern magic");
        return false;
    }
    
    let version = u16::from_le_bytes([data[2], data[3]]);
    if version != 1 {
        log("ERROR", "Unsupported pattern version");
        return false;
    }
    
    let row_count = u16::from_le_bytes([data[4], data[5]]) as usize;
    
    // ✅ ЗАЩИТНЫЕ ПРОВЕРКИ РАЗМЕРОВ
    if row_count > 400 {
        log("ERROR", "Pattern has more than 400 rows");
        return false;
    }

    let mut rows = Vec::with_capacity(row_count);
    let mut offset = 6;
    let mut max_width = 0;
    
    for _ in 0..row_count {
        if offset + 2 > data.len() {
            log("ERROR", "Unexpected end of data reading row width");
            return false;
        }
        let width = u16::from_le_bytes([data[offset], data[offset+1]]) as usize;
        offset += 2;
        if width > max_width { max_width = width; }
        
        if max_width > 200 {
            log("ERROR", "Pattern is wider than 200 needles");
            return false;
        }
        
        let byte_count = (width + 7) / 8;
        if offset + byte_count > data.len() {
            log("ERROR", "Unexpected end of data reading row bits");
            return false;
        }
        
        let mut row = Vec::with_capacity(width);
        for i in 0..width {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            let bit = (data[offset + byte_idx] >> bit_idx) & 1;
            row.push(bit == 1);
        }
        rows.push(row);
        offset += byte_count;
    }
    
    crate::pattern::replace_pattern(rows, max_width, row_count);
    log_fmt!("INFO", "Pattern snapshot loaded: {} rows, width={}", row_count, max_width);
    true
}

pub fn request_pattern_snapshot(client: *mut esp_http_client) -> Result<bool, anyhow::Error> {
    unsafe {
        let url_str = format!("http://{}:6666/full_pattern", get_server_ip_str());
        let c_url = CString::new(url_str).context("Не удалось создать CString для URL")?;
        
        esp_http_client_set_url(client, c_url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_GET);
        
        let err = esp_http_client_open(client, 0);
        if err != ESP_OK {
            esp_http_client_close(client);
            let err_name = CStr::from_ptr(esp_err_to_name(err)).to_string_lossy().into_owned();
            return Err(anyhow::anyhow!("ESP-IDF error: {} (код: {})", err_name, err))
                .context("esp_http_client_open завершился с ошибкой");
        }
        
        let content_length = esp_http_client_fetch_headers(client);
        if content_length < 0 {
            esp_http_client_close(client);
            return Err(anyhow::anyhow!("content_length = {}", content_length))
                .context("esp_http_client_fetch_headers не смог получить заголовки");
        }
        
        // ✅ БЕЗОПАСНЫЙ БУФЕР: Выделяем в heap (через Vec), а не на стеке!
        let mut output_buffer: Vec<u8> = vec![0u8; BUFFER_SIZE as usize];
        
        let data_read = esp_http_client_read_response(
            client, 
            output_buffer.as_mut_ptr(), 
            BUFFER_SIZE
        );
        
        esp_http_client_close(client);
        if data_read <= 0 {
            return Err(anyhow::anyhow!("data_read = {}", data_read))
                .context("Не удалось прочитать данные ответа");
        }
        
        let data_slice = &output_buffer[0..data_read as usize];
        log("INFO", &format!("Received {} bytes from server", data_read));
        
        // ✅ Вызываем переименованную функцию
        if process_pattern_data(data_slice) {
            log("INFO", "Pattern snapshot loaded from server");
            return Ok(true);
        }
        
        Err(anyhow::anyhow!("process_pattern_data вернул false"))
            .context("Не удалось обработать полученные данные паттерна")
    }
}


pub fn queue_row_info(row: i32, direction: bool) {
    crate::state::ROW_INFO_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
    crate::state::ROW_INFO_ROW.store(row, core::sync::atomic::Ordering::Relaxed);
    crate::state::ROW_INFO_DIR.store(direction, core::sync::atomic::Ordering::Relaxed);
}

pub fn send_queued_row_info(client: *mut esp_http_client) -> bool {
    let row = crate::state::ROW_INFO_ROW.load(core::sync::atomic::Ordering::Relaxed);
    let direction = crate::state::ROW_INFO_DIR.load(core::sync::atomic::Ordering::Relaxed);
    unsafe {
        let dir_str = if direction { "right" } else { "left" };
        let url_str = format!("http://{}:6666/row_info", get_server_ip_str());
        let c_url = match CString::new(url_str) {
            Ok(u) => u,
            Err(_) => { log("ERROR", "Failed to create URL for row_info"); return false; }
        };
        
        let body = format!("{{\"row\":{},\"dir\":\"{}\"}}", row, dir_str);
        let body_bytes = body.as_bytes();

        esp_http_client_set_url(client, c_url.as_ptr());
        esp_http_client_set_method(client, esp_http_client_method_t_HTTP_METHOD_POST);

        let hdr_name = CString::new("Content-Type").unwrap();
        let hdr_val = CString::new("application/json").unwrap();
        esp_http_client_set_header(client, hdr_name.as_ptr(), hdr_val.as_ptr());

        let err = esp_http_client_open(client, body_bytes.len() as i32);
        if err != ESP_OK {
            esp_http_client_close(client);
            return false;
        }

        let written = esp_http_client_write(client, body_bytes.as_ptr() as *const u8, body_bytes.len() as i32);
        if written < 0 {
            esp_http_client_close(client);
            return false;
        }

        let mut discard_buf = [0u8; 256];
        let _ = esp_http_client_read_response(client, discard_buf.as_mut_ptr(), 256);
        esp_http_client_close(client);
        true
    }
}

pub fn create_client() -> *mut esp_http_client {
    let mut config: esp_http_client_config_t = esp_http_client_config_t::default();
    
    config.host = get_server_ip_ptr();
    config.path = b"/\0".as_ptr() as *const c_char;
    config.transport_type = esp_http_client_transport_t_HTTP_TRANSPORT_OVER_TCP;
    config.event_handler = None; // Отключаем, так как читаем вручную
    config.user_data = ptr::null_mut();
    
    unsafe {
        let client = esp_http_client_init(&config);
        client
    }
}