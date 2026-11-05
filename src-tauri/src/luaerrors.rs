//! Reads Lua errors from the game's `Logs\General.log` and works out which addon
//! caused each one.
//!
//! The game writes a fresh log every time it starts. An error looks like:
//!
//! ```text
//! 11/4 22:23:59.287  [E][Lua] Lua Error: ...ddOns/Blizzard_UnitFrame/...lua:698: attempt to ... (... tainted by 'TomTom')
//! [Interface/AddOns/Blizzard_UnitFrame/Shared/CompactUnitFrame.lua]:698: in function '...'
//! [C]: in function 'Show'
//!
//! ```
//!
//! followed by stack lines until a blank line or the next timestamped line.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;

/// Only the end of a huge log is read; the game starts a new one every launch anyway.
const MAX_READ: u64 = 8 * 1024 * 1024;
const MAX_STACK: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Blame {
    /// The addon's own code failed.
    Code,
    /// The game's UI failed because the addon tainted it (common with protected frames).
    Taint,
}

#[derive(Debug, Clone, Serialize)]
pub struct LuaError {
    /// `11/4 22:23:59` as the game wrote it.
    pub time: String,
    pub message: String,
    pub stack: Vec<String>,
    /// Folder name of the addon this points at, if any.
    pub folder: Option<String>,
    pub blame: Blame,
}

#[derive(Debug, Clone, Default)]
pub struct Log {
    /// The first timestamp in the file; identifies one game session.
    pub started: Option<String>,
    pub errors: Vec<LuaError>,
}

pub fn read(path: &Path, known_folders: &[String]) -> Option<Log> {
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut first = String::new();
    {
        let mut head = [0u8; 256];
        let n = f.read(&mut head).ok()?;
        let text = String::from_utf8_lossy(&head[..n]);
        if let Some(line) = text.lines().next() {
            first = line.to_string();
        }
    }
    let start = len.saturating_sub(MAX_READ);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut log = parse(&text, known_folders);
    log.started = timestamp(&first).map(str::to_string);
    Some(log)
}

/// `11/4 22:03:50.940` at the start of a line, without the milliseconds.
fn timestamp(line: &str) -> Option<&str> {
    let (date, rest) = line.split_once(' ')?;
    let time = rest.split_whitespace().next()?;
    let ok = date.contains('/')
        && date.chars().all(|c| c.is_ascii_digit() || c == '/')
        && time.contains(':')
        && time
            .chars()
            .all(|c| c.is_ascii_digit() || c == ':' || c == '.');
    if !ok {
        return None;
    }
    let end = date.len() + 1 + time.find('.').unwrap_or(time.len());
    Some(&line[..end])
}

pub fn parse(text: &str, known_folders: &[String]) -> Log {
    const MARK: &str = "[E][Lua] Lua Error: ";
    let mut log = Log::default();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(at) = line.find(MARK) else { continue };
        let message = line[at + MARK.len()..].trim().to_string();
        let time = timestamp(line).unwrap_or("").to_string();
        let mut stack = vec![];
        while let Some(next) = lines.peek() {
            let n = next.trim_end();
            if n.is_empty() || timestamp(n).is_some() {
                break;
            }
            if stack.len() < MAX_STACK {
                stack.push(n.to_string());
            }
            lines.next();
        }
        let (folder, blame) = match culprit(&message, &stack, known_folders) {
            Some((f, b)) => (Some(f), b),
            None => (None, Blame::Code),
        };
        log.errors.push(LuaError {
            time,
            message,
            stack,
            folder,
            blame,
        });
    }
    log
}

fn is_game_folder(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("blizzard_") || n == "blizzard" || n.is_empty()
}

/// Which addon an error points at, most specific evidence first:
/// 1. the game naming the addon that tainted its code,
/// 2. a full `AddOns/<Folder>/` path in the message, then in the stack,
/// 3. a shortened path (`...s/Questie/Modules/x.lua`) whose segments name an
///    installed folder.
pub fn culprit(message: &str, stack: &[String], known: &[String]) -> Option<(String, Blame)> {
    for marker in ["tainted by '", "AddOn '"] {
        if let Some(name) = quoted_after(message, marker) {
            if !is_game_folder(&name) {
                return Some((name, Blame::Taint));
            }
        }
    }
    let lines = std::iter::once(message).chain(stack.iter().map(String::as_str));
    for l in lines.clone() {
        if let Some(f) = addons_folders(l).into_iter().find(|f| !is_game_folder(f)) {
            return Some((f, Blame::Code));
        }
    }
    for l in lines {
        if let Some(f) = shortened_path_folder(l, known) {
            return Some((f, Blame::Code));
        }
    }
    None
}

