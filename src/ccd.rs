//! CCD (Connecting and Configuring Displays) backend for saving and restoring
//! full monitor-layout profiles.
//!
//! Mirrors the structure of [`crate::ddc`]: a [`thiserror`] error enum, plain
//! serde data structs, and blocking functions. The application runs these via
//! `tokio::task::spawn_blocking` wrapped in `cosmic::app::Task::perform`.
//!
//! Adapter LUIDs and endpoint IDs are not persistent monitor identities. Saved
//! configurations are conservatively remapped by monitor device path before
//! Windows validates and applies them.

mod remap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_0, DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_RATIONAL, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ALL_PATHS, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig, SDC_APPLY, SDC_SAVE_TO_DATABASE,
    SDC_USE_SUPPLIED_DISPLAY_CONFIG, SDC_VALIDATE, SetDisplayConfig,
};
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, LUID};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, PostMessageW, SC_MONITORPOWER, WM_SYSCOMMAND,
};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum CcdError {
    #[error("Failed to query display buffer sizes (Win32 error {0})")]
    BufferSizes(u32),

    #[error("Failed to query display configuration (Win32 error {0})")]
    Query(u32),

    #[error(
        "Display configuration query still reported insufficient buffers after {attempts} attempts"
    )]
    QueryRetryExhausted { attempts: usize },

    #[error("Failed to apply display configuration (Win32 error {0})")]
    Apply(i32),

    #[error("Profile contains no display paths")]
    Empty,

    #[error(
        "Invalid display profile: {0}. Arrange the displays in Windows and recapture the profile"
    )]
    InvalidConfig(String),

    #[error("Cannot safely restore display profile: {0}")]
    Remap(String),

    #[error(
        "Windows rejected the restored layout (Win32 error {0}); check connected displays and supported modes, then recapture the profile if necessary"
    )]
    Validate(i32),
}

pub type Result<T> = std::result::Result<T, CcdError>;

// ---------------------------------------------------------------------------
// Serde mirror types
//
// These mirror the Win32 `DISPLAYCONFIG_*` arrays. The mode union payload is
// stored verbatim as opaque bytes. Remapping changes only endpoint IDs and
// associations, preserving resolution, position and timing payloads. Unsupported
// virtual-mode/desktop-image layouts remain readable but are rejected on apply.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Luid {
    pub low: u32,
    pub high: i32,
}

impl From<LUID> for Luid {
    fn from(l: LUID) -> Self {
        Luid {
            low: l.LowPart,
            high: l.HighPart,
        }
    }
}

