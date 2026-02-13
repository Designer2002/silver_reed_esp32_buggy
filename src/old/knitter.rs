use std::sync::{Arc, Mutex};
use esp_idf_hal::gpio::{PinDriver, AnyIOPin, AnyOutputPin, Output, Input, InterruptType};
use crate::{constants::{TOTAL_COLUMNS, TOTAL_ROWS, ccp_isr_handler, PATTERN, GLOBAL_KNITTING_ACTIVE, CURRENT_COLUMN, CURRENT_ROW}, types::SilverLinkEmulator, logger::add_log};

impl SilverLinkEmulator {
    pub fn new(
        mut ccp: PinDriver<'static, AnyIOPin, Input>,
        hok: PinDriver<'static, AnyIOPin, Input>,
        ksl: PinDriver<'static, AnyIOPin, Input>,
        nd1: PinDriver<'static, AnyIOPin, Input>,
        dob: PinDriver<'static, AnyOutputPin, Output>,
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
        add_log("INFO", "Start knitting thread initiated.");

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
                add_log("INFO", "Knitting stopped externally during row loop.");
                break;
            }

            CURRENT_ROW.store(row_idx, Ordering::Relaxed);
            CURRENT_COLUMN.store(0, Ordering::Relaxed);

            add_log("INFO", &format!("Processing row {}", row_idx));

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
            add_log("DEBUG", "Waiting for KSL HIGH...");
            while !self.ksl.is_high() && GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                FreeRtos::delay_ms(10);
            }
            if !GLOBAL_KNITTING_ACTIVE.load(Ordering::Relaxed) {
                break;
            }

            let is_right_to_left = self.hok.is_high();
            add_log(
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
                        add_log(
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
        add_log("INFO", "Knitting completed or stopped.");
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


pub fn parse_pattern(pattern_text: &str) -> KnitPattern {
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