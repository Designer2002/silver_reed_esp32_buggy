use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

pub static DIR_RIGHT: AtomicBool = AtomicBool::new(true);
pub static INSIDE_PATTERN: AtomicBool = AtomicBool::new(false);
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

pub static NEEDLE: AtomicI32 = AtomicI32::new(0);
pub static ROW: AtomicUsize = AtomicUsize::new(0);

pub static WIDTH: AtomicUsize = AtomicUsize::new(0);
pub static HEIGHT: AtomicUsize = AtomicUsize::new(0);
