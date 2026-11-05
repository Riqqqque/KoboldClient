//! Stores the CurseForge API key encrypted for the current Windows user (DPAPI),
//! so it never sits on disk as plain text.

use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::util::write_atomic;

const CF_KEY: &str = "curseforge.key.bin";
const GH_TOKEN: &str = "github.token.bin";
const WAGO_KEY: &str = "wago.key.bin";

fn save(data_dir: &Path, file: &str, value: &str) -> Result<()> {
    let blob = protect(value.as_bytes())?;
    write_atomic(&data_dir.join(file), &blob)
}

fn load(data_dir: &Path, file: &str) -> Option<String> {
    let blob = fs::read(data_dir.join(file)).ok()?;
    match unprotect(&blob) {
        Ok(bytes) => String::from_utf8(bytes).ok().filter(|s| !s.is_empty()),
        Err(e) => {
            tracing::warn!(error = %e, file, "stored secret couldn't be decrypted");
            None
        }
    }
}

fn clear(data_dir: &Path, file: &str) {
    let _ = fs::remove_file(data_dir.join(file));
}

pub fn save_cf_key(data_dir: &Path, key: &str) -> Result<()> {
    save(data_dir, CF_KEY, key)
}

pub fn load_cf_key(data_dir: &Path) -> Option<String> {
    load(data_dir, CF_KEY)
}

pub fn clear_cf_key(data_dir: &Path) {
    clear(data_dir, CF_KEY)
}

pub fn save_gh_token(data_dir: &Path, token: &str) -> Result<()> {
    save(data_dir, GH_TOKEN, token)
}

pub fn load_gh_token(data_dir: &Path) -> Option<String> {
    load(data_dir, GH_TOKEN)
}

pub fn clear_gh_token(data_dir: &Path) {
    clear(data_dir, GH_TOKEN)
}

pub fn save_wago_key(data_dir: &Path, key: &str) -> Result<()> {
    save(data_dir, WAGO_KEY, key)
}

pub fn load_wago_key(data_dir: &Path) -> Option<String> {
    load(data_dir, WAGO_KEY)
}

pub fn clear_wago_key(data_dir: &Path) {
    clear(data_dir, WAGO_KEY)
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi(data, true)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi(data, false)
}

#[cfg(windows)]
fn dpapi(data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(Error::Other("Windows couldn't protect the API key".into()));
    }
    let out = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(out)
}

#[cfg(not(windows))]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        super::save_cf_key(dir.path(), "abc$123").unwrap();
        let raw = std::fs::read(dir.path().join("curseforge.key.bin")).unwrap();
        assert!(!raw.windows(7).any(|w| w == b"abc$123"));
        assert_eq!(super::load_cf_key(dir.path()).as_deref(), Some("abc$123"));
        assert!(super::load_gh_token(dir.path()).is_none());
        super::save_gh_token(dir.path(), "ghp_x").unwrap();
        assert_eq!(super::load_gh_token(dir.path()).as_deref(), Some("ghp_x"));
        super::clear_gh_token(dir.path());
        assert!(super::load_gh_token(dir.path()).is_none());
    }
}
