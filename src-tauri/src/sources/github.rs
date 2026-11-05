//! Addons published as GitHub Releases, usually by the BigWigs packager, which
//! uploads a `release.json` describing which zip is for which game flavor.

use serde::Deserialize;

use super::{RemoteAddon, Resolved};
use crate::error::{Error, Result};
use crate::manifest::Source;
use crate::net::{check, Expected};
use crate::toc::FOREVER_INTERFACE;

const API: &str = "https://api.github.com";

#[derive(Debug, Deserialize)]
struct Repo {
    name: String,
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    html_url: String,
    owner: Owner,
    #[serde(default)]
    pushed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Owner {
    login: String,
    avatar_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PackagerJson {
    releases: Vec<PackagerRelease>,
}

#[derive(Debug, Deserialize)]
struct PackagerRelease {
    filename: String,
    #[serde(default)]
    nolib: bool,
    #[serde(default)]
    metadata: Vec<PackagerMeta>,
}

#[derive(Debug, Deserialize)]
struct PackagerMeta {
    flavor: String,
    #[serde(default)]
    interface: u32,
}

/// Accepts `owner/repo`, `github.com/owner/repo` or any URL inside the repo.
pub fn parse_repo(input: &str) -> Option<(String, String)> {
    let s = input.trim().trim_end_matches('/');
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);
    let s = s.strip_prefix("github.com/").unwrap_or(s);
    let mut parts = s.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim().trim_end_matches(".git");
    let valid = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    (valid(owner) && valid(repo)).then(|| (owner.to_string(), repo.to_string()))
}

pub struct GitHub<'a> {
    pub http: &'a reqwest::Client,
}

impl GitHub<'_> {
    async fn get<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        let resp = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;
        Ok(check(resp).await?.json().await?)
    }

    pub async fn latest_release(&self, owner: &str, repo: &str) -> Result<Release> {
        self.get(&format!("{API}/repos/{owner}/{repo}/releases/latest"))
            .await
            .map_err(|e| match e {
                Error::NotFound(_) => Error::NotFound(format!(
                    "{owner}/{repo} has no published releases to install."
                )),
                other => other,
            })
    }

    pub async fn resolve(&self, owner: &str, repo: &str) -> Result<Resolved> {
        let info: Repo = self
            .get(&format!("{API}/repos/{owner}/{repo}"))
            .await
            .map_err(|e| match e {
                Error::NotFound(_) => Error::NotFound(format!(
                    "Couldn't find the GitHub repository {owner}/{repo}."
                )),
                other => other,
            })?;
        let release = self.latest_release(owner, repo).await?;
        let asset = self
            .pick_asset(&release)
            .await?
            .ok_or_else(|| Error::NoForeverBuild(info.name.clone()))?;
        let expected = asset
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|h| Expected::Sha256(h.to_string()))
            .unwrap_or(Expected::None);
        let addon = RemoteAddon {
            id: format!("gh:{}", info.full_name.to_ascii_lowercase()),
            source: Source::GitHub,
            name: info.name.clone(),
            author: info.owner.login.clone(),
            summary: info.description.unwrap_or_default(),
            downloads: 0,
            updated: release.published_at.clone().or(info.pushed_at),
            icon_url: Some(avatar(&info.owner.avatar_url)),
            project_url: info.html_url,
            downloadable: true,
            has_forever_build: true,
        };
        Ok(Resolved {
            addon,
            version: release.tag_name.trim_start_matches(['v', 'V']).to_string(),
            download_url: asset.browser_download_url,
            expected,
            file_id: None,
            release_tag: Some(release.tag_name),
            requires: vec![],
        })
    }

    /// Picks the Forever zip from a release: `release.json` first, then file names.
    async fn pick_asset(&self, release: &Release) -> Result<Option<Asset>> {
        let zips: Vec<&Asset> = release
            .assets
            .iter()
            .filter(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
            .collect();
        if let Some(meta) = release.assets.iter().find(|a| a.name == "release.json") {
            match self.http.get(&meta.browser_download_url).send().await {
                Ok(resp) => match check(resp).await?.json::<PackagerJson>().await {
                    Ok(pj) => {
                        if let Some(name) = pick_from_packager(&pj) {
                            if let Some(a) = zips.iter().find(|a| a.name == name) {
                                return Ok(Some((*a).clone()));
                            }
                        }
                        tracing::info!("release.json has no Forever entry");
                        return Ok(None);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "release.json unreadable, falling back to file names")
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "release.json download failed, falling back to file names")
                }
            }
        }
        Ok(pick_by_name(&zips).cloned())
    }
}

