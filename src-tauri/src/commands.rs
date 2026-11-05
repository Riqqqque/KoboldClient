//! Thin Tauri command layer. All logic lives in [`crate::service::Core`].

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{Error, Result};
use crate::game::GameInfo;
use crate::service::{AddonView, Core, DeletePreview, Progress, Removed, StateView};
use crate::settings::Settings;
use crate::sources::RemoteAddon;

type Ctx<'a> = State<'a, Arc<Core>>;

fn emitter(app: &AppHandle) -> impl Fn(Progress) + Send + Sync + '_ {
    move |p: Progress| {
        let _ = app.emit("install-progress", p);
    }
}

#[tauri::command]
pub fn get_state(core: Ctx) -> StateView {
    core.state()
}

#[tauri::command]
pub async fn detect_game(core: Ctx<'_>) -> Result<Vec<GameInfo>> {
    let c = core.inner().clone();
    tokio::task::spawn_blocking(move || c.detect_game())
        .await
        .map_err(|e| Error::Other(e.to_string()))
}

#[tauri::command]
pub fn set_game_root(core: Ctx, path: PathBuf) -> Result<GameInfo> {
    core.set_game_root(&path)
}

#[tauri::command]
pub fn update_settings(core: Ctx, settings: Settings) -> Result<Settings> {
    core.update_settings(|s| {
        // The game folder is only changed through set_game_root, which validates it.
        let root = s.game_root.clone();
        *s = settings;
        s.game_root = root;
    })
}

#[tauri::command]
pub async fn save_cf_key(core: Ctx<'_>, key: String) -> Result<()> {
    core.save_cf_key(&key).await
}

#[tauri::command]
pub fn clear_cf_key(core: Ctx) {
    core.clear_cf_key()
}

#[tauri::command]
pub fn list_installed(core: Ctx) -> Result<Vec<AddonView>> {
    core.list_installed()
}

#[tauri::command]
pub async fn search(
    core: Ctx<'_>,
    source: String,
    query: String,
    sort: String,
) -> Result<Vec<RemoteAddon>> {
    core.search(&source, &query, &sort).await
}

#[tauri::command]
pub async fn install(app: AppHandle, core: Ctx<'_>, id: String) -> Result<AddonView> {
    core.install(&id, &emitter(&app)).await
}

#[tauri::command]
pub async fn import_zip(app: AppHandle, core: Ctx<'_>, path: PathBuf) -> Result<AddonView> {
    core.import_zip(&path, &emitter(&app)).await
}

#[tauri::command]
pub async fn check_updates(core: Ctx<'_>) -> Result<Vec<AddonView>> {
    core.check_updates().await
}

#[tauri::command]
pub fn delete_preview(core: Ctx, id: String) -> Result<DeletePreview> {
    core.delete_preview(&id)
}

#[tauri::command]
pub async fn remove_addon(core: Ctx<'_>, id: String, delete_settings: bool) -> Result<Removed> {
    core.remove(&id, delete_settings).await
}

#[tauri::command]
pub async fn undo_remove(core: Ctx<'_>, token: String) -> Result<()> {
    core.undo_remove(&token).await
}

#[tauri::command]
pub async fn set_enabled(core: Ctx<'_>, id: String, enabled: bool) -> Result<()> {
    core.set_enabled(&id, enabled).await
}

#[tauri::command]
pub fn set_pinned(core: Ctx, id: String, pinned: bool) -> Result<()> {
    core.set_pinned(&id, pinned)
}

#[tauri::command]
pub fn open_folder(core: Ctx, which: String) -> Result<()> {
    let path = match which.as_str() {
        "logs" => core.log_dir.clone(),
        "addons" => core.game().ok_or(Error::NoGame)?.addons_dir,
        _ => return Err(Error::NotFound("Unknown folder".into())),
    };
    std::fs::create_dir_all(&path).map_err(|e| Error::io("create", &path, e))?;
    std::process::Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|e| Error::io("open", &path, e))?;
    Ok(())
}

#[tauri::command]
pub fn export_diagnostics(app: AppHandle, core: Ctx) -> Result<PathBuf> {
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().desktop_dir())
        .map_err(|e| Error::Other(e.to_string()))?;
    let out = core.export_diagnostics(&dir)?;
    let _ = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(&out)
        .spawn();
    Ok(out)
}

/// Lets the UI write to the log, so UI-side failures show up in bug reports.
#[tauri::command]
pub fn log_from_ui(level: String, message: String) {
    match level.as_str() {
        "error" => tracing::error!(target: "ui", "{message}"),
        "warn" => tracing::warn!(target: "ui", "{message}"),
        _ => tracing::info!(target: "ui", "{message}"),
    }
}
