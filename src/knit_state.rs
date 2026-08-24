//! Сохранение/восстановление прогресса вязания в NVS (flash)
//! При аварийном выключении прогресс сохраняется и восстанавливается при перезагрузке.

use esp_idf_svc::nvs::EspNvs;
use esp_idf_svc::nvs::NvsDefault;

use crate::log_fmt;
use crate::logger::log;
use crate::state::KEY_GLOBAL_ROW;
use crate::state::KNIT_NAMESPACE;
use crate::state::KNIT_NVS;

/// Инициализация NVS для сохранения прогресса вязания
/// nvs_partition — уже существующая партиция из main.rs (WiFi)
pub fn init_knit_nvs(nvs_partition: esp_idf_svc::nvs::EspNvsPartition<NvsDefault>) {
    let nvs = EspNvs::new(nvs_partition, KNIT_NAMESPACE, true).unwrap();
    *KNIT_NVS.lock().unwrap() = Some(nvs);
    log("INFO", "Knit NVS initialized");
}

/// Сохранить прогресс в NVS
pub fn save_progress(global_row: i32) {
    let mut guard = KNIT_NVS.lock().unwrap();
    if let Some(ref mut nvs) = *guard {
        let _ = nvs.set_i32(KEY_GLOBAL_ROW, global_row);
        // Flush не нужен — NVS автоматически сохраняет
    }
}

/// Восстановить прогресс из NVS
/// Возвращает global_row или None если нет сохранённых данных
pub fn restore_progress() -> Option<i32> {
    let mut guard = KNIT_NVS.lock().unwrap();
    if let Some(ref mut nvs) = *guard {
        let global_row = nvs.get_i32(KEY_GLOBAL_ROW).ok().flatten();
        
        if let Some(gr) = global_row {
            log_fmt!("INFO", "Restored knit progress: row={}", gr);
            return Some(gr);
        }
    }
    log("INFO","No saved knit progress found, starting fresh");
    None
}

/// Сбросить сохранённый прогресс (начать заново)
pub fn reset_progress() {
    let mut guard = KNIT_NVS.lock().unwrap();
    if let Some(ref mut nvs) = *guard {
        let _ = nvs.remove(KEY_GLOBAL_ROW);
    }
    log("INFO","Knit progress reset");
}
