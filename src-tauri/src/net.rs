//! HTTP client with a host allow-list, retries, and verified streaming downloads.

use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::error::{Error, Result};

pub const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

/// Hosts Kobold will talk to. Anything else, including redirects, is refused.
const ALLOWED_HOSTS: [&str; 8] = [
    "api.curseforge.com",
    "api.github.com",
    "github.com",
    "codeload.github.com",
    "addons.wago.io",
    "cdn.wago.io",
    "api.mmoui.com",
    "cdn.wowinterface.com",
];
/// The only host a download is ever sent a key to (Wago wants it on the download link).
pub const WAGO_DOWNLOAD_HOST: &str = "addons.wago.io";
const ALLOWED_SUFFIXES: [&str; 2] = [".forgecdn.net", ".githubusercontent.com"];

pub fn is_trusted(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    ALLOWED_HOSTS.contains(&host.as_str()) || ALLOWED_SUFFIXES.iter().any(|s| host.ends_with(s))
}

pub fn client() -> reqwest::Client {
    // Same TLS backend the updater uses, so only one is compiled in.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 8 {
            attempt.error("too many redirects")
        } else if is_trusted(attempt.url()) {
            attempt.follow()
        } else {
            tracing::warn!(url = %attempt.url(), "refused redirect to untrusted host");
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .user_agent(concat!(
            "KoboldClient/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/Riqqqque/KoboldClient)"
        ))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .redirect(policy)
        .https_only(true)
        .build()
        .expect("http client")
}

/// Turns a non-success response into a readable error.
pub async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let url = resp.url().clone();
    let body = resp.text().await.unwrap_or_default();
    tracing::warn!(%url, %status, body = %body.chars().take(300).collect::<String>(), "request failed");
    Err(match status.as_u16() {
        404 => Error::NotFound("Not found on the server.".into()),
        429 => {
            Error::Http("The server is rate limiting requests. Try again in a few minutes.".into())
        }
        403 if url.host_str() == Some("api.github.com") => {
            Error::Http("GitHub's hourly request limit was reached. Try again later, or add a GitHub token in Settings to raise the limit.".into())
        }
        s if s >= 500 => Error::Http(format!(
            "{} is having trouble right now ({status}).",
            url.host_str().unwrap_or("The server")
        )),
        _ => Error::Http(format!("Request failed ({status}).")),
    })
}

fn retryable(e: &Error) -> bool {
    match e {
        Error::Offline { .. } => true,
        Error::Http(m) => m.contains("having trouble") || m.contains("Network error"),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expected {
    Sha1(String),
    Md5(String),
    Sha256(String),
    None,
}

#[derive(Debug, Clone)]
pub struct Downloaded {
    pub sha256: String,
    pub bytes: u64,
}

/// Streams `url` into `dest`, checking the expected hash. Retries transient failures.
/// `progress` receives (bytes so far, total if known).
pub async fn download(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected: &Expected,
    bearer: Option<&str>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Downloaded> {
    let parsed = Url::parse(url).map_err(|_| Error::UntrustedUrl(url.to_string()))?;
    if !is_trusted(&parsed) {
        return Err(Error::UntrustedUrl(url.to_string()));
    }
    let mut attempt = 0;
    loop {
        attempt += 1;
        match download_once(http, url, dest, expected, bearer, &mut progress).await {
            Ok(d) => return Ok(d),
            Err(e) if attempt < 3 && retryable(&e) => {
                tracing::warn!(attempt, error = %e, "download failed, retrying");
                tokio::time::sleep(Duration::from_millis(700 * attempt)).await;
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(dest).await;
                return Err(e);
            }
        }
    }
}

async fn download_once(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected: &Expected,
    bearer: Option<&str>,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<Downloaded> {
    let mut req = http.get(url);
    if let Some(token) = bearer {
        // Redirects to another host drop this header (reqwest strips it).
        if Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .as_deref()
            == Some(WAGO_DOWNLOAD_HOST)
        {
            req = req.bearer_auth(token);
        }
    }
    let resp = check(req.send().await?).await?;
    if !is_trusted(resp.url()) {
        return Err(Error::UntrustedUrl(resp.url().to_string()));
    }
    let total = resp.content_length();
    if let Some(t) = total {
        if t > MAX_DOWNLOAD {
            return Err(Error::TooLarge(t / 1024 / 1024));
        }
    }
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| Error::io("create", dir, e))?;
    }
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| Error::io("write", dest, e))?;
    let (mut s1, mut m5, mut s256) = (Sha1::new(), Md5::new(), Sha256::new());
    let mut got = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        got += chunk.len() as u64;
        if got > MAX_DOWNLOAD {
            return Err(Error::TooLarge(got / 1024 / 1024));
        }
        s1.update(&chunk);
        m5.update(&chunk);
        s256.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| Error::io("write", dest, e))?;
        progress(got, total);
    }
    file.flush()
        .await
        .map_err(|e| Error::io("write", dest, e))?;
    file.sync_all()
        .await
        .map_err(|e| Error::io("write", dest, e))?;
    drop(file);

    let sha256 = hex::encode(s256.finalize());
    let ok = match expected {
        Expected::Sha1(h) => hex::encode(s1.finalize()).eq_ignore_ascii_case(h),
        Expected::Md5(h) => hex::encode(m5.finalize()).eq_ignore_ascii_case(h),
        Expected::Sha256(h) => sha256.eq_ignore_ascii_case(h),
        Expected::None => true,
    };
    if !ok {
        tracing::error!(url, ?expected, actual_sha256 = %sha256, "checksum mismatch");
        let _ = tokio::fs::remove_file(dest).await;
        return Err(Error::HashMismatch);
    }
    tracing::info!(
        url,
        bytes = got,
        verified = !matches!(expected, Expected::None),
        "download complete"
    );
    Ok(Downloaded { sha256, bytes: got })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list() {
        let ok = |s: &str| is_trusted(&Url::parse(s).unwrap());
        assert!(ok("https://edge.forgecdn.net/files/1/2/a.zip"));
        assert!(ok("https://api.curseforge.com/v1/games"));
        assert!(ok("https://release-assets.githubusercontent.com/x"));
        assert!(!ok("http://edge.forgecdn.net/a.zip"));
        assert!(!ok("https://forgecdn.net.evil.com/a.zip"));
        assert!(!ok("https://example.com/a.zip"));
        assert!(ok(
            "https://cdn.wowinterface.com/downloads/getfile.php?id=1"
        ));
        assert!(ok("https://cdn.wago.io/public/addons/x.zip"));
        assert!(!ok("https://wago.io.evil.com/x.zip"));
    }
}
