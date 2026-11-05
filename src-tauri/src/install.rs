//! Safe install: extract to staging, normalise the layout, swap folders in with a
//! journal, and roll back on any failure. A crash at any point is repaired on the
//! next launch by [`recover`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::game::GameInfo;
use crate::toc;
use crate::util::{op_id, remove_dir_quiet, rename_retry};

const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const JUNK_DIRS: [&str; 3] = ["__macosx", ".git", ".github"];

/// One addon folder ready to move into `AddOns`.
#[derive(Debug, Clone)]
pub struct StagedFolder {
    pub name: String,
    pub path: PathBuf,
}

/// Extracts `archive` into a fresh folder under `staging_root` and returns the addon
/// folders found inside it.
pub fn stage(archive: &Path, staging_root: &Path) -> Result<(PathBuf, Vec<StagedFolder>)> {
    let dir = staging_root.join(op_id());
    fs::create_dir_all(&dir).map_err(|e| Error::io("create", &dir, e))?;
    match extract(archive, &dir).and_then(|_| find_addon_folders(&dir)) {
        Ok(folders) => Ok((dir, folders)),
        Err(e) => {
            remove_dir_quiet(&dir);
            Err(e)
        }
    }
}

fn extract(archive: &Path, out: &Path) -> Result<()> {
    let file = fs::File::open(archive).map_err(|e| Error::io("open", archive, e))?;
    let mut zip = zip::ZipArchive::new(io::BufReader::new(file))?;
    if zip.len() > MAX_ENTRIES {
        return Err(Error::Archive(format!("too many files ({})", zip.len())));
    }
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // enclosed_name rejects absolute paths, drive prefixes and `..` (zip-slip).
        let Some(rel) = entry.enclosed_name() else {
            return Err(Error::Archive(format!(
                "unsafe path in archive: {}",
                entry.name().map(|n| n.to_string()).unwrap_or_default()
            )));
        };
        let target = out.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| Error::io("create", &target, e))?;
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > MAX_UNPACKED {
            return Err(Error::Archive("unpacks to more than 2 GB".into()));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io("create", parent, e))?;
        }
        let mut f = fs::File::create(&target).map_err(|e| Error::io("write", &target, e))?;
        // Cap the copy at the declared size so a lying header can't fill the disk.
        let mut limited = io::Read::take(&mut entry, MAX_UNPACKED);
        io::copy(&mut limited, &mut f).map_err(|e| Error::io("write", &target, e))?;
    }
    Ok(())
}

/// Works out which folders in an extracted archive are addons. Handles:
/// - the normal case (one or more top-level addon folders),
/// - TOC files at the archive root (no folder),
/// - an extra wrapper folder (`Repo-1.2/Addon/...`, GitHub source zips),
/// - folders whose name doesn't match the TOC inside (renamed to the TOC name).
pub fn find_addon_folders(root: &Path) -> Result<Vec<StagedFolder>> {
    let mut base = root.to_path_buf();
    for _ in 0..3 {
        // Loose TOCs at this level: the files belong in a folder named after the TOC.
        let names = toc::declared_names(&base);
        if names.len() == 1
            && subdirs(&base)
                .iter()
                .all(|d| toc::declared_names(d).is_empty())
        {
            let name = names[0].clone();
            let wrap = root.join(format!(".wrap-{}", op_id()));
            let target = wrap.join(&name);
            fs::create_dir_all(&target).map_err(|e| Error::io("create", &target, e))?;
            let children: Vec<PathBuf> = fs::read_dir(&base)
                .map_err(|e| Error::io("read", &base, e))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p != &wrap)
                .collect();
            for child in children {
                rename_retry(&child, &target.join(child.file_name().unwrap()))?;
            }
            return Ok(vec![StagedFolder { name, path: target }]);
        }
        let dirs = subdirs(&base);
        let addon_dirs: Vec<&PathBuf> = dirs
            .iter()
            .filter(|d| !toc::declared_names(d).is_empty())
            .collect();
        if !addon_dirs.is_empty() {
            let mut out = vec![];
            for d in addon_dirs {
                let current = d.file_name().unwrap().to_string_lossy().to_string();
                let declared = toc::declared_names(d);
                let name = if declared.iter().any(|n| n.eq_ignore_ascii_case(&current)) {
                    current
                } else if declared.len() == 1 {
                    tracing::info!(from = %current, to = %declared[0], "renaming folder to match its TOC");
                    declared[0].clone()
                } else {
                    current
                };
                out.push(StagedFolder {
                    name,
                    path: d.clone(),
                });
            }
            let skipped: Vec<_> = dirs
                .iter()
                .filter(|d| toc::declared_names(d).is_empty())
                .collect();
            if !skipped.is_empty() {
                tracing::debug!(?skipped, "ignoring top-level folders without a TOC");
            }
            return Ok(out);
        }
        // No addons at this level; descend if there's exactly one wrapper folder.
        if dirs.len() == 1 {
            base = dirs[0].clone();
        } else {
            break;
        }
    }
    Err(Error::NoAddonInArchive)
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(p) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            let n = p
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            !JUNK_DIRS.contains(&n.as_str()) && !n.starts_with(".wrap-")
        })
        .collect();
    v.sort();
    v
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    committed: bool,
    /// Folders moved into AddOns by this operation.
    placed: Vec<PathBuf>,
    /// (original location, backup location) for folders moved out of the way.
    moved_aside: Vec<(PathBuf, PathBuf)>,
}

