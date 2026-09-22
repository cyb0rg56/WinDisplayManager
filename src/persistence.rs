//! Crash-resistant, same-directory writes shared by config and profile storage.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub enum LoadOutcome<T> {
    Missing,
    Loaded(T),
    Failed(io::Error),
}

pub fn load<T>(path: &Path, decode: impl FnOnce(&[u8]) -> io::Result<T>) -> LoadOutcome<T> {
    match fs::read(path) {
        Ok(bytes) => match decode(&bytes) {
            Ok(value) => LoadOutcome::Loaded(value),
            Err(error) => LoadOutcome::Failed(error),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => LoadOutcome::Missing,
        Err(error) => LoadOutcome::Failed(error),
    }
}

/// Check the version before deserializing fields whose shape may change later.
pub fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    #[derive(Deserialize)]
    struct Version {
        #[serde(default)]
        schema_version: u32,
    }
    let version: Version = serde_json::from_slice(bytes).map_err(invalid_data)?;
    if version.schema_version > SCHEMA_VERSION {
        return Err(invalid_data(format!(
            "Unsupported schema version {}; this application supports up to {SCHEMA_VERSION}",
            version.schema_version
        )));
    }
    serde_json::from_slice(bytes).map_err(invalid_data)
}

pub fn invalid_data(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    CreateNew,
    Replace,
}

