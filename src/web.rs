use crate::event_bus::{Event, push_event};
use crate::gpio::get_pin_state_json;
use crate::pattern::KNITTING_PATTERN;
use crate::logger::{get_logs, log};
use anyhow::Ok;
use esp_idf_svc::http::server::EspHttpServer;
use esp_idf_svc::io::Write;
use esp_idf_svc::{
    http::Method,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use log::info;

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

pub fn init_server(mut server: EspHttpServer) -> anyhow::Result<()> {
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
            push_event(Event::StartKnit);
            log("INFO", "HTTP START");
            req.into_ok_response()?.write_all(b"OK")?;
            Ok(())
        },
    )?;
    server.fn_handler(
        "/stop_knitting",
        Method::Post,
        |req| -> anyhow::Result<()> {
            push_event(Event::StopKnit);
            log("INFO", "HTTP STOP");
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
    server.fn_handler("/signals", Method::Get, |req| -> anyhow::Result<()> {
        let json = get_pin_state_json().clone();
        let mut resp = req.into_ok_response()?;
        resp.write_all(json.as_bytes())?;
        Ok(())
    })?;

    Ok(())
}
