//! Finding the WoW Forever install and checking whether the game is running.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

pub const FLAVOR_DIR: &str = "_forever_";
pub const PRODUCT_CODE: &str = "wowf";

#[derive(Debug, Clone, Serialize)]
pub struct GameInfo {
    /// The `_forever_` folder.
    pub root: PathBuf,
    pub version: Option<String>,
    pub addons_dir: PathBuf,
    pub running: bool,
}

impl GameInfo {
    pub fn interface_dir(&self) -> PathBuf {
        self.root.join("Interface")
    }
    /// Work folders live next to `AddOns` so moves stay on the same drive (instant and atomic).
    pub fn staging_dir(&self) -> PathBuf {
        self.interface_dir().join(".kobold-staging")
    }
    pub fn backup_dir(&self) -> PathBuf {
        self.interface_dir().join(".kobold-old")
    }
    pub fn disabled_dir(&self) -> PathBuf {
        self.interface_dir().join(".kobold-disabled")
    }
    pub fn wtf_dir(&self) -> PathBuf {
        self.root.join("WTF")
    }
}

/// Accepts either the `_forever_` folder or the `World of Warcraft` folder above it.
pub fn validate(path: &Path) -> Option<GameInfo> {
    let root = if is_flavor_dir(path) {
        path.to_path_buf()
    } else if is_flavor_dir(&path.join(FLAVOR_DIR)) {
        path.join(FLAVOR_DIR)
    } else {
        return None;
    };
    let version = root.parent().and_then(read_build_version);
    Some(GameInfo {
        addons_dir: root.join("Interface").join("AddOns"),
        running: is_running(&root),
        version,
        root,
    })
}

fn is_flavor_dir(p: &Path) -> bool {
    let named = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(FLAVOR_DIR))
        .unwrap_or(false);
    named
        && (p.join("Wow.exe").is_file()
            || p.join("WowB.exe").is_file()
            || p.join("WowT.exe").is_file())
}

