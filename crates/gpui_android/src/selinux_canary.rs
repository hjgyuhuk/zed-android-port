//! Logs the SELinux domain of the app process at boot.
//!
//! The domain decides what the app's children may exec: the askpass
//! helper runs from the APK's native library dir, the only app-owned
//! location exec is allowed from at any targetSdk. The line is a
//! diagnostic for reading a spawn failure against the domain, not a gate.

pub fn check_selinux_context() {
    let context = std::fs::read_to_string("/proc/self/attr/current").ok();
    log::info!(
        "selinux_canary: /proc/self/attr/current = {:?}",
        context.as_deref().map(str::trim)
    );
}
