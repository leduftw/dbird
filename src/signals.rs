//! Graceful shutdown flags for signals that would otherwise skip terminal cleanup.

use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub struct ShutdownSignals {
    requested: Arc<AtomicBool>,
}

impl ShutdownSignals {
    pub fn install() -> io::Result<Self> {
        let requested = Arc::new(AtomicBool::new(false));
        install_platform_handlers(Arc::clone(&requested))?;
        Ok(Self { requested })
    }

    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}

#[cfg(unix)]
fn install_platform_handlers(requested: Arc<AtomicBool>) -> io::Result<()> {
    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::flag;

    for signal in [SIGINT, SIGTERM, SIGHUP] {
        flag::register(signal, Arc::clone(&requested))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn install_platform_handlers(_requested: Arc<AtomicBool>) -> io::Result<()> {
    Ok(())
}
