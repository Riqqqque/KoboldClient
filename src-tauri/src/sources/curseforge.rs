//! CurseForge Core API (https://docs.curseforge.com/rest-api/).

use serde::Deserialize;
use tokio::sync::OnceCell;

use super::{RemoteAddon, Resolved};
use crate::error::{Error, Result};
use crate::manifest::Source;
use crate::net::{check, Expected};

const API: &str = "https://api.curseforge.com";
pub const WOW_GAME_ID: u32 = 1;

#[derive(Debug, Clone, Copy)]
pub enum Sort {
    Popular,
    Downloads,
    Updated,
}

impl Sort {
    fn field(self) -> u8 {
        match self {
            Sort::Popular => 2,
            Sort::Updated => 3,
            Sort::Downloads => 6,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Wrap<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionType {
    id: u32,
    name: String,
    #[serde(default)]
    slug: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: f64,
    #[serde(default)]
    pub links: Links,
    #[serde(default)]
    pub logo: Option<Logo>,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub latest_files_indexes: Vec<FileIndex>,
    #[serde(default)]
    pub allow_mod_distribution: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(default)]
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logo {
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Author {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndex {
    pub file_id: u64,
    #[serde(default)]
    pub release_type: u8,
    #[serde(default)]
    pub game_version_type_id: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    id: u64,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<Hash>,
    #[serde(default = "yes")]
    is_available: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct Hash {
    value: String,
    algo: u8,
}

pub struct CurseForge<'a> {
    pub http: &'a reqwest::Client,
    pub key: &'a str,
    /// Cache for the Forever game version type id, shared across calls.
    pub forever_type: &'a OnceCell<u32>,
}

impl CurseForge<'_> {
    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let resp = self
            .http
            .get(format!("{API}{path}"))
            .header("x-api-key", self.key)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?;
        if matches!(resp.status().as_u16(), 401 | 403) {
            return Err(Error::BadCurseForgeKey);
        }
        Ok(check(resp).await?.json::<Wrap<T>>().await?.data)
    }

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let resp = self
            .http
            .post(format!("{API}{path}"))
            .header("x-api-key", self.key)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;
        if matches!(resp.status().as_u16(), 401 | 403) {
            return Err(Error::BadCurseForgeKey);
        }
        Ok(check(resp).await?.json::<Wrap<T>>().await?.data)
    }

    /// Checks that the key works. Used when the user saves it.
    pub async fn verify_key(&self) -> Result<()> {
        let _: serde_json::Value = self.get(&format!("/v1/games/{WOW_GAME_ID}"), &[]).await?;
        Ok(())
    }

    /// The id CurseForge uses for the Forever flavor. Looked up instead of hard-coded
    /// because the game is new and the id could change.
    pub async fn forever_type_id(&self) -> Result<u32> {
        self.forever_type
            .get_or_try_init(|| async {
                let types: Vec<VersionType> = self.get(&format!("/v1/games/{WOW_GAME_ID}/version-types"), &[]).await?;
                let found = types
                    .iter()
                    .find(|t| t.slug.to_ascii_lowercase().contains("forever") || t.name.to_ascii_lowercase().contains("forever"));
                match found {
                    Some(t) => {
                        tracing::info!(id = t.id, name = %t.name, "CurseForge Forever version type");
                        Ok(t.id)
                    }
                    None => {
                        let names: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
                        tracing::error!(?names, "no Forever version type on CurseForge");
                        Err(Error::NoForeverVersion)
                    }
                }
            })
            .await
            .copied()
    }

    pub async fn search(&self, query: &str, sort: Sort, index: u32) -> Result<Vec<RemoteAddon>> {
        let t = self.forever_type_id().await?;
        let mods: Vec<Mod> = self
            .get(
                "/v1/mods/search",
                &[
                    ("gameId", WOW_GAME_ID.to_string()),
                    ("gameVersionTypeId", t.to_string()),
                    ("searchFilter", query.trim().to_string()),
                    ("sortField", sort.field().to_string()),
                    ("sortOrder", "desc".into()),
                    ("pageSize", "30".into()),
                    ("index", index.min(9_970).to_string()),
                ],
            )
            .await?;
        Ok(mods.iter().map(|m| to_remote(m, t)).collect())
    }

    pub async fn mods(&self, ids: &[u64]) -> Result<Vec<Mod>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        self.post(
            "/v1/mods",
            serde_json::json!({ "modIds": ids, "filterPcOnly": true }),
        )
        .await
    }

    /// The newest Forever file id for a mod, preferring stable releases.
    pub fn latest_forever_file(m: &Mod, forever: u32) -> Option<u64> {
        let candidates = m
            .latest_files_indexes
            .iter()
            .filter(|f| f.game_version_type_id == Some(forever));
        // release_type: 1 = release, 2 = beta, 3 = alpha
        candidates
            .min_by_key(|f| (f.release_type.max(1), u64::MAX - f.file_id))
            .map(|f| f.file_id)
    }

    pub async fn resolve(&self, mod_id: u64) -> Result<Resolved> {
        let t = self.forever_type_id().await?;
        let m: Mod = self.get(&format!("/v1/mods/{mod_id}"), &[]).await?;
        self.resolve_mod(&m, t).await
    }

    pub async fn resolve_mod(&self, m: &Mod, forever: u32) -> Result<Resolved> {
        let addon = to_remote(m, forever);
        let Some(file_id) = Self::latest_forever_file(m, forever) else {
            return Err(Error::NoForeverBuild(m.name.clone()));
        };
        if m.allow_mod_distribution == Some(false) {
            return Err(Error::DistributionBlocked {
                name: m.name.clone(),
                url: addon.project_url.clone(),
            });
        }
        let file: File = self
            .get(&format!("/v1/mods/{}/files/{file_id}", m.id), &[])
            .await?;
        let Some(url) = file
            .download_url
            .filter(|u| !u.is_empty() && file.is_available)
        else {
            return Err(Error::DistributionBlocked {
                name: m.name.clone(),
                url: addon.project_url.clone(),
            });
        };
        let expected = file
            .hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| Expected::Sha1(h.value.clone()))
            .or_else(|| {
                file.hashes
                    .iter()
                    .find(|h| h.algo == 2)
                    .map(|h| Expected::Md5(h.value.clone()))
            })
            .unwrap_or(Expected::None);
        if expected == Expected::None {
            tracing::warn!(
                mod_id = m.id,
                file_id,
                "CurseForge file has no hash; relying on HTTPS only"
            );
        }
        Ok(Resolved {
            version: clean_display_name(&file.display_name, &m.name),
            addon,
            download_url: url,
            expected,
            file_id: Some(file.id),
            release_tag: None,
        })
    }
}

