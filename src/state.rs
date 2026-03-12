use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize};

pub static ROW: AtomicI32 = AtomicI32::new(0);
pub static NEEDLE: AtomicI32 = AtomicI32::new(0);
pub static DIR_RIGHT: AtomicBool = AtomicBool::new(true);
pub static INSIDE_PATTERN: AtomicBool = AtomicBool::new(false);
pub static KNITTING: AtomicBool = AtomicBool::new(false);
pub static WIDTH: AtomicUsize = AtomicUsize::new(0);
pub static HEIGHT: AtomicUsize = AtomicUsize::new(0);
pub static WAS_INSIDE: AtomicBool = AtomicBool::new(false);

pub const DOB: i32 = 23;
pub const CCP: i32 = 18;
pub const HOK: i32 = 19;
pub const KSL: i32 = 21;
pub const ND1: i32 = 22; 