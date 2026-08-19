
fn init_logger() {
    critical_section::with(|cs| unsafe {
        LOG_QUEUE = Some(Queue::new(256));
        let queue = LOG_QUEUE.as_mut().unwrap();
        let (prod, cons) = queue.split();

        *LOG_PRODUCER.borrow_ref_mut(cs) = Some(prod);
        *LOG_CONSUMER.borrow_ref_mut(cs) = Some(cons);
    });
}

fn log_state(state: u8, timestamp: u64) {
    let entry = LogEntry { state, timestamp };

    critical_section::with(|cs| {
        if let Some(prod) = LOG_PRODUCER.borrow_ref_mut(cs).as_mut() {
            let _ = prod.enqueue(entry); // если очередь полна — просто дроп
        }
    });
}

fn logger_task() {
    loop {
        critical_section::with(|cs| {
            if let Some(cons) = LOG_CONSUMER.borrow_ref_mut(cs).as_mut() {
                while let Some(entry) = cons.dequeue() {
                    println!("STATE={} TIME={}", entry.state, entry.timestamp);
                }
            }
        });

        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn add_log(level: u8, text: &str) {
    let ts = (esp_idf_hal::timer::EspTimerService::new().unwrap()
        .now()
        / 1000) as u32;

    let mut msg = [0u8; 64];
    let bytes = text.as_bytes();
    let len = bytes.len().min(63);
    msg[..len].copy_from_slice(&bytes[..len]);

    let entry = LogEntry { ts, level, msg };

    critical_section::with(|cs| {
        if let Some(prod) = LOG_PROD.borrow_ref_mut(cs).as_mut() {
            let _ = prod.enqueue(entry);
        }
    });

    log::info!("[{}] {}", level, text);
}