impl From<Luid> for LUID {
    fn from(l: Luid) -> Self {
        LUID {
            LowPart: l.low,
            HighPart: l.high,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rational {
    pub num: u32,
    pub den: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PathSourceInfo {
    pub adapter_id: Luid,
    pub id: u32,
    pub mode_info_idx: u32,
    pub status_flags: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PathTargetInfo {
    pub adapter_id: Luid,
    pub id: u32,
    pub mode_info_idx: u32,
    pub output_technology: i32,
    pub rotation: i32,
    pub scaling: i32,
    pub refresh_rate: Rational,
    pub scan_line_ordering: i32,
    /// Win32 `BOOL` (`i32`).
    pub target_available: i32,
    pub status_flags: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PathInfo {
    pub source: PathSourceInfo,
    pub target: PathTargetInfo,
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    /// Win32 `DISPLAYCONFIG_MODE_INFO_TYPE` (1 = source, 2 = target, 3 = desktop image).
    pub info_type: i32,
    pub id: u32,
    pub adapter_id: Luid,
    /// Raw bytes of the `DISPLAYCONFIG_MODE_INFO` union, preserved verbatim.
    pub payload: Vec<u8>,
}

/// Additional per-monitor EDID info, parallel-indexed to [`DisplayConfig::modes`].
/// Source-mode slots hold an invalid (`valid == false`) entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub valid: bool,
    #[serde(default)]
    pub manufacture_id: u16,
    #[serde(default)]
    pub product_code_id: u16,
    #[serde(default)]
    pub friendly_name: String,
    #[serde(default)]
    pub device_path: String,
}

/// A full, serialisable capture of the desktop display topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub paths: Vec<PathInfo>,
    pub modes: Vec<ModeInfo>,
    /// Parallel-indexed to `modes`.
    pub monitors: Vec<MonitorInfo>,
    /// Target identities parallel-indexed to `paths`, even without target modes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_monitors: Vec<MonitorInfo>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Capture the currently active display configuration.
pub fn capture_active_config() -> Result<DisplayConfig> {
    query(QDC_ONLY_ACTIVE_PATHS)
}

/// Apply only after identity remapping, structural checks and Windows validation.
pub fn apply_config(saved: &DisplayConfig) -> Result<()> {
    remap::validate_layout(&saved.paths, &saved.modes)?;
    let live = query(QDC_ALL_PATHS)?;
    apply_config_with(saved, &live, set_display_config)
}

// Injection is limited to the final API boundary so tests never change displays.
fn apply_config_with(
    saved: &DisplayConfig,
    live: &DisplayConfig,
    mut set: impl FnMut(&[DISPLAYCONFIG_PATH_INFO], &[DISPLAYCONFIG_MODE_INFO], u32) -> i32,
) -> Result<()> {
    let config = remap::remap_config(saved, live)?;
    remap::validate_layout(&config.paths, &config.modes)?;
    let paths: Vec<_> = config.paths.iter().map(path_to_raw).collect();
    let modes = config
        .modes
        .iter()
        .map(mode_to_raw)
        .collect::<Result<Vec<_>>>()?;

    // ALLOW_CHANGES would let Windows silently change the saved layout/modes.
    // SAVE_TO_DATABASE is legal only on the APPLY call, not on VALIDATE.
    let err = set(
        &paths,
        &modes,
        SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_VALIDATE,
    );
    if err != 0 {
        return Err(CcdError::Validate(err));
    }
    let err = set(
        &paths,
        &modes,
        SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_APPLY | SDC_SAVE_TO_DATABASE,
    );
    if err != 0 {
        return Err(CcdError::Apply(err));
    }
    Ok(())
}

/// Broadcast the system "monitor off" power command.
///
/// Best-effort: on modern Windows the monitors may re-wake on the next mouse or
/// keyboard input. A short delay lets the tray menu close and keys settle first
/// (mirrors the original MonitorSwitcher behaviour).
pub fn turn_off_monitors() {
    std::thread::sleep(std::time::Duration::from_millis(500));
    // SAFETY: PostMessageW with a broadcast handle and plain integer parameters.
    unsafe {
        let _ = PostMessageW(
            HWND_BROADCAST,
            WM_SYSCOMMAND,
            SC_MONITORPOWER as usize,
            2isize,
        );
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

const QUERY_MAX_ATTEMPTS: usize = 3;

fn query_with_retry(
    mut get_sizes: impl FnMut(&mut u32, &mut u32) -> u32,
    mut query_config: impl FnMut(
        &mut u32,
        &mut [DISPLAYCONFIG_PATH_INFO],
        &mut u32,
        &mut [DISPLAYCONFIG_MODE_INFO],
    ) -> u32,
) -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
    for _ in 0..QUERY_MAX_ATTEMPTS {
        let mut num_paths = 0;
        let mut num_modes = 0;
        let err = get_sizes(&mut num_paths, &mut num_modes);
        if err != 0 {
            return Err(CcdError::BufferSizes(err));
        }

        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); num_modes as usize];
        let err = query_config(&mut num_paths, &mut paths, &mut num_modes, &mut modes);
        if err == ERROR_INSUFFICIENT_BUFFER {
            // Topology can grow between calls; discard all counts and buffers.
            continue;
        }
        if err != 0 {
            return Err(CcdError::Query(err));
        }
        paths.truncate(num_paths as usize);
        modes.truncate(num_modes as usize);
        return Ok((paths, modes));
    }

    Err(CcdError::QueryRetryExhausted {
        attempts: QUERY_MAX_ATTEMPTS,
    })
}

fn query_raw(flags: u32) -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
    query_with_retry(
        // SAFETY: both output counts point to initialized, writable values.
        |num_paths, num_modes| unsafe { GetDisplayConfigBufferSizes(flags, num_paths, num_modes) },
        // SAFETY: the wrapper allocates initialized buffers matching the input
        // counts on each attempt. No topology ID is requested for these flags.
        |num_paths, paths, num_modes, modes| unsafe {
            QueryDisplayConfig(
                flags,
                num_paths,
                paths.as_mut_ptr(),
                num_modes,
                modes.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        },
    )
}

/// An active CCD endpoint, not an enumeration position or a persistent LUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveTarget {
    pub source_adapter: Luid,
    pub source_id: u32,
    pub target_adapter: Luid,
    pub target_id: u32,
    pub gdi_name: String,
    pub device_path: String,
    pub friendly_name: String,
}

pub(crate) fn active_targets() -> Result<Vec<ActiveTarget>> {
    let (paths, _) = query_raw(QDC_ONLY_ACTIVE_PATHS)?;
    let mut targets = Vec::new();
    for path in paths {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
        source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        source.header.adapterId = path.sourceInfo.adapterId;
        source.header.id = path.sourceInfo.id;
        // SAFETY: a correctly sized source-name request with its header first.
        let err = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) };
        if err != 0 {
            return Err(CcdError::Query(err as u32));
        }
        let target = get_monitor_additional_info(path.targetInfo.adapterId, path.targetInfo.id);
        // Keep failed/empty target names in the association set. Dropping them
        // could make a cloned source look falsely one-to-one.
        let record = ActiveTarget {
            source_adapter: path.sourceInfo.adapterId.into(),
            source_id: path.sourceInfo.id,
            target_adapter: path.targetInfo.adapterId.into(),
            target_id: path.targetInfo.id,
            gdi_name: wide_to_string(&source.viewGdiDeviceName).to_ascii_lowercase(),
            device_path: target.device_path,
            friendly_name: target.friendly_name,
        };
        if !targets.contains(&record) {
            targets.push(record);
        }
    }
    Ok(targets)
}

/// Query the display configuration for the given flags, returning mirror types.
fn query(flags: u32) -> Result<DisplayConfig> {
    let (raw_paths, raw_modes) = query_raw(flags)?;
    Ok(config_from_raw(
        &raw_paths,
        &raw_modes,
        get_monitor_additional_info,
    ))
}

fn config_from_raw(
    raw_paths: &[DISPLAYCONFIG_PATH_INFO],
    raw_modes: &[DISPLAYCONFIG_MODE_INFO],
    mut monitor_info: impl FnMut(LUID, u32) -> MonitorInfo,
) -> DisplayConfig {
    let mut targets = std::collections::HashMap::new();
    let path_monitors = raw_paths
        .iter()
        .map(|path| {
            let target = &path.targetInfo;
            // QDC_ALL_PATHS includes inactive connected targets with no modes and
            // multiple possible sources. Query each adapter-qualified target once.
            targets
                .entry((Luid::from(target.adapterId), target.id))
                .or_insert_with(|| monitor_info(target.adapterId, target.id))
                .clone()
        })
        .collect();
    let monitors = raw_modes
        .iter()
        .map(|mode| {
            if mode.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
                targets
                    .get(&(Luid::from(mode.adapterId), mode.id))
                    .cloned()
                    .unwrap_or_default()
            } else {
                MonitorInfo::default()
            }
        })
        .collect();
    DisplayConfig {
        paths: raw_paths.iter().map(path_to_mirror).collect(),
        modes: raw_modes.iter().map(mode_to_mirror).collect(),
        monitors,
        path_monitors,
    }
}

