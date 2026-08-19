mod core;
mod gpio;
mod pattern;
mod state;

use std::sync::atomic::Ordering;

use crate::gpio::{
    install_ccp_interrupt, install_hok_interrupt, install_ksl_interrupt, install_nd1_interrupt,
};
use crate::pattern::PATTERN;
use crate::state::*;

pub fn init_pattern() {
    WIDTH.store(PATTERN.width, Ordering::Relaxed);
    HEIGHT.store(PATTERN.height, Ordering::Relaxed);
}

fn main() -> anyhow::Result<()> {
    init_pattern();

    install_ccp_interrupt();
    

    install_hok_interrupt();
    
    install_nd1_interrupt();
    install_ksl_interrupt();
    

    ACTIVE.store(true, Ordering::Relaxed);
    Ok(())
}
