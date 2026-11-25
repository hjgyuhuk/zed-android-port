//! Where the `zed-askpass-helper` binary lives.
//!
//! The askpass crate (`crates/askpass/src/askpass.rs`) sets `SSH_ASKPASS`
//! to whatever path `askpass::set_askpass_program` registered. Desktop
//! platforms use `current_exe()` (the zed binary itself, with
//! `--askpass=<sock>`). On Android `current_exe()` is
//! `/system/bin/app_process64` (the Zygote launcher hosting the DEX
//! runtime), and ssh exec'ing it outside of an Activity context aborts
//! with `Error changing dalvik-cache ownership: Permission denied`. The
//! helper is a tiny static aarch64 ELF (no JVM, no DEX) doing the same
//! netcat-style relay the desktop `askpass::main` does: read the prompt,
//! write it to a Unix socket the gpui process is listening on, read the
//! password back, print it to stdout.
//!
//! It ships as `libaskpass.so` in the APK's native library dir, extracted
//! there by the installer: the one app-owned location the kernel may exec
//! from at any targetSdk (W^X forbids exec'ing anything from the app's
//! writable dirs). It also stands in for `zed --printenv`, which Zed's
//! shell capture runs (`zdroid_hooks::register_zed_helper`).

use std::path::PathBuf;

use android_activity::AndroidApp;
use anyhow::{Context, Result};
use jni::objects::JString;

/// The helper's file name in the native library dir. Gradle's
/// `stageAskpassHelperJniLib` task stages the built binary there.
const HELPER_FILE_NAME: &str = "libaskpass.so";

/// Absolute path of the askpass helper inside the native library dir.
/// The file exists only when the APK was packaged with
/// `extractNativeLibs="true"` (see `packaging.jniLibs` in build.gradle.kts).
pub fn helper_path(android_app: &AndroidApp) -> Result<PathBuf> {
    Ok(native_library_dir(android_app)?.join(HELPER_FILE_NAME))
}

/// `ApplicationInfo.nativeLibraryDir` via JNI: where the installer put
/// this APK's `lib/arm64-v8a/*.so`. The path changes on every install
/// (`/data/app/~~<random>/com.zdroid-<random>/lib/arm64`), so nothing
/// persistent may embed it; look it up at boot.
fn native_library_dir(android_app: &AndroidApp) -> Result<PathBuf> {
    let dir = crate::jni_env::with_activity(android_app, |env, activity| {
        let value = env
            .call_method(activity, "nativeLibraryDir", "()Ljava/lang/String;", &[])
            .context("MainActivity.nativeLibraryDir")?;
        let jstr: JString = value
            .l()
            .context("nativeLibraryDir returned non-object")?
            .into();
        let dir: String = env
            .get_string(&jstr)
            .context("decode nativeLibraryDir")?
            .into();
        Ok(dir)
    })?;
    anyhow::ensure!(!dir.is_empty(), "nativeLibraryDir is empty");
    Ok(PathBuf::from(dir))
}