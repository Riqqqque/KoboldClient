//! Wago Addons (https://addons.wago.io), through its external API.
//!
//! The API needs a personal key, which Wago gives to its subscribers. Wago lists
//! Forever as its own game version (`forever`), so only Forever releases come back.
//! Releases carry no checksum, so downloads rely on HTTPS alone.

use serde::Deserialize;

use super::{RemoteAddon, Resolved};
use crate::error::{Error, Result};
use crate::manifest::Source;
use crate::net::{check, Expected};
use crate::util::clean_version;

const API: &str = "https://addons.wago.io/api/external";
pub const GAME_VERSION: &str = "forever";
pub const SUBSCRIBE_URL: &str = "https://addons.wago.io/patreon";

#[derive(Debug, Deserialize)]
struct SearchResult {
    #[serde(default)]
    data: Vec<Addon>,
}

#[derive(Debug, Clone, Deserialize)]
struct Addon {
    id: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default, alias = "name")]
    display_name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default, alias = "thumbnail")]
    thumbnail_image: Option<String>,
    #[serde(default)]
    authors: Authors,
    #[serde(default)]
    download_count: Option<f64>,
    #[serde(default)]
    website_url: Option<String>,
    /// Details use `recent_release`, search results `releases`.
    #[serde(default, alias = "releases", deserialize_with = "releases")]
    recent_release: Releases,
}

/// Wago sends a list of names in some responses and a single string in others.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(untagged)]
enum Authors {
    #[default]
    None,
    One(String),
    Many(Vec<String>),
}

impl Authors {
    fn first(&self) -> String {
        match self {
            Authors::None => String::new(),
            Authors::One(s) => s.clone(),
            Authors::Many(v) => v.first().cloned().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Releases {
    #[serde(default)]
    stable: Option<Release>,
    #[serde(default)]
    beta: Option<Release>,
    // Alpha builds are never installed, so they aren't read.
}

/// An addon without releases comes back as `[]` instead of an object.
fn releases<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Releases, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    if v.is_object() {
        serde_json::from_value(v).map_err(serde::de::Error::custom)
    } else {
        Ok(Releases::default())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Release {
    #[serde(default)]
    label: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default, alias = "link")]
    download_link: Option<String>,
}

impl Releases {
    /// Stable if there is one, otherwise beta. Alpha builds are never picked.
    fn best(&self) -> Option<&Release> {
        self.stable
            .as_ref()
            .filter(|r| r.download_link.is_some())
            .or_else(|| self.beta.as_ref().filter(|r| r.download_link.is_some()))
    }
}

/// Accepts a Wago addon URL, `wago:<id>` or a bare id/slug.
pub fn parse_ref(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let s = s.strip_prefix("wago:").unwrap_or(s);
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = s
        .strip_prefix("addons.wago.io/addons/")
        .unwrap_or(s)
        .split(['/', '?', '#'])
        .next()?;
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| s.to_string())
}

pub struct Wago<'a> {
    pub http: &'a reqwest::Client,
    pub key: &'a str,
}

impl Wago<'_> {
    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let resp = self
            .http
            .get(format!("{API}{path}"))
            .bearer_auth(self.key)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?;
        match resp.status().as_u16() {
            401 => {
                return Err(Error::BadWagoKey(
                    "Wago didn't accept the API key. Check it in Settings.".into(),
                ))
            }
            403 => {
                return Err(Error::BadWagoKey(
                    "The Wago API key has expired. Get a new one from Wago.".into(),
                ))
            }
            423 => {
                return Err(Error::BadWagoKey(
                    "Wago has blocked this API key. Get a new one from Wago.".into(),
                ))
            }
            _ => {}
        }
        Ok(check(resp).await?.json().await?)
    }

    pub async fn verify_key(&self) -> Result<()> {
        let _: serde_json::Value = self
            .get("/addons/popular", &[("game_version", GAME_VERSION)])
            .await?;
        Ok(())
    }

    pub async fn search(&self, query: &str) -> Result<Vec<RemoteAddon>> {
        let q = query.trim();
        let r: SearchResult = if q.is_empty() {
            self.get("/addons/popular", &[("game_version", GAME_VERSION)])
                .await?
        } else {
            self.get(
                "/addons/_search",
                &[
                    ("query", q),
                    ("game_version", GAME_VERSION),
                    ("stability", "stable"),
                ],
            )
            .await?
        };
        Ok(r.data.iter().map(to_remote).collect())
    }

    pub async fn resolve(&self, id_or_slug: &str) -> Result<Resolved> {
        let a: Addon = self
            .get(
                &format!("/addons/{id_or_slug}"),
                &[("game_version", GAME_VERSION)],
            )
            .await
            .map_err(|e| match e {
                Error::NotFound(_) => {
                    Error::NotFound(format!("Couldn't find {id_or_slug} on Wago."))
                }
                other => other,
            })?;
        let addon = to_remote(&a);
        let Some(rel) = a.recent_release.best() else {
            return Err(Error::NoForeverBuild(addon.name));
        };
        Ok(Resolved {
            version: clean_version(&rel.label),
            download_url: rel.download_link.clone().unwrap_or_default(),
            expected: Expected::None,
            file_id: None,
            release_tag: Some(rel.label.clone()),
            requires: vec![],
            addon,
        })
    }
}