/// Retrieve identity/EDID info for a target endpoint, with or without a mode.
fn get_monitor_additional_info(adapter_id: LUID, target_id: u32) -> MonitorInfo {
    // SAFETY: `header` is the first field of the struct; the OS uses `size` to
    // fill the remainder.
    unsafe {
        let mut name: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
        name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        name.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        name.header.adapterId = adapter_id;
        name.header.id = target_id;
        let err = DisplayConfigGetDeviceInfo(&mut name.header);
        if err != 0 {
            return MonitorInfo::default();
        }
        MonitorInfo {
            valid: true,
            manufacture_id: name.edidManufactureId,
            product_code_id: name.edidProductCodeId,
            friendly_name: wide_to_string(&name.monitorFriendlyDeviceName),
            device_path: wide_to_string(&name.monitorDevicePath),
        }
    }
}

/// Call only with arrays structurally checked by `apply_config_with`.
fn set_display_config(
    paths: &[DISPLAYCONFIG_PATH_INFO],
    modes: &[DISPLAYCONFIG_MODE_INFO],
    flags: u32,
) -> i32 {
    // SAFETY: pointers refer to validated arrays, with lengths checked to fit u32.
    unsafe {
        SetDisplayConfig(
            paths.len() as u32,
            paths.as_ptr(),
            modes.len() as u32,
            modes.as_ptr(),
            flags,
        )
    }
}

