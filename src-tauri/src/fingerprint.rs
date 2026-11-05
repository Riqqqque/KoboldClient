//! CurseForge fingerprints, used to recognise addons that were installed by hand.
//!
//! CurseForge identifies an addon folder by a number derived from the files its TOC
//! loads: each file is hashed with MurmurHash2 (whitespace ignored), the hashes are
//! sorted, joined as decimal text, and that text is hashed again. Only files reached
//! from the TOC (and `Bindings.xml`) count, so stray files like readmes or `.git`
//! leftovers don't change the result.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// TOC suffixes CurseForge's scanner accepts after `<Folder>` and `-`/`_`.
const TOC_FLAVORS: [&str; 12] = [
    "mainline", "bcc", "tbc", "classic", "vanilla", "wrath", "wotlkc", "cata", "mists", "forever",
    "camelot", "",
];

/// Folder walks stop here; real addons are far smaller.
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

fn is_space(b: u8) -> bool {
    matches!(b, 9 | 10 | 13 | 32)
}

/// MurmurHash2 with seed 1, skipping tab, CR, LF and space, as CurseForge computes it.
pub fn murmur2(data: &[u8]) -> u32 {
    const M: u32 = 1_540_483_477;
    let len = data.iter().filter(|b| !is_space(**b)).count() as u32;
    let mut h: u32 = 1 ^ len;
    let (mut acc, mut shift) = (0u32, 0u32);
    for &b in data.iter().filter(|b| !is_space(**b)) {
        acc |= (b as u32) << shift;
        shift += 8;
        if shift == 32 {
            let k = acc.wrapping_mul(M);
            let k = (k ^ (k >> 24)).wrapping_mul(M);
            h = h.wrapping_mul(M) ^ k;
            acc = 0;
            shift = 0;
        }
    }
    if shift > 0 {
        h = (h ^ acc).wrapping_mul(M);
    }
    let h = (h ^ (h >> 13)).wrapping_mul(M);
    h ^ (h >> 15)
}

/// Fingerprint of one addon folder, or `None` if it has no TOC CurseForge would read.
pub fn folder_fingerprint(dir: &Path) -> Option<u32> {
    let name = dir.file_name()?.to_str()?.to_ascii_lowercase();
    // Lower-cased path -> real path, so includes with the wrong case still resolve.
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => {
                    files.insert(key(&p), p);
                }
                _ => {}
            }
            if files.len() > MAX_FILES {
                tracing::debug!(?dir, "too many files to fingerprint");
                return None;
            }
        }
    }

    let mut matched: Vec<PathBuf> = vec![];
    let mut tocs = vec![];
    for p in files.values() {
        let Ok(rel) = p.strip_prefix(dir) else {
            continue;
        };
        if rel.components().count() != 1 {
            continue;
        }
        let file = rel.to_string_lossy().to_ascii_lowercase();
        if is_addon_toc(&file, &name) {
            tocs.push(p.clone());
        } else if file == "bindings.xml" {
            matched.push(p.clone());
        }
    }
    if tocs.is_empty() {
        return None;
    }
    tocs.sort();
    for toc in tocs {
        include(&files, &mut matched, &toc, 0);
    }

    let mut hashes: Vec<u32> = matched
        .iter()
        .filter_map(|p| {
            let meta = fs::metadata(p).ok()?;
            if meta.len() > MAX_FILE_BYTES {
                return None;
            }
            fs::read(p).ok().map(|b| murmur2(&b))
        })
        .collect();
    hashes.sort_unstable();
    let joined: String = hashes.iter().map(|h| h.to_string()).collect();
    Some(murmur2(joined.as_bytes()))
}

fn key(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// `Name.toc`, or `Name-Flavor.toc` / `Name_Flavor.toc` for a flavor CurseForge knows.
fn is_addon_toc(file: &str, folder: &str) -> bool {
    let Some(stem) = file.strip_suffix(".toc") else {
        return false;
    };
    let Some(rest) = stem.strip_prefix(folder) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let mut chars = rest.chars();
    let sep = chars.next();
    matches!(sep, Some('-' | '_' | '|'))
        && TOC_FLAVORS
            .iter()
            .any(|f| !f.is_empty() && chars.as_str() == *f)
}

/// Adds `file` and, for TOC and XML files, everything they load.
fn include(files: &HashMap<String, PathBuf>, out: &mut Vec<PathBuf>, file: &Path, depth: u32) {
    let Some(real) = files.get(&key(file)) else {
        return;
    };
    if out.contains(real) || depth > 64 {
        return;
    }
    out.push(real.clone());
    let ext = real
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "toc" && ext != "xml" {
        return;
    }
    let Ok(bytes) = fs::read(real) else { return };
    let text = String::from_utf8_lossy(&bytes);
    let includes = if ext == "toc" {
        toc_includes(&text)
    } else {
        xml_includes(&text)
    };
    let dir = real.parent().unwrap_or(Path::new(""));
    for inc in includes {
        if inc.chars().any(|c| c == '|' || (c as u32) < 32) {
            // CurseForge's scanner stops reading this file's includes here.
            break;
        }
        include(files, out, &dir.join(inc.replace('/', "\\")), depth + 1);
    }
}

/// Strips `#` comments the way CurseForge's scanner does: a `#` and the rest of its
/// line go, along with any whitespace (including line breaks) right before it.
fn strip_hash_comments(text: &str) -> String {
    let parts: Vec<&str> = text.split('#').collect();
    let mut out = String::with_capacity(text.len());
    for (i, part) in parts.iter().enumerate() {
        let part = if i == 0 {
            part
        } else {
            // Drop the comment up to (not including) the line break.
            match part.find('\n') {
                Some(n) => &part[n..],
                None => "",
            }
        };
        out.push_str(part);
        // Whitespace before the next `#` belongs to the comment.
        if i + 1 < parts.len() {
            let trimmed = out.trim_end().len();
            out.truncate(trimmed);
        }
    }
    out
}

fn toc_includes(text: &str) -> Vec<String> {
    strip_hash_comments(text)
        .split('\n')
        .map(str::trim)
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            l.len() > 4 && (lower.ends_with(".lua") || lower.ends_with(".xml")) && !l.contains("..")
        })
        .map(str::to_string)
        .collect()
}

