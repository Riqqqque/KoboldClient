use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Short random-ish id for operations and temp folders.
pub fn op_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{:x}{:04x}",
        nanos & 0xffff_ffff_ffff,
        std::process::id() & 0xffff
    )
}

/// Rename with a few retries. Antivirus scanners and the Windows indexer briefly lock
/// freshly written files, which makes a single rename fail for no lasting reason.
pub fn rename_retry(from: &Path, to: &Path) -> Result<()> {
    let mut last = None;
    for attempt in 0..5u64 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::debug!(?from, ?to, attempt, error = %e, "rename failed, retrying");
                last = Some(e);
                sleep(Duration::from_millis(150 * (attempt + 1)));
            }
        }
    }
    Err(Error::io("move", from, last.expect("at least one attempt")))
}

pub fn remove_dir_quiet(path: &Path) {
    if path.exists() {
        if let Err(e) = fs::remove_dir_all(path) {
            tracing::warn!(?path, error = %e, "couldn't clean up folder");
        }
    }
}

/// Writes a file atomically: temp file, flush to disk, then rename over the target.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io("create", dir, e))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| Error::io("write", &tmp, e))?;
        f.write_all(data).map_err(|e| Error::io("write", &tmp, e))?;
        f.sync_all().map_err(|e| Error::io("write", &tmp, e))?;
    }
    rename_retry(&tmp, path)
}

pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Total size of a folder in bytes. Errors are skipped; this is only for display.
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(rd) = fs::read_dir(&p) else { continue };
        for entry in rd.flatten() {
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(entry.path()),
                Ok(_) => total += entry.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => {}
            }
        }
    }
    total
}

/// Removes WoW colour codes (`|cffRRGGBB ... |r`) and texture escapes from TOC text.
pub fn strip_wow_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '|' {
            match chars.peek().copied() {
                Some('c') | Some('C') => {
                    chars.next();
                    for _ in 0..8 {
                        chars.next();
                    }
                    continue;
                }
                Some('r') | Some('R') => {
                    chars.next();
                    continue;
                }
                Some('T') | Some('A') => {
                    // |T...|t and |A...|a texture/atlas escapes
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '|' {
                            chars.next();
                            break;
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colour_codes() {
        assert_eq!(strip_wow_markup("|cff00ff00Questie|r"), "Questie");
        assert_eq!(strip_wow_markup("|TInterface\\Icons\\x:16|t Bags"), "Bags");
        assert_eq!(strip_wow_markup("Plain"), "Plain");
    }
}
