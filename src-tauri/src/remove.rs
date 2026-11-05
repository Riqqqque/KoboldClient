//! Deleting (to the Recycle Bin), undo, and enable/disable.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::game::GameInfo;
use crate::util::rename_retry;

/// Where a folder currently lives: `AddOns` when enabled, the disabled folder otherwise.
pub fn folder_path(game: &GameInfo, name: &str, enabled: bool) -> PathBuf {
    if enabled {
        game.addons_dir.join(name)
    } else {
        game.disabled_dir().join(name)
    }
}

/// SavedVariables files for the given addon folders, account-wide and per character.
pub fn saved_variables(game: &GameInfo, folders: &[String]) -> Vec<PathBuf> {
    let mut out = vec![];
    let accounts = game.wtf_dir().join("Account");
    let Ok(rd) = fs::read_dir(&accounts) else {
        return out;
    };
    let mut sv_dirs = vec![];
    for acct in rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        sv_dirs.push(acct.join("SavedVariables"));
        // Account/<acct>/<realm>/<character>/SavedVariables
        for realm in read_dirs(&acct) {
            for ch in read_dirs(&realm) {
                sv_dirs.push(ch.join("SavedVariables"));
            }
        }
    }
    for dir in sv_dirs {
        for f in folders {
            for ext in ["lua", "lua.bak"] {
                let p = dir.join(format!("{f}.{ext}"));
                if p.is_file() {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn read_dirs(p: &Path) -> Vec<PathBuf> {
    fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

/// Sends paths to the Recycle Bin. Paths that don't exist are skipped.
pub fn to_recycle_bin(paths: &[PathBuf]) -> Result<()> {
    let existing: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return Ok(());
    }
    trash::delete_all(&existing).map_err(|e| {
        tracing::error!(error = %e, ?existing, "recycle bin move failed");
        Error::Other(format!("Couldn't move to the Recycle Bin: {e}"))
    })?;
    tracing::info!(count = existing.len(), "moved to recycle bin");
    Ok(())
}

/// Restores the most recently deleted copies of `paths` from the Recycle Bin.
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
pub fn restore_from_recycle_bin(paths: &[PathBuf]) -> Result<usize> {
    use trash::os_limited;
    let items = os_limited::list()
        .map_err(|e| Error::Other(format!("Couldn't read the Recycle Bin: {e}")))?;
    let mut picked = vec![];
    for p in paths {
        let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else {
            continue;
        };
        let best = items
            .iter()
            .filter(|i| i.original_parent == parent && std::ffi::OsStr::new(&i.name) == name)
            .max_by_key(|i| i.time_deleted);
        if let Some(item) = best {
            picked.push(item.clone());
        }
    }
    let n = picked.len();
    if n > 0 {
        os_limited::restore_all(picked)
            .map_err(|e| Error::Other(format!("Couldn't restore from the Recycle Bin: {e}")))?;
    }
    Ok(n)
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
pub fn restore_from_recycle_bin(_paths: &[PathBuf]) -> Result<usize> {
    Err(Error::Other("Undo isn't supported on this system.".into()))
}

/// Moves folders between `AddOns` and the disabled folder. Moves are undone if one fails.
pub fn set_enabled(game: &GameInfo, folders: &[String], enabled: bool) -> Result<()> {
    let (from_dir, to_dir) = if enabled {
        (game.disabled_dir(), game.addons_dir.clone())
    } else {
        (game.addons_dir.clone(), game.disabled_dir())
    };
    fs::create_dir_all(&to_dir).map_err(|e| Error::io("create", &to_dir, e))?;
    let mut moved: Vec<(PathBuf, PathBuf)> = vec![];
    for f in folders {
        let (from, to) = (from_dir.join(f), to_dir.join(f));
        if !from.exists() {
            continue;
        }
        if to.exists() {
            // A copy already exists at the destination (e.g. reinstalled by hand). Keep it.
            tracing::warn!(?to, "destination already exists; leaving source in place");
            continue;
        }
        if let Err(e) = rename_retry(&from, &to) {
            for (a, b) in moved.iter().rev() {
                let _ = rename_retry(b, a);
            }
            return Err(e);
        }
        moved.push((from, to));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_saved_variables_everywhere() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("_forever_");
        let acct = root.join("WTF/Account/ACC1");
        fs::create_dir_all(acct.join("SavedVariables")).unwrap();
        fs::create_dir_all(acct.join("Realm/Char/SavedVariables")).unwrap();
        fs::write(acct.join("SavedVariables/Bagnon.lua"), "").unwrap();
        fs::write(acct.join("SavedVariables/Bagnon.lua.bak"), "").unwrap();
        fs::write(acct.join("Realm/Char/SavedVariables/Bagnon.lua"), "").unwrap();
        fs::write(acct.join("SavedVariables/Other.lua"), "").unwrap();
        let g = GameInfo {
            addons_dir: root.join("Interface/AddOns"),
            root,
            version: None,
            running: false,
        };
        assert_eq!(saved_variables(&g, &["Bagnon".into()]).len(), 3);
    }

    #[test]
    fn disable_and_enable_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("_forever_");
        let g = GameInfo {
            addons_dir: root.join("Interface/AddOns"),
            root,
            version: None,
            running: false,
        };
        fs::create_dir_all(g.addons_dir.join("A")).unwrap();
        set_enabled(&g, &["A".into()], false).unwrap();
        assert!(!g.addons_dir.join("A").exists());
        assert!(g.disabled_dir().join("A").exists());
        set_enabled(&g, &["A".into()], true).unwrap();
        assert!(g.addons_dir.join("A").exists());
    }
}
