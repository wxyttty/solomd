//! Portable mode for the Windows zip build (#295).
//!
//! If a folder named `data` sits next to `SoloMD.exe`, everything SoloMD
//! keeps about the user goes there instead of into the profile, so copying
//! the program folder to another machine carries the settings with it. This
//! is the VS Code convention: no switch, no flag — the folder's presence is
//! the switch, and without it nothing changes for anyone.
//!
//!   data/webview   WebView2 user data. The settings, recent files, tab
//!                  sets and the rest of the UI state live in localStorage
//!                  here — this is the part that matters.
//!   data/config    what `app_config_dir()` would hold (themes, dictionaries,
//!                  MCP profiles, cost meter, port files)
//!   data/appdata   what `app_data_dir()` would hold (workspace index cache)
//!
//! Not portable: API keys. They are in Windows Credential Manager, which is
//! per machine by design, so they have to be entered again on a new machine
//! (README.txt in the zip says so). Window size/position is kept by the
//! window-state plugin in the profile and does not follow either.
//!
//! WebView2 honours `WEBVIEW2_USER_DATA_FOLDER` over the folder the host
//! passes in (Tauri always passes `%LOCALAPPDATA%\<identifier>`), per the
//! `CreateCoreWebView2EnvironmentWithOptions` docs, so setting that variable
//! before the first webview is created moves every window's data at once —
//! including windows we open later (new window, quick capture).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::{path::PathResolver, Runtime};

static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

/// `<exe_dir>/data` when running portable, else `None`. Windows only.
pub fn root() -> Option<&'static Path> {
    ROOT.get_or_init(|| {
        if !cfg!(target_os = "windows") {
            return None;
        }
        let exe = std::env::current_exe().ok()?;
        detect(exe.parent()?)
    })
    .as_deref()
}

/// The portable data folder for a program directory, if it has one.
fn detect(exe_dir: &Path) -> Option<PathBuf> {
    let d = exe_dir.join("data");
    d.is_dir().then_some(d)
}

/// Portable replacement for `app.path().app_config_dir()`.
pub fn app_config_dir<R: Runtime>(p: &PathResolver<R>) -> tauri::Result<PathBuf> {
    match root() {
        Some(r) => Ok(r.join("config")),
        None => p.app_config_dir(),
    }
}

/// Portable replacement for `app.path().app_data_dir()`.
pub fn app_data_dir<R: Runtime>(p: &PathResolver<R>) -> tauri::Result<PathBuf> {
    match root() {
        Some(r) => Ok(r.join("appdata")),
        None => p.app_data_dir(),
    }
}

/// `data/config` for the pure-Rust call sites that have no `AppHandle`.
pub fn config_root() -> Option<PathBuf> {
    root().map(|r| r.join("config"))
}

/// Call first thing in `run()`, before any webview exists.
pub fn init() {
    let Some(root) = root() else { return };
    let webview = root.join("webview");
    // First portable launch on a machine that already ran the installed or
    // older portable build: bring the settings along once, so creating
    // `data/` doesn't look like a factory reset. Only localStorage — it holds
    // the settings and is small; caches are not worth copying.
    if !webview.exists() {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let old = PathBuf::from(local)
                .join("app.solomd")
                .join("EBWebView")
                .join("Default")
                .join("Local Storage");
            let new = webview.join("EBWebView").join("Default").join("Local Storage");
            if old.is_dir() {
                let _ = copy_dir(&old, &new);
            }
        }
    }
    let _ = std::fs::create_dir_all(&webview);
    let _ = std::fs::create_dir_all(root.join("config"));
    // Set on the main thread before the Tauri runtime starts any other thread.
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview);
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_an_existing_data_folder() {
        let tmp = std::env::temp_dir().join(format!("solomd-portable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(detect(&tmp), None);
        // A file named `data` is not the portable folder.
        std::fs::write(tmp.join("data"), b"").unwrap();
        assert_eq!(detect(&tmp), None);
        std::fs::remove_file(tmp.join("data")).unwrap();
        std::fs::create_dir(tmp.join("data")).unwrap();
        assert_eq!(detect(&tmp), Some(tmp.join("data")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn copy_dir_copies_nested_files() {
        let tmp = std::env::temp_dir().join(format!("solomd-portable-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("src/leveldb");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("000003.log"), b"settings").unwrap();
        copy_dir(&tmp.join("src"), &tmp.join("dst")).unwrap();
        assert_eq!(std::fs::read(tmp.join("dst/leveldb/000003.log")).unwrap(), b"settings");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn not_portable_off_windows() {
        if !cfg!(target_os = "windows") {
            assert_eq!(root(), None);
            assert_eq!(config_root(), None);
        }
    }
}
