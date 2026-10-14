use std::{sync::OnceLock, time::Instant};

static START: OnceLock<Instant> = OnceLock::new();
static ENABLED: OnceLock<bool> = OnceLock::new();

pub fn begin() {
    START.get_or_init(Instant::now);
    ENABLED.get_or_init(|| std::env::var_os("PETALOOK_STARTUP_TRACE").is_some());
    mark("main");
    if ENABLED.get().copied().unwrap_or(false) {
        eprintln!("startup_pid: {}", std::process::id());
    }
}

pub fn mark(stage: &str) {
    if ENABLED.get().copied().unwrap_or(false) {
        eprintln!(
            "startup {stage}: {:.3} ms",
            START.get().unwrap().elapsed().as_secs_f64() * 1000.
        );
    }
}
