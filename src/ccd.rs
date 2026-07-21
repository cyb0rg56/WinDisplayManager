//! CCD (Connecting and Configuring Displays) backend for saving and restoring
//! full monitor-layout profiles.
//!
//! Mirrors the structure of [`crate::ddc`]: a [`thiserror`] error enum, plain
//! serde data structs, and blocking functions. The application runs these via
//! `tokio::task::spawn_blocking` wrapped in `cosmic::app::Task::perform`.
//!
//! This is a faithful port of the Win32 CCD apply/remap algorithm used by the
//! original *MonitorSwitcher* tool. Adapter LUIDs are not stable across reboots
//! or replug, so a saved configuration must be remapped onto the live hardware
//! before it can be applied.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_0,
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_RATIONAL, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ALL_PATHS, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig, SDC_ALLOW_CHANGES, SDC_APPLY, SDC_SAVE_TO_DATABASE,
    SDC_USE_SUPPLIED_DISPLAY_CONFIG, SetDisplayConfig,
};
use windows_sys::Win32::Foundation::LUID;
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

    #[error("Failed to apply display configuration (Win32 error {0})")]
    Apply(i32),

    #[error("Profile contains no display paths")]
    Empty,
}

pub type Result<T> = std::result::Result<T, CcdError>;

// ---------------------------------------------------------------------------
// Serde mirror types
//
// These mirror the Win32 `DISPLAYCONFIG_*` arrays. The mode union payload is
// stored verbatim as opaque bytes: the remap algorithm only ever touches
// `adapter_id`, so the union contents never need to be interpreted and always
// round-trip losslessly (including the rare desktop-image variant).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Capture the currently active display configuration.
pub fn capture_active_config() -> Result<DisplayConfig> {
    let (paths, modes, monitors) = query(QDC_ONLY_ACTIVE_PATHS)?;
    Ok(DisplayConfig {
        paths,
        modes,
        monitors,
    })
}

/// Apply a saved display configuration, remapping its adapter LUIDs onto the
/// live hardware first. Falls back to matching monitors by EDID friendly name.
pub fn apply_config(saved: &DisplayConfig) -> Result<()> {
    if saved.paths.is_empty() {
        return Err(CcdError::Empty);
    }

    // Query the live hardware (all paths, needed for remapping).
    let (live_paths, live_modes, live_monitors) = query(QDC_ALL_PATHS)?;

    let mut paths = saved.paths.clone();
    let mut modes = saved.modes.clone();

    // ---- Pass 1: remap path adapter LUIDs by (source id, target id) ----
    for sp in paths.iter_mut() {
        for lp in &live_paths {
            if sp.source.id == lp.source.id && sp.target.id == lp.target.id {
                sp.source.adapter_id.low = lp.source.adapter_id.low;
                sp.target.adapter_id.low = lp.target.adapter_id.low;
                break;
            }
        }
    }

    // ---- Pass 2: remap mode adapter LUIDs using the (remapped) paths ----
    for m in 0..modes.len() {
        if modes[m].info_type != DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
            continue;
        }
        for n in 0..paths.len() {
            if modes[m].id == paths[n].target.id {
                let m_low = modes[m].adapter_id.low;
                let source_id = paths[n].source.id;
                let source_low = paths[n].source.adapter_id.low;
                let target_low = paths[n].target.adapter_id.low;
                // Fix the matching source mode first.
                for k in 0..modes.len() {
                    if modes[k].id == source_id
                        && modes[k].adapter_id.low == m_low
                        && modes[k].info_type == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE
                    {
                        modes[k].adapter_id.low = source_low;
                        break;
                    }
                }
                modes[m].adapter_id.low = target_low;
                break;
            }
        }
    }

    // ---- Primary apply ----
    let flags =
        SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_APPLY | SDC_SAVE_TO_DATABASE | SDC_ALLOW_CHANGES;
    let err = set_display_config(&paths, &modes, flags);
    if err == 0 {
        return Ok(());
    }
    log::warn!("Primary display apply failed (error {err}); trying EDID fallback");

    // ---- Fallback: match monitors by EDID friendly name, remap full LUID ----
    if !saved.monitors.is_empty() {
        // Reset to the original saved arrays.
        let mut paths = saved.paths.clone();
        let mut modes = saved.modes.clone();

        let count = modes.len().min(saved.monitors.len());
        for m in 0..count {
            for j in 0..live_monitors.len() {
                let saved_name = &saved.monitors[m].friendly_name;
                let live_name = &live_monitors[j].friendly_name;
                if saved_name.is_empty() || live_name.is_empty() || saved_name != live_name {
                    continue;
                }
                let old = modes[m].adapter_id; // full LUID (low + high)
                let new = live_modes[j].adapter_id;
                for sp in paths.iter_mut() {
                    if sp.target.adapter_id == old {
                        sp.target.adapter_id = new;
                        sp.source.adapter_id = new;
                    }
                }
                for sm in modes.iter_mut() {
                    if sm.adapter_id == old {
                        sm.adapter_id = new;
                    }
                }
                modes[m].adapter_id = new;
                break;
            }
        }

        let err = set_display_config(&paths, &modes, flags);
        if err == 0 {
            return Ok(());
        }
        return Err(CcdError::Apply(err));
    }

    Err(CcdError::Apply(err))
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

