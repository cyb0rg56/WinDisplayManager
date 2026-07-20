use ddc::Ddc;
use ddc_winapi::Monitor;
use display_info::DisplayInfo;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum DdcError {
    #[error("No monitors with DDC/CI support found. Ensure your monitor supports DDC/CI and it is enabled.")]
    NoMonitorsFound,

    #[error("Monitor with ID {0} not found")]
    MonitorNotFound(u32),

    #[error("DDC/CI communication error: {0}")]
    DdcCommunication(String),

    #[error("Display info error: {0}")]
    DisplayInfoError(String),
}

pub type Result<T> = std::result::Result<T, DdcError>;

// ---------------------------------------------------------------------------
// VCP Feature codes
// ---------------------------------------------------------------------------

pub const VCP_BRIGHTNESS: u8 = 0x10;
pub const VCP_CONTRAST: u8 = 0x12;
pub const VCP_INPUT_SOURCE: u8 = 0x60;
pub const VCP_POWER_MODE: u8 = 0xD6;

// ---------------------------------------------------------------------------
// Input source mapping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputSource {
    Vga1,
    Vga2,
    Dvi1,
    Dvi2,
    Dp1,
    Dp2,
    Hdmi1,
    Hdmi2,
    UsbC1,
    UsbC2,
    Custom(u16),
}

impl InputSource {
    /// DDC/CI VCP value for this input source.
    pub fn vcp_value(self) -> u16 {
        match self {
            InputSource::Vga1 => 0x01,
            InputSource::Vga2 => 0x02,
            InputSource::Dvi1 => 0x03,
            InputSource::Dvi2 => 0x04,
            InputSource::Dp1 => 0x0F,
            InputSource::Dp2 => 0x10,
            InputSource::Hdmi1 => 0x11,
            InputSource::Hdmi2 => 0x12,
            InputSource::UsbC1 => 0x13,
            InputSource::UsbC2 => 0x14,
            InputSource::Custom(v) => v,
        }
    }

    /// Try to map a raw VCP value back to a known input source.
    pub fn from_vcp_value(value: u16) -> Self {
        match value {
            0x01 => InputSource::Vga1,
            0x02 => InputSource::Vga2,
            0x03 => InputSource::Dvi1,
            0x04 => InputSource::Dvi2,
            0x0F => InputSource::Dp1,
            0x10 => InputSource::Dp2,
            0x11 => InputSource::Hdmi1,
            0x12 => InputSource::Hdmi2,
            0x13 => InputSource::UsbC1,
            0x14 => InputSource::UsbC2,
            other => InputSource::Custom(other),
        }
    }

}

impl fmt::Display for InputSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputSource::Vga1 => write!(f, "VGA 1"),
            InputSource::Vga2 => write!(f, "VGA 2"),
            InputSource::Dvi1 => write!(f, "DVI 1"),
            InputSource::Dvi2 => write!(f, "DVI 2"),
            InputSource::Dp1 => write!(f, "DisplayPort 1"),
            InputSource::Dp2 => write!(f, "DisplayPort 2"),
            InputSource::Hdmi1 => write!(f, "HDMI 1"),
            InputSource::Hdmi2 => write!(f, "HDMI 2"),
            InputSource::UsbC1 => write!(f, "USB-C 1"),
            InputSource::UsbC2 => write!(f, "USB-C 2"),
            InputSource::Custom(v) => write!(f, "Custom (0x{:02X})", v),
        }
    }
}

// ---------------------------------------------------------------------------
// Monitor information
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// 1-indexed monitor ID for user display.
    pub id: u32,
    /// Display position X.
    pub x: i32,
    /// Display position Y.
    pub y: i32,
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// Display name / identifier from the OS.
    pub name: String,
    /// Whether this monitor is the primary display.
    pub is_primary: bool,
    /// Scale factor.
    pub scale_factor: f32,
}

impl fmt::Display for MonitorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Monitor {} - {}x{} at ({},{})",
            self.id, self.width, self.height, self.x, self.y
        )
    }
}

/// Combined state for a monitor: its info plus current DDC values.
#[derive(Debug, Clone)]
pub struct MonitorState {
    pub info: MonitorInfo,
    pub brightness: u16,
    pub brightness_max: u16,
    pub contrast: u16,
    pub contrast_max: u16,
    pub input_source: InputSource,
}

// ---------------------------------------------------------------------------
// Core DDC/CI functions
// ---------------------------------------------------------------------------