/// Keep the previous *validated* file as .bak. Invalid or unreadable originals
/// are never replaced by an ordinary save, nor copied over a good backup.
pub fn save_with_backup(
    path: &Path,
    bytes: &[u8],
    mode: WriteMode,
    validate: impl FnOnce(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    save_impl(path, bytes, mode, validate, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    TempCreated,
    Written,
    Synced,
    BeforeReplace,
}

fn save_impl(
    path: &Path,
    bytes: &[u8],
    mode: WriteMode,
    validate: impl FnOnce(&[u8]) -> io::Result<()>,
    checkpoint: impl Fn(Stage) -> io::Result<()>,
) -> io::Result<()> {
    let previous = if mode == WriteMode::Replace {
        match fs::read(path) {
            Ok(bytes) => {
                validate(&bytes)?;
                Some(bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    // A file appearing after a missing-file read must not be overwritten.
    let mode = if previous.is_none() {
        WriteMode::CreateNew
    } else {
        mode
    };
    let temp = TempFile::prepare(path, bytes, &checkpoint)?;
    if let Some(previous) = previous {
        atomic_write(&backup_path(path), &previous, WriteMode::Replace)?;
    }
    checkpoint(Stage::BeforeReplace)?;
    temp.commit(path, mode)
}

/// No backup rotation: used for explicit recovery/reset, so the known-good
/// backup survives even when the primary contains malformed or newer data.
pub fn atomic_write(path: &Path, bytes: &[u8], mode: WriteMode) -> io::Result<()> {
    TempFile::prepare(path, bytes, &|_| Ok(()))?.commit(path, mode)
}

struct TempFile(PathBuf);

impl TempFile {
    fn prepare(
        path: &Path,
        bytes: &[u8],
        checkpoint: &impl Fn(Stage) -> io::Result<()>,
    ) -> io::Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let name = path
            .file_name()
            .ok_or_else(|| invalid_data("Missing file name"))?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let (temp, mut file) = loop {
            let mut temp_name = name.to_os_string();
            temp_name.push(format!(
                ".{}.{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let temp_path = parent.join(temp_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
            {
                Ok(file) => break (Self(temp_path), file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        // Close the handle before cleanup or publication (important on Windows).
        let result = (|| {
            checkpoint(Stage::TempCreated)?;
            file.write_all(bytes)?;
            checkpoint(Stage::Written)?;
            file.sync_all()?;
            checkpoint(Stage::Synced)
        })();
        drop(file);
        result?;
        Ok(temp)
    }

    fn commit(self, path: &Path, mode: WriteMode) -> io::Result<()> {
        publish(&self.0, path, mode)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        match fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "Could not clean temporary file {}: {error}",
                self.0.display()
            ),
        }
    }
}

#[cfg(windows)]
fn publish(temp: &Path, path: &Path, mode: WriteMode) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path contains NUL",
            ));
        }
        value.push(0);
        Ok(value)
    }
    let temp = wide(temp)?;
    let path = wide(path)?;
    let flags = MOVEFILE_WRITE_THROUGH
        | match mode {
            WriteMode::CreateNew => 0,
            WriteMode::Replace => MOVEFILE_REPLACE_EXISTING,
        };
    // SAFETY: Both paths are NUL-terminated UTF-16 buffers, alive for the call.
    // The temp is in the destination directory, so this cannot become a copy
    // across volumes. Omitting REPLACE_EXISTING makes creation race-safe.
    if unsafe { MoveFileExW(temp.as_ptr(), path.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn publish(temp: &Path, path: &Path, mode: WriteMode) -> io::Result<()> {
    match mode {
        WriteMode::Replace => fs::rename(temp, path)?,
        WriteMode::CreateNew => fs::hard_link(temp, path)?,
    }
    fs::File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?
    .sync_all()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub struct TestDir(pub PathBuf);

    impl TestDir {
        pub fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "windisplaymanager-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Cannot create isolated test directory: {error}"),
                }
            }
        }

        pub fn assert_no_temps(&self) {
            assert!(fs::read_dir(&self.0).unwrap().all(|entry| {
                entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_none_or(|ext| ext != "tmp")
            }));
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn injected_failures_preserve_original_and_clean_temps() {
        for failed_stage in [
            Stage::TempCreated,
            Stage::Written,
            Stage::Synced,
            Stage::BeforeReplace,
        ] {
            let dir = TestDir::new();
            let path = dir.0.join("config.json");
            fs::write(&path, b"old").unwrap();
            let result = save_impl(
                &path,
                b"new",
                WriteMode::Replace,
                |_| Ok(()),
                |stage| {
                    if stage == failed_stage {
                        Err(io::Error::other("injected failure"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(fs::read(&path).unwrap(), b"old");
            dir.assert_no_temps();
        }
    }

    #[test]
    fn concurrent_creation_is_never_overwritten() {
        for mode in [WriteMode::CreateNew, WriteMode::Replace] {
            let dir = TestDir::new();
            let path = dir.0.join("profile.json");
            let result = save_impl(
                &path,
                b"new",
                mode,
                |_| Ok(()),
                |stage| {
                    if stage == Stage::BeforeReplace {
                        fs::write(&path, b"another writer")?;
                    }
                    Ok(())
                },
            );
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(&path).unwrap(), b"another writer");
            assert!(!backup_path(&path).exists());
            dir.assert_no_temps();
        }
    }

    #[test]
    fn backup_failure_prevents_replacement() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        fs::write(&path, b"old").unwrap();
        fs::create_dir(backup_path(&path)).unwrap();
        assert!(save_with_backup(&path, b"new", WriteMode::Replace, |_| Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        dir.assert_no_temps();
    }

    #[test]
    fn replace_keeps_last_good_backup_and_rejects_invalid_original() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        atomic_write(&path, b"old", WriteMode::CreateNew).unwrap();
        save_with_backup(&path, b"new", WriteMode::Replace, |_| Ok(())).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"old");
        fs::write(&path, b"broken").unwrap();
        assert!(
            save_with_backup(&path, b"new", WriteMode::Replace, |_| Err(invalid_data(
                "bad"
            )))
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"old");
        dir.assert_no_temps();
    }

    #[cfg(windows)]
    #[test]
    fn windows_sharing_violation_preserves_original_and_cleans_temp() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        fs::write(&path, b"old").unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(atomic_write(&path, b"new", WriteMode::Replace).is_err());
        drop(locked);
        assert_eq!(fs::read(&path).unwrap(), b"old");
        dir.assert_no_temps();
    }
}
