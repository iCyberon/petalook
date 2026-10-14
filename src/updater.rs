// Keep update delivery behind an OS boundary; Sparkle is the macOS backend.
#[cfg(target_os = "macos")]
mod macos {
    unsafe extern "C" {
        fn sf_updater_start();
        fn sf_updater_check();
    }

    pub fn start() {
        unsafe { sf_updater_start() }
    }

    pub fn check() {
        unsafe { sf_updater_check() }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{check, start};
