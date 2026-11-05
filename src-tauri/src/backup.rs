//! Backups of the game's `WTF` folder, where addons keep their settings.
//!
//! Each backup is one zip named `wtf-<unix seconds>-<kind>.zip`. Kobold makes one
//! before updating addons (only when something changed since the last one) and before
//! a restore, so a restore can itself be undone.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::game::GameInfo;
use crate::install;
use crate::util::{now_secs, remove_dir_quiet, rename_retry};

/// Automatic backups kept; older ones are deleted. Manual ones are only deleted by hand.
pub const KEEP_AUTO: usize = 10;
/// Files the game writes as its own one-step backups; not worth keeping twice.
const SKIP_EXT: [&str; 2] = ["bak", "old"];
const NEW_DIR: &str = ".kobold-wtf-new";
const OLD_DIR: &str = ".kobold-wtf-old";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Made before an addon update.
    Auto,
    /// Made from the Back up now button.
    Manual,
    /// Made right before a restore replaced the folder.
    Restore,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::Auto => "auto",
            Kind::Manual => "manual",
            Kind::Restore => "restore",
        }
    }
    fn from_tag(s: &str) -> Option<Kind> {
        match s {
            "auto" => Some(Kind::Auto),
            "manual" => Some(Kind::Manual),
            "restore" => Some(Kind::Restore),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    /// File name, used as the id.
    pub name: String,
    pub created: u64,
    pub kind: Kind,
    pub bytes: u64,
}

fn parse_name(name: &str) -> Option<(u64, Kind)> {
    let stem = name.strip_prefix("wtf-")?.strip_suffix(".zip")?;
    let (secs, kind) = stem.split_once('-')?;
    Some((secs.parse().ok()?, Kind::from_tag(kind)?))
}

/// Backups in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let (created, kind) = parse_name(&name)?;
                    let bytes = e.metadata().ok()?.len();
                    Some(Backup {
                        name,
                        created,
                        kind,
                        bytes,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.created.cmp(&a.created).then(b.name.cmp(&a.name)));
    out
}

/// Resolves a backup name to its path, refusing anything that isn't a plain backup
/// file name (no folders, no `..`).
pub fn path_of(dir: &Path, name: &str) -> Result<PathBuf> {
    if parse_name(name).is_none() || name.contains(['/', '\\', ':']) {
        return Err(Error::NotFound("That backup doesn't exist.".into()));
    }
    let p = dir.join(name);
    if !p.is_file() {
        return Err(Error::NotFound("That backup doesn't exist anymore.".into()));
    }
    Ok(p)
}

fn files_under(root: &Path) -> Vec<(PathBuf, fs::Metadata)> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => {
                    let skip = p
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| SKIP_EXT.contains(&x.to_ascii_lowercase().as_str()))
                        .unwrap_or(false);
                    if !skip {
                        if let Ok(m) = e.metadata() {
                            out.push((p, m));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Newest modification time of any file in `WTF`.
fn wtf_changed_at(game: &GameInfo) -> Option<SystemTime> {
    files_under(&game.wtf_dir())
        .iter()
        .filter_map(|(_, m)| m.modified().ok())
        .max()
}

/// True when `WTF` has changed since the newest backup was written (or there is none).
/// Compares precise file times: the backup zip is written after every file it holds.
pub fn needs_backup(game: &GameInfo, dir: &Path) -> bool {
    if !game.wtf_dir().is_dir() {
        return false;
    }
    let newest = list(dir)
        .first()
        .and_then(|b| fs::metadata(dir.join(&b.name)).ok())
        .and_then(|m| m.modified().ok());
    match (newest, wtf_changed_at(game)) {
        (Some(backup), Some(changed)) => changed > backup,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// Zips `WTF` into `dir`. Returns `None` when there's no `WTF` folder yet (a fresh
/// install that has never been played).
pub fn create(game: &GameInfo, dir: &Path, kind: Kind) -> Result<Option<Backup>> {
    let wtf = game.wtf_dir();
    if !wtf.is_dir() {
        return Ok(None);
    }
    fs::create_dir_all(dir).map_err(|e| Error::io("create", dir, e))?;
    let mut created = now_secs();
    // Two backups in the same second would share a name.
    while dir
        .join(format!("wtf-{created}-{}.zip", kind.tag()))
        .exists()
    {
        created += 1;
    }
    let name = format!("wtf-{created}-{}.zip", kind.tag());
    let part = dir.join(format!("{name}.part"));
    let result = (|| -> Result<()> {
        let file = fs::File::create(&part).map_err(|e| Error::io("create", &part, e))?;
        let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let mut files = files_under(&wtf);
        files.sort_by(|a, b| a.0.cmp(&b.0));
        for (p, _) in files {
            let rel = p.strip_prefix(&wtf).unwrap_or(&p);
            let entry = rel.to_string_lossy().replace('\\', "/");
            // A file the game holds open can fail to read; skip it rather than lose
            // the whole backup.
            let data = match fs::read(&p) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(?p, error = %e, "skipped a file in the settings backup");
                    continue;
                }
            };
            zip.start_file(entry, opts)?;
            zip.write_all(&data)
                .map_err(|e| Error::io("write", &part, e))?;
        }
        let w = zip.finish()?;
        w.into_inner()
            .map_err(|e| Error::io("write", &part, e.into_error()))?
            .sync_all()
            .map_err(|e| Error::io("write", &part, e))?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    let path = dir.join(&name);
    rename_retry(&part, &path)?;
    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    tracing::info!(%name, bytes, "settings backup created");
    prune(dir);
    Ok(Some(Backup {
        name,
        created,
        kind,
        bytes,
    }))
}

/// Deletes automatic backups beyond the newest [`KEEP_AUTO`], and leftovers from
/// interrupted backups.
pub fn prune(dir: &Path) {
    let autos = list(dir)
        .into_iter()
        .filter(|b| b.kind != Kind::Manual)
        .skip(KEEP_AUTO);
    for b in autos {
        let _ = fs::remove_file(dir.join(&b.name));
        tracing::info!(name = %b.name, "removed old settings backup");
    }
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(".zip.part") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

/// Replaces `WTF` with the backup's contents. The caller makes a backup of the current
/// folder first. The swap is two renames on the same drive, and [`recover`] finishes
/// or undoes it if the PC goes down in between.
pub fn restore(game: &GameInfo, archive: &Path) -> Result<()> {
    let wtf = game.wtf_dir();
    let new = game.root.join(NEW_DIR);
    let old = game.root.join(OLD_DIR);
    remove_dir_quiet(&new);
    remove_dir_quiet(&old);
    fs::create_dir_all(&new).map_err(|e| Error::io("create", &new, e))?;
    if let Err(e) = install::extract(archive, &new) {
        remove_dir_quiet(&new);
        return Err(e);
    }
    let looks_right = new.join("Account").is_dir() || new.join("Config.wtf").is_file();
    if !looks_right {
        remove_dir_quiet(&new);
        return Err(Error::Archive(
            "this isn't a backup of the WTF folder".into(),
        ));
    }
    let had_wtf = wtf.exists();
    if had_wtf {
        rename_retry(&wtf, &old)?;
    }
    if let Err(e) = rename_retry(&new, &wtf) {
        if had_wtf {
            let _ = rename_retry(&old, &wtf);
        }
        remove_dir_quiet(&new);
        return Err(e);
    }
    remove_dir_quiet(&old);
    tracing::info!(?archive, "settings restored from backup");
    Ok(())
}

/// Finishes or undoes a restore that was interrupted.
pub fn recover(game: &GameInfo) {
    let wtf = game.wtf_dir();
    let new = game.root.join(NEW_DIR);
    let old = game.root.join(OLD_DIR);
    if old.exists() && !wtf.exists() {
        // Stopped between the two renames: put the original back.
        tracing::warn!("an interrupted settings restore was undone");
        if let Err(e) = rename_retry(&old, &wtf) {
            tracing::error!(error = %e, "couldn't put the WTF folder back");
            return;
        }
    }
    remove_dir_quiet(&new);
    if wtf.exists() {
        remove_dir_quiet(&old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(t: &Path) -> GameInfo {
        let root = t.join("_forever_");
        fs::create_dir_all(root.join("WTF/Account/ACC/SavedVariables")).unwrap();
        fs::write(root.join("WTF/Config.wtf"), "SET a 1").unwrap();
        fs::write(root.join("WTF/Account/ACC/SavedVariables/Bagnon.lua"), "v1").unwrap();
        fs::write(
            root.join("WTF/Account/ACC/SavedVariables/Bagnon.lua.bak"),
            "old",
        )
        .unwrap();
        // Character folders can have accented names.
        let ch = root.join("WTF/Account/ACC/Réalm/Çhâr/SavedVariables");
        fs::create_dir_all(&ch).unwrap();
        fs::write(ch.join("Pawn.lua"), "ü").unwrap();
        GameInfo {
            addons_dir: root.join("Interface/AddOns"),
            root,
            version: None,
            running: false,
        }
    }

    #[test]
    fn back_up_change_and_restore() {
        let t = tempfile::tempdir().unwrap();
        let g = game(t.path());
        let dir = t.path().join("backups");
        assert!(needs_backup(&g, &dir));
        let b = create(&g, &dir, Kind::Auto).unwrap().unwrap();
        assert_eq!(list(&dir).len(), 1);
        assert!(b.bytes > 0);

        let sv = g.wtf_dir().join("Account/ACC/SavedVariables/Bagnon.lua");
        fs::write(&sv, "broken").unwrap();
        fs::write(g.wtf_dir().join("Extra.txt"), "new").unwrap();
        restore(&g, &dir.join(&b.name)).unwrap();
        assert_eq!(fs::read_to_string(&sv).unwrap(), "v1");
        let accented = g
            .wtf_dir()
            .join("Account/ACC/Réalm/Çhâr/SavedVariables/Pawn.lua");
        assert_eq!(fs::read_to_string(accented).unwrap(), "ü");
        assert!(
            !g.wtf_dir().join("Extra.txt").exists(),
            "restore replaces the folder"
        );
        assert!(!g
            .wtf_dir()
            .join("Account/ACC/SavedVariables/Bagnon.lua.bak")
            .exists());
        assert!(!g.root.join(OLD_DIR).exists() && !g.root.join(NEW_DIR).exists());
    }

    #[test]
    fn names_are_checked() {
        let t = tempfile::tempdir().unwrap();
        assert!(path_of(t.path(), "../secret.zip").is_err());
        assert!(path_of(t.path(), "wtf-1-auto.zip").is_err());
        fs::write(t.path().join("wtf-1-auto.zip"), "").unwrap();
        assert!(path_of(t.path(), "wtf-1-auto.zip").is_ok());
        assert!(path_of(t.path(), "wtf-1-auto.zip/../x").is_err());
    }

    #[test]
    fn only_old_automatic_backups_are_pruned() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..14 {
            fs::write(t.path().join(format!("wtf-{}-auto.zip", 100 + i)), "").unwrap();
        }
        fs::write(t.path().join("wtf-1-manual.zip"), "").unwrap();
        fs::write(t.path().join("wtf-2-auto.zip.part"), "").unwrap();
        prune(t.path());
        let left = list(t.path());
        assert_eq!(left.len(), KEEP_AUTO + 1);
        assert!(left.iter().any(|b| b.kind == Kind::Manual));
        assert_eq!(left[0].created, 113);
        assert!(!t.path().join("wtf-2-auto.zip.part").exists());
    }

    #[test]
    fn bad_archive_leaves_wtf_alone() {
        let t = tempfile::tempdir().unwrap();
        let g = game(t.path());
        let junk = t.path().join("wtf-5-manual.zip");
        let mut z = zip::ZipWriter::new(fs::File::create(&junk).unwrap());
        z.start_file("readme.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"hi").unwrap();
        z.finish().unwrap();
        assert!(restore(&g, &junk).is_err());
        assert!(g.wtf_dir().join("Config.wtf").exists());
    }

    #[test]
    fn interrupted_restore_is_undone() {
        let t = tempfile::tempdir().unwrap();
        let g = game(t.path());
        fs::rename(g.wtf_dir(), g.root.join(OLD_DIR)).unwrap();
        fs::create_dir_all(g.root.join(NEW_DIR)).unwrap();
        recover(&g);
        assert!(g.wtf_dir().join("Config.wtf").exists());
        assert!(!g.root.join(NEW_DIR).exists());
    }
}