fn strip_xml_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(end) => rest = &rest[start + 4 + end + 3..],
            None => {
                // An unclosed comment isn't a comment to the regex CurseForge uses.
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// First `<Include file="..."/>` or `<Script file="..."/>` on each line.
fn xml_includes(text: &str) -> Vec<String> {
    strip_xml_comments(text)
        .split('\n')
        .filter_map(|line| xml_include_on_line(line.trim()))
        .collect()
}

fn xml_include_on_line(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find('<').map(|p| p + from) {
        from = pos + 1;
        let after = &lower[pos + 1..];
        let tag_len = if after.starts_with("include") {
            7
        } else if after.starts_with("script") {
            6
        } else {
            continue;
        };
        let mut i = pos + 1 + tag_len;
        let ws = lower[i..].len() - lower[i..].trim_start().len();
        if ws == 0 {
            continue;
        }
        i += ws;
        if !lower[i..].starts_with("file=") {
            continue;
        }
        i += 5;
        if !matches!(lower.as_bytes().get(i), Some(b'"' | b'\'')) {
            continue;
        }
        let body_start = i + 1;
        // Greedy, like the original pattern: the last quote followed by `/>`.
        let tail = &line[body_start..];
        let mut best = None;
        for (q, c) in tail.char_indices() {
            if c == '"' || c == '\'' {
                let after_q = tail[q + 1..].trim_start();
                if after_q.starts_with("/>") {
                    best = Some(q);
                }
            }
        }
        let end = best?;
        let path = tail[..end].trim();
        if path.is_empty() || path.contains("..") {
            return None;
        }
        return Some(path.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur_ignores_whitespace() {
        assert_eq!(murmur2(b"abc def\r\n"), murmur2(b"abcdef"));
        assert_ne!(murmur2(b"abcdef"), murmur2(b"abcdeg"));
        assert_eq!(murmur2(b""), murmur2(b" \t\r\n"));
    }

    #[test]
    fn toc_names() {
        assert!(is_addon_toc("questie.toc", "questie"));
        assert!(is_addon_toc("questie-forever.toc", "questie"));
        assert!(is_addon_toc("questie_mainline.toc", "questie"));
        assert!(!is_addon_toc("questie-xptr.toc", "questie"));
        assert!(!is_addon_toc("other.toc", "questie"));
    }

    #[test]
    fn comments_and_includes() {
        let toc = "## Title: X\n## Interface: 16001\nCore.lua # main\n  Sub\\File.xml\n#Skip.lua\n..\\Evil.lua\nreadme.txt\n";
        assert_eq!(toc_includes(toc), vec!["Core.lua", "Sub\\File.xml"]);
        let xml = "<Ui>\n<!-- <Script file=\"no.lua\"/> -->\n<Script file=\"a.lua\"/>\n  <Include file='b.xml' />\n<Frame name=\"x\"/>\n</Ui>";
        assert_eq!(xml_includes(xml), vec!["a.lua", "b.xml"]);
    }

    #[test]
    fn folder_fingerprint_follows_includes_only() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("Addon");
        fs::create_dir_all(d.join("Libs")).unwrap();
        fs::write(
            d.join("Addon.toc"),
            "## Interface: 16001\nMain.lua\nLibs\\Libs.xml\n",
        )
        .unwrap();
        fs::write(d.join("Main.lua"), "print(1)").unwrap();
        fs::write(
            d.join("Libs/Libs.xml"),
            "<Ui><Script file=\"Lib.lua\"/></Ui>",
        )
        .unwrap();
        fs::write(d.join("Libs/Lib.lua"), "-- lib").unwrap();
        let a = folder_fingerprint(&d).unwrap();
        // Files nothing loads don't count.
        fs::write(d.join("README.md"), "hello").unwrap();
        assert_eq!(folder_fingerprint(&d), Some(a));
        // Whitespace changes don't count either.
        fs::write(d.join("Main.lua"), "print(1)\r\n").unwrap();
        assert_eq!(folder_fingerprint(&d), Some(a));
        // A loaded file changing does.
        fs::write(d.join("Libs/Lib.lua"), "-- lib v2").unwrap();
        assert_ne!(folder_fingerprint(&d), Some(a));
        assert!(folder_fingerprint(&t.path().join("Missing")).is_none());
    }
}
