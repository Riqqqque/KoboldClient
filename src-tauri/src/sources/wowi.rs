//! WoWInterface (https://www.wowinterface.com), through its public API at api.mmoui.com.
//!
//! No key is needed. WoWInterface doesn't record which addons support Forever, so
//! search can't filter for it; the TOC check after install labels addons that aren't
//! built for Forever. Files carry an MD5, which every download is checked against.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer};
use tokio::sync::Mutex;

use super::{RemoteAddon, Resolved};
use crate::error::{Error, Result};
use crate::manifest::Source;
use crate::net::{check, Expected};
use crate::util::clean_version;

const API: &str = "https://api.mmoui.com/v3/game/WOW";
/// The full file list is about 700 KB; it's fetched at most this often.
const LIST_TTL: Duration = Duration::from_secs(30 * 60);
pub const NOTE: &str =
    "WoWInterface doesn't say which addons support Forever. Kobold checks after installing.";

/// WoWInterface sends some numbers as strings.
fn num<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0).max(0.0) as u64,
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct ListEntry {
    #[serde(deserialize_with = "num")]
    UID: u64,
    #[serde(default, deserialize_with = "num")]
    UIDate: u64,
    #[serde(default)]
    UIName: String,
    #[serde(default)]
    UIAuthorName: String,
    #[serde(default)]
    UIFileInfoURL: Option<String>,
    #[serde(default, deserialize_with = "num")]
    UIDownloadTotal: u64,
    #[serde(default)]
    UIIMG_Thumbs: Option<Vec<String>>,
    #[serde(default)]
    UIDir: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct Details {
    #[serde(deserialize_with = "num")]
    UID: u64,
    #[serde(default)]
    UIVersion: String,
    #[serde(default, deserialize_with = "num")]
    UIDate: u64,
    #[serde(default)]
    UIMD5: Option<String>,
    #[serde(default)]
    UIDownload: Option<String>,
    #[serde(default)]
    UIName: String,
    #[serde(default)]
    UIAuthorName: String,
    #[serde(default)]
    UIDescription: String,
}

/// Shared cache of the file list, kept by the caller across requests.
#[derive(Default)]
pub struct ListCache(Mutex<Option<(Instant, Arc<Vec<ListEntry>>)>>);

/// Accepts a WoWInterface addon URL (`.../downloads/info12345-Name.html`,
/// `fileinfo.php?id=12345`), `wowi:12345` or a bare number.
pub fn parse_ref(input: &str) -> Option<u64> {
    let s = input.trim();
    let s = s.strip_prefix("wowi:").unwrap_or(s);
    if let Ok(n) = s.parse::<u64>() {
        return (n > 0).then_some(n);
    }
    let lower = s.to_ascii_lowercase();
    if !lower.contains("wowinterface.com") {
        return None;
    }
    let digits_after = |marker: &str| -> Option<u64> {
        let at = lower.find(marker)? + marker.len();
        let d: String = lower[at..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        d.parse().ok().filter(|n| *n > 0)
    };
    digits_after("/info")
        .or_else(|| digits_after("?id="))
        .or_else(|| digits_after("&id="))
}

pub struct WowInterface<'a> {
    pub http: &'a reqwest::Client,
    pub cache: &'a ListCache,
}

impl WowInterface<'_> {
    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .get(format!("{API}{path}"))
            .header("Accept", "application/json")
            .send()
            .await?;
        Ok(check(resp).await?.json().await?)
    }

    async fn list(&self) -> Result<Arc<Vec<ListEntry>>> {
        let mut c = self.cache.0.lock().await;
        if let Some((at, list)) = c.as_ref() {
            if at.elapsed() < LIST_TTL {
                return Ok(list.clone());
            }
        }
        let list: Vec<ListEntry> = self.get("/filelist.json").await?;
        tracing::info!(count = list.len(), "WoWInterface file list loaded");
        let list = Arc::new(list);
        *c = Some((Instant::now(), list.clone()));
        Ok(list)
    }

    /// Matches every word of the query against addon names, most downloaded first.
    /// Addons with "Forever" in the name come first, since that's the only hint
    /// WoWInterface gives.
    pub async fn search(&self, query: &str) -> Result<Vec<RemoteAddon>> {
        if let Some(id) = parse_ref(query) {
            return Ok(vec![self.resolve(id).await?.addon]);
        }
        let list = self.list().await?;
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
        let mut hits: Vec<&ListEntry> = list
            .iter()
            .filter(|e| {
                let n = e.UIName.to_lowercase();
                words.iter().all(|w| n.contains(w.as_str()))
            })
            .collect();
        let forever = |e: &ListEntry| {
            let n = e.UIName.to_lowercase();
            !(n.contains("forever") || n.contains("camelot"))
        };
        hits.sort_by(|a, b| {
            forever(a)
                .cmp(&forever(b))
                .then(b.UIDownloadTotal.cmp(&a.UIDownloadTotal))
        });
        Ok(hits.into_iter().take(40).map(list_to_remote).collect())
    }

    async fn details(&self, ids: &[u64]) -> Result<Vec<Details>> {
        let joined: Vec<String> = ids.iter().map(u64::to_string).collect();
        self.get(&format!("/filedetails/{}.json", joined.join(",")))
            .await
    }

    pub async fn resolve(&self, id: u64) -> Result<Resolved> {
        let d = self
            .details(&[id])
            .await
            .map_err(|e| match e {
                Error::NotFound(_) => {
                    Error::NotFound(format!("WoWInterface has no addon with id {id}."))
                }
                other => other,
            })?
            .into_iter()
            .find(|d| d.UID == id)
            .ok_or_else(|| Error::NotFound(format!("WoWInterface has no addon with id {id}.")))?;
        // The list has the picture and page link; use it if it's already loaded.
        let cached = self
            .cache
            .0
            .lock()
            .await
            .as_ref()
            .and_then(|(_, l)| l.iter().find(|e| e.UID == id).cloned());
        resolved(d, cached.as_ref())
    }

    /// Current version stamps (`UIDate`) for several addons at once.
    pub async fn versions(&self, ids: &[u64]) -> Result<Vec<(u64, String)>> {
        let mut out = vec![];
        for chunk in ids.chunks(50) {
            for d in self.details(chunk).await? {
                out.push((d.UID, d.UIDate.to_string()));
            }
        }
        Ok(out)
    }
}

