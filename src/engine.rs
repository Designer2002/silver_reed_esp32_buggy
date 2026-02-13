use crate::event_bus::{pop_event, push_event, Event};
use crate::pattern::PATTERN;
use crate::knit_state::KnitState;
use crate::logger::log;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub fn start_engine() -> Arc<Mutex<KnitState>> {

    let state = Arc::new(Mutex::new(
        KnitState::new(PATTERN.width, PATTERN.height)
    ));

    let engine_state = state.clone();

    thread::spawn(move || {

        log("INFO", "ENGINE STARTED");
        
        loop {
            if let Some(evt) = pop_event() {

                let mut s = engine_state.lock().unwrap();

                match evt {

                    Event::StartKnit => {
                        s.reset();
                        log("INFO","KNITTING STARTED");
                    }

                    Event::StopKnit => {
                        s.stop();
                        log("INFO","KNITTING STOPPED");
                    }

                    Event::Ksl(v) => s.ksl_high = v,
                    Event::Nd1(v) => s.nd1_high = v,
                    Event::Hok(v) => s.dir_right_to_left = v,

                    Event::CCP => {
                        if s.active {
                            process_ccp(&mut s);
                        }
                    }

                    _ => {}
                }
            }

            thread::sleep(Duration::from_millis(1));
        }
    });

    state
}

fn process_ccp(state: &mut KnitState) {

    if !state.ksl_high {
        return;
    }

    if state.row >= state.height {
        state.active = false;
        log("INFO","PATTERN DONE");
        return;
    }

    let bit = if state.dir_right_to_left {
        PATTERN.rows[state.row][state.width - 1 - state.col]
    } else {
        PATTERN.rows[state.row][state.col]
    };

    if bit {
        push_event(Event::DobFire);
    }

    state.col += 1;

    if state.col >= state.width {
        state.col = 0;
        state.row += 1;

        log("INFO",&format!("ROW {} DONE", state.row));
    }
}