/// CurseForge display names are often "Name-1.2.3" or "Name v1.2.3"; keep the version.
fn clean_display_name(display: &str, name: &str) -> String {
    let d = display.trim();
    let stripped = d
        .strip_prefix(name)
        .map(|r| {
            r.trim_start_matches([' ', '-', '_', ':'])
                .trim_start_matches(['v', 'V'])
        })
        .filter(|r| !r.is_empty());
    stripped.unwrap_or(d).to_string()
}

pub fn to_remote(m: &Mod, forever: u32) -> RemoteAddon {
    RemoteAddon {
        id: format!("cf:{}", m.id),
        source: Source::CurseForge,
        name: m.name.trim().to_string(),
        author: m
            .authors
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        summary: m.summary.trim().to_string(),
        downloads: m.download_count.max(0.0) as u64,
        updated: m.date_modified.clone(),
        icon_url: m
            .logo
            .as_ref()
            .and_then(|l| l.thumbnail_url.clone().or_else(|| l.url.clone())),
        project_url: m
            .links
            .website_url
            .clone()
            .unwrap_or_else(|| format!("https://www.curseforge.com/wow/addons/{}", m.id)),
        downloadable: m.allow_mod_distribution != Some(false),
        has_forever_build: CurseForge::latest_forever_file(m, forever).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_cleanup() {
        assert_eq!(clean_display_name("Questie-10.4.3", "Questie"), "10.4.3");
        assert_eq!(clean_display_name("Bagnon v1.60.3", "Bagnon"), "1.60.3");
        assert_eq!(clean_display_name("1.2.0", "X"), "1.2.0");
    }

    #[test]
    fn picks_stable_forever_file() {
        let m: Mod = serde_json::from_value(serde_json::json!({
            "id": 1, "name": "X",
            "latestFilesIndexes": [
                { "fileId": 50, "releaseType": 2, "gameVersionTypeId": 99 },
                { "fileId": 40, "releaseType": 1, "gameVersionTypeId": 99 },
                { "fileId": 60, "releaseType": 1, "gameVersionTypeId": 517 }
            ]
        }))
        .unwrap();
        assert_eq!(CurseForge::latest_forever_file(&m, 99), Some(40));
        assert_eq!(CurseForge::latest_forever_file(&m, 1), None);
    }
}
