# Kobold Client — Development Plan

A small, fast addon manager for **World of Warcraft: Forever**. Find addons, install them, keep them updated, and remove them cleanly, without the bloat of the big launchers.

Author: Rique

---

## 1. Goals and non-goals

**Goals**
- Small download, low memory use, starts in under a second. No background services and no tray process unless the user turns one on.
- Install, update, enable or disable, and delete addons with one click each.
- Only verifiable sources: every download comes over HTTPS from a known provider and is checked against the provider's hash when one is published.
- An install or update that fails never leaves a broken addon folder behind.
- Logs good enough to diagnose a user's problem from one exported file.

**Non-goals (for 1.0)**
- No ads, accounts, telemetry, or bundled launcher.
- No WeakAuras-style string or profile syncing.
- No macOS or Linux builds (the code stays portable; it just isn't packaged for them yet).

## 2. Stack

| Layer | Choice | Why |
|---|---|---|
| Core | Rust (stable 1.94) | All logic: file I/O, downloads, manifest, sources |
| Shell | Tauri 2 | Uses the system WebView2, so the installer stays around 5–8 MB, not 100+ MB like Electron |
| UI | Svelte 5 + TypeScript + Vite | Compiles to plain JS with a tiny runtime (~35 KB gzipped) |
| HTTP | `reqwest` (rustls) + `tokio` | Async, streaming downloads, timeouts |
| Archive | `zip` | Extraction with path checks |
| Hashing | `sha1`, `md-5`, `sha2` | Provider hash checks (CurseForge fingerprints via `murmur2` are planned) |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` | Structured, daily-rolling log files |
| Errors | `thiserror` (typed core errors), `anyhow` at the edges | |
| Secrets | Windows DPAPI | Encrypts the CurseForge API key for the current Windows user |
| Delete | `trash` | Sends removed folders to the Recycle Bin |

## 3. Architecture

```
kobold-client/
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs            app setup, logging init, panic hook
│  │  ├─ commands.rs        thin Tauri command layer (UI <-> core)
│  │  ├─ game/              install detection, flavor, TOC parsing
│  │  ├─ sources/           trait AddonSource + curseforge, github, wowinterface, wago
│  │  ├─ install/           download -> verify -> stage -> swap pipeline
│  │  ├─ manifest/          installed-addon database (JSON, versioned schema)
│  │  ├─ scan/              finds and matches manually installed folders
│  │  └─ diag/              log export, diagnostic bundle
│  └─ tests/                integration tests with fixture zips and a mock HTTP server
└─ ui/                      Svelte app (views: Installed, Browse, Settings)
```

All side effects live in the Rust core. The UI only calls commands and renders state, and progress arrives as Tauri events (`install://progress`).

### Addon source trait
```rust
trait AddonSource {
    fn id(&self) -> SourceId;
    async fn search(&self, q: &Query) -> Result<Vec<AddonSummary>>;
    async fn latest_file(&self, addon: &AddonRef, flavor: Flavor) -> Result<RemoteFile>;
    async fn resolve_download(&self, file: &RemoteFile) -> Result<Download>; // url + expected hash
}
```

### Sources
| Source | Phase | Auth | Verification |
|---|---|---|---|
| CurseForge | 1 | API key (`x-api-key`) from console.curseforge.com | `hashes[]` SHA-1/MD5 on every file |
| GitHub Releases | 1 | none; optional token for rate limits | `release.json` (BigWigs packager) to pick the Forever asset. GitHub's published asset digest where present |
| Manual zip import | 1 | none | SHA-256 recorded locally at install |
| WoWInterface | 2 | none (public API) | MD5 from the API |
| Wago Addons | 2 | API key | per their API |

"Verifiable" means:
1. HTTPS only, with an allow-listed host for each source.
2. If the provider publishes a hash, the download must match it or it is rejected.
3. The installed folder's own SHA-256 is recorded so later tampering or corruption can be detected.

## 4. Game detection (WoW Forever)

- Product code `wowf`, install folder `_forever_`, client 1.60.x, TOC interface `16001`.
- Detection order:
  1. a saved path
  2. Battle.net's `.product.db` / `.build.info` next to the install
  3. common locations on every fixed drive
  4. folder picker
- `Interface\AddOns` doesn't exist on a fresh install. Create it on first install.
- TOC selection follows the client's rules: `Name_Forever.toc` or `Name_Camelot.toc` first, then `Name_Mainline.toc`, then `Name.toc`. Addons with no Forever-compatible TOC are flagged "Not built for Forever" instead of being hidden silently.
- The flavor layer is an enum from day one, so adding Retail or Classic later is a config change, not a rewrite.

## 5. Data model

`%APPDATA%\com.rique.koboldclient\manifest.json` (schema-versioned, written atomically: write a temp file, fsync, rename; the last 5 copies are kept as `manifest.json.bak.N`):

```json
{
  "schema": 1,
  "game_root": "D:\\games\\World of Warcraft\\_forever_",
  "addons": [{
    "id": "cf:12345",
    "name": "Questie",
    "source": "curseforge",
    "version": "10.4.2",
    "file_id": 678910,
    "folders": ["Questie"],
    "folder_hashes": { "Questie": "sha256:..." },
    "installed_at": "2026-11-04T18:00:00Z",
    "pinned": false,
    "enabled": true
  }]
}
```

Settings live in `settings.json` next to it. The CurseForge key is never written to disk in plain text; it is encrypted with DPAPI (`curseforge.key.bin`).

## 6. Install / update pipeline (fail-safe)

1. Resolve the file. If CurseForge returns no download URL (the author has blocked third-party downloads), stop and offer **Open on CurseForge** plus **Import zip**.
2. Stream it to `%LOCALAPPDATA%\com.rique.koboldclient\cache\dl\` with timeouts and a size cap (3 tries with backoff).
3. Verify the hash. On a mismatch, delete the file, log both hashes, and show an error.
4. Extract into `Interface\.kobold-staging\<op>\` (same drive as AddOns, so moves are instant), rejecting any entry with `..`, an absolute path, or a drive prefix (zip-slip), and capping the total extracted size (zip bomb).
5. Normalize the layout. Handle zips with no top-level folder, nested `Name/Name/`, and stray files at the root.
6. Check for conflicts. If a target folder belongs to another managed addon, ask the user. If it is an unmanaged folder, offer to take it over.
7. Swap atomically. Rename the existing folders to `Interface\.kobold-old\<op>\` (with a journal), move the staged folders in, and on any failure move the old ones back.
8. Remove stale folders that the previous version had but the new one doesn't.
9. Commit. Write the manifest **last**, then clean up `.kobold-old`.
10. Recover on launch. Leftover `.part`, `staging`, or `.kobold-old` directories from a crash are rolled back or cleaned, and the event is logged.

## 7. Deleting addons

The top requirement is that delete is easy and safe.
- One click on the row's trash icon opens a confirm dialog listing exactly which folders will go.
- Removes every folder the addon installed. If another managed addon also claims a folder (a shared library), that folder is kept and the dialog says so.
- Folders go to the **Recycle Bin**, never a hard delete, so a mistaken delete is a one-step undo, and an in-app **Undo** toast is shown for 10 seconds.
- Deleting saved settings (`WTF\Account\*\SavedVariables\<Addon>.lua`) is a separate checkbox, off by default.
- Bulk delete is available through multi-select.
- **Disable** without deleting: the folders move to `Interface\.kobold-disabled\`, so the game no longer sees them for any account or character. Enabling moves them back. This works while the game is running (takes effect on `/reload`) and never races the client rewriting `AddOns.txt`.

## 8. Edge cases checklist

- [ ] Game running: allow installs, warn that `/reload` or a restart is needed, and never touch files the client locks
- [ ] Folder in use or locked by antivirus: retry with backoff, then a clear message naming the file
- [ ] Offline, DNS failure, timeouts, HTTP 429/5xx: backoff, cached search results, offline badge
- [ ] Disk full in the middle of an extract: rollback, with the free space shown in the error
- [ ] Paths longer than 260 characters: use `\\?\` prefixed paths internally
- [ ] Folder names that differ only in case (`questie` vs `Questie`)
- [ ] An addon that ships several folders, plus libraries shared between addons
- [ ] A new version that renames or drops a folder
- [ ] Manually installed addons: CurseForge fingerprint match (murmur2) → "Managed". Otherwise mark them "Unmanaged" and never auto-delete them
- [ ] A Forever TOC is missing → "Not built for Forever" badge, install only after a warning
- [ ] Downloads blocked by the author on CurseForge → open the page, or import the zip
- [ ] Invalid or expired API key → Settings shows the reason, and the other sources keep working
- [ ] Corrupt manifest → load the newest valid `.bak`, otherwise rebuild it from a folder scan
- [ ] Two app instances → single-instance lock (`tauri-plugin-single-instance`)
- [ ] Game moved or reinstalled → re-detect and re-link the manifest

## 9. Logging and troubleshooting

- Daily rolling files in `%LOCALAPPDATA%\com.rique.koboldclient\logs\`, keeping 14 days.
- Default level `info`. Settings has a **Verbose logging** toggle (`debug`) that needs no restart.
- Every operation gets a span carrying `op_id`, `addon_id`, and `source`, so one install can be traced from start to finish.
- A panic hook writes a crash report to the log before the app exits.
- API keys are never logged. The diagnostics export masks the Windows user name.
- In-app buttons: **Open logs folder** and **Export diagnostics** (a zip of the logs, the redacted manifest, settings, and the app and OS versions).

## 10. Testing

- Unit tests for TOC parsing, flavor selection, zip layout normalization, zip-slip rejection, and hash checks.
- Integration tests that run the full install/update/delete cycle against a temp "AddOns" folder, with a `wiremock` server standing in for each source, including fault injection (cut connection, bad hash, disk-full simulation, lock errors).
- CI on GitHub Actions: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, the UI type-check, and a Windows build artifact.

## 11. Milestones

Each milestone ends with a green build and tests, and is pushed to GitHub.

| # | Milestone | Done when |
|---|---|---|
| M0 | Repo + skeleton | Tauri app opens with the chosen theme and an empty Installed view; CI runs |
| M1 | Game + scan | Forever install detected, AddOns folder listed, TOCs parsed, unmanaged addons shown |
| M2 | Install pipeline | Manual zip import works end to end, including rollback tests |
| M3 | Delete + disable | Recycle Bin delete, shared-folder protection, undo, AddOns.txt toggling |
| M4 | CurseForge | Search, install, update, fingerprint matching, blocked-download fallback |
| M5 | GitHub Releases | Add by URL, `release.json` asset picking |
| M6 | Updates | Update check, "Update all", pinning, change summaries |
| M7 | Polish + hardening | Edge-case checklist complete, diagnostics export, accessibility pass |
| M8 | Release 1.0 | NSIS installer + portable zip on GitHub Releases, README with screenshots |
| M9 | Phase 2 sources | WoWInterface, Wago |

## 12. Status

- Done in 0.1.0: M0–M7 (skeleton, detection, install pipeline, delete/disable, CurseForge, GitHub, updates, hardening, self-update).
- Next: M8 (first public release), then fingerprint matching for hand-installed addons and the phase 2 sources.

## 13. Known constraints

- **CurseForge API key**: the API needs a key requested from CurseForge for third-party apps. Until one is approved, CurseForge works only through "Import zip". GitHub and manual import work without it.
- **Code signing**: an unsigned installer triggers SmartScreen's "unrecognized app" warning. A signing certificate removes it but costs money, so 1.0 ships unsigned unless decided otherwise.
- **Forever is new**: the TOC suffix and CurseForge's Forever version id are read at runtime (`/v1/games/1/version-types`), not hard-coded, so they can change without breaking the app.
