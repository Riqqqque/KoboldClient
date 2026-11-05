//! Saving the addon list to a file and reading it back, to set up the same addons on
//! another PC or share a setup with someone.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::manifest::Source;
use crate::sources::{github, wago, wowi};
use crate::util::{now_secs, write_atomic};

pub const FORMAT: u32 = 1;
/// Anything bigger isn't an addon list.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonList {
    /// Identifies the file; also its format version.
    pub kobold_addon_list: u32,
    #[serde(default)]
    pub exported_at: u64,
    #[serde(default)]
    pub game_version: Option<String>,
    pub addons: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// `cf:<id>` or `gh:<owner>/<repo>` for addons Kobold can install; anything else
    /// (zip imports, hand-installed folders) is listed so it can be found by hand.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: Option<Source>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub project_url: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Entry {
    /// True when the id is one Kobold can download.
    pub fn installable(&self) -> bool {
        if let Some(n) = self.id.strip_prefix("cf:") {
            return !n.is_empty() && n.len() < 12 && n.chars().all(|c| c.is_ascii_digit());
        }
        if let Some(r) = self.id.strip_prefix("gh:") {
            return github::parse_repo(r).is_some();
        }
        if let Some(r) = self.id.strip_prefix("wowi:") {
            return wowi::parse_ref(r).is_some();
        }
        if let Some(r) = self.id.strip_prefix("wago:") {
            return wago::parse_ref(r).is_some();
        }
        false
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportPreview {
    /// Kobold can install these.
    pub to_install: Vec<Entry>,
    /// Already installed here.
    pub present: Vec<Entry>,
    /// Have to be installed by hand (zip files, folders copied in).
    pub manual: Vec<Entry>,
}

pub fn save(path: &Path, list: &AddonList) -> Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(list)?)
}

pub fn new_list(entries: Vec<Entry>, game_version: Option<String>) -> AddonList {
    AddonList {
        kobold_addon_list: FORMAT,
        exported_at: now_secs(),
        game_version,
        addons: entries,
    }
}

pub fn load(path: &Path) -> Result<AddonList> {
    let meta = fs::metadata(path).map_err(|e| Error::io("open", path, e))?;
    if meta.len() > MAX_BYTES {
        return Err(Error::Other(
            "That file is too large to be an addon list.".into(),
        ));
    }
    let bytes = fs::read(path).map_err(|e| Error::io("read", path, e))?;
    let not_a_list = || Error::Other("That file isn't a Kobold Client addon list.".into());
    let list: AddonList = serde_json::from_slice(&bytes).map_err(|_| not_a_list())?;
    if list.kobold_addon_list == 0 {
        return Err(not_a_list());
    }
    if list.kobold_addon_list > FORMAT {
        return Err(Error::Other(
            "That list was saved by a newer Kobold Client. Update Kobold first.".into(),
        ));
    }
    if list.addons.len() > MAX_ENTRIES {
        return Err(Error::Other("That list has too many entries.".into()));
    }
    Ok(list)
}

/// Sorts a list's entries against what's installed. `is_present` answers by id, and
/// for entries Kobold can't install, by name.
pub fn preview(list: AddonList, is_present: impl Fn(&Entry) -> bool) -> ImportPreview {
    let mut p = ImportPreview::default();
    let mut seen = std::collections::HashSet::new();
    for mut e in list.addons {
        e.name = e.name.trim().chars().take(120).collect();
        if e.name.is_empty() {
            e.name = e.id.clone();
        }
        // Only keep web links; the UI opens them.
        e.project_url = e.project_url.filter(|u| u.starts_with("https://"));
        if !seen.insert(e.id.to_ascii_lowercase()) {
            continue;
        }
        if is_present(&e) {
            p.present.push(e);
        } else if e.installable() {
            p.to_install.push(e);
        } else {
            p.manual.push(e);
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            source: None,
            version: None,
            project_url: None,
            pinned: false,
            enabled: true,
        }
    }

    #[test]
    fn round_trip_and_preview() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("addons.json");
        let list = new_list(
            vec![
                entry("cf:334372", "Questie"),
                entry("gh:bigwigsmods/bigwigs", "BigWigs"),
                entry("zip:mybars", "MyBars"),
                entry("cf:18808", "TomTom"),
                entry("cf:18808", "TomTom again"),
                entry("cf:12x", "Broken id"),
            ],
            Some("1.60.1".into()),
        );
        save(&path, &list).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.addons.len(), 6);
        let p = preview(loaded, |e| e.id == "cf:18808");
        let ids = |v: &[Entry]| v.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&p.to_install),
            vec!["cf:334372", "gh:bigwigsmods/bigwigs"]
        );
        assert_eq!(ids(&p.present), vec!["cf:18808"]);
        assert_eq!(ids(&p.manual), vec!["zip:mybars", "cf:12x"]);
    }

    #[test]
    fn rejects_other_files() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("x.json");
        fs::write(&p, r#"{"addons":[]}"#).unwrap();
        assert!(load(&p).is_err());
        fs::write(&p, r#"{"kobold_addon_list":9,"addons":[]}"#).unwrap();
        assert!(load(&p).unwrap_err().to_string().contains("newer"));
        fs::write(&p, "not json").unwrap();
        assert!(load(&p).is_err());
    }
}
