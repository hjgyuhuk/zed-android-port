//! The Android code that Zed's crates call from small `cfg(target_os =
//! "android")` blocks: path spelling (`fs`), a safe working directory for
//! spawns (`util::command`), the project environment (`project`), and the
//! program Zed's shell capture runs (`util`). Keeping it here leaves one
//! call in each of those crates, which are merged at every Zed sync.
//!
//! The app registers its paths at startup. A second registration, when the
//! Activity is recreated, is ignored.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;

static FILES_DIR_SPELLINGS: OnceLock<Option<(PathBuf, PathBuf)>> = OnceLock::new();
static ZED_HELPER: OnceLock<PathBuf> = OnceLock::new();
static LOGIN_ENVIRONMENT: OnceLock<Shared<oneshot::Receiver<()>>> = OnceLock::new();

fn register<T>(slot: &OnceLock<T>, value: T, name: &str) {
    if slot.set(value).is_err() {
        log::debug!("zdroid_hooks: {name} already registered; ignoring the new value");
    }
}

/// Registers the files dir as the editor spells it (`/data/user/0/...`,
/// what its stored paths use) and as the kernel reports it
/// (`/data/data/...` at targetSdk 35). See `android-files-dir-spelling.md`.
pub fn register_files_dir_spellings(editor: PathBuf, kernel: PathBuf) {
    let spellings = (editor != kernel).then_some((editor, kernel));
    register(&FILES_DIR_SPELLINGS, spellings, "files dir spellings");
}

/// Registers the program that stands in for Zed's own executable where a
/// child program runs it: the app's is `app_process64`, which only Android's
/// app launcher can start. Zed's shell capture runs it as `zed --printenv`.
pub fn register_zed_helper(path: PathBuf) {
    register(&ZED_HELPER, path, "Zed helper");
}

/// `util::get_shell_safe_zed_path` on Android.
pub fn zed_helper() -> Option<&'static Path> {
    ZED_HELPER.get().map(PathBuf::as_path)
}

/// Registers when the app has applied the login shell's environment, which
/// it captures once at startup, as Zed's `main()` does.
pub fn register_login_environment(loaded: oneshot::Receiver<()>) {
    register(&LOGIN_ENVIRONMENT, loaded.shared(), "login environment");
}

/// `project::environment`'s directory environment on Android, which is the
/// app's own: it waits for the login shell's, as desktop Zed waits for a
/// login shell in the project's folder.
pub async fn login_environment_loaded() {
    if let Some(loaded) = LOGIN_ENVIRONMENT.get() {
        loaded.clone().await.ok();
    }
}

/// `path` with the kernel's spelling of the files dir replaced by the
/// editor's, or `None` when `path` isn't under it or the spellings agree.
pub fn respell_kernel_path(path: &Path) -> Option<PathBuf> {
    let (editor, kernel) = FILES_DIR_SPELLINGS.get()?.as_ref()?;
    let rest = path.strip_prefix(kernel).ok()?;
    Some(editor.join(rest))
}

/// `util::command::Command::current_dir` on Android.
///
/// A spawned process's `chdir(2)` into Android's FUSE-emulated shared
/// storage fails with ENOENT, even though the app itself can list and open
/// files there, so every language server or build tool spawned for a
/// project on `/sdcard` failed. Such directories are replaced with the
/// app's home. Language servers still get the project's path through
/// `workspaceFolders`.
pub fn safe_current_dir(dir: &Path) -> PathBuf {
    const SHARED_STORAGE_PREFIXES: &[&str] = &[
        "/storage/emulated/",
        "/storage/self/",
        "/sdcard/",
        "/mnt/runtime/",
        "/mnt/user/",
        "/mnt/media_rw/",
    ];
    let dir_string = dir.to_string_lossy();
    if !SHARED_STORAGE_PREFIXES
        .iter()
        .any(|prefix| dir_string.starts_with(prefix))
    {
        return dir.to_path_buf();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if home.is_dir() {
            log::debug!("zdroid_hooks: running in {home:?} instead of shared storage {dir:?}");
            return home;
        }
    }
    PathBuf::from("/system/bin")
}

/// `child`'s path relative to `parent`, when one of `child`'s ancestors is
/// the same directory as `parent` under another name. Android reaches the
/// app's data directory as both `/data/data/<package>` and
/// `/data/user/0/<package>`: two bind mounts of one directory, which
/// `canonicalize` doesn't unify, so the device and inode numbers are
/// compared instead.
pub fn strip_prefix_by_identity(child: &Path, parent: &Path) -> Option<PathBuf> {
    let parent_identity = file_identity(parent)?;
    child
        .ancestors()
        .find(|ancestor| file_identity(ancestor) == Some(parent_identity))
        .and_then(|ancestor| child.strip_prefix(ancestor).ok())
        .map(Path::to_path_buf)
}

fn file_identity(path: &Path) -> Option<(u64, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefix_by_identity_finds_an_ancestor() {
        let directory = std::env::temp_dir();
        let child = directory.join("nested").join("file.txt");
        assert_eq!(
            strip_prefix_by_identity(&child, &directory),
            Some(PathBuf::from("nested/file.txt"))
        );
        assert_eq!(
            strip_prefix_by_identity(&directory, &directory),
            Some(PathBuf::new())
        );
    }

    #[test]
    fn strip_prefix_by_identity_without_a_match() {
        let parent = std::env::temp_dir();
        assert_eq!(
            strip_prefix_by_identity(Path::new("/no/such/path"), &parent),
            None
        );
        assert_eq!(
            strip_prefix_by_identity(&parent, Path::new("/no/such/path")),
            None
        );
    }
}