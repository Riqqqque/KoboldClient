pub mod curseforge;
pub mod github;

use serde::Serialize;

use crate::manifest::{Requirement, Source};
use crate::net::Expected;

/// A search result or addon page, ready for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteAddon {
    pub id: String,
    pub source: Source,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub downloads: u64,
    pub updated: Option<String>,
    pub icon_url: Option<String>,
    pub project_url: String,
    /// False when the author only allows downloads from the website.
    pub downloadable: bool,
    /// False when there is no Forever build to install.
    pub has_forever_build: bool,
}

/// Everything needed to download and record one addon version.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub addon: RemoteAddon,
    pub version: String,
    pub download_url: String,
    pub expected: Expected,
    pub file_id: Option<u64>,
    pub release_tag: Option<String>,
    /// Addons this version can't work without (CurseForge "required dependency").
    pub requires: Vec<Requirement>,
}