fn to_remote(a: &Addon) -> RemoteAddon {
    let slug = a.slug.clone().unwrap_or_else(|| a.id.clone());
    RemoteAddon {
        id: format!("wago:{}", a.id),
        source: Source::Wago,
        name: a.display_name.trim().to_string(),
        author: a.authors.first(),
        summary: a.summary.clone().unwrap_or_default().trim().to_string(),
        downloads: a.download_count.unwrap_or(0.0).max(0.0) as u64,
        updated: a.recent_release.best().and_then(|r| r.created_at.clone()),
        icon_url: a
            .thumbnail_image
            .clone()
            .filter(|u| u.starts_with("https://")),
        project_url: a
            .website_url
            .clone()
            .filter(|u| u.starts_with("https://"))
            .unwrap_or_else(|| format!("https://addons.wago.io/addons/{slug}")),
        downloadable: true,
        has_forever_build: a.recent_release.best().is_some(),
        note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs() {
        assert_eq!(
            parse_ref("https://addons.wago.io/addons/bigwigs").as_deref(),
            Some("bigwigs")
        );
        assert_eq!(
            parse_ref("addons.wago.io/addons/bigwigs/versions?x=1").as_deref(),
            Some("bigwigs")
        );
        assert_eq!(parse_ref("wago:5NRegwG3").as_deref(), Some("5NRegwG3"));
        assert_eq!(parse_ref("../etc"), None);
        assert_eq!(parse_ref(""), None);
    }

    #[test]
    fn reads_details_and_search_shapes() {
        let details: Addon = serde_json::from_value(serde_json::json!({
            "id": "5NRegwG3", "slug": "bigwigs", "display_name": "BigWigs",
            "summary": "Boss mods", "authors": ["Funkeh"], "download_count": 1000,
            "website_url": "https://addons.wago.io/addons/bigwigs",
            "recent_release": {
                "alpha": { "label": "v1-alpha", "created_at": "2026-11-03T00:00:00Z", "download_link": "https://addons.wago.io/download/a" },
                "beta": { "label": "v428.0-beta", "created_at": "2026-11-02T00:00:00Z", "download_link": "https://addons.wago.io/download/b" },
                "stable": { "label": "v427.8", "created_at": "2026-11-01T00:00:00Z", "download_link": "https://addons.wago.io/download/s" }
            }
        }))
        .unwrap();
        assert_eq!(details.recent_release.best().unwrap().label, "v427.8");
        let r = to_remote(&details);
        assert_eq!(r.id, "wago:5NRegwG3");
        assert_eq!(r.author, "Funkeh");

        // Search results use `releases`, and an addon with nothing released sends `[]`.
        let empty: Addon = serde_json::from_value(serde_json::json!({
            "id": "x", "display_name": "X", "authors": "Someone", "releases": []
        }))
        .unwrap();
        assert!(empty.recent_release.best().is_none());
        assert!(!to_remote(&empty).has_forever_build);
        let beta_only: Addon = serde_json::from_value(serde_json::json!({
            "id": "y", "display_name": "Y",
            "releases": { "beta": { "label": "2.0b", "link": "https://addons.wago.io/download/y" } }
        }))
        .unwrap();
        assert_eq!(beta_only.recent_release.best().unwrap().label, "2.0b");
    }
}