fn quoted_after(s: &str, marker: &str) -> Option<String> {
    let start = s.find(marker)? + marker.len();
    let end = s[start..].find('\'')?;
    let name = s[start..start + end].trim();
    (!name.is_empty() && name.len() < 100).then(|| name.to_string())
}

/// Every `<Folder>` that follows `AddOns/` or `AddOns\` in a line.
fn addons_folders(line: &str) -> Vec<String> {
    let lower = line.to_ascii_lowercase();
    let mut out = vec![];
    let mut from = 0;
    while let Some(p) = lower[from..].find("addons").map(|p| p + from) {
        from = p + 6;
        let rest = &line[p + 6..];
        let Some(rest) = rest.strip_prefix('/').or_else(|| rest.strip_prefix('\\')) else {
            continue;
        };
        let end = rest.find(['/', '\\', ']', ':', '>']).unwrap_or(rest.len());
        if end < rest.len() && matches!(rest.as_bytes()[end], b'/' | b'\\') {
            out.push(rest[..end].to_string());
        }
    }
    out
}

/// For paths the game shortened with `...`: the first path segment, other than the cut
/// one, that is an installed addon folder.
fn shortened_path_folder(line: &str, known: &[String]) -> Option<String> {
    let at = line.find("...")?;
    let path = &line[at + 3..];
    let path = &path[..path.find([':', ']', '>', ' ']).unwrap_or(path.len())];
    path.split(['/', '\\'])
        .skip(1)
        .filter(|seg| !seg.is_empty() && !seg.contains('.'))
        .find_map(|seg| known.iter().find(|k| k.eq_ignore_ascii_case(seg)).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "11/4 22:03:50.940  [N][BleepTokenManager] Constructing\n\
11/4 21:34:54.461  [E][Lua] Lua Error: Interface/AddOns/Questie/Database/Zones/zoneDB.lua:27: attempt to index global 'LibQuestieDB' (a nil value)\n\
[Interface/AddOns/Questie/Database/Zones/zoneDB.lua]:27: in main chunk\n\
\n\
11/4 21:34:55.100  [E][Lua] Lua Error: ...s/Questie/Modules/Journey/QuestieJourneyFactions.lua:4: attempt to index global 'LibQuestieDB' (a nil value)\n\
11/4 22:23:59.287  [E][Lua] Lua Error: ...ddOns/Blizzard_UnitFrame/Shared/CompactUnitFrame.lua:698: attempt to compare local 'oldR' (a secret number value, while execution tainted by 'TomTom')\n\
[Interface/AddOns/Blizzard_UnitFrame/Shared/CompactUnitFrame.lua]:698: in function 'CompactUnitFrame_UpdateHealthColor'\n\
[C]: in function 'Show'\n\
[tail call]: ?\n\
\n\
11/4 22:24:05.810  [N][AccountData] GameUI::Shutdown\n\
11/4 22:30:00.000  [E][Lua] Lua Error: [string \"/run x()\"]:1: attempt to call a nil value\n";

    #[test]
    fn parses_and_blames() {
        let known = vec!["Questie".to_string(), "TomTom".to_string()];
        let log = parse(SAMPLE, &known);
        assert_eq!(log.errors.len(), 4);
        let e = &log.errors[0];
        assert_eq!(e.time, "11/4 21:34:54");
        assert_eq!(e.folder.as_deref(), Some("Questie"));
        assert_eq!(e.stack.len(), 1);
        // Shortened path, found through the installed folder list.
        assert_eq!(log.errors[1].folder.as_deref(), Some("Questie"));
        assert!(log.errors[1].stack.is_empty());
        // Blizzard code failing because TomTom tainted it.
        let t = &log.errors[2];
        assert_eq!(t.folder.as_deref(), Some("TomTom"));
        assert_eq!(t.blame, Blame::Taint);
        assert_eq!(t.stack.len(), 3);
        // A macro or script error belongs to no addon.
        assert!(log.errors[3].folder.is_none());
    }

    #[test]
    fn stack_is_used_when_the_message_has_no_path() {
        let stack = vec![
            "[C]: in function 'error'".to_string(),
            "[Interface/AddOns/Blizzard_SharedXML/Mixin.lua]:10: in function 'x'".to_string(),
            "[Interface\\AddOns\\RareScanner\\Core.lua]:55: in function 'y'".to_string(),
        ];
        assert_eq!(
            culprit("bad argument #1 to 'pairs'", &stack, &[]),
            Some(("RareScanner".into(), Blame::Code))
        );
        assert_eq!(culprit("plain", &[], &[]), None);
    }

    #[test]
    fn reads_session_start_from_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("General.log");
        fs::write(&p, SAMPLE).unwrap();
        let log = read(&p, &[]).unwrap();
        assert_eq!(log.started.as_deref(), Some("11/4 22:03:50"));
        assert!(read(&t.path().join("missing.log"), &[]).is_none());
    }
}