fn page_url(id: u64) -> String {
    format!("https://www.wowinterface.com/downloads/info{id}")
}

fn list_to_remote(e: &ListEntry) -> RemoteAddon {
    RemoteAddon {
        id: format!("wowi:{}", e.UID),
        source: Source::WowInterface,
        name: e.UIName.trim().to_string(),
        author: e.UIAuthorName.trim().to_string(),
        summary: e
            .UIDir
            .as_ref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("Folders: {}", d.join(", ")))
            .unwrap_or_default(),
        downloads: e.UIDownloadTotal,
        updated: iso_from_ms(e.UIDate),
        icon_url: e
            .UIIMG_Thumbs
            .as_ref()
            .and_then(|t| t.first().cloned())
            .filter(|u| u.starts_with("https://")),
        project_url: e
            .UIFileInfoURL
            .clone()
            .filter(|u| u.starts_with("https://"))
            .unwrap_or_else(|| page_url(e.UID)),
        downloadable: true,
        has_forever_build: true,
        note: Some(NOTE.into()),
    }
}

fn resolved(d: Details, listed: Option<&ListEntry>) -> Result<Resolved> {
    let url = d
        .UIDownload
        .clone()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| Error::NotFound(format!("{} has no download yet.", d.UIName)))?;
    let expected = d
        .UIMD5
        .as_deref()
        .map(str::trim)
        .filter(|h| h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|h| Expected::Md5(h.to_string()))
        .unwrap_or(Expected::None);
    let mut addon = match listed {
        Some(e) => list_to_remote(e),
        None => RemoteAddon {
            id: format!("wowi:{}", d.UID),
            source: Source::WowInterface,
            name: d.UIName.trim().to_string(),
            author: d.UIAuthorName.trim().to_string(),
            summary: String::new(),
            downloads: 0,
            updated: iso_from_ms(d.UIDate),
            icon_url: None,
            project_url: page_url(d.UID),
            downloadable: true,
            has_forever_build: true,
            note: Some(NOTE.into()),
        },
    };
    if addon.summary.is_empty() {
        addon.summary = first_sentence(&d.UIDescription);
    }
    Ok(Resolved {
        addon,
        version: clean_version(&d.UIVersion),
        download_url: url,
        expected,
        file_id: None,
        release_tag: Some(d.UIDate.to_string()),
        requires: vec![],
    })
}

/// The description is BBCode; keep a short plain first line for the list.
fn first_sentence(bbcode: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in bbcode.chars() {
        match c {
            '[' => in_tag = true,
            ']' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| l.len() > 20)
        .unwrap_or("")
        .to_string();
    line.chars().take(200).collect()
}

/// `UIDate` is milliseconds since 1970; the UI wants an ISO date.
fn iso_from_ms(ms: u64) -> Option<String> {
    if ms == 0 {
        return None;
    }
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs() {
        assert_eq!(
            parse_ref("https://www.wowinterface.com/downloads/info3826-CTMod.html"),
            Some(3826)
        );
        assert_eq!(
            parse_ref("https://www.wowinterface.com/downloads/fileinfo.php?id=3826"),
            Some(3826)
        );
        assert_eq!(parse_ref("wowi:3826"), Some(3826));
        assert_eq!(parse_ref("3826"), Some(3826));
        assert_eq!(parse_ref("bagnon"), None);
        assert_eq!(parse_ref("https://example.com/info12"), None);
    }

    #[test]
    fn reads_api_shapes() {
        let e: ListEntry = serde_json::from_value(serde_json::json!({
            "UID": "3826", "UIVersion": "11.0.2.3", "UIDate": 1724089877000u64, "UIName": "CTMod",
            "UIAuthorName": "DahkCeles", "UIFileInfoURL": "https://www.wowinterface.com/downloads/info3826-CTMod.html",
            "UIDownloadTotal": "598625", "UICompatibility": null, "UIDir": ["CT_Core"],
            "UIIMG_Thumbs": ["https://cdn-wow.mmoui.com/preview/tiny/pvw29.jpg"]
        }))
        .unwrap();
        let r = list_to_remote(&e);
        assert_eq!(r.id, "wowi:3826");
        assert_eq!(r.downloads, 598_625);
        assert_eq!(r.updated.as_deref(), Some("2024-08-19T17:51:17Z"));

        let d: Details = serde_json::from_value(serde_json::json!({
            "UID": "3826", "UIVersion": "v11.0.2.3", "UIDate": 1724089877000u64,
            "UIMD5": "801d2daf73ad195c02c58d50be7f1b22",
            "UIDownload": "https://cdn.wowinterface.com/downloads/getfile.php?id=3826&d=1724089877&minion",
            "UIName": "CTMod", "UIAuthorName": "DahkCeles", "UIDescription": "[B]CTMod[/B]\r\nOne of the longest-running collections of addons."
        }))
        .unwrap();
        let res = resolved(d, None).unwrap();
        assert_eq!(res.version, "11.0.2.3");
        assert_eq!(res.release_tag.as_deref(), Some("1724089877000"));
        assert!(matches!(res.expected, Expected::Md5(_)));
        assert!(res.addon.summary.starts_with("One of the longest"));
    }
}