fn path_to_mirror(p: &DISPLAYCONFIG_PATH_INFO) -> PathInfo {
    PathInfo {
        source: PathSourceInfo {
            adapter_id: p.sourceInfo.adapterId.into(),
            id: p.sourceInfo.id,
            // SAFETY: reading the `modeInfoIdx` member of the modeInfoIdx union.
            mode_info_idx: unsafe { p.sourceInfo.Anonymous.modeInfoIdx },
            status_flags: p.sourceInfo.statusFlags,
        },
        target: PathTargetInfo {
            adapter_id: p.targetInfo.adapterId.into(),
            id: p.targetInfo.id,
            // SAFETY: reading the `modeInfoIdx` member of the modeInfoIdx union.
            mode_info_idx: unsafe { p.targetInfo.Anonymous.modeInfoIdx },
            output_technology: p.targetInfo.outputTechnology,
            rotation: p.targetInfo.rotation,
            scaling: p.targetInfo.scaling,
            refresh_rate: Rational {
                num: p.targetInfo.refreshRate.Numerator,
                den: p.targetInfo.refreshRate.Denominator,
            },
            scan_line_ordering: p.targetInfo.scanLineOrdering,
            target_available: p.targetInfo.targetAvailable,
            status_flags: p.targetInfo.statusFlags,
        },
        flags: p.flags,
    }
}

fn path_to_raw(p: &PathInfo) -> DISPLAYCONFIG_PATH_INFO {
    let mut raw = DISPLAYCONFIG_PATH_INFO::default();
    raw.sourceInfo.adapterId = p.source.adapter_id.into();
    raw.sourceInfo.id = p.source.id;
    raw.sourceInfo.Anonymous.modeInfoIdx = p.source.mode_info_idx;
    raw.sourceInfo.statusFlags = p.source.status_flags;
    raw.targetInfo.adapterId = p.target.adapter_id.into();
    raw.targetInfo.id = p.target.id;
    raw.targetInfo.Anonymous.modeInfoIdx = p.target.mode_info_idx;
    raw.targetInfo.outputTechnology = p.target.output_technology;
    raw.targetInfo.rotation = p.target.rotation;
    raw.targetInfo.scaling = p.target.scaling;
    raw.targetInfo.refreshRate = DISPLAYCONFIG_RATIONAL {
        Numerator: p.target.refresh_rate.num,
        Denominator: p.target.refresh_rate.den,
    };
    raw.targetInfo.scanLineOrdering = p.target.scan_line_ordering;
    raw.targetInfo.targetAvailable = p.target.target_available;
    raw.targetInfo.statusFlags = p.target.status_flags;
    raw.flags = p.flags;
    raw
}

