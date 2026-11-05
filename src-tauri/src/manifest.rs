//! The list of addons Kobold installed, stored as JSON with rolling backups.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::util::{eq_ignore_case, now_secs, write_atomic};

pub const SCHEMA: u32 = 1;
const BACKUPS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    CurseForge,
    GitHub,
    Zip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAddon {
    /// `cf:<modId>`, `gh:<owner>/<repo>` or `zip:<folder>`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub summary: String,
    pub source: Source,
    pub version: String,
    /// CurseForge file id, used for update checks.
    #[serde(default)]
    pub file_id: Option<u64>,
    /// GitHub release tag, used for update checks.
    #[serde(default)]
    pub release_tag: Option<String>,
    #[serde(default)]
    pub project_url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub folders: Vec<String>,
    #[serde(default)]
    pub installed_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub pinned: bool,
    /// SHA-256 of the downloaded archive, kept for troubleshooting.
    #[serde(default)]
    pub archive_sha256: Option<String>,
    /// Other addons this one can't work without, as the source lists them.
    /// `None` means it hasn't been looked up yet (lists written by 0.1.1 and older).
    #[serde(default)]
    pub requires: Option<Vec<Requirement>>,
}

/// A required addon, by id (`cf:<modId>`) and display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub name: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub addons: Vec<InstalledAddon>,
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            schema: SCHEMA,
            addons: vec![],
        }
    }
}

impl Manifest {
    pub fn get(&self, id: &str) -> Option<&InstalledAddon> {
        self.addons.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut InstalledAddon> {
        self.addons.iter_mut().find(|a| a.id == id)
    }

    /// Ids of every addon that claims `folder`, excluding `except`.
    pub fn owners_of(&self, folder: &str, except: &str) -> Vec<&InstalledAddon> {
        self.addons
            .iter()
            .filter(|a| a.id != except && a.folders.iter().any(|f| eq_ignore_case(f, folder)))
            .collect()
    }

    pub fn owns(&self, folder: &str) -> bool {
        self.addons
            .iter()
            .any(|a| a.folders.iter().any(|f| eq_ignore_case(f, folder)))
    }

    /// Inserts or replaces by id, keeping the original install time and user flags.
    pub fn upsert(&mut self, mut addon: InstalledAddon) {
        if let Some(old) = self.get_mut(&addon.id) {
            addon.installed_at = old.installed_at;
            addon.pinned = old.pinned;
            addon.updated_at = now_secs();
            *old = addon;
        } else {
            if addon.installed_at == 0 {
                addon.installed_at = now_secs();
            }
            addon.updated_at = addon.installed_at;
            self.addons.push(addon);
        }
        self.addons.sort_by_key(|a| a.name.to_ascii_lowercase());
    }

    /// Names of installed addons that list `id` as required.
    pub fn needed_by(&self, id: &str) -> Vec<String> {
        self.addons
            .iter()
            .filter(|a| a.id != id && a.requires.iter().flatten().any(|r| r.id == id))
            .map(|a| a.name.clone())
            .collect()
    }

    /// Requirements of `addon` that aren't installed.
    pub fn missing_for(&self, addon: &InstalledAddon) -> Vec<Requirement> {
        addon
            .requires
            .iter()
            .flatten()
            .filter(|r| r.id != addon.id && self.get(&r.id).is_none())
            .cloned()
            .collect()
    }

    pub fn remove(&mut self, id: &str) -> Option<InstalledAddon> {
        let pos = self.addons.iter().position(|a| a.id == id)?;
        Some(self.addons.remove(pos))
    }
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Self {
        Store {
            path: data_dir.join("manifest.json"),
        }
    }

    fn backup(&self, n: usize) -> PathBuf {
        self.path.with_extension(format!("json.bak{n}"))
    }

    /// Loads the manifest. A damaged file falls back to the newest good backup,
    /// and the damaged copy is kept aside for inspection.
    pub fn load(&self) -> Manifest {
        if !self.path.exists() {
            return Manifest::default();
        }
        match read(&self.path) {
            Ok(m) => return m,
            Err(e) => {
                tracing::error!(error = %e, "manifest is damaged, trying backups");
                let aside = self
                    .path
                    .with_extension(format!("corrupt-{}.json", now_secs()));
                let _ = fs::copy(&self.path, aside);
            }
        }
        for n in 1..=BACKUPS {
            if let Ok(m) = read(&self.backup(n)) {
                tracing::warn!(backup = n, "restored manifest from backup");
                return m;
            }
        }
        tracing::error!("no usable manifest backup; starting with an empty list");
        Manifest::default()
    }

    pub fn save(&self, m: &Manifest) -> Result<()> {
        if self.path.exists() {
            for n in (1..BACKUPS).rev() {
                let from = self.backup(n);
                if from.exists() {
                    let _ = fs::rename(&from, self.backup(n + 1));
                }
            }
            let _ = fs::copy(&self.path, self.backup(1));
        }
        let data = serde_json::to_vec_pretty(m)?;
        write_atomic(&self.path, &data)
    }
}

fn read(path: &Path) -> Result<Manifest> {
    let bytes = fs::read(path).map_err(|e| Error::io("read", path, e))?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    if m.schema > SCHEMA {
        return Err(Error::Other(format!(
            "manifest was written by a newer Kobold Client (schema {})",
            m.schema
        )));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon(id: &str, folders: &[&str]) -> InstalledAddon {
        InstalledAddon {
            id: id.into(),
            name: id.into(),
            author: String::new(),
            summary: String::new(),
            source: Source::Zip,
            version: "1".into(),
            file_id: None,
            release_tag: None,
            project_url: None,
            icon_url: None,
            folders: folders.iter().map(|s| s.to_string()).collect(),
            installed_at: 0,
            updated_at: 0,
            enabled: true,
            pinned: false,
            archive_sha256: None,
            requires: None,
        }
    }

    #[test]
    fn shared_folder_owners() {
        let mut m = Manifest::default();
        m.upsert(addon("a", &["A", "Libs"]));
        m.upsert(addon("b", &["B", "libs"]));
        assert_eq!(m.owners_of("LIBS", "a").len(), 1);
        assert!(m.owners_of("A", "a").is_empty());
    }

    #[test]
    fn missing_requirements() {
        let mut m = Manifest::default();
        let mut q = addon("cf:1", &["Questie"]);
        q.requires = Some(vec![
            Requirement {
                id: "cf:2".into(),
                name: "QuestieDB".into(),
            },
            Requirement {
                id: "cf:3".into(),
                name: "QuestieTrace".into(),
            },
        ]);
        m.upsert(q.clone());
        m.upsert(addon("cf:3", &["QuestieTrace"]));
        let missing = m.missing_for(&q);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "QuestieDB");
        assert_eq!(m.needed_by("cf:3"), vec!["cf:1".to_string()]);
        assert!(m.needed_by("cf:1").is_empty());
    }

    #[test]
    fn old_lists_load_without_requirements() {
        let json = r#"{"schema":1,"addons":[{"id":"cf:1","name":"X","source":"curseforge","version":"1","folders":["X"]}]}"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert!(m.addons[0].requires.is_none());
    }

    #[test]
    fn recovers_from_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let mut m = Manifest::default();
        m.upsert(addon("a", &["A"]));
        store.save(&m).unwrap();
        store.save(&m).unwrap(); // creates bak1
        fs::write(dir.path().join("manifest.json"), b"{ not json").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.addons.len(), 1);
    }
}