/// A swap in progress. Call [`Swap::commit`] after the manifest is saved, or drop /
/// [`Swap::rollback`] to restore everything.
pub struct Swap {
    dir: PathBuf,
    journal: Journal,
    done: bool,
}

impl Swap {
    fn write_journal(&self) -> Result<()> {
        let data = serde_json::to_vec(&self.journal)?;
        crate::util::write_atomic(&self.dir.join("journal.json"), &data)
    }

    pub fn commit(mut self) {
        self.journal.committed = true;
        if let Err(e) = self.write_journal() {
            tracing::warn!(error = %e, "couldn't mark swap as committed");
        }
        remove_dir_quiet(&self.dir);
        self.done = true;
    }

    pub fn rollback(mut self) {
        rollback_journal(&self.journal);
        remove_dir_quiet(&self.dir);
        self.done = true;
    }
}

impl Drop for Swap {
    fn drop(&mut self) {
        if !self.done {
            tracing::warn!("swap dropped without commit; rolling back");
            rollback_journal(&self.journal);
            remove_dir_quiet(&self.dir);
        }
    }
}

fn rollback_journal(j: &Journal) {
    for placed in &j.placed {
        remove_dir_quiet(placed);
    }
    for (orig, backup) in j.moved_aside.iter().rev() {
        if backup.exists() {
            if let Err(e) = rename_retry(backup, orig) {
                tracing::error!(?orig, error = %e, "rollback couldn't restore folder");
            }
        }
    }
}

/// Moves the staged folders into `AddOns`, moving existing ones (and `stale` folders
/// from the previous version) aside first. Nothing is permanently removed until commit.
pub fn swap_in(game: &GameInfo, staged: &[StagedFolder], stale: &[String]) -> Result<Swap> {
    let addons = &game.addons_dir;
    fs::create_dir_all(addons).map_err(|e| Error::io("create", addons, e))?;
    let dir = game.backup_dir().join(op_id());
    fs::create_dir_all(&dir).map_err(|e| Error::io("create", &dir, e))?;
    let mut swap = Swap {
        dir,
        journal: Journal {
            committed: false,
            placed: vec![],
            moved_aside: vec![],
        },
        done: false,
    };
    swap.write_journal()?;

    let aside = |name: &str, swap: &mut Swap| -> Result<()> {
        let orig = addons.join(name);
        if orig.exists() {
            let backup = swap.dir.join(name);
            rename_retry(&orig, &backup)?;
            swap.journal.moved_aside.push((orig, backup));
            swap.write_journal()?;
        }
        Ok(())
    };
    for f in staged {
        aside(&f.name, &mut swap)?;
    }
    for name in stale {
        if !staged.iter().any(|f| f.name.eq_ignore_ascii_case(name)) {
            aside(name, &mut swap)?;
        }
    }
    for f in staged {
        let target = addons.join(&f.name);
        rename_retry(&f.path, &target)?;
        swap.journal.placed.push(target);
        swap.write_journal()?;
    }
    Ok(swap)
}

