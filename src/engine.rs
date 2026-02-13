use crate::event_bus::{pop_event, Event};
use crate::logger::log;
use crate::pattern::PATTERN;
use std::thread;

#[derive(Debug)]
pub struct EngineState {
    pub row: usize,
    pub needle: i32,
    pub dir_right: bool,
    pub inside_pattern: bool,
    pub active: bool,
    pub width: usize,
    pub height: usize,
}

impl EngineState {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            row: 0,
            needle: 0,
            dir_right: true,
            inside_pattern: false,
            active: false,
            width,
            height,
        }
    }

    pub fn reset(&mut self) {
        self.row = 0;
        self.needle = 0;
        self.active = true;
    }
}

pub fn start_engine() {
    thread::spawn(|| {

        log("INFO","KNIT realtime engine started");

        let mut direction_right = true;
        let mut current_needle: i32 = 0;
        let mut row: usize = 0;

        let mut inside_pattern = false;
        let mut knitting = false;

        let mut prev_ccp = true;
        let mut prev_nd1 = true;
        let mut prev_ksl = true;

        loop {

            // читаем GPIO напрямую
            let (ccp, hok, ksl, nd1) = match crate::gpio::read_inputs() {
                Some(v) => v,
                None => {
                    thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
            };

            direction_right = hok;

            // --- CCP ---
            if ccp != prev_ccp {
                prev_ccp = ccp;

                if ccp {
                    if direction_right {
                        current_needle += 1;
                    } else {
                        current_needle -= 1;
                    }

                    if knitting && inside_pattern {
                        fire_if_needed(row, current_needle, direction_right);
                    }
                }
            }

            // --- KSL ---
            if ksl != prev_ksl {
                prev_ksl = ksl;
                inside_pattern = ksl;

                if inside_pattern {
                    if direction_right {
                        current_needle = -1;
                    } else {
                        current_needle = PATTERN.width as i32;
                    }
                } else {
                    row += 1;
                    log("ROW", Box::leak(format!("{}",row).into_boxed_str()));

                    if row >= PATTERN.height {
                        knitting = false;
                        log("INFO","PATTERN DONE");
                    }
                }
            }

            // --- ND1 ---
            if nd1 != prev_nd1 {
                prev_nd1 = nd1;

                if !nd1 {
                    if direction_right {
                        current_needle = -1;
                    }
                }
            }

            // --- events start/stop ---
            if let Some(evt) = pop_event() {
                match evt {
                    Event::StartKnit => {
                        knitting = true;
                        row = 0;
                        log("INFO","KNIT START");
                    }
                    Event::StopKnit => {
                        knitting = false;
                        log("INFO","KNIT STOP");
                    }
                    _ => {}
                }
            }

            esp_idf_hal::delay::Ets::delay_us(50);
        }
    });
}

fn fire_if_needed(row: usize, needle: i32, dir_right: bool) {
    if needle < 0 || needle >= PATTERN.width as i32 {
        return;
    }

    let col = needle as usize;

    let bit = if dir_right {
        PATTERN.rows[row][col]
    } else {
        PATTERN.rows[row][PATTERN.width-1-col]
    };

    if bit {
        crate::gpio::dob_fire();
    }
}