/// Reads the client version for the Forever product from Battle.net's `.build.info`.
pub fn read_build_version(wow_root: &Path) -> Option<String> {
    let text = fs::read_to_string(wow_root.join(".build.info")).ok()?;
    let mut lines = text.lines();
    let header: Vec<String> = lines
        .next()?
        .split('|')
        .map(|h| h.split('!').next().unwrap_or("").to_ascii_lowercase())
        .collect();
    let col = |name: &str| header.iter().position(|h| h == name);
    let (vcol, pcol) = (col("version")?, col("product"));
    for line in lines {
        let cells: Vec<&str> = line.split('|').collect();
        let product_ok = pcol
            .and_then(|i| cells.get(i))
            .map(|p| p.eq_ignore_ascii_case(PRODUCT_CODE))
            .unwrap_or(true);
        if product_ok {
            if let Some(v) = cells.get(vcol).filter(|v| !v.is_empty()) {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Looks for Forever installs, fastest and most reliable sources first.
pub fn detect() -> Vec<GameInfo> {
    let started = Instant::now();
    let mut seen = HashSet::new();
    let mut found: Vec<GameInfo> = Vec::new();
    fn consider(seen: &mut HashSet<String>, found: &mut Vec<GameInfo>, p: PathBuf, how: &str) {
        if !seen.insert(p.to_string_lossy().to_ascii_lowercase()) {
            return;
        }
        if let Some(info) = validate(&p) {
            let k2 = info.root.to_string_lossy().to_ascii_lowercase();
            if !found
                .iter()
                .any(|g| g.root.to_string_lossy().to_ascii_lowercase() == k2)
            {
                tracing::info!(root = ?info.root, how, "found WoW Forever");
                found.push(info);
            }
        }
    }

    for exe in running_wow_exes() {
        if let Some(dir) = exe.parent() {
            consider(&mut seen, &mut found, dir.to_path_buf(), "running game");
        }
    }
    for p in battle_net_paths() {
        consider(&mut seen, &mut found, p, "Battle.net database");
    }
    for drive in fixed_drives() {
        for rel in [
            "World of Warcraft",
            "Games\\World of Warcraft",
            "Program Files (x86)\\World of Warcraft",
            "Program Files\\World of Warcraft",
            "Blizzard\\World of Warcraft",
            "Blizzard Games\\World of Warcraft",
            "Battle.net\\World of Warcraft",
        ] {
            consider(&mut seen, &mut found, drive.join(rel), "common location");
        }
    }
    if found.is_empty() {
        // Last resort: a shallow, time-boxed walk of each fixed drive.
        let deadline = started + Duration::from_secs(4);
        for drive in fixed_drives() {
            for p in shallow_find(&drive, "World of Warcraft", 3, deadline) {
                consider(&mut seen, &mut found, p, "drive search");
            }
        }
    }
    tracing::info!(
        count = found.len(),
        ms = started.elapsed().as_millis() as u64,
        "game detection finished"
    );
    found
}

/// Pulls folder paths out of Battle.net's product databases. The files are protobuf,
/// but install paths appear as plain strings, which is all we need.
fn battle_net_paths() -> Vec<PathBuf> {
    let mut dbs = vec![];
    if let Some(pd) = std::env::var_os("ProgramData") {
        dbs.push(
            PathBuf::from(pd)
                .join("Battle.net")
                .join("Agent")
                .join("product.db"),
        );
    }
    let mut out = vec![];
    for db in dbs {
        if let Ok(bytes) = fs::read(&db) {
            out.extend(path_strings(&bytes));
        }
    }
    out
}

pub(crate) fn path_strings(bytes: &[u8]) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut cur = Vec::new();
    for &b in bytes.iter().chain(std::iter::once(&0u8)) {
        if (0x20..0x7f).contains(&b) {
            cur.push(b);
            continue;
        }
        if cur.len() >= 4 {
            let s = String::from_utf8_lossy(&cur);
            // Look for "X:/..." or "X:\..." anywhere in the run.
            if let Some(pos) = s.find(":/").or_else(|| s.find(":\\")) {
                if pos >= 1 {
                    let path = &s[pos - 1..];
                    out.push(PathBuf::from(path.replace('/', "\\")));
                }
            }
        }
        cur.clear();
    }
    out
}

fn shallow_find(root: &Path, name: &str, depth: u32, deadline: Instant) -> Vec<PathBuf> {
    const SKIP: [&str; 6] = [
        "windows",
        "$recycle.bin",
        "system volume information",
        "programdata",
        "users",
        "msocache",
    ];
    let mut out = vec![];
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    while let Some((dir, d)) = stack.pop() {
        if Instant::now() > deadline {
            tracing::debug!("drive search hit its time limit");
            break;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let fname = e.file_name().to_string_lossy().to_string();
            if fname.eq_ignore_ascii_case(name) {
                out.push(e.path());
            } else if d + 1 < depth
                && !SKIP.contains(&fname.to_ascii_lowercase().as_str())
                && !fname.starts_with('.')
            {
                stack.push((e.path(), d + 1));
            }
        }
    }
    out
}

pub fn is_running(root: &Path) -> bool {
    let root_l = root.to_string_lossy().to_ascii_lowercase();
    running_wow_exes().iter().any(|exe| {
        exe.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(&root_l)
    })
}

#[cfg(windows)]
fn fixed_drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
        })
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
fn fixed_drives() -> Vec<PathBuf> {
    vec![]
}

/// Full paths of every running `Wow*.exe`.
#[cfg(windows)]
pub fn running_wow_exes() -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
            if name.starts_with("wow") && name.ends_with(".exe") && !name.contains("voice") {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !h.is_null() {
                    let mut buf = [0u16; 1024];
                    let mut size = buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        h,
                        PROCESS_NAME_WIN32,
                        buf.as_mut_ptr(),
                        &mut size,
                    ) != 0
                    {
                        out.push(PathBuf::from(std::ffi::OsString::from_wide(
                            &buf[..size as usize],
                        )));
                    }
                    CloseHandle(h);
                }
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
pub fn running_wow_exes() -> Vec<PathBuf> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_paths_in_binary_blob() {
        let blob = b"\x0a\x04wowf\x12\x1bD:/games/World of Warcraft\x1a\x02us";
        let paths = path_strings(blob);
        assert_eq!(paths, vec![PathBuf::from("D:\\games\\World of Warcraft")]);
    }

    #[test]
    fn reads_build_info_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".build.info"),
            "Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\nus|1|1.60.1.70790|wowf\n",
        )
        .unwrap();
        assert_eq!(
            read_build_version(dir.path()).as_deref(),
            Some("1.60.1.70790")
        );
    }

    /// Read-only check against the real machine: `cargo test detect_on_this_pc -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn detect_on_this_pc() {
        for g in detect() {
            println!("{:?} version={:?} running={}", g.root, g.version, g.running);
        }
    }

    #[test]
    fn validates_either_folder_level() {
        let dir = tempfile::tempdir().unwrap();
        let flavor = dir.path().join(FLAVOR_DIR);
        fs::create_dir(&flavor).unwrap();
        assert!(validate(dir.path()).is_none());
        fs::write(flavor.join("Wow.exe"), b"").unwrap();
        assert_eq!(validate(dir.path()).unwrap().root, flavor);
        assert_eq!(validate(&flavor).unwrap().root, flavor);
    }
}
