//! Де лаунчер тримає файли. Бібліотеки, ассети й Java спільні, гра — у своїй теці.
use std::path::PathBuf;
use std::sync::OnceLock;

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Windows: %APPDATA%\.altherra · macOS: ~/Library/Application Support/altherra · Linux: ~/.altherra
pub fn root() -> &'static PathBuf {
    ROOT.get_or_init(|| {
        if let Some(custom) = std::env::var_os("ALTHERRA_HOME") {
            return PathBuf::from(custom);
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join(".altherra")
        } else if cfg!(target_os = "macos") {
            home.join("Library/Application Support/altherra")
        } else {
            home.join(".altherra")
        }
    })
}

pub fn game_dir() -> PathBuf {
    root().join("game")
}
pub fn libraries() -> PathBuf {
    root().join("libraries")
}
pub fn assets() -> PathBuf {
    root().join("assets")
}
pub fn versions() -> PathBuf {
    root().join("versions")
}
pub fn runtime() -> PathBuf {
    root().join("runtime")
}
pub fn natives(version: &str) -> PathBuf {
    root().join("natives").join(version)
}
pub fn settings_file() -> PathBuf {
    root().join("settings.json")
}
pub fn accounts_file() -> PathBuf {
    // Offline-версія тримає акаунти окремо, щоб не чіпати ліцензійні акаунти офіційної.
    root().join(if crate::OFFICIAL { "accounts.json" } else { "accounts-offline.json" })
}
pub fn manifest_cache() -> PathBuf {
    root().join("manifest.json")
}
pub fn sync_state() -> PathBuf {
    root().join("sync-state.json")
}