/// Repairs anything a crash left behind: uncommitted swaps are rolled back, and
/// leftover staging folders are removed.
pub fn recover(game: &GameInfo) {
    remove_dir_quiet(&game.staging_dir());
    let Ok(rd) = fs::read_dir(game.backup_dir()) else {
        return;
    };
    for entry in rd.flatten() {
        let dir = entry.path();
        match fs::read(dir.join("journal.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Journal>(&b).ok())
        {
            Some(j) if !j.committed => {
                tracing::warn!(?dir, "rolling back an interrupted install");
                rollback_journal(&j);
            }
            _ => {}
        }
        remove_dir_quiet(&dir);
    }
    let _ = fs::remove_dir(game.backup_dir());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let f = fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    fn game(dir: &Path) -> GameInfo {
        let root = dir.join("_forever_");
        fs::create_dir_all(root.join("Interface").join("AddOns")).unwrap();
        GameInfo {
            addons_dir: root.join("Interface").join("AddOns"),
            root,
            version: None,
            running: false,
        }
    }

    #[test]
    fn normal_layout() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("a.zip");
        make_zip(
            &z,
            &[
                ("Bagnon/Bagnon.toc", "## Interface: 16001"),
                ("Bagnon_Config/Bagnon_Config.toc", ""),
                ("Bagnon/x.lua", ""),
            ],
        );
        let (_d, f) = stage(&z, &t.path().join("st")).unwrap();
        let names: Vec<_> = f.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["Bagnon", "Bagnon_Config"]);
    }

    #[test]
    fn loose_toc_and_wrapper_folder() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("a.zip");
        make_zip(&z, &[("TomTom.toc", ""), ("TomTom.lua", "")]);
        let (_d, f) = stage(&z, &t.path().join("st")).unwrap();
        assert_eq!(f[0].name, "TomTom");
        assert!(f[0].path.join("TomTom.lua").exists());

        let z2 = t.path().join("b.zip");
        make_zip(
            &z2,
            &[
                ("tomtom-v4.1.0/TomTom.toc", ""),
                ("tomtom-v4.1.0/TomTom_Forever.toc", ""),
            ],
        );
        let (_d, f) = stage(&z2, &t.path().join("st")).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "TomTom");
    }

    #[test]
    fn rejects_zip_slip() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("evil.zip");
        make_zip(&z, &[("../../evil.txt", "x"), ("A/A.toc", "")]);
        let err = stage(&z, &t.path().join("st")).unwrap_err();
        assert!(matches!(err, Error::Archive(_)));
        assert!(!t.path().join("evil.txt").exists());
    }

    #[test]
    fn no_addon_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("a.zip");
        make_zip(&z, &[("readme.txt", "hi")]);
        assert!(matches!(
            stage(&z, &t.path().join("st")).unwrap_err(),
            Error::NoAddonInArchive
        ));
    }

    #[test]
    fn swap_commit_and_rollback() {
        let t = tempfile::tempdir().unwrap();
        let g = game(t.path());
        let old = g.addons_dir.join("A");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("old.txt"), "old").unwrap();
        fs::create_dir_all(g.addons_dir.join("A_Stale")).unwrap();

        // Rollback restores the old version and the stale folder.
        let z = t.path().join("a.zip");
        make_zip(&z, &[("A/A.toc", ""), ("A/new.txt", "new")]);
        let (_d, staged) = stage(&z, &g.staging_dir()).unwrap();
        let swap = swap_in(&g, &staged, &["A_Stale".into()]).unwrap();
        assert!(g.addons_dir.join("A/new.txt").exists());
        assert!(!g.addons_dir.join("A_Stale").exists());
        swap.rollback();
        assert!(g.addons_dir.join("A/old.txt").exists());
        assert!(!g.addons_dir.join("A/new.txt").exists());
        assert!(g.addons_dir.join("A_Stale").exists());

        // Commit keeps the new version and drops backups.
        let (_d, staged) = stage(&z, &g.staging_dir()).unwrap();
        swap_in(&g, &staged, &["A_Stale".into()]).unwrap().commit();
        assert!(g.addons_dir.join("A/new.txt").exists());
        assert!(!g.addons_dir.join("A_Stale").exists());
        assert_eq!(fs::read_dir(g.backup_dir()).unwrap().count(), 0);
    }

    #[test]
    fn recover_rolls_back_crash() {
        let t = tempfile::tempdir().unwrap();
        let g = game(t.path());
        fs::create_dir_all(g.addons_dir.join("A")).unwrap();
        fs::write(g.addons_dir.join("A/old.txt"), "old").unwrap();
        let z = t.path().join("a.zip");
        make_zip(&z, &[("A/A.toc", "")]);
        let (_d, staged) = stage(&z, &g.staging_dir()).unwrap();
        let swap = swap_in(&g, &staged, &[]).unwrap();
        std::mem::forget(swap); // simulate a crash: no commit, no drop
        recover(&g);
        assert!(g.addons_dir.join("A/old.txt").exists());
        assert!(!g.backup_dir().exists());
    }
}