fn mode_to_mirror(m: &DISPLAYCONFIG_MODE_INFO) -> ModeInfo {
    // SAFETY: copying the raw bytes of the union out as an opaque blob.
    let payload = unsafe {
        let ptr = &m.Anonymous as *const DISPLAYCONFIG_MODE_INFO_0 as *const u8;
        std::slice::from_raw_parts(ptr, std::mem::size_of::<DISPLAYCONFIG_MODE_INFO_0>()).to_vec()
    };
    ModeInfo {
        info_type: m.infoType,
        id: m.id,
        adapter_id: m.adapterId.into(),
        payload,
    }
}

fn mode_to_raw(m: &ModeInfo) -> Result<DISPLAYCONFIG_MODE_INFO> {
    remap::validate_mode(m)?;
    let mut raw = DISPLAYCONFIG_MODE_INFO {
        infoType: m.info_type,
        id: m.id,
        adapterId: m.adapter_id.into(),
        // SAFETY: zero-initialised union, overwritten with the saved payload below.
        Anonymous: unsafe { std::mem::zeroed() },
    };
    let n = std::mem::size_of::<DISPLAYCONFIG_MODE_INFO_0>();
    // SAFETY: validation requires an exact-sized payload; `dst` is that union.
    unsafe {
        let dst = &mut raw.Anonymous as *mut DISPLAYCONFIG_MODE_INFO_0 as *mut u8;
        std::ptr::copy_nonoverlapping(m.payload.as_ptr(), dst, n);
    }
    Ok(raw)
}

