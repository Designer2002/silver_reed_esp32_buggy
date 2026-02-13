use crate::event_bus::{pop_event, push_event, Event};
use crate::logger::log;
use crate::pattern::PATTERN;
use std::sync::{Arc, Mutex};
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

pub fn start_engine() -> Arc<Mutex<EngineState>> {
    let state = Arc::new(Mutex::new(EngineState::new(PATTERN.width, PATTERN.height)));

    let engine_state = state.clone();

    thread::spawn(move || {
        log("INFO", "ENGINE STARTED");

        loop {
            if let Some(evt) = pop_event() {
                let mut s = engine_state.lock().unwrap();

                match evt {
                    Event::StartKnit => {
                        s.reset();
                        log("INFO", "KNIT START");
                    }

                    Event::StopKnit => {
                        s.active = false;
                        log("INFO", "KNIT STOP");
                    }

                    Event::Hok(v) => {
                        s.dir_right = v;
                    }

                    Event::Ksl(v) => {
                        if v && !s.inside_pattern {
                            // вошли в диапазон
                            s.inside_pattern = true;

                            if s.dir_right {
                                s.needle = -1;
                            } else {
                                s.needle = s.width as i32;
                            }
                        } else if !v && s.inside_pattern {
                            // вышли из диапазона = новая строка
                            s.inside_pattern = false;
                            s.row += 1;
                            let dynamic_message = format!("ROW {}", s.row);
                            let static_message: &'static str =
                                Box::leak(dynamic_message.into_boxed_str());
                            log("DEBUG", static_message);

                            if s.row >= s.height {
                                s.active = false;
                                log("INFO", "PATTERN DONE");
                            }
                        }
                    }

                    Event::Nd1(low) => {
                        if low {
                            if s.dir_right {
                                s.needle = -1;
                            }
                        }
                    }

                    Event::CCP => {
                        if s.active {
                            process_ccp(&mut s);
                        }
                    }

                    _ => {}
                }
            }
        }
    });

    state
}

fn process_ccp(s: &mut EngineState) {
    if !s.inside_pattern {
        return;
    }

    if s.dir_right {
        s.needle += 1;
    } else {
        s.needle -= 1;
    }

    let col = s.needle;

    if col < 0 || col >= s.width as i32 {
        return;
    }

    let col = col as usize;

    let bit = if s.dir_right {
        PATTERN.rows[s.row][col]
    } else {
        PATTERN.rows[s.row][s.width - 1 - col]
    };

    if bit {
        push_event(Event::DobFire);
    }
}
