//! Reading addon `.toc` files and deciding whether an addon is built for Forever.
//!
//! The client picks the most specific TOC in a folder: `Name_Forever.toc` (or the
//! older `_Camelot` name), then `Name_Mainline.toc`, then plain `Name.toc`. Both `_`
//! and `-` separators are accepted.

use std::fs;
use std::path::{Path, PathBuf};

use crate::util::strip_wow_markup;

/// Interface numbers used by the Forever client (1.60.x -> 16001).
pub const FOREVER_INTERFACE: std::ops::RangeInclusive<u32> = 16000..=16999;

const GAME_SUFFIXES: [&str; 2] = ["forever", "camelot"];
const FAMILY_SUFFIXES: [&str; 1] = ["mainline"];
/// Suffixes that mean "not for this client". Used to strip names and skip files.
const OTHER_SUFFIXES: [&str; 12] = [
    "classic",
    "vanilla",
    "tbc",
    "bcc",
    "wrath",
    "wotlkc",
    "cata",
    "mists",
    "standard",
    "plunderstorm",
    "wowhack",
    "anniversary",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Toc {
    pub file: PathBuf,
    pub title: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub interfaces: Vec<u32>,
    /// True when the file was a Forever/Camelot specific TOC.
    pub game_specific: bool,
}

impl Toc {
    pub fn built_for_forever(&self) -> bool {
        self.game_specific
            || self
                .interfaces
                .iter()
                .any(|i| FOREVER_INTERFACE.contains(i))
    }

    /// The `## Version` value, unless the packager left a placeholder in it.
    pub fn clean_version(&self) -> Option<String> {
        self.version
            .as_ref()
            .filter(|v| !v.contains('@') && !v.is_empty())
            .cloned()
    }
}

pub fn parse(text: &str) -> Toc {
    let mut toc = Toc::default();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        let Some(rest) = line.strip_prefix("##") else {
            continue;
        };
        let Some((key, value)) = rest.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "title" => toc.title = Some(strip_wow_markup(value)),
            "author" => toc.author = Some(strip_wow_markup(value)),
            "version" => toc.version = Some(value.to_string()),
            "notes" => toc.notes = Some(strip_wow_markup(value)),
            "interface" => {
                toc.interfaces = value
                    .split(',')
                    .filter_map(|v| v.trim().parse::<u32>().ok())
                    .collect()
            }
            _ => {}
        }
    }
    toc
}

/// Splits a TOC file stem into (addon name, flavor suffix), e.g.
/// `Bagnon_Forever` -> (`Bagnon`, Some("forever")).
pub fn split_stem(stem: &str) -> (&str, Option<String>) {
    if let Some(idx) = stem.rfind(['_', '-']) {
        let suffix = stem[idx + 1..].to_ascii_lowercase();
        let known = GAME_SUFFIXES.contains(&suffix.as_str())
            || FAMILY_SUFFIXES.contains(&suffix.as_str())
            || OTHER_SUFFIXES.contains(&suffix.as_str());
        if known && idx > 0 {
            return (&stem[..idx], Some(suffix));
        }
    }
    (stem, None)
}

/// All `.toc` file stems directly inside a folder.
pub fn toc_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return vec![];
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| e.eq_ignore_ascii_case("toc"))
                    .unwrap_or(false)
        })
        .collect()
}

/// Picks the TOC the Forever client would load for `dir`, if any.
pub fn read_for_folder(dir: &Path) -> Option<Toc> {
    let folder = dir.file_name()?.to_str()?;
    let mut best: Option<(u8, PathBuf, bool)> = None;
    for path in toc_files(dir) {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let (name, suffix) = split_stem(stem);
        if !name.eq_ignore_ascii_case(folder) {
            continue;
        }
        let (rank, game_specific) = match suffix.as_deref() {
            Some(s) if GAME_SUFFIXES.contains(&s) => (3, true),
            Some(s) if FAMILY_SUFFIXES.contains(&s) => (2, false),
            None => (1, false),
            Some(_) => continue,
        };
        if best.as_ref().map(|b| rank > b.0).unwrap_or(true) {
            best = Some((rank, path, game_specific));
        }
    }
    let (_, path, game_specific) = best?;
    let bytes = fs::read(&path).ok()?;
    let mut toc = parse(&String::from_utf8_lossy(&bytes));
    toc.file = path;
    toc.game_specific = game_specific;
    Some(toc)
}

/// The addon names (folder names) declared by TOC files inside `dir`,
/// with flavor suffixes removed. Used to repair badly packed archives.
pub fn declared_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = toc_files(dir)
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()))
        .map(|s| split_stem(s).0.to_string())
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields() {
        let t = parse(
            "\u{feff}## Interface: 11509, 16001\n## Title: |cff33ff99Bagnon|r\n## Version: 1.2.3\n## Author: Jali\nBagnon.lua\n",
        );
        assert_eq!(t.title.as_deref(), Some("Bagnon"));
        assert_eq!(t.interfaces, vec![11509, 16001]);
        assert!(t.built_for_forever());
        assert_eq!(t.clean_version().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn placeholder_version_is_ignored() {
        let t = parse("## Version: @project-version@\n");
        assert_eq!(t.clean_version(), None);
    }

    #[test]
    fn splits_flavor_suffixes() {
        assert_eq!(
            split_stem("Bagnon_Forever"),
            ("Bagnon", Some("forever".into()))
        );
        assert_eq!(
            split_stem("Bagnon-Mainline"),
            ("Bagnon", Some("mainline".into()))
        );
        assert_eq!(split_stem("Bagnon_Config"), ("Bagnon_Config", None));
        assert_eq!(split_stem("DBM-Core"), ("DBM-Core", None));
    }

    #[test]
    fn prefers_forever_toc() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("Thing");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("Thing.toc"), "## Interface: 120100\n").unwrap();
        fs::write(a.join("Thing_Mainline.toc"), "## Interface: 120100\n").unwrap();
        fs::write(a.join("Thing_Forever.toc"), "## Interface: 16001\n").unwrap();
        fs::write(a.join("Thing_Classic.toc"), "## Interface: 11509\n").unwrap();
        let t = read_for_folder(&a).unwrap();
        assert!(t.game_specific);
        assert!(t.file.ends_with("Thing_Forever.toc"));
    }

    #[test]
    fn retail_only_is_not_forever() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("Old");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("Old.toc"), "## Interface: 120100\n").unwrap();
        assert!(!read_for_folder(&a).unwrap().built_for_forever());
    }
}