/// Convert a NUL-terminated wide string (fixed array) to a `String`.
fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER};

    #[test]
    fn query_retries_with_fresh_sizes_counts_and_buffers() {
        let size_calls = Cell::new(0);
        let query_calls = Cell::new(0);
        let (paths, modes) = query_with_retry(
            |num_paths, num_modes| {
                assert_eq!(size_calls.get(), query_calls.get());
                assert_eq!((*num_paths, *num_modes), (0, 0));
                size_calls.set(size_calls.get() + 1);
                (*num_paths, *num_modes) = match size_calls.get() {
                    1 => (1, 2),
                    2 => (3, 4),
                    _ => panic!("unexpected sizing retry"),
                };
                0
            },
            |num_paths, paths, num_modes, modes| {
                query_calls.set(query_calls.get() + 1);
                assert_eq!(size_calls.get(), query_calls.get());
                assert!(paths.iter().all(|path| path.flags == 0));
                assert!(modes.iter().all(|mode| mode.id == 0));
                match query_calls.get() {
                    1 => {
                        assert_eq!((*num_paths, *num_modes), (1, 2));
                        assert_eq!((paths.len(), modes.len()), (1, 2));
                        paths[0].flags = 99;
                        modes[0].id = 99;
                        (*num_paths, *num_modes) = (99, 99);
                        ERROR_INSUFFICIENT_BUFFER
                    }
                    2 => {
                        assert_eq!((*num_paths, *num_modes), (3, 4));
                        assert_eq!((paths.len(), modes.len()), (3, 4));
                        paths[0].flags = 7;
                        modes[0].id = 8;
                        (*num_paths, *num_modes) = (1, 1);
                        0
                    }
                    _ => panic!("unexpected query retry"),
                }
            },
        )
        .unwrap();

        assert_eq!((size_calls.get(), query_calls.get()), (2, 2));
        assert_eq!((paths.len(), modes.len()), (1, 1));
        assert_eq!(paths[0].flags, 7);
        assert_eq!(modes[0].id, 8);
    }

    #[test]
    fn query_retries_are_bounded() {
        let mut size_calls = 0;
        let mut query_calls = 0;
        let result = query_with_retry(
            |num_paths, num_modes| {
                size_calls += 1;
                assert_eq!((*num_paths, *num_modes), (0, 0));
                (*num_paths, *num_modes) = (1, 1);
                0
            },
            |num_paths, paths, num_modes, modes| {
                query_calls += 1;
                assert_eq!((*num_paths, *num_modes), (1, 1));
                assert_eq!((paths.len(), modes.len()), (1, 1));
                (*num_paths, *num_modes) = (99, 99);
                ERROR_INSUFFICIENT_BUFFER
            },
        );

        let error = result.err().expect("query should exhaust its retries");
        assert!(matches!(
            error,
            CcdError::QueryRetryExhausted { attempts: 3 }
        ));
        assert_eq!((size_calls, query_calls), (3, 3));
        assert!(
            error
                .to_string()
                .contains("insufficient buffers after 3 attempts")
        );
    }

    #[test]
    fn query_sizing_errors_are_preserved_without_retry() {
        for code in [ERROR_INSUFFICIENT_BUFFER, ERROR_ACCESS_DENIED] {
            let mut size_calls = 0;
            let result = query_with_retry(
                |_, _| {
                    size_calls += 1;
                    code
                },
                |_, _, _, _| panic!("query must not run after a sizing error"),
            );

            assert!(matches!(result, Err(CcdError::BufferSizes(error)) if error == code));
            assert_eq!(size_calls, 1);
        }
    }

    #[test]
    fn query_other_errors_are_preserved_without_retry() {
        for code in [ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER] {
            let mut size_calls = 0;
            let mut query_calls = 0;
            let result = query_with_retry(
                |num_paths, num_modes| {
                    size_calls += 1;
                    (*num_paths, *num_modes) = (1, 1);
                    0
                },
                |_, _, _, _| {
                    query_calls += 1;
                    code
                },
            );

            assert!(matches!(result, Err(CcdError::Query(error)) if error == code));
            assert_eq!((size_calls, query_calls), (1, 1));
        }
    }

    #[test]
    fn query_truncates_buffers_to_returned_counts() {
        for (returned_paths, returned_modes) in [(2, 1), (0, 0)] {
            let mut query_calls = 0;
            let (paths, modes) = query_with_retry(
                |num_paths, num_modes| {
                    (*num_paths, *num_modes) = (3, 4);
                    0
                },
                |num_paths, paths, num_modes, modes| {
                    query_calls += 1;
                    assert_eq!((*num_paths, *num_modes), (3, 4));
                    assert_eq!((paths.len(), modes.len()), (3, 4));
                    (*num_paths, *num_modes) = (returned_paths, returned_modes);
                    0
                },
            )
            .unwrap();

            assert_eq!(query_calls, 1);
            assert_eq!(paths.len(), returned_paths as usize);
            assert_eq!(modes.len(), returned_modes as usize);
        }
    }

    #[test]
    fn display_config_json_roundtrips() {
        let cfg = DisplayConfig {
            paths: vec![PathInfo {
                source: PathSourceInfo {
                    adapter_id: Luid { low: 0, high: 0 },
                    id: 0,
                    mode_info_idx: 0,
                    status_flags: 1,
                },
                target: PathTargetInfo {
                    adapter_id: Luid { low: 0, high: 0 },
                    id: 4353,
                    mode_info_idx: 1,
                    output_technology: 5,
                    rotation: 1,
                    scaling: 1,
                    refresh_rate: Rational {
                        num: 60000,
                        den: 1000,
                    },
                    scan_line_ordering: 1,
                    target_available: 1,
                    status_flags: 1,
                },
                flags: 1,
            }],
            modes: vec![ModeInfo {
                info_type: 2,
                id: 4353,
                adapter_id: Luid { low: 5, high: 0 },
                payload: vec![7u8; 48],
            }],
            path_monitors: Vec::new(),
            monitors: vec![MonitorInfo {
                valid: true,
                manufacture_id: 4142,
                product_code_id: 16706,
                friendly_name: "DELL U2720Q".into(),
                device_path: "\\\\?\\DISPLAY#DELA123#".into(),
            }],
        };

        let json = serde_json::to_string(&cfg).unwrap();
        let back: DisplayConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(back.paths.len(), 1);
        assert_eq!(back.paths[0].target.id, 4353);
        assert_eq!(back.modes.len(), 1);
        assert_eq!(back.modes[0].payload, vec![7u8; 48]);
        assert_eq!(back.modes[0].adapter_id, Luid { low: 5, high: 0 });
        assert_eq!(back.monitors[0].friendly_name, "DELL U2720Q");
        assert!(back.monitors[0].valid);
    }
}
