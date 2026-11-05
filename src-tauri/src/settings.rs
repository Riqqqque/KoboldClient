use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util::write_atomic;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub game_root: Option<PathBuf>,
    pub theme: String,
    pub check_updates_on_launch: bool,
    pub auto_update_addons: bool,
    pub verbose_logging: bool,
    /// Zip the WTF folder before addon updates, when it changed since the last backup.
    pub backup_before_update: bool,
    /// The Play button installs pending addon updates first.
    pub update_before_play: bool,
    /// The Lua error session the user last dismissed (`<log start>|<count>`).
    pub lua_errors_seen: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            game_root: None,
            theme: "ember".into(),
            check_updates_on_launch: true,
            auto_update_addons: false,
            verbose_logging: false,
            backup_before_update: true,
            update_before_play: true,
            lua_errors_seen: None,
        }
    }
}

pub fn load(data_dir: &Path) -> Settings {
    let path = data_dir.join("settings.json");
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "settings file unreadable, using defaults");
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

pub fn save(data_dir: &Path, s: &Settings) -> Result<()> {
    write_atomic(
        &data_dir.join("settings.json"),
        &serde_json::to_vec_pretty(s)?,
    )
}
