//! Where CSV Edit keeps its settings (language).

use std::path::PathBuf;

/// `%APPDATA%\CSV Edit` on Windows, `~/.config/csvedit` elsewhere.
pub fn dir() -> PathBuf {
    // Tests never touch the user's settings.
    if cfg!(test) {
        return std::env::temp_dir().join("csvedit-test-config");
    }
    if let Some(roaming) = std::env::var_os("APPDATA") {
        PathBuf::from(roaming).join("CSV Edit")
    } else if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config).join("csvedit")
    } else {
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join(".config").join("csvedit")
    }
}
