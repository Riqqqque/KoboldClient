//! High-level operations used by the UI. Everything that changes files on disk runs
//! under one lock so two tasks can never edit the AddOns folder at the same time.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tokio::sync::OnceCell;
use tracing::Instrument;

use crate::error::{Error, Result};
use crate::game::{self, GameInfo};
use crate::install;
use crate::logging::LogHandle;
use crate::manifest::{InstalledAddon, Manifest, Source, Store};
use crate::net;
use crate::remove;
use crate::secret;
use crate::settings::{self, Settings};
use crate::sources::curseforge::{CurseForge, Sort};
use crate::sources::github::{self, GitHub};
use crate::sources::{RemoteAddon, Resolved};
use crate::toc;
use crate::util::{eq_ignore_case, now_secs, op_id, remove_dir_quiet};

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub id: String,
    /// "download", "verify", "install", "done", "error"
    pub stage: &'static str,
    pub pct: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddonView {
    pub id: String,
    pub name: String,
    pub author: String,
    pub summary: String,
    /// `None` for folders installed by hand.
    pub source: Option<Source>,
    pub version: Option<String>,
    pub new_version: Option<String>,
    pub icon_url: Option<String>,
    pub project_url: Option<String>,
    pub enabled: bool,
    pub pinned: bool,
    pub folders: Vec<String>,
    pub built_for_forever: bool,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StateView {
    pub game: Option<GameInfo>,
    pub settings: Settings,
    pub has_cf_key: bool,
    pub app_version: &'static str,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletePreview {
    pub name: String,
    pub folders: Vec<String>,
    /// (folder, name of the other addon that still uses it)
    pub kept: Vec<(String, String)>,
    pub saved_variables: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Removed {
    pub token: String,
    pub name: String,
}

struct UndoEntry {
    addon: Option<InstalledAddon>,
    paths: Vec<PathBuf>,
}

pub struct Core {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub http: reqwest::Client,
    pub log: Option<LogHandle>,
    settings: Mutex<Settings>,
    manifest: Mutex<Manifest>,
    store: Store,
    cf_type: OnceCell<u32>,
    updates: Mutex<HashMap<String, Resolved>>,
    undo: Mutex<HashMap<String, UndoEntry>>,
    op_lock: tokio::sync::Mutex<()>,
}

impl Core {
    pub fn new(
        data_dir: PathBuf,
        cache_dir: PathBuf,
        log_dir: PathBuf,
        log: Option<LogHandle>,
    ) -> Self {
        let _ = fs::create_dir_all(&data_dir);
        let settings = settings::load(&data_dir);
        let store = Store::new(&data_dir);
        let manifest = store.load();
        remove_dir_quiet(&cache_dir.join("dl"));
        let core = Core {
            http: net::client(),
            settings: Mutex::new(settings),
            manifest: Mutex::new(manifest),
            store,
            cf_type: OnceCell::new(),
            updates: Mutex::new(HashMap::new()),
            undo: Mutex::new(HashMap::new()),
            op_lock: tokio::sync::Mutex::new(()),
            data_dir,
            cache_dir,
            log_dir,
            log,
        };
        if let Some(g) = core.game() {
            install::recover(&g);
        }
        core
    }

    // ---------- state ----------

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn game(&self) -> Option<GameInfo> {
        let root = self.settings.lock().unwrap().game_root.clone()?;
        game::validate(&root)
    }

    fn require_game(&self) -> Result<GameInfo> {
        self.game().ok_or(Error::NoGame)
    }

    pub fn state(&self) -> StateView {
        StateView {
            game: self.game(),
            settings: self.settings(),
            has_cf_key: secret::load_cf_key(&self.data_dir).is_some(),
            app_version: env!("CARGO_PKG_VERSION"),
            log_dir: self.log_dir.clone(),
        }
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut s = self.settings.lock().unwrap();
        let before_verbose = s.verbose_logging;
        f(&mut s);
        settings::save(&self.data_dir, &s)?;
        if s.verbose_logging != before_verbose {
            if let Some(l) = &self.log {
                l.set_verbose(s.verbose_logging);
            }
        }
        Ok(s.clone())
    }

    pub fn set_game_root(&self, path: &Path) -> Result<GameInfo> {
        let info = game::validate(path).ok_or(Error::NotAGameFolder)?;
        tracing::info!(root = ?info.root, version = ?info.version, "game folder set");
        let root = info.root.clone();
        self.update_settings(|s| s.game_root = Some(root))?;
        install::recover(&info);
        Ok(info)
    }

    /// Detects the game and saves it if nothing is set yet. Returns all candidates.
    pub fn detect_game(&self) -> Vec<GameInfo> {
        game::detect()
    }

    // ---------- CurseForge key ----------

    fn cf_key(&self) -> Result<String> {
        secret::load_cf_key(&self.data_dir).ok_or(Error::NoCurseForgeKey)
    }

    fn cf<'a>(&'a self, key: &'a str) -> CurseForge<'a> {
        CurseForge {
            http: &self.http,
            key,
            forever_type: &self.cf_type,
        }
    }

    pub async fn save_cf_key(&self, key: &str) -> Result<()> {
        let key = key.trim();
        if key.len() < 20 || key.contains(char::is_whitespace) {
            return Err(Error::BadCurseForgeKey);
        }
        self.cf(key).verify_key().await?;
        secret::save_cf_key(&self.data_dir, key)?;
        tracing::info!("CurseForge key saved");
        Ok(())
    }

    pub fn clear_cf_key(&self) {
        secret::clear_cf_key(&self.data_dir);
        tracing::info!("CurseForge key removed");
    }

    // ---------- listing ----------

    pub fn list_installed(&self) -> Result<Vec<AddonView>> {
        let game = self.require_game()?;
        let manifest = self.manifest.lock().unwrap().clone();
        let updates = self.updates.lock().unwrap();
        let mut out = vec![];
        for a in &manifest.addons {
            let mut built = true;
            let mut missing = true;
            for f in &a.folders {
                let p = remove::folder_path(&game, f, a.enabled);
                if p.exists() {
                    missing = false;
                }
                if eq_ignore_case(f, &a.folders[0]) {
                    if let Some(t) = toc::read_for_folder(&p) {
                        built = t.built_for_forever();
                    }
                }
            }
            out.push(AddonView {
                id: a.id.clone(),
                name: a.name.clone(),
                author: a.author.clone(),
                summary: a.summary.clone(),
                source: Some(a.source),
                version: Some(a.version.clone()),
                new_version: updates.get(&a.id).map(|r| r.version.clone()),
                icon_url: a.icon_url.clone(),
                project_url: a.project_url.clone(),
                enabled: a.enabled,
                pinned: a.pinned,
                folders: a.folders.clone(),
                built_for_forever: built,
                missing,
            });
        }
        out.extend(unmanaged(&game, &manifest));
        out.sort_by_key(|v| v.name.to_ascii_lowercase());
        Ok(out)
    }

    // ---------- search ----------

    pub async fn search(&self, source: &str, query: &str, sort: &str) -> Result<Vec<RemoteAddon>> {
        let sort = match sort {
            "downloads" => Sort::Downloads,
            "updated" => Sort::Updated,
            _ => Sort::Popular,
        };
        match source {
            "github" => {
                let (o, r) = github::parse_repo(query).ok_or_else(|| {
                    Error::NotFound("Enter a GitHub repository, like owner/name.".into())
                })?;
                Ok(vec![
                    GitHub { http: &self.http }.resolve(&o, &r).await?.addon,
                ])
            }
            _ => {
                let key = self.cf_key()?;
                self.cf(&key).search(query, sort, 0).await
            }
        }
    }

    // ---------- install / update ----------

    pub async fn resolve(&self, id: &str) -> Result<Resolved> {
        if let Some(rest) = id.strip_prefix("cf:") {
            let mod_id: u64 = rest
                .parse()
                .map_err(|_| Error::NotFound(format!("Bad addon id {id}")))?;
            let key = self.cf_key()?;
            self.cf(&key).resolve(mod_id).await
        } else if let Some(rest) = id.strip_prefix("gh:") {
            let (o, r) = github::parse_repo(rest)
                .ok_or_else(|| Error::NotFound(format!("Bad addon id {id}")))?;
            GitHub { http: &self.http }.resolve(&o, &r).await
        } else {
            Err(Error::NotFound(
                "This addon wasn't installed from an online source.".into(),
            ))
        }
    }

    pub async fn install(
        &self,
        id: &str,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<AddonView> {
        let pending = self.updates.lock().unwrap().get(id).cloned();
        let resolved = match pending {
            Some(r) => r,
            None => self.resolve(id).await?,
        };
        let span = tracing::info_span!("install", op = %op_id(), addon = %resolved.addon.id, version = %resolved.version);
        self.install_resolved(resolved, progress)
            .instrument(span)
            .await
    }

    async fn install_resolved(
        &self,
        r: Resolved,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<AddonView> {
        let _guard = self.op_lock.lock().await;
        let game = self.require_game()?;
        let id = r.addon.id.clone();
        tracing::info!(url = %r.download_url, "starting install");

        let report = |stage, pct| {
            progress(Progress {
                id: id.clone(),
                stage,
                pct,
            })
        };
        report("download", 0);
        let dest = self.cache_dir.join("dl").join(format!("{}.zip", op_id()));
        let mut last_pct = 0u8;
        let dl = net::download(
            &self.http,
            &r.download_url,
            &dest,
            &r.expected,
            |got, total| {
                if let Some(t) = total.filter(|t| *t > 0) {
                    let pct = ((got * 80) / t).min(80) as u8;
                    if pct != last_pct {
                        last_pct = pct;
                        report("download", pct);
                    }
                }
            },
        )
        .await;
        let dl = match dl {
            Ok(d) => d,
            Err(e) => {
                report("error", 0);
                return Err(e);
            }
        };
        report("verify", 85);

        let template = InstalledAddon {
            id: id.clone(),
            name: r.addon.name.clone(),
            author: r.addon.author.clone(),
            summary: r.addon.summary.clone(),
            source: r.addon.source,
            version: r.version.clone(),
            file_id: r.file_id,
            release_tag: r.release_tag.clone(),
            project_url: Some(r.addon.project_url.clone()),
            icon_url: r.addon.icon_url.clone(),
            folders: vec![],
            installed_at: 0,
            updated_at: 0,
            enabled: true,
            pinned: false,
            archive_sha256: Some(dl.sha256),
        };
        let result = self.install_archive_locked(&game, &dest, template, &report);
        let _ = fs::remove_file(&dest);
        match &result {
            Ok(_) => {
                self.updates.lock().unwrap().remove(&id);
                report("done", 100);
            }
            Err(e) => {
                tracing::error!(error = %e, "install failed");
                report("error", 0);
            }
        }
        result
    }

    /// Installs a zip the user picked from disk.
    pub async fn import_zip(
        &self,
        path: &Path,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<AddonView> {
        let _guard = self.op_lock.lock().await;
        let game = self.require_game()?;
        tracing::info!(?path, "importing zip");
        let id_tmp = "import".to_string();
        let report = |stage, pct| {
            progress(Progress {
                id: id_tmp.clone(),
                stage,
                pct,
            })
        };
        report("install", 50);
        let template = InstalledAddon {
            id: String::new(),
            name: String::new(),
            author: String::new(),
            summary: String::new(),
            source: Source::Zip,
            version: String::new(),
            file_id: None,
            release_tag: None,
            project_url: None,
            icon_url: None,
            folders: vec![],
            installed_at: 0,
            updated_at: 0,
            enabled: true,
            pinned: false,
            archive_sha256: None,
        };
        let r = self.install_archive_locked(&game, path, template, &report);
        report(if r.is_ok() { "done" } else { "error" }, 100);
        r
    }

    /// The shared part of every install. Caller holds `op_lock`.
    fn install_archive_locked(
        &self,
        game: &GameInfo,
        archive: &Path,
        mut addon: InstalledAddon,
        report: &dyn Fn(&'static str, u8),
    ) -> Result<AddonView> {
        report("install", 90);
        let (stage_dir, staged) = install::stage(archive, &game.staging_dir())?;
        let cleanup = scopeguard(stage_dir.clone());

        let primary = &staged[0];
        let main_toc = toc::read_for_folder(&primary.path);
        if addon.id.is_empty() {
            // Zip import: name and version come from the TOC.
            addon.id = format!("zip:{}", primary.name.to_ascii_lowercase());
            addon.name = main_toc
                .as_ref()
                .and_then(|t| t.title.clone())
                .unwrap_or_else(|| primary.name.clone());
            addon.author = main_toc
                .as_ref()
                .and_then(|t| t.author.clone())
                .unwrap_or_default();
            addon.summary = main_toc
                .as_ref()
                .and_then(|t| t.notes.clone())
                .unwrap_or_default();
            addon.version = main_toc
                .as_ref()
                .and_then(|t| t.clean_version())
                .unwrap_or_else(|| "unknown".into());
        }
        addon.folders = staged.iter().map(|f| f.name.clone()).collect();

        let previous = self.manifest.lock().unwrap().get(&addon.id).cloned();
        if let Some(prev) = &previous {
            addon.enabled = prev.enabled;
            if !prev.enabled {
                remove::set_enabled(game, &prev.folders, true)?;
            }
        }
        let stale: Vec<String> = {
            let m = self.manifest.lock().unwrap();
            previous
                .as_ref()
                .map(|p| {
                    p.folders
                        .iter()
                        .filter(|f| !addon.folders.iter().any(|n| eq_ignore_case(n, f)))
                        .filter(|f| m.owners_of(f, &addon.id).is_empty())
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        };
        for f in &addon.folders {
            let others = self
                .manifest
                .lock()
                .unwrap()
                .owners_of(f, &addon.id)
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>();
            if !others.is_empty() {
                tracing::info!(folder = %f, ?others, "folder is shared with other addons");
            } else if game.addons_dir.join(f).exists() && previous.is_none() {
                tracing::info!(folder = %f, "replacing a folder that was installed by hand");
            }
        }

        let swap = install::swap_in(game, &staged, &stale)?;
        {
            let mut m = self.manifest.lock().unwrap();
            let before = m.clone();
            m.upsert(addon.clone());
            if let Err(e) = self.store.save(&m) {
                *m = before;
                drop(m);
                swap.rollback();
                return Err(e);
            }
        }
        swap.commit();
        drop(cleanup);
        if !addon.enabled {
            if let Err(e) = remove::set_enabled(game, &addon.folders, false) {
                tracing::warn!(error = %e, "couldn't re-disable updated addon");
            }
        }
        tracing::info!(folders = ?addon.folders, "install committed");
        self.list_installed()?
            .into_iter()
            .find(|v| v.id == addon.id)
            .ok_or_else(|| Error::Other("Installed, but couldn't read it back.".into()))
    }

    // ---------- updates ----------

    /// Checks every non-pinned addon for a newer Forever build. Failures for single
    /// addons are logged and skipped so one bad addon can't block the rest.
    pub async fn check_updates(&self) -> Result<Vec<AddonView>> {
        let addons: Vec<InstalledAddon> = self
            .manifest
            .lock()
            .unwrap()
            .addons
            .iter()
            .filter(|a| !a.pinned)
            .cloned()
            .collect();
        let mut found: HashMap<String, Resolved> = HashMap::new();

        let cf_addons: Vec<&InstalledAddon> = addons
            .iter()
            .filter(|a| a.source == Source::CurseForge)
            .collect();
        if !cf_addons.is_empty() {
            match self.cf_key() {
                Ok(key) => {
                    let cf = self.cf(&key);
                    let ids: Vec<u64> = cf_addons
                        .iter()
                        .filter_map(|a| a.id.strip_prefix("cf:")?.parse().ok())
                        .collect();
                    match (cf.forever_type_id().await, cf.mods(&ids).await) {
                        (Ok(t), Ok(mods)) => {
                            for m in mods {
                                let id = format!("cf:{}", m.id);
                                let installed = cf_addons
                                    .iter()
                                    .find(|a| a.id == id)
                                    .and_then(|a| a.file_id);
                                let latest = CurseForge::latest_forever_file(&m, t);
                                if latest.is_some() && latest != installed {
                                    match cf.resolve_mod(&m, t).await {
                                        Ok(r) => {
                                            found.insert(id, r);
                                        }
                                        Err(e) => {
                                            tracing::warn!(addon = %id, error = %e, "update check failed")
                                        }
                                    }
                                }
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            tracing::warn!(error = %e, "CurseForge update check failed")
                        }
                    }
                }
                Err(_) => tracing::info!("no CurseForge key; skipping CurseForge update checks"),
            }
        }

        let gh = GitHub { http: &self.http };
        for a in addons.iter().filter(|a| a.source == Source::GitHub) {
            let Some((o, r)) = a.id.strip_prefix("gh:").and_then(github::parse_repo) else {
                continue;
            };
            match gh.latest_release(&o, &r).await {
                Ok(rel) if Some(&rel.tag_name) != a.release_tag.as_ref() => {
                    match gh.resolve(&o, &r).await {
                        Ok(res) => {
                            found.insert(a.id.clone(), res);
                        }
                        Err(e) => tracing::warn!(addon = %a.id, error = %e, "update check failed"),
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(addon = %a.id, error = %e, "update check failed"),
            }
        }
        tracing::info!(
            updates = found.len(),
            checked = addons.len(),
            "update check finished"
        );
        *self.updates.lock().unwrap() = found;
        Ok(self
            .list_installed()?
            .into_iter()
            .filter(|v| v.new_version.is_some())
            .collect())
    }

    // ---------- delete / disable ----------

    fn find_view(&self, id: &str) -> Result<AddonView> {
        self.list_installed()?
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| Error::NotFound("That addon isn't installed anymore.".into()))
    }

    pub fn delete_preview(&self, id: &str) -> Result<DeletePreview> {
        let game = self.require_game()?;
        let view = self.find_view(id)?;
        let m = self.manifest.lock().unwrap();
        let mut folders = vec![];
        let mut kept = vec![];
        for f in &view.folders {
            match m.owners_of(f, id).first() {
                Some(other) if view.source.is_some() => kept.push((f.clone(), other.name.clone())),
                _ => folders.push(f.clone()),
            }
        }
        let saved_variables = remove::saved_variables(&game, &folders).len();
        Ok(DeletePreview {
            name: view.name,
            folders,
            kept,
            saved_variables,
        })
    }

    pub async fn remove(&self, id: &str, delete_settings: bool) -> Result<Removed> {
        let _guard = self.op_lock.lock().await;
        let game = self.require_game()?;
        let view = self.find_view(id)?;
        let preview = self.delete_preview(id)?;
        let mut paths: Vec<PathBuf> = preview
            .folders
            .iter()
            .map(|f| remove::folder_path(&game, f, view.enabled))
            .collect();
        if delete_settings {
            paths.extend(remove::saved_variables(&game, &preview.folders));
        }
        tracing::info!(addon = %id, ?paths, delete_settings, "removing addon");
        let to_trash = paths.clone();
        tokio::task::spawn_blocking(move || remove::to_recycle_bin(&to_trash))
            .await
            .map_err(|e| Error::Other(e.to_string()))??;

        let addon = {
            let mut m = self.manifest.lock().unwrap();
            let a = m.remove(id);
            if a.is_some() {
                self.store.save(&m)?;
            }
            a
        };
        self.updates.lock().unwrap().remove(id);
        let token = op_id();
        self.undo
            .lock()
            .unwrap()
            .insert(token.clone(), UndoEntry { addon, paths });
        Ok(Removed {
            token,
            name: view.name,
        })
    }

    pub async fn undo_remove(&self, token: &str) -> Result<()> {
        let _guard = self.op_lock.lock().await;
        let entry = self
            .undo
            .lock()
            .unwrap()
            .remove(token)
            .ok_or_else(|| Error::NotFound("Nothing to undo.".into()))?;
        let paths = entry.paths.clone();
        let restored =
            tokio::task::spawn_blocking(move || remove::restore_from_recycle_bin(&paths))
                .await
                .map_err(|e| Error::Other(e.to_string()))??;
        tracing::info!(restored, "undo delete");
        if let Some(a) = entry.addon {
            let mut m = self.manifest.lock().unwrap();
            m.upsert(a);
            self.store.save(&m)?;
        }
        Ok(())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let _guard = self.op_lock.lock().await;
        let game = self.require_game()?;
        let view = self.find_view(id)?;
        let folders: Vec<String> = {
            let m = self.manifest.lock().unwrap();
            view.folders
                .iter()
                .filter(|f| enabled || !m.owners_of(f, id).iter().any(|o| o.enabled))
                .cloned()
                .collect()
        };
        remove::set_enabled(&game, &folders, enabled)?;
        let mut m = self.manifest.lock().unwrap();
        if let Some(a) = m.get_mut(id) {
            a.enabled = enabled;
            self.store.save(&m)?;
        }
        tracing::info!(addon = %id, enabled, "toggled addon");
        Ok(())
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let mut m = self.manifest.lock().unwrap();
        let a = m
            .get_mut(id)
            .ok_or_else(|| Error::NotFound("Only managed addons can be pinned.".into()))?;
        a.pinned = pinned;
        self.store.save(&m)?;
        if pinned {
            self.updates.lock().unwrap().remove(id);
        }
        Ok(())
    }

    // ---------- diagnostics ----------

    /// Zips logs, the manifest and settings into `dest_dir`, with the Windows user
    /// name masked. Never includes the CurseForge key.
    pub fn export_diagnostics(&self, dest_dir: &Path) -> Result<PathBuf> {
        use std::io::Write;
        let out = dest_dir.join(format!("KoboldClient-diagnostics-{}.zip", now_secs()));
        let file = fs::File::create(&out).map_err(|e| Error::io("create", &out, e))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mask = |text: String| -> String {
            match std::env::var("USERNAME") {
                Ok(u) if u.len() > 1 => text.replace(&u, "<user>"),
                _ => text,
            }
        };
        let mut add = |name: &str, text: String| -> Result<()> {
            zip.start_file(name, opts)?;
            zip.write_all(mask(text).as_bytes())
                .map_err(|e| Error::io("write", &out, e))?;
            Ok(())
        };
        let state = self.state();
        add(
            "info.txt",
            format!(
                "Kobold Client {}\nOS: {} {}\nGame: {:?}\nCurseForge key set: {}\n",
                state.app_version,
                std::env::consts::OS,
                std::env::consts::ARCH,
                state.game,
                state.has_cf_key
            ),
        )?;
        add(
            "settings.json",
            serde_json::to_string_pretty(&state.settings)?,
        )?;
        add(
            "manifest.json",
            serde_json::to_string_pretty(&*self.manifest.lock().unwrap())?,
        )?;
        if let Ok(rd) = fs::read_dir(&self.log_dir) {
            for e in rd.flatten() {
                if let Ok(text) = fs::read_to_string(e.path()) {
                    add(&format!("logs/{}", e.file_name().to_string_lossy()), text)?;
                }
            }
        }
        zip.finish()?;
        tracing::info!(?out, "diagnostics exported");
        Ok(out)
    }
}

/// Removes a staging folder when dropped, whatever happened.
struct StagingGuard(PathBuf);
impl Drop for StagingGuard {
    fn drop(&mut self) {
        remove_dir_quiet(&self.0);
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}
fn scopeguard(p: PathBuf) -> StagingGuard {
    StagingGuard(p)
}

/// Folders in `AddOns` (or disabled) that Kobold didn't install, grouped so that
/// `Name_Config` and similar sit under `Name`.
fn unmanaged(game: &GameInfo, m: &Manifest) -> Vec<AddonView> {
    let mut entries: Vec<(String, bool)> = vec![];
    for (dir, enabled) in [(&game.addons_dir, true), (&game.disabled_dir(), false)] {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir || name.starts_with('.') || name.starts_with("Blizzard_") || m.owns(&name) {
                continue;
            }
            if toc::toc_files(&e.path()).is_empty() {
                continue;
            }
            entries.push((name, enabled));
        }
    }
    entries.sort_by_key(|(n, _)| n.to_ascii_lowercase());

    let mut groups: Vec<(String, bool, Vec<String>)> = vec![];
    for (name, enabled) in entries {
        let lower = name.to_ascii_lowercase();
        let parent = groups.iter_mut().find(|(g, _, _)| {
            let g = g.to_ascii_lowercase();
            lower.starts_with(&format!("{g}_")) || lower.starts_with(&format!("{g}-"))
        });
        match parent {
            Some(g) => g.2.push(name),
            None => groups.push((name.clone(), enabled, vec![name])),
        }
    }
    groups
        .into_iter()
        .map(|(name, enabled, folders)| {
            let dir = if enabled {
                game.addons_dir.join(&name)
            } else {
                game.disabled_dir().join(&name)
            };
            let t = toc::read_for_folder(&dir);
            AddonView {
                id: format!("local:{name}"),
                name: t
                    .as_ref()
                    .and_then(|t| t.title.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| name.clone()),
                author: t
                    .as_ref()
                    .and_then(|t| t.author.clone())
                    .unwrap_or_default(),
                summary: t.as_ref().and_then(|t| t.notes.clone()).unwrap_or_default(),
                source: None,
                version: t.as_ref().and_then(|t| t.clean_version()),
                new_version: None,
                icon_url: None,
                project_url: None,
                enabled,
                pinned: false,
                folders,
                built_for_forever: t.as_ref().map(|t| t.built_for_forever()).unwrap_or(false),
                missing: false,
            }
        })
        .collect()
}
