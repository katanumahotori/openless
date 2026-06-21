#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Local persistence: history JSON, user preferences JSON, vocab JSON, and
//! platform-backed credentials vault.
//!
//! Storage roots:
//! - macOS:   `~/Library/Application Support/OpenLess`
//! - Windows: `%APPDATA%\OpenLess`
//! - Linux:   `$XDG_DATA_HOME/OpenLess` or `~/.local/share/OpenLess`
//!
//! Credential storage policy: provider credentials are stored in the OS
//! credential vault (macOS Keychain, Windows Credential Manager, Linux keyring).
//! A legacy plaintext JSON file is read once as a migration source and removed
//! after a successful vault write; new writes never persist plaintext secrets.
//!
//! This module is split into focused submodules; everything that was previously
//! reachable as `crate::persistence::*` stays reachable via the glob re-exports
//! below. The shared filesystem helpers and the two cross-cutting constants live
//! here so every submodule can reach them through `super::`.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;
use uuid::Uuid;

mod correction;
mod credentials;
mod dictionary;
mod history;
mod paths;
mod preferences;
mod style_pack;

pub use correction::*;
pub use credentials::*;
pub use dictionary::*;
pub use history::*;
pub use paths::*;
pub use preferences::*;
pub use style_pack::*;

const HISTORY_CAP: usize = 200;
const PREFERENCES_FILE: &str = "preferences.json";

#[cfg(test)]
static TEST_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

#[cfg(test)]
fn test_data_dir() -> PathBuf {
    TEST_DATA_DIR
        .get_or_init(|| {
            if let Some(dir) = std::env::var_os("OPENLESS_TEST_DATA_DIR") {
                return PathBuf::from(dir);
            }
            std::env::temp_dir().join(format!(
                "openless-test-data-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ))
        })
        .clone()
}

#[cfg(test)]
fn data_dir() -> Result<PathBuf> {
    Ok(test_data_dir().join("OpenLess"))
}

#[cfg(not(test))]
fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("OpenLess"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").context("APPDATA not set")?;
        Ok(PathBuf::from(appdata).join("OpenLess"))
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join("OpenLess"));
            }
        }
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("OpenLess"))
    }

    #[cfg(target_os = "android")]
    {
        if let Ok(dir) = std::env::var("TAURI_ANDROID_APP_DATA_DIR") {
            return Ok(PathBuf::from(dir).join("OpenLess"));
        }
        Ok(std::env::temp_dir().join("OpenLess"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn test_persistence_paths_are_isolated_from_real_user_data() {
        let dir = super::data_dir().expect("test data dir");
        assert!(dir.to_string_lossy().contains("openless-test-data-"));

        #[cfg(target_os = "windows")]
        if let Ok(appdata) = std::env::var("APPDATA") {
            assert!(!dir.starts_with(PathBuf::from(appdata).join("OpenLess")));
        }

        let prefs = super::PreferencesStore::new().expect("prefs store");
        let style_packs = super::StylePackStore::new(&prefs).expect("style pack store");
        assert!(prefs.path_for_tests().starts_with(&dir));
        assert!(style_packs.path_for_tests().starts_with(&dir));
        assert!(style_packs.asset_root_for_tests().starts_with(&dir));

        let legacy_credentials = super::credentials_path().expect("legacy creds path");
        assert!(legacy_credentials.starts_with(&dir));
    }
}

fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create dir failed: {}", dir.display()))?;
    Ok(())
}

/// Atomic write: write to a unique `*.tmp-<uuid>` first, then rename onto the
/// target path. The unique suffix lets concurrent writers each own their own
/// tmp file, so a parallel rename never finds its source already taken.
fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4().simple()));
    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tmp failed: {}", tmp_path.display()))?;
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err).with_context(|| format!("rename failed: {}", path.display()));
    }
    Ok(())
}

fn read_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice::<T>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))
}
