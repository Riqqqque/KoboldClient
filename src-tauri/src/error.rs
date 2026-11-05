use std::path::{Path, PathBuf};

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every error the core can produce. The `Display` text is shown to the user as is,
/// so keep it short and say what to do next where possible.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No WoW Forever folder is set yet. Pick one in Settings.")]
    NoGame,
    #[error("That folder doesn't look like a WoW Forever install.")]
    NotAGameFolder,
    #[error("Add a CurseForge API key in Settings to use CurseForge.")]
    NoCurseForgeKey,
    #[error("CurseForge didn't accept the API key. Check it in Settings.")]
    BadCurseForgeKey,
    #[error("Add a Wago API key in Settings to use Wago.")]
    NoWagoKey,
    #[error("{0}")]
    BadWagoKey(String),
    #[error("CurseForge doesn't list a Forever game version yet.")]
    NoForeverVersion,
    #[error("{name} can only be downloaded from the CurseForge website.")]
    DistributionBlocked { name: String, url: String },
    #[error("There's no Forever build of {0} yet.")]
    NoForeverBuild(String),
    #[error("Couldn't reach {host}. Check your connection and try again.")]
    Offline { host: String },
    #[error("{0}")]
    Http(String),
    #[error("The download was damaged (checksum mismatch). Nothing was changed.")]
    HashMismatch,
    #[error("The download is too large ({0} MB).")]
    TooLarge(u64),
    #[error("Blocked a download from an untrusted address: {0}")]
    UntrustedUrl(String),
    #[error("The archive is damaged or unsafe: {0}")]
    Archive(String),
    #[error("No addon was found in that archive.")]
    NoAddonInArchive,
    #[error("{0}")]
    NotFound(String),
    #[error("Another task is still running. Try again in a moment.")]
    Busy,
    #[error("Couldn't {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn io(action: &'static str, path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Error::Io {
            action,
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Error::NoGame => "no_game",
            Error::NotAGameFolder => "not_game_folder",
            Error::NoCurseForgeKey => "no_cf_key",
            Error::BadCurseForgeKey => "bad_cf_key",
            Error::NoWagoKey => "no_wago_key",
            Error::BadWagoKey(_) => "bad_wago_key",
            Error::NoForeverVersion => "no_forever_version",
            Error::DistributionBlocked { .. } => "distribution_blocked",
            Error::NoForeverBuild(_) => "no_forever_build",
            Error::Offline { .. } => "offline",
            Error::Http(_) => "http",
            Error::HashMismatch => "hash_mismatch",
            Error::TooLarge(_) => "too_large",
            Error::UntrustedUrl(_) => "untrusted_url",
            Error::Archive(_) => "archive",
            Error::NoAddonInArchive => "no_addon",
            Error::NotFound(_) => "not_found",
            Error::Busy => "busy",
            Error::Io { .. } => "io",
            Error::Other(_) => "other",
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Error", 3)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        let url = match self {
            Error::DistributionBlocked { url, .. } => Some(url.as_str()),
            _ => None,
        };
        st.serialize_field("url", &url)?;
        st.end()
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() || e.is_timeout() {
            let host = e
                .url()
                .and_then(|u| u.host_str())
                .unwrap_or("the server")
                .to_string();
            Error::Offline { host }
        } else {
            Error::Http(format!("Network error: {e}"))
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Other(format!("Unexpected data: {e}"))
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Archive(e.to_string())
    }
}
