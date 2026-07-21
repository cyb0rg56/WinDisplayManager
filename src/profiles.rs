//! Profile store: monitor-layout profiles persisted as JSON files.
//!
//! Profiles live under `%APPDATA%\MonitorSwitcher\Profiles` (matching the
//! original *MonitorSwitcher* location, i.e. `dirs::config_dir()`), one
//! `<name>.json` per profile.

use crate::ccd::{self, DisplayConfig};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum ProfileError {
    #[error("Invalid profile name")]
    InvalidName,

    #[error("Profile not found: {0}")]
    NotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Ccd(#[from] ccd::CcdError),
}

pub type Result<T> = std::result::Result<T, ProfileError>;

// ---------------------------------------------------------------------------
// Profile data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayProfile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    pub config: DisplayConfig,
}

// ---------------------------------------------------------------------------
// Paths / naming
// ---------------------------------------------------------------------------

/// Directory holding profile JSON files.
pub fn profiles_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("MonitorSwitcher").join("Profiles")
}

fn ensure_dir() -> Result<()> {
    fs::create_dir_all(profiles_dir())?;
    Ok(())
}

/// Strip characters invalid in Windows filenames and reject traversal/empty
/// names. Returns a safe file stem, or `None` if nothing usable remains.
fn sanitize_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !c.is_control()
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }
    Some(trimmed.to_string())
}

fn profile_path(name: &str) -> Result<PathBuf> {
    let safe = sanitize_name(name).ok_or(ProfileError::InvalidName)?;
    Ok(profiles_dir().join(format!("{safe}.json")))
}

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

/// List profile names (file stems of `*.json`), sorted.
pub fn list_profiles() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(profiles_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort_unstable();
    names
}

/// Save a captured configuration as a named profile.
pub fn save_profile(name: &str, config: &DisplayConfig) -> Result<()> {
    let safe = sanitize_name(name).ok_or(ProfileError::InvalidName)?;
    ensure_dir()?;
    let profile = DisplayProfile {
        name: safe.clone(),
        created: Some(now_iso8601()),
        config: config.clone(),
    };
    let json = serde_json::to_string_pretty(&profile)?;
    fs::write(profiles_dir().join(format!("{safe}.json")), json)?;
    Ok(())
}

/// Capture the current active layout and save it under `name`.
pub fn save_current(name: &str) -> Result<()> {
    let config = ccd::capture_active_config()?;
    save_profile(name, &config)
}

/// Load a profile by name.
pub fn load_profile(name: &str) -> Result<DisplayProfile> {
    let path = profile_path(name)?;
    if !path.exists() {
        return Err(ProfileError::NotFound(name.to_string()));
    }
    let contents = fs::read_to_string(&path)?;
    let profile: DisplayProfile = serde_json::from_str(&contents)?;
    Ok(profile)
}

/// Delete a profile by name.
pub fn delete_profile(name: &str) -> Result<()> {
    let path = profile_path(name)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Load and apply a profile by name.
pub fn apply_profile(name: &str) -> Result<()> {
    let profile = load_profile(name)?;
    ccd::apply_config(&profile.config)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Timestamp (dependency-free ISO-8601 UTC)
// ---------------------------------------------------------------------------

fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert days since the Unix epoch to a `(year, month, day)` civil date.
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_traversal_and_empty() {
        assert!(sanitize_name("..").is_none());
        assert!(sanitize_name("   ").is_none());
        assert!(sanitize_name("../../etc").is_some()); // slashes stripped -> "etc"
        assert_eq!(sanitize_name("../../etc").unwrap(), "etc");
        assert_eq!(sanitize_name("Home Dual").unwrap(), "Home Dual");
        assert_eq!(sanitize_name("a<b>c:d").unwrap(), "abcd");
    }

    #[test]
    fn iso8601_epoch_is_correct() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-03-01 is 11017 days after the epoch.
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }
}