/// Enumerate all DDC/CI monitors, returning their handles.
fn get_ddc_monitors() -> Result<Vec<Monitor>> {
    let monitors =
        Monitor::enumerate().map_err(|e| DdcError::DdcCommunication(e.to_string()))?;
    if monitors.is_empty() {
        return Err(DdcError::NoMonitorsFound);
    }
    Ok(monitors)
}

/// Retrieve display-info metadata for all monitors.
fn get_display_info() -> Result<Vec<DisplayInfo>> {
    DisplayInfo::all()
        .map_err(|_| DdcError::DisplayInfoError("Failed to get display information".into()))
}

/// Detect monitors and return a `MonitorInfo` for each one.
pub fn detect_monitors() -> Result<Vec<MonitorInfo>> {
    let ddc_monitors = get_ddc_monitors()?;
    let display_infos = get_display_info().unwrap_or_default();

    let mut result = Vec::with_capacity(ddc_monitors.len());
    for (i, _mon) in ddc_monitors.iter().enumerate() {
        let id = (i + 1) as u32;
        if let Some(info) = display_infos.get(i) {
            result.push(MonitorInfo {
                id,
                x: info.x,
                y: info.y,
                width: info.width,
                height: info.height,
                name: format!("Monitor {}", id),
                is_primary: info.is_primary,
                scale_factor: info.scale_factor,
            });
        } else {
            result.push(MonitorInfo {
                id,
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                name: format!("Monitor {}", id),
                is_primary: false,
                scale_factor: 1.0,
            });
        }
    }
    Ok(result)
}


/// Set brightness for the given 1-indexed monitor.
pub fn set_brightness(monitor_id: u32, value: u16) -> Result<()> {
    let mut monitors = get_ddc_monitors()?;
    let idx = validated_index(monitor_id, monitors.len())?;
    let mon = &mut monitors[idx];
    mon.set_vcp_feature(VCP_BRIGHTNESS, value)
        .map_err(|e| DdcError::DdcCommunication(e.to_string()))?;
    Ok(())
}


/// Set contrast for the given 1-indexed monitor.
pub fn set_contrast(monitor_id: u32, value: u16) -> Result<()> {
    let mut monitors = get_ddc_monitors()?;
    let idx = validated_index(monitor_id, monitors.len())?;
    let mon = &mut monitors[idx];
    mon.set_vcp_feature(VCP_CONTRAST, value)
        .map_err(|e| DdcError::DdcCommunication(e.to_string()))?;
    Ok(())
}


/// Set the input source for the given 1-indexed monitor.
pub fn set_input_source(monitor_id: u32, source: InputSource) -> Result<()> {
    let mut monitors = get_ddc_monitors()?;
    let idx = validated_index(monitor_id, monitors.len())?;
    let mon = &mut monitors[idx];
    mon.set_vcp_feature(VCP_INPUT_SOURCE, source.vcp_value())
        .map_err(|e| DdcError::DdcCommunication(e.to_string()))?;
    Ok(())
}

/// Read full monitor state (brightness, contrast, input) for the given 1-indexed monitor.
pub fn read_monitor_state(monitor_id: u32, info: MonitorInfo) -> Result<MonitorState> {
    let mut monitors = get_ddc_monitors()?;
    let idx = validated_index(monitor_id, monitors.len())?;
    let mon = &mut monitors[idx];

    // Read brightness
    let (brightness, brightness_max) = match mon.get_vcp_feature(VCP_BRIGHTNESS) {
        Ok(val) => (val.value(), val.maximum()),
        Err(_) => (0, 100),
    };

    // Read contrast
    let (contrast, contrast_max) = match mon.get_vcp_feature(VCP_CONTRAST) {
        Ok(val) => (val.value(), val.maximum()),
        Err(_) => (0, 100),
    };

    // Read input source
    let input_source = match mon.get_vcp_feature(VCP_INPUT_SOURCE) {
        Ok(val) => InputSource::from_vcp_value(val.value()),
        Err(_) => InputSource::Custom(0),
    };

    Ok(MonitorState {
        info,
        brightness,
        brightness_max,
        contrast,
        contrast_max,
        input_source,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a 1-indexed monitor ID to a validated 0-indexed array index.
fn validated_index(monitor_id: u32, count: usize) -> Result<usize> {
    if monitor_id < 1 || monitor_id as usize > count {
        return Err(DdcError::MonitorNotFound(monitor_id));
    }
    Ok((monitor_id - 1) as usize)
}
