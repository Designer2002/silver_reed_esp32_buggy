use std::sync::atomic::Ordering;
use esp_idf_sys::{ESP_OK, esp_http_client, esp_http_client_cleanup, esp_http_client_config_t, esp_http_client_init, esp_http_client_perform};
use local_ip_address::local_ip;
use crate::gpio::get_pin_state_json;
use crate::pattern::KNITTING_PATTERN;
use crate::state::{HEIGHT, INSIDE_PATTERN, KNITTING, NEEDLE, ROW, WIDTH};
use crate::tasks::{start_knitting, stop_knitting};
use crate::logger::{get_logs};
use anyhow::Ok;
use esp_idf_svc::http::server::EspHttpServer;
use esp_idf_svc::io::Write;
use esp_idf_svc::{
    http::Method,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use log::info;
use crate::logger::log;

const SSID: &str = dotenvy_macro::dotenv!("WIFI_SSID");
const PASS: &str = dotenvy_macro::dotenv!("WIFI_PASS");
const INDEX_HTML: &str = include_str!("index.html");
const CSS: &str = include_str!("style.css");

pub fn connect_wifi(wifi: &mut BlockingWifi<EspWifi<'static>>) -> anyhow::Result<()> {
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

pub fn init_server(server: &mut EspHttpServer) -> anyhow::Result<()> {
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

    server.fn_handler(
        "/start_knitting",
        Method::Post,
        |req| -> anyhow::Result<()> {
            start_knitting();
            req.into_ok_response()?.write_all(b"OK")?;
            Ok(())
        },
    )?;
    server.fn_handler(
        "/stop_knitting",
        Method::Post,
        |req| -> anyhow::Result<()> {
            stop_knitting();
            req.into_ok_response()?.write_all(b"OK")?;
            Ok(())
        },
    )?;

    server.fn_handler("/logs", Method::Get, |req| -> anyhow::Result<()> {
        let logs = get_logs();

        let mut json = String::from("[");
        for (i, l) in logs.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }

            json.push_str(&format!(
                r#"{{"t":"{}","lvl":"{}","msg":"{}"}}"#,
                l.timestamp, l.level, l.message
            ));
        }
        json.push(']');

        let headers = [
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-cache"),
        ];

        let mut resp = req.into_response(200, Some("OK"), &headers)?;
        resp.write_all(json.as_bytes())?;
        Ok(())
    })?;
    server.fn_handler("/signal_status", Method::Get, |req| -> anyhow::Result<()> {
        let json = get_pin_state_json();
        let mut resp = req.into_ok_response()?;
        resp.write_all(json.as_bytes())?;
        Ok(())
    })?;

    // Статус вязания
server.fn_handler("/knitting_status", Method::Get, |_req| -> anyhow::Result<()> {
    let row = ROW.load(Ordering::Relaxed);
    let needle = NEEDLE.load(Ordering::Relaxed);
    let width = WIDTH.load(Ordering::Relaxed);
    let height = HEIGHT.load(Ordering::Relaxed);
    let is_knitting = KNITTING.load(Ordering::Relaxed);
    let inside = INSIDE_PATTERN.load(Ordering::Relaxed);

    let response_json = format!(
        r#"{{"currentRow": {}, "currentColumn": {}, "totalRows": {}, "totalColumns": {}, "isKnitting": {}, "insidePattern": {}}}"#,
        row,
        needle,
        height,
        width,
        is_knitting,
        inside
    );
    
    let mut resp = _req.into_ok_response()?;
    resp.write_all(response_json.as_bytes())?;
    Ok(())
})?;

    Ok(())
}

pub unsafe fn create_client() -> *mut esp_http_client{
    let my_local_ip = local_ip().unwrap();
    let mut client_config: esp_http_client_config_t = esp_http_client_config_t::default();
    client_config.host = my_local_ip.to_string().as_ptr() as *const u8;
    client_config.port =6666;
    let client = esp_http_client_init(&client_config);
    let err = esp_http_client_perform(client);
    if err != ESP_OK {
        let msg = Box::leak(Box::new(format!("HTTP client perform failed with error code: {}", err)));
        log("ERROR", msg);
    }
    client
}

pub fn cleanup_client(client: *mut esp_http_client) {
    unsafe {
        esp_http_client_cleanup(client);
    }
}