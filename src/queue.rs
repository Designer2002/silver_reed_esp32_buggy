use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Clone, Copy, Debug)]
pub struct EngineEvent {
    pub kind: u8,
    pub seq: u32,
    pub timestamp_us: u32,
    pub level: bool,
}

pub const EVT_CCP: u8 = 1;
pub const EVT_ND1: u8 = 2;
pub const EVT_KSL: u8 = 3;
pub const EVT_HOK: u8 = 4;

pub static CCP_EVENT_PENDING: AtomicBool = AtomicBool::new(false);
pub static CCP_EVENT_SEQ: AtomicU32 = AtomicU32::new(0);
pub static CCP_EVENT_TS: AtomicU32 = AtomicU32::new(0);
pub static CCP_EVENT_LEVEL: AtomicBool = AtomicBool::new(false);

pub static ND1_EVENT_PENDING: AtomicBool = AtomicBool::new(false);
pub static ND1_EVENT_SEQ: AtomicU32 = AtomicU32::new(0);
pub static ND1_EVENT_TS: AtomicU32 = AtomicU32::new(0);
pub static ND1_EVENT_LEVEL: AtomicBool = AtomicBool::new(false);

pub static KSL_EVENT_PENDING: AtomicBool = AtomicBool::new(false);
pub static KSL_EVENT_SEQ: AtomicU32 = AtomicU32::new(0);
pub static KSL_EVENT_TS: AtomicU32 = AtomicU32::new(0);
pub static KSL_EVENT_LEVEL: AtomicBool = AtomicBool::new(false);

pub static HOK_EVENT_PENDING: AtomicBool = AtomicBool::new(false);
pub static HOK_EVENT_SEQ: AtomicU32 = AtomicU32::new(0);
pub static HOK_EVENT_TS: AtomicU32 = AtomicU32::new(0);
pub static HOK_EVENT_LEVEL: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn queue_event(kind: u8, seq: u32, timestamp_us: u32, level: bool) {
    match kind {
        EVT_CCP => {
            CCP_EVENT_PENDING.store(true, Ordering::Release);
            CCP_EVENT_SEQ.store(seq, Ordering::Relaxed);
            CCP_EVENT_TS.store(timestamp_us as u32, Ordering::Relaxed);
            CCP_EVENT_LEVEL.store(level, Ordering::Relaxed);
        }
        EVT_ND1 => {
            ND1_EVENT_PENDING.store(true, Ordering::Release);
            ND1_EVENT_SEQ.store(seq, Ordering::Relaxed);
            ND1_EVENT_TS.store(timestamp_us as u32, Ordering::Relaxed);
            ND1_EVENT_LEVEL.store(level, Ordering::Relaxed);
        }
        EVT_KSL => {
            KSL_EVENT_PENDING.store(true, Ordering::Release);
            KSL_EVENT_SEQ.store(seq, Ordering::Relaxed);
            KSL_EVENT_TS.store(timestamp_us as u32, Ordering::Relaxed);
            KSL_EVENT_LEVEL.store(level, Ordering::Relaxed);
        }
        EVT_HOK => {
            HOK_EVENT_PENDING.store(true, Ordering::Release);
            HOK_EVENT_SEQ.store(seq, Ordering::Relaxed);
            HOK_EVENT_TS.store(timestamp_us as u32, Ordering::Relaxed);
            HOK_EVENT_LEVEL.store(level, Ordering::Relaxed);
        }
        _ => {}
    }
}

#[inline(always)]
pub fn pop_event() -> Option<EngineEvent> {
    if CCP_EVENT_PENDING.swap(false, Ordering::AcqRel) {
        return Some(EngineEvent {
            kind: EVT_CCP,
            seq: CCP_EVENT_SEQ.load(Ordering::Relaxed),
            timestamp_us: CCP_EVENT_TS.load(Ordering::Relaxed),
            level: CCP_EVENT_LEVEL.load(Ordering::Relaxed),
        });
    }

    if ND1_EVENT_PENDING.swap(false, Ordering::AcqRel) {
        return Some(EngineEvent {
            kind: EVT_ND1,
            seq: ND1_EVENT_SEQ.load(Ordering::Relaxed),
            timestamp_us: ND1_EVENT_TS.load(Ordering::Relaxed),
            level: ND1_EVENT_LEVEL.load(Ordering::Relaxed),
        });
    }

    if KSL_EVENT_PENDING.swap(false, Ordering::AcqRel) {
        return Some(EngineEvent {
            kind: EVT_KSL,
            seq: KSL_EVENT_SEQ.load(Ordering::Relaxed),
            timestamp_us: KSL_EVENT_TS.load(Ordering::Relaxed),
            level: KSL_EVENT_LEVEL.load(Ordering::Relaxed),
        });
    }

    if HOK_EVENT_PENDING.swap(false, Ordering::AcqRel) {
        return Some(EngineEvent {
            kind: EVT_HOK,
            seq: HOK_EVENT_SEQ.load(Ordering::Relaxed),
            timestamp_us: HOK_EVENT_TS.load(Ordering::Relaxed),
            level: HOK_EVENT_LEVEL.load(Ordering::Relaxed),
        });
    }

    None
}

pub struct EngineEventQueue;

impl EngineEventQueue {
    #[inline(always)]
    pub fn send_back(&self, value: EngineEvent) -> Result<(), EngineEvent> {
        queue_event(value.kind, value.seq, value.timestamp_us, value.level);
        Ok(())
    }

    #[inline(always)]
    pub fn send_back_isr(&self, value: EngineEvent) -> Result<(), EngineEvent> {
        queue_event(value.kind, value.seq, value.timestamp_us, value.level);
        Ok(())
    }

    #[inline(always)]
    pub fn recv_front(&self) -> Option<EngineEvent> {
        pop_event()
    }
}

pub static QUEUE: EngineEventQueue = EngineEventQueue;