fn pick_from_packager(pj: &PackagerJson) -> Option<String> {
    let full = pj.releases.iter().filter(|r| !r.nolib);
    let forever = |r: &&PackagerRelease| {
        r.metadata.iter().any(|m| {
            let f = m.flavor.to_ascii_lowercase();
            f == "forever" || f == "camelot" || FOREVER_INTERFACE.contains(&m.interface)
        })
    };
    full.clone()
        .find(forever)
        .or_else(|| pj.releases.iter().find(forever))
        .map(|r| r.filename.clone())
}

fn pick_by_name<'a>(zips: &[&'a Asset]) -> Option<&'a Asset> {
    const OTHER: [&str; 10] = [
        "classic", "vanilla", "tbc", "bcc", "wrath", "wotlk", "cata", "mists", "mainline", "retail",
    ];
    if let Some(a) = zips
        .iter()
        .find(|a| a.name.to_ascii_lowercase().contains("forever"))
    {
        return Some(a);
    }
    let generic: Vec<&&Asset> = zips
        .iter()
        .filter(|a| {
            let n = a.name.to_ascii_lowercase();
            !n.contains("nolib") && !OTHER.iter().any(|o| n.contains(o))
        })
        .collect();
    // Only guess when there's exactly one candidate; the TOC check after install
    // flags it if it turns out not to be built for Forever.
    (generic.len() == 1).then(|| *generic[0])
}

fn avatar(url: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}s=128")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_inputs() {
        let want = Some(("Owner".into(), "Repo".into()));
        assert_eq!(parse_repo("Owner/Repo"), want);
        assert_eq!(parse_repo("https://github.com/Owner/Repo"), want);
        assert_eq!(parse_repo("github.com/Owner/Repo/releases/tag/v1"), want);
        assert_eq!(parse_repo("https://github.com/Owner/Repo.git"), want);
        assert_eq!(parse_repo("nonsense"), None);
        assert_eq!(parse_repo("a b/c"), None);
    }

    #[test]
    fn packager_picks_forever() {
        let pj: PackagerJson = serde_json::from_value(serde_json::json!({
            "releases": [
                { "filename": "A-1.zip", "nolib": false, "metadata": [{ "flavor": "mainline", "interface": 120100 }] },
                { "filename": "A-1-forever.zip", "nolib": false, "metadata": [{ "flavor": "forever", "interface": 16001 }] },
                { "filename": "A-1-nolib.zip", "nolib": true, "metadata": [{ "flavor": "forever", "interface": 16001 }] }
            ]
        }))
        .unwrap();
        assert_eq!(pick_from_packager(&pj).as_deref(), Some("A-1-forever.zip"));
    }

    #[test]
    fn name_fallback() {
        let a = |n: &str| Asset {
            name: n.into(),
            browser_download_url: String::new(),
            digest: None,
        };
        let (x, y, z) = (
            a("Addon-1.0.zip"),
            a("Addon-1.0-classic.zip"),
            a("Addon-1.0-forever.zip"),
        );
        assert_eq!(pick_by_name(&[&x, &y]).unwrap().name, "Addon-1.0.zip");
        assert_eq!(
            pick_by_name(&[&x, &y, &z]).unwrap().name,
            "Addon-1.0-forever.zip"
        );
        let w = a("Addon-1.0-other.zip");
        assert!(pick_by_name(&[&x, &w]).is_none());
    }
}
