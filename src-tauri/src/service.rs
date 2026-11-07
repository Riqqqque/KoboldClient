//! High-level operations used by the UI. Everything that changes files on disk runs
//! under one lock so two tasks can never edit the AddOns folder at the same time.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tokio::sync::OnceCell;
use tracing::Instrument;

use crate::addonlist::{self, ImportPreview};
use crate::backup::{self, Backup};
use crate::error::{Error, Result};
use crate::fingerprint;
use crate::game::{self, GameInfo};
use crate::install;
use crate::logging::LogHandle;
use crate::luaerrors::{self, Blame};
use crate::manifest::{InstalledAddon, Manifest, Requirement, Source, Store};
use crate::net;
use crate::remove;
use crate::secret;
use crate::settings::{self, Settings};
use crate::sources::curseforge::{self, CurseForge, Sort};
use crate::sources::github::{self, GitHub};
use crate::sources::wago::{self, Wago};
use crate::sources::wowi::{self, WowInterface};
use crate::sources::{RemoteAddon, Resolved};
use crate::toc;
use crate::util::{eq_ignore_case, now_secs, op_id, remove_dir_quiet};

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub id: String,
    /// "download", "verify", "backup", "install", "done", "error"
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
    /// Required addons that aren't installed, so this one won't work yet.
    pub missing_deps: Vec<Requirement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallOutcome {
    pub addon: AddonView,
    /// Names of required addons that were installed along with it.
    pub also_installed: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RepairOutcome {
    /// Names of required addons that were installed.
    pub installed: Vec<String>,
    /// Names of the addons that needed them.
    pub needed_by: Vec<String>,
    /// One message per addon whose requirements couldn't be installed.
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StateView {
    pub game: Option<GameInfo>,
    pub settings: Settings,
    pub has_cf_key: bool,
    pub has_gh_token: bool,
    pub has_wago_key: bool,
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
    /// Installed addons that list this one as required.
    pub needed_by: Vec<String>,
}

/// Lua errors from the last game session, grouped by the addon that caused them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ErrorReport {
    /// Identifies this session and error count, for "dismiss".
    pub session: String,
    /// When the session started, as the game wrote it (`11/4 22:03:50`).
    pub started: Option<String>,
    /// The user already dismissed exactly this report.
    pub seen: bool,
    /// Errors tied to an addon.
    pub total: usize,
    pub addons: Vec<AddonErrors>,
    /// Errors that don't point at any addon (macros, scripts, the game itself).
    pub other: Vec<ErrorGroup>,
    /// Taint errors since the interface last loaded. They keep coming until the
    /// player types `/reload`.
    pub taint: usize,
    /// Addons the game named in those taint errors.
    pub taint_by: Vec<String>,
    /// When the interface last reloaded mid-session (`11/7 01:30:00`), if some
    /// errors came before it, and how many did.
    pub reloaded: Option<String>,
    pub before_reload: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddonErrors {
    /// Id of the installed addon, if the folder belongs to one.
    pub id: Option<String>,
    pub name: String,
    pub folder: String,
    pub source: Option<Source>,
    pub enabled: bool,
    pub new_version: Option<String>,
    pub project_url: Option<String>,
    pub count: usize,
    pub groups: Vec<ErrorGroup>,
}

/// One distinct error message and how often it happened.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorGroup {
    pub message: String,
    pub count: usize,
    pub first: String,
    pub last: String,
    pub stack: Vec<String>,
    pub blame: Blame,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LinkOutcome {
    /// Names of hand-installed addons now linked to CurseForge.
    pub linked: Vec<String>,
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
    wowi_list: wowi::ListCache,
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
            wowi_list: wowi::ListCache::default(),
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
            backup::recover(&g);
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
            has_gh_token: secret::load_gh_token(&self.data_dir).is_some(),
            has_wago_key: secret::load_wago_key(&self.data_dir).is_some(),
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
        backup::recover(&info);
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

    // ---------- Wago key ----------

    fn wago_key(&self) -> Result<String> {
        secret::load_wago_key(&self.data_dir).ok_or(Error::NoWagoKey)
    }

    pub async fn save_wago_key(&self, key: &str) -> Result<()> {
        let key = key.trim();
        if key.len() < 16 || key.len() > 400 || !key.chars().all(|c| c.is_ascii_graphic()) {
            return Err(Error::BadWagoKey(
                "That doesn't look like a Wago API key. Copy the whole key from Wago.".into(),
            ));
        }
        Wago {
            http: &self.http,
            key,
        }
        .verify_key()
        .await?;
        secret::save_wago_key(&self.data_dir, key)?;
        tracing::info!("Wago key saved");
        Ok(())
    }

    pub fn clear_wago_key(&self) {
        secret::clear_wago_key(&self.data_dir);
        tracing::info!("Wago key removed");
    }

    fn wowi(&self) -> WowInterface<'_> {
        WowInterface {
            http: &self.http,
            cache: &self.wowi_list,
        }
    }

    // ---------- GitHub token ----------

    fn gh(&self) -> GitHub<'_> {
        GitHub {
            http: &self.http,
            token: secret::load_gh_token(&self.data_dir),
        }
    }

    /// Checks the token with GitHub before saving it. Returns the hourly request limit.
    pub async fn save_gh_token(&self, token: &str) -> Result<u32> {
        let token = token.trim();
        if token.len() < 20 || token.len() > 400 || !token.chars().all(|c| c.is_ascii_graphic()) {
            return Err(Error::Http(
                "That doesn't look like a GitHub token. Copy the whole token from GitHub.".into(),
            ));
        }
        let limit = GitHub {
            http: &self.http,
            token: Some(token.to_string()),
        }
        .verify_token()
        .await?;
        secret::save_gh_token(&self.data_dir, token)?;
        tracing::info!(limit, "GitHub token saved");
        Ok(limit)
    }

    pub fn clear_gh_token(&self) {
        secret::clear_gh_token(&self.data_dir);
        tracing::info!("GitHub token removed");
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
                missing_deps: missing_requirements(&game, &manifest, a),
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
            "wago" => {
                let key = self.wago_key()?;
                let w = Wago {
                    http: &self.http,
                    key: &key,
                };
                if query.contains("wago.io/") {
                    let slug = wago::parse_ref(query)
                        .ok_or_else(|| Error::NotFound("That isn't a Wago addon link.".into()))?;
                    return Ok(vec![w.resolve(&slug).await?.addon]);
                }
                w.search(query).await
            }
            "wowinterface" => self.wowi().search(query).await,
            "github" => {
                let (o, r) = github::parse_repo(query).ok_or_else(|| {
                    Error::NotFound("Enter a GitHub repository, like owner/name.".into())
                })?;
                Ok(vec![self.gh().resolve(&o, &r).await?.addon])
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
        } else if let Some(rest) = id.strip_prefix("wago:") {
            let r = wago::parse_ref(rest)
                .ok_or_else(|| Error::NotFound(format!("Bad addon id {id}")))?;
            let key = self.wago_key()?;
            Wago {
                http: &self.http,
                key: &key,
            }
            .resolve(&r)
            .await
        } else if let Some(rest) = id.strip_prefix("wowi:") {
            let n = wowi::parse_ref(rest)
                .ok_or_else(|| Error::NotFound(format!("Bad addon id {id}")))?;
            self.wowi().resolve(n).await
        } else if let Some(rest) = id.strip_prefix("gh:") {
            let (o, r) = github::parse_repo(rest)
                .ok_or_else(|| Error::NotFound(format!("Bad addon id {id}")))?;
            self.gh().resolve(&o, &r).await
        } else {
            Err(Error::NotFound(
                "This addon wasn't installed from an online source.".into(),
            ))
        }
    }

    /// Installs or updates an addon, and first any required addons that are missing.
    pub async fn install(
        &self,
        id: &str,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<InstallOutcome> {
        let pending = self.updates.lock().unwrap().get(id).cloned();
        let resolved = match pending {
            Some(r) => r,
            None => self.resolve(id).await?,
        };
        let name = resolved.addon.name.clone();
        let also_installed = self
            .install_missing(&name, resolved.requires.clone(), progress)
            .await?;
        let span = tracing::info_span!("install", op = %op_id(), addon = %resolved.addon.id, version = %resolved.version);
        let addon = self
            .install_resolved(resolved, progress)
            .instrument(span)
            .await?;
        Ok(InstallOutcome {
            addon,
            also_installed,
        })
    }

    /// Installs the missing required addons of one installed addon.
    pub async fn install_requirements(
        &self,
        id: &str,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<Vec<String>> {
        let game = self.require_game()?;
        let (name, missing) = {
            let m = self.manifest.lock().unwrap();
            let a = m
                .get(id)
                .ok_or_else(|| Error::NotFound("That addon isn't installed anymore.".into()))?;
            (a.name.clone(), missing_requirements(&game, &m, a))
        };
        self.install_missing(&name, missing, progress).await
    }

    /// Looks up requirements for addons installed before Kobold tracked them, then
    /// installs any that are missing. Costs one request, and only while such addons
    /// are still in the list.
    pub async fn repair_requirements(
        &self,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<RepairOutcome> {
        let game = self.require_game()?;
        let legacy: Vec<(String, u64)> = self
            .manifest
            .lock()
            .unwrap()
            .addons
            .iter()
            .filter(|a| a.source == Source::CurseForge && a.requires.is_none())
            .filter_map(|a| Some((a.id.clone(), a.file_id?)))
            .collect();
        let mut out = RepairOutcome::default();
        if legacy.is_empty() {
            return Ok(out);
        }
        let Ok(key) = self.cf_key() else {
            return Ok(out);
        };
        let file_ids: Vec<u64> = legacy.iter().map(|(_, f)| *f).collect();
        let found = self.cf(&key).file_requirements(&file_ids).await?;
        {
            let mut m = self.manifest.lock().unwrap();
            for (id, file_id) in &legacy {
                if let (Some(reqs), Some(a)) = (found.get(file_id), m.get_mut(id)) {
                    a.requires = Some(reqs.clone());
                }
            }
            self.store.save(&m)?;
        }
        tracing::info!(
            checked = legacy.len(),
            "looked up requirements for older installs"
        );

        for (id, _) in &legacy {
            let (name, missing) = {
                let m = self.manifest.lock().unwrap();
                let Some(a) = m.get(id) else { continue };
                (a.name.clone(), missing_requirements(&game, &m, a))
            };
            if missing.is_empty() {
                continue;
            }
            match self.install_missing(&name, missing, progress).await {
                Ok(names) => {
                    for n in names {
                        if !out.installed.contains(&n) {
                            out.installed.push(n);
                        }
                    }
                    out.needed_by.push(name);
                }
                Err(e) => {
                    tracing::warn!(addon = %id, error = %e, "couldn't install requirements");
                    out.failed.push(e.to_string());
                }
            }
        }
        Ok(out)
    }

    /// Resolves every missing requirement (and theirs) before touching any files, then
    /// installs them, deepest first. Returns the names installed.
    async fn install_missing(
        &self,
        for_name: &str,
        wanted: Vec<Requirement>,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<Vec<String>> {
        const MAX: usize = 20;
        if wanted.is_empty() {
            return Ok(vec![]);
        }
        let game = self.require_game()?;
        let mut queue: VecDeque<Requirement> = wanted.into();
        let mut seen: HashSet<String> = HashSet::new();
        let mut plan: Vec<Resolved> = vec![];
        while let Some(req) = queue.pop_front() {
            if !seen.insert(req.id.clone()) {
                continue;
            }
            let present = {
                let m = self.manifest.lock().unwrap();
                requirement_present(&game, &m, &req)
            };
            if present {
                continue;
            }
            if plan.len() >= MAX {
                return Err(Error::Other(format!(
                    "{for_name} lists more than {MAX} required addons, so Kobold stopped."
                )));
            }
            tracing::info!(requirement = %req.id, name = %req.name, needed_by = %for_name, "resolving required addon");
            let r = self
                .resolve(&req.id)
                .await
                .map_err(|e| needed_for(for_name, &req.name, e))?;
            queue.extend(r.requires.iter().cloned());
            plan.push(r);
        }
        plan.reverse();
        let mut names = vec![];
        for r in plan {
            let name = r.addon.name.clone();
            let span = tracing::info_span!("install", op = %op_id(), addon = %r.addon.id, version = %r.version, needed_by = %for_name);
            self.install_resolved(r, progress)
                .instrument(span)
                .await
                .map_err(|e| needed_for(for_name, &name, e))?;
            names.push(name);
        }
        Ok(names)
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
        let wago_key = (r.addon.source == Source::Wago)
            .then(|| secret::load_wago_key(&self.data_dir))
            .flatten();
        let dl = net::download(
            &self.http,
            &r.download_url,
            &dest,
            &r.expected,
            wago_key.as_deref(),
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
            requires: Some(r.requires.clone()),
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
            requires: None,
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
        if previous.is_some() {
            report("backup", 88);
            self.auto_backup(game)?;
        }
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

        let gh = self.gh();
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
        let wago_addons: Vec<&InstalledAddon> =
            addons.iter().filter(|a| a.source == Source::Wago).collect();
        if !wago_addons.is_empty() {
            match self.wago_key() {
                Ok(key) => {
                    let w = Wago {
                        http: &self.http,
                        key: &key,
                    };
                    for a in wago_addons {
                        let Some(id) = a.id.strip_prefix("wago:") else {
                            continue;
                        };
                        match w.resolve(id).await {
                            Ok(r) if r.release_tag != a.release_tag => {
                                found.insert(a.id.clone(), r);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!(addon = %a.id, error = %e, "update check failed")
                            }
                        }
                    }
                }
                Err(_) => tracing::info!("no Wago key; skipping Wago update checks"),
            }
        }

        let wowi_addons: Vec<(u64, &InstalledAddon)> = addons
            .iter()
            .filter(|a| a.source == Source::WowInterface)
            .filter_map(|a| Some((wowi::parse_ref(a.id.strip_prefix("wowi:")?)?, a)))
            .collect();
        if !wowi_addons.is_empty() {
            let wi = self.wowi();
            let ids: Vec<u64> = wowi_addons.iter().map(|(n, _)| *n).collect();
            match wi.versions(&ids).await {
                Ok(stamps) => {
                    for (n, a) in &wowi_addons {
                        let newer = stamps
                            .iter()
                            .any(|(id, stamp)| id == n && Some(stamp) != a.release_tag.as_ref());
                        if newer {
                            match wi.resolve(*n).await {
                                Ok(r) => {
                                    found.insert(a.id.clone(), r);
                                }
                                Err(e) => {
                                    tracing::warn!(addon = %a.id, error = %e, "update check failed")
                                }
                            }
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "WoWInterface update check failed"),
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
            needed_by: m.needed_by(id),
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
        tracing::info!(restored, wanted = entry.paths.len(), "undo delete");
        if restored == 0 && !entry.paths.is_empty() {
            // Nothing came back (Recycle Bin emptied, disabled, or the drive has none).
            // Don't re-add the addon, or the list would point at folders that don't exist.
            return Err(Error::NotFound(
                "The removed folders aren't in the Recycle Bin anymore, so they can't be restored."
                    .into(),
            ));
        }
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

    // ---------- Lua errors ----------

    /// Reads the game's log for this session's Lua errors.
    pub fn lua_errors(&self) -> Result<ErrorReport> {
        let game = self.require_game()?;
        let views = self.list_installed()?;
        let known: Vec<String> = views.iter().flat_map(|v| v.folders.clone()).collect();
        let Some(log) = luaerrors::read(&game.root.join("Logs").join("General.log"), &known) else {
            return Ok(ErrorReport::default());
        };

        let owner = |folder: &str| {
            views
                .iter()
                .find(|v| v.folders.iter().any(|f| eq_ignore_case(f, folder)))
        };
        let mut addons: Vec<AddonErrors> = vec![];
        let mut other: Vec<ErrorGroup> = vec![];
        let (reloaded, before_reload) = match log.reloaded {
            Some((time, n)) => (Some(time), n),
            None => (None, 0),
        };
        let mut taint = 0;
        let mut taint_by: Vec<String> = vec![];
        for (i, e) in log.errors.into_iter().enumerate() {
            if e.tainted && i >= before_reload {
                taint += 1;
                if let (Blame::Taint, Some(folder)) = (&e.blame, &e.folder) {
                    let name = owner(folder)
                        .map(|v| v.name.clone())
                        .unwrap_or_else(|| folder.clone());
                    if !taint_by.contains(&name) {
                        taint_by.push(name);
                    }
                }
            }
            let groups = match &e.folder {
                Some(folder) => {
                    let view = owner(folder);
                    let key = view.map(|v| v.id.clone());
                    let pos = addons.iter().position(|a| match (&a.id, &key) {
                        (Some(a), Some(k)) => a == k,
                        (None, None) => eq_ignore_case(&a.folder, folder),
                        _ => false,
                    });
                    let i = match pos {
                        Some(i) => i,
                        None => {
                            addons.push(AddonErrors {
                                id: key,
                                name: view
                                    .map(|v| v.name.clone())
                                    .unwrap_or_else(|| folder.clone()),
                                folder: folder.clone(),
                                source: view.and_then(|v| v.source),
                                enabled: view.map(|v| v.enabled).unwrap_or(true),
                                new_version: view.and_then(|v| v.new_version.clone()),
                                project_url: view.and_then(|v| v.project_url.clone()),
                                count: 0,
                                groups: vec![],
                            });
                            addons.len() - 1
                        }
                    };
                    addons[i].count += 1;
                    &mut addons[i].groups
                }
                None => &mut other,
            };
            match groups.iter_mut().find(|g| g.message == e.message) {
                Some(g) => {
                    g.count += 1;
                    g.last = e.time;
                }
                None => groups.push(ErrorGroup {
                    message: e.message,
                    count: 1,
                    first: e.time.clone(),
                    last: e.time,
                    stack: e.stack,
                    blame: e.blame,
                }),
            }
        }
        addons.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
        let total = addons.iter().map(|a| a.count).sum();
        let session = format!("{}|{total}", log.started.clone().unwrap_or_default());
        let seen = self.settings().lua_errors_seen.as_deref() == Some(session.as_str());
        Ok(ErrorReport {
            session,
            started: log.started,
            seen,
            total,
            addons,
            other,
            taint,
            taint_by,
            reloaded,
            before_reload,
        })
    }

    pub fn dismiss_lua_errors(&self, session: &str) -> Result<()> {
        self.update_settings(|s| s.lua_errors_seen = Some(session.to_string()))?;
        Ok(())
    }

    // ---------- settings backups ----------

    pub fn backups_dir(&self) -> PathBuf {
        self.cache_dir
            .parent()
            .map(|p| p.join("backups"))
            .unwrap_or_else(|| self.cache_dir.join("backups"))
    }

    pub fn list_backups(&self) -> Vec<Backup> {
        backup::list(&self.backups_dir())
    }

    async fn make_backup(&self, game: &GameInfo, kind: backup::Kind) -> Result<Option<Backup>> {
        let (game, dir) = (game.clone(), self.backups_dir());
        tokio::task::spawn_blocking(move || backup::create(&game, &dir, kind))
            .await
            .map_err(|e| Error::Other(e.to_string()))?
    }

    /// Before an update: back up `WTF` if it changed since the last backup. Runs inside
    /// the install, which already does blocking file work under `op_lock`.
    fn auto_backup(&self, game: &GameInfo) -> Result<()> {
        if !self.settings().backup_before_update {
            return Ok(());
        }
        let dir = self.backups_dir();
        if !backup::needs_backup(game, &dir) {
            return Ok(());
        }
        backup::create(game, &dir, backup::Kind::Auto)
            .map(|_| ())
            .map_err(|e| {
                tracing::error!(error = %e, "settings backup before update failed");
                Error::Other(format!(
                    "Couldn't back up your addon settings, so the update was stopped: {e} \
                     You can turn off backups before updates in Settings."
                ))
            })
    }

    pub async fn backup_now(&self) -> Result<Backup> {
        let _guard = self.op_lock.lock().await;
        let game = self.require_game()?;
        self.make_backup(&game, backup::Kind::Manual)
            .await?
            .ok_or_else(|| {
                Error::NotFound(
                    "There are no addon settings to back up yet. Play the game once first.".into(),
                )
            })
    }

    /// Replaces `WTF` with a backup, after backing up the current folder.
    pub async fn restore_backup(&self, name: &str) -> Result<()> {
        let _guard = self.op_lock.lock().await;
        let game = self.require_game()?;
        if game::is_running(&game.root) {
            return Err(Error::Other(
                "Close WoW Forever first. The game saves its settings when it exits and would undo the restore.".into(),
            ));
        }
        let archive = backup::path_of(&self.backups_dir(), name)?;
        self.make_backup(&game, backup::Kind::Restore).await?;
        tracing::info!(%name, "restoring settings backup");
        tokio::task::spawn_blocking(move || backup::restore(&game, &archive))
            .await
            .map_err(|e| Error::Other(e.to_string()))?
    }

    pub fn delete_backup(&self, name: &str) -> Result<()> {
        let p = backup::path_of(&self.backups_dir(), name)?;
        fs::remove_file(&p).map_err(|e| Error::io("delete", &p, e))?;
        tracing::info!(%name, "settings backup deleted");
        Ok(())
    }

    // ---------- linking hand-installed addons ----------

    /// Fingerprints folders Kobold doesn't manage (and zip imports) and asks CurseForge
    /// which addon files they are. Exact matches become CurseForge addons, so they get
    /// updates. Files on disk aren't touched.
    pub async fn link_hand_installed(&self) -> Result<LinkOutcome> {
        let mut out = LinkOutcome::default();
        let Ok(key) = self.cf_key() else {
            return Ok(out);
        };
        let game = self.require_game()?;
        let manifest = self.manifest.lock().unwrap().clone();
        let zip_folders: Vec<String> = manifest
            .addons
            .iter()
            .filter(|a| a.source == Source::Zip)
            .flat_map(|a| a.folders.clone())
            .collect();
        let candidates = {
            let (g, zf, m) = (game.clone(), zip_folders.clone(), manifest.clone());
            tokio::task::spawn_blocking(move || link_candidates(&g, &m, &zf))
                .await
                .map_err(|e| Error::Other(e.to_string()))?
        };
        if candidates.is_empty() {
            return Ok(out);
        }
        let cf = self.cf(&key);
        let fps: Vec<u32> = candidates.iter().map(|c| c.fingerprint).collect();
        let matches = cf.match_fingerprints(&fps).await?;
        tracing::info!(
            folders = candidates.len(),
            matches = matches.len(),
            "checked hand-installed folders with CurseForge"
        );

        // Keep matches whose every folder is here, unchanged, and in one state.
        let mut picked = vec![];
        for m in matches {
            if m.modules.is_empty() || manifest.get(&format!("cf:{}", m.mod_id)).is_some() {
                continue;
            }
            let local: Vec<&Candidate> = m
                .modules
                .iter()
                .filter_map(|md| {
                    candidates.iter().find(|c| {
                        eq_ignore_case(&c.folder, &md.name)
                            && u64::from(c.fingerprint) == md.fingerprint
                    })
                })
                .collect();
            if local.len() != m.modules.len() || local.iter().any(|c| c.enabled != local[0].enabled)
            {
                continue;
            }
            // A zip import is replaced only if all of its folders are in this file.
            let replaces: Vec<String> = manifest
                .addons
                .iter()
                .filter(|a| a.source == Source::Zip)
                .filter(|a| {
                    a.folders
                        .iter()
                        .any(|f| local.iter().any(|c| eq_ignore_case(&c.folder, f)))
                })
                .map(|a| {
                    let all_in = a
                        .folders
                        .iter()
                        .all(|f| local.iter().any(|c| eq_ignore_case(&c.folder, f)));
                    if all_in {
                        Ok(a.id.clone())
                    } else {
                        Err(())
                    }
                })
                .collect::<std::result::Result<_, _>>()
                .unwrap_or_else(|_| vec!["!".into()]);
            if replaces.iter().any(|r| r == "!") {
                continue;
            }
            let folders: Vec<String> = local.iter().map(|c| c.folder.clone()).collect();
            let enabled = local[0].enabled;
            picked.push((m, folders, enabled, replaces));
        }
        if picked.is_empty() {
            return Ok(out);
        }

        let mod_ids: Vec<u64> = picked.iter().map(|p| p.0.mod_id).collect();
        let forever = cf.forever_type_id().await?;
        let mods = cf.mods(&mod_ids).await?;
        let mut req_ids: Vec<u64> = picked.iter().flat_map(|p| p.0.requires.clone()).collect();
        req_ids.sort_unstable();
        req_ids.dedup();
        let named = cf.named(&req_ids).await?;

        let _guard = self.op_lock.lock().await;
        let mut m = self.manifest.lock().unwrap();
        let before = m.clone();
        for (file, folders, enabled, replaces) in picked {
            let Some(info) = mods.iter().find(|x| x.id == file.mod_id) else {
                continue;
            };
            // Re-check under the lock; something may have been installed meanwhile.
            if folders
                .iter()
                .any(|f| m.owners_of(f, "").iter().any(|o| !replaces.contains(&o.id)))
            {
                continue;
            }
            for id in &replaces {
                m.remove(id);
            }
            let remote = curseforge::to_remote(info, forever);
            let requires = file
                .requires
                .iter()
                .filter_map(|id| named.iter().find(|r| r.id == format!("cf:{id}")).cloned())
                .collect();
            m.upsert(InstalledAddon {
                id: remote.id.clone(),
                name: remote.name.clone(),
                author: remote.author.clone(),
                summary: remote.summary.clone(),
                source: Source::CurseForge,
                version: curseforge::clean_display_name(&file.display_name, &remote.name),
                file_id: Some(file.file_id),
                release_tag: None,
                project_url: Some(remote.project_url.clone()),
                icon_url: remote.icon_url.clone(),
                folders,
                installed_at: 0,
                updated_at: 0,
                enabled,
                pinned: false,
                archive_sha256: None,
                requires: Some(requires),
            });
            tracing::info!(addon = %remote.id, name = %remote.name, file = file.file_id, "linked hand-installed addon to CurseForge");
            out.linked.push(remote.name);
        }
        if out.linked.is_empty() {
            return Ok(out);
        }
        if let Err(e) = self.store.save(&m) {
            *m = before;
            return Err(e);
        }
        Ok(out)
    }

    // ---------- addon list ----------

    /// Writes every installed addon to `path`. Returns how many were written.
    pub fn export_addon_list(&self, path: &Path) -> Result<usize> {
        let views = self.list_installed()?;
        let entries: Vec<addonlist::Entry> = views
            .into_iter()
            .map(|v| addonlist::Entry {
                id: v.id,
                name: v.name,
                source: v.source,
                version: v.version,
                project_url: v.project_url,
                pinned: v.pinned,
                enabled: v.enabled,
            })
            .collect();
        let n = entries.len();
        let version = self.game().and_then(|g| g.version);
        addonlist::save(path, &addonlist::new_list(entries, version))?;
        tracing::info!(?path, count = n, "addon list exported");
        Ok(n)
    }

    /// Reads a saved list and sorts it into what can be installed, what's already
    /// here, and what has to be installed by hand.
    pub fn preview_addon_list(&self, path: &Path) -> Result<ImportPreview> {
        let list = addonlist::load(path)?;
        let views = self.list_installed()?;
        Ok(addonlist::preview(list, |e| {
            views.iter().any(|v| {
                v.id.eq_ignore_ascii_case(&e.id)
                    || (!e.installable() && eq_ignore_case(&v.name, &e.name))
            })
        }))
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

/// Keeps errors the UI acts on (blocked downloads, key and network problems) as they
/// are, and says which addon needed the requirement for everything else.
fn needed_for(for_name: &str, dep: &str, e: Error) -> Error {
    match e {
        Error::DistributionBlocked { .. }
        | Error::NoCurseForgeKey
        | Error::BadCurseForgeKey
        | Error::Offline { .. } => e,
        other => Error::Other(format!(
            "{for_name} needs {dep}, but it couldn't be installed: {other}"
        )),
    }
}

/// A requirement counts as present if Kobold manages it, or if a folder with its name
/// was installed by hand (for example from a zip import).
fn requirement_present(game: &GameInfo, m: &Manifest, req: &Requirement) -> bool {
    if m.get(&req.id).is_some() {
        return true;
    }
    let name = req.name.trim();
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && !name.contains("..")
        && game.addons_dir.join(name).is_dir()
}

fn missing_requirements(game: &GameInfo, m: &Manifest, a: &InstalledAddon) -> Vec<Requirement> {
    m.missing_for(a)
        .into_iter()
        .filter(|r| !requirement_present(game, m, r))
        .collect()
}

struct Candidate {
    folder: String,
    enabled: bool,
    fingerprint: u32,
}

/// Addon folders Kobold could link: not managed, or from a zip import.
fn link_candidates(game: &GameInfo, m: &Manifest, zip_folders: &[String]) -> Vec<Candidate> {
    let mut out = vec![];
    for (dir, enabled) in [
        (game.addons_dir.clone(), true),
        (game.disabled_dir(), false),
    ] {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let managed = m.owns(&name) && !zip_folders.iter().any(|z| eq_ignore_case(z, &name));
            if !is_dir || name.starts_with('.') || name.starts_with("Blizzard_") || managed {
                continue;
            }
            if let Some(fp) = fingerprint::folder_fingerprint(&e.path()) {
                out.push(Candidate {
                    folder: name,
                    enabled,
                    fingerprint: fp,
                });
            }
        }
    }
    out
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
                missing_deps: vec![],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon(id: &str, name: &str, requires: Option<Vec<Requirement>>) -> InstalledAddon {
        InstalledAddon {
            id: id.into(),
            name: name.into(),
            author: String::new(),
            summary: String::new(),
            source: Source::CurseForge,
            version: "1".into(),
            file_id: Some(1),
            release_tag: None,
            project_url: None,
            icon_url: None,
            folders: vec![name.into()],
            installed_at: 0,
            updated_at: 0,
            enabled: true,
            pinned: false,
            archive_sha256: None,
            requires,
        }
    }

    fn req(id: &str, name: &str) -> Requirement {
        Requirement {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn requirements_count_managed_and_hand_installed_folders() {
        let dir = tempfile::tempdir().unwrap();
        let addons_dir = dir.path().join("Interface").join("AddOns");
        fs::create_dir_all(addons_dir.join("QuestieTrace")).unwrap();
        let game = GameInfo {
            root: dir.path().to_path_buf(),
            version: None,
            addons_dir,
            running: false,
        };
        let mut m = Manifest::default();
        let questie = addon(
            "cf:1",
            "Questie",
            Some(vec![
                req("cf:2", "QuestieDB"),
                req("cf:3", "QuestieTrace"),
                req("cf:4", "LibThing"),
                req("cf:5", "../Questie"),
            ]),
        );
        m.upsert(questie.clone());
        m.upsert(addon("cf:4", "LibThing", Some(vec![])));
        let missing: Vec<String> = missing_requirements(&game, &m, &questie)
            .into_iter()
            .map(|r| r.name)
            .collect();
        // QuestieTrace is on disk, LibThing is managed, and a path-like name never
        // counts as a folder.
        assert_eq!(missing, vec!["QuestieDB", "../Questie"]);
    }

    #[test]
    fn requirement_errors_name_the_addon() {
        let e = needed_for(
            "Questie",
            "QuestieDB",
            Error::NoForeverBuild("QuestieDB".into()),
        );
        assert!(e.to_string().starts_with("Questie needs QuestieDB"));
        let blocked = needed_for(
            "Questie",
            "QuestieDB",
            Error::DistributionBlocked {
                name: "QuestieDB".into(),
                url: "u".into(),
            },
        );
        assert_eq!(blocked.code(), "distribution_blocked");
    }
}