/// Query the display configuration for the given flags, returning mirror types.
fn query(flags: u32) -> Result<(Vec<PathInfo>, Vec<ModeInfo>, Vec<MonitorInfo>)> {
    // SAFETY: standard two-call CCD query pattern with correctly sized buffers.
    unsafe {
        let mut num_paths: u32 = 0;
        let mut num_modes: u32 = 0;
        let err = GetDisplayConfigBufferSizes(flags, &mut num_paths, &mut num_modes);
        if err != 0 {
            return Err(CcdError::BufferSizes(err));
        }

        let mut raw_paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
        let mut raw_modes = vec![DISPLAYCONFIG_MODE_INFO::default(); num_modes as usize];
        let err = QueryDisplayConfig(
            flags,
            &mut num_paths,
            raw_paths.as_mut_ptr(),
            &mut num_modes,
            raw_modes.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        if err != 0 {
            return Err(CcdError::Query(err));
        }
        raw_paths.truncate(num_paths as usize);
        raw_modes.truncate(num_modes as usize);

        // Additional monitor info is parallel-indexed to the modes array.
        let mut monitors = Vec::with_capacity(raw_modes.len());
        for m in &raw_modes {
            if m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
                monitors.push(get_monitor_additional_info(m.adapterId, m.id));
            } else {
                monitors.push(MonitorInfo::default());
            }
        }

        let paths = raw_paths.iter().map(path_to_mirror).collect();
        let modes = raw_modes.iter().map(mode_to_mirror).collect();
        Ok((paths, modes, monitors))
    }
}

/// Retrieve EDID/friendly-name info for a target mode. Invalid on failure.
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

/// Convert saved mirror arrays to Win32 arrays and call `SetDisplayConfig`.
fn set_display_config(paths: &[PathInfo], modes: &[ModeInfo], flags: u32) -> i32 {
    let raw_paths: Vec<DISPLAYCONFIG_PATH_INFO> = paths.iter().map(path_to_raw).collect();
    let raw_modes: Vec<DISPLAYCONFIG_MODE_INFO> = modes.iter().map(mode_to_raw).collect();
    // SAFETY: pointers/lengths refer to the freshly built arrays above.
    unsafe {
        SetDisplayConfig(
            raw_paths.len() as u32,
            raw_paths.as_ptr(),
            raw_modes.len() as u32,
            raw_modes.as_ptr(),
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

fn mode_to_raw(m: &ModeInfo) -> DISPLAYCONFIG_MODE_INFO {
    let mut raw = DISPLAYCONFIG_MODE_INFO {
        infoType: m.info_type,
        id: m.id,
        adapterId: m.adapter_id.into(),
        // SAFETY: zero-initialised union, overwritten with the saved payload below.
        Anonymous: unsafe { std::mem::zeroed() },
    };
    let n = std::mem::size_of::<DISPLAYCONFIG_MODE_INFO_0>().min(m.payload.len());
    // SAFETY: `dst` points at the union; `n` is bounded by both buffer sizes.
    unsafe {
        let dst = &mut raw.Anonymous as *mut DISPLAYCONFIG_MODE_INFO_0 as *mut u8;
        std::ptr::copy_nonoverlapping(m.payload.as_ptr(), dst, n);
    }
    raw
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
