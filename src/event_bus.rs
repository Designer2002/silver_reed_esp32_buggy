use heapless::spsc::Queue;
use std::sync::{Arc, Mutex};
use crate::logger::LogEntry;

#[derive(Clone, Debug)]
pub enum Event {
    StartKnit,
    StopKnit,

    CCP,            // импульс иглы
    Ksl(bool),      // диапазон игл
    Nd1(bool),      // начало строки
    Hok(bool),      // направление

    DobFire,        // команда дернуть DOB
    Log(LogEntry),
}

static EVENT_QUEUE_INNER: once_cell::sync::Lazy<
    Arc<Mutex<Queue<Event, 512>>>
> = once_cell::sync::Lazy::new(|| {
    Arc::new(Mutex::new(Queue::new()))
});

pub fn push_event(evt: Event) {
    if let Ok(mut q) = EVENT_QUEUE_INNER.lock() {
        let _ = q.enqueue(evt);
    }
}

pub fn pop_event() -> Option<Event> {
    if let Ok(mut q) = EVENT_QUEUE_INNER.lock() {
        q.dequeue()
    } else {
        None
    }
}
