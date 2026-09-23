use crate::ccd::{self, ActiveTarget};
use ddc::Ddc;
use ddc_winapi::Monitor;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFOEXW};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum DdcError {
    #[error(
        "No monitors with DDC/CI support found. Ensure your monitor supports DDC/CI and it is enabled."
    )]
    NoMonitorsFound,

    #[error("Monitor {0} is disconnected or cannot be identified safely")]
    MonitorNotFound(MonitorKey),

    #[error("Ambiguous monitor association: {0}")]
    Ambiguous(String),

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
// Power mode mapping (VCP feature 0xD6)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PowerMode {
    On,
    Standby,
    Suspend,
    Off,
}

impl PowerMode {
    /// DDC/CI VCP value (feature 0xD6) for this power mode.
    pub fn vcp_value(self) -> u16 {
        match self {
            PowerMode::On => 0x01,
            PowerMode::Standby => 0x02,
            PowerMode::Suspend => 0x03,
            PowerMode::Off => 0x04,
        }
    }
}

impl fmt::Display for PowerMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerMode::On => write!(f, "On"),
            PowerMode::Standby => write!(f, "Standby"),
            PowerMode::Suspend => write!(f, "Suspend"),
            PowerMode::Off => write!(f, "Off"),
        }
    }
}

// ---------------------------------------------------------------------------
// Monitor information
// ---------------------------------------------------------------------------

/// Full Windows device-interface identity. It is instance/connection scoped,
/// not a guarantee of physical-unit identity after changing ports or drivers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MonitorKey(String);

impl MonitorKey {
    pub fn from_device_path(path: &str) -> Option<Self> {
        if !path.starts_with(r"\\?\") || path.len() <= 4 || path.contains('\0') {
            return None;
        }
        Some(Self(format!(
            "windows-device-path:v1:{}",
            path.to_ascii_lowercase()
        )))
    }

    pub fn is_supported(&self) -> bool {
        self.0
            .strip_prefix("windows-device-path:v1:")
            .and_then(Self::from_device_path)
            .as_ref()
            == Some(self)
    }
}

impl fmt::Display for MonitorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// 1-indexed monitor ID for user display.
    pub id: u32,
    pub key: MonitorKey,
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
    pub brightness_read_error: Option<String>,
    pub contrast: u16,
    pub contrast_max: u16,
    pub contrast_read_error: Option<String>,
    pub input_source: InputSource,
    pub input_source_read_error: Option<String>,
    /// Input sources from the capabilities string. `None` when that read did not
    /// produce a usable `0x60` value list; the standard sources are shown instead.
    pub advertised_inputs: Option<Vec<InputSource>>,
}

/// Inputs offered when a monitor does not list discrete `0x60` values.
pub const STANDARD_INPUT_SOURCES: &[InputSource] = &[
    InputSource::Hdmi1,
    InputSource::Hdmi2,
    InputSource::Dp1,
    InputSource::Dp2,
    InputSource::UsbC1,
    InputSource::UsbC2,
];

/// Power modes offered when a monitor does not list discrete `0xD6` values.
pub const STANDARD_POWER_MODES: &[PowerMode] = &[
    PowerMode::On,
    PowerMode::Standby,
    PowerMode::Suspend,
    PowerMode::Off,
];

pub(crate) const NO_SHARED_OPTION_NOTE: &str = "Selected displays do not share this option.";

/// One VCP code from an MCCS capabilities string.
///
/// `values` is `None` when the code appears without a parenthesized list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AdvertisedFeature {
    code: u8,
    values: Option<Vec<u16>>,
}

/// Features declared by a parsed `vcp(...)` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorCapabilities {
    features: Vec<AdvertisedFeature>,
}

impl MonitorCapabilities {
    pub(crate) fn contains(&self, code: u8) -> bool {
        self.features.iter().any(|feature| feature.code == code)
    }

    /// Discrete values for `code`.
    ///
    /// `None` means the code is absent or present without a value list.
    /// Check [`Self::contains`] to tell those cases apart.
    pub(crate) fn discrete_values(&self, code: u8) -> Option<&[u16]> {
        self.features
            .iter()
            .find(|feature| feature.code == code)
            .and_then(|feature| feature.values.as_deref())
    }
}

/// Dropdown contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureOptions<T> {
    pub options: Vec<T>,
    /// Selected monitors advertise no value in common. `options` is only the saved value.
    pub no_shared_option: bool,
}

/// Parse the `vcp(...)` section of an MCCS capabilities string.
///
/// Returns `None` when that section is missing or not closed.
pub fn parse_capabilities(raw: &str) -> Option<MonitorCapabilities> {
    let marker = "vcp(";
    let start = raw
        .to_ascii_lowercase()
        .find(marker)
        .map(|index| index + marker.len())?;
    let body = &raw[start..];
    let mut depth = 1_i32;
    let mut end = None;
    for (index, ch) in body.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(index);
                    break;
                }
            }
            _ => {}
        }
    }
    let section = &body[..end?];
    Some(MonitorCapabilities {
        features: parse_vcp_features(section),
    })
}

fn parse_vcp_features(section: &str) -> Vec<AdvertisedFeature> {
    let tokens = vcp_tokens(section);
    let mut features = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let VcpToken::Hex(code) = tokens[index] else {
            index += 1;
            continue;
        };
        index += 1;
        let Ok(code) = u8::try_from(code) else {
            continue;
        };
        if tokens.get(index) == Some(&VcpToken::Open) {
            index += 1;
            let mut values = Vec::new();
            let mut depth = 1_i32;
            while index < tokens.len() && depth > 0 {
                match tokens[index] {
                    VcpToken::Open => depth += 1,
                    VcpToken::Close => {
                        depth -= 1;
                        if depth == 0 {
                            index += 1;
                            break;
                        }
                    }
                    VcpToken::Hex(value) if depth == 1 => {
                        if let Ok(value) = u16::try_from(value)
                            && !values.contains(&value)
                        {
                            values.push(value);
                        }
                    }
                    VcpToken::Hex(_) => {}
                }
                if depth > 0 {
                    index += 1;
                }
            }
            remember_feature(&mut features, code, Some(values));
        } else {
            remember_feature(&mut features, code, None);
        }
    }
    features
}

fn remember_feature(features: &mut Vec<AdvertisedFeature>, code: u8, values: Option<Vec<u16>>) {
    if let Some(existing) = features.iter_mut().find(|feature| feature.code == code) {
        if existing.values.is_none() {
            existing.values = values;
        } else if let (Some(current), Some(extra)) = (&mut existing.values, values) {
            for value in extra {
                if !current.contains(&value) {
                    current.push(value);
                }
            }
        }
        return;
    }
    features.push(AdvertisedFeature { code, values });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VcpToken {
    Hex(u32),
    Open,
    Close,
}

fn vcp_tokens(section: &str) -> Vec<VcpToken> {
    let mut tokens = Vec::new();
    let mut chars = section.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '(' => tokens.push(VcpToken::Open),
            ')' => tokens.push(VcpToken::Close),
            ch if ch.is_ascii_hexdigit() => {
                let mut value = ch.to_digit(16).expect("hex digit");
                while let Some(next) = chars.peek().copied().and_then(|c| c.to_digit(16)) {
                    chars.next();
                    value = value.saturating_mul(16).saturating_add(next);
                }
                tokens.push(VcpToken::Hex(value));
            }
            _ => {}
        }
    }
    tokens
}

/// Dropdown contents for one monitor's advertised inputs.
///
/// `None` uses the standard HDMI, DisplayPort, and USB-C list. The current
/// source is appended when it is not already present.
pub(crate) fn input_choices(
    advertised: Option<&[InputSource]>,
    current: Option<InputSource>,
) -> FeatureOptions<InputSource> {
    let options = match advertised {
        Some(sources) if !sources.is_empty() => sources.to_vec(),
        _ => STANDARD_INPUT_SOURCES.to_vec(),
    };
    FeatureOptions {
        options: append_current(options, current),
        no_shared_option: false,
    }
}

/// Inputs shared by the selected monitors.
///
/// A `None` list contributes the standard sources and does not shrink the result.
pub(crate) fn shared_input_choices(
    advertised: &[Option<Vec<InputSource>>],
    current: Option<InputSource>,
) -> FeatureOptions<InputSource> {
    let known: Vec<&[InputSource]> = advertised
        .iter()
        .filter_map(|sources| sources.as_deref().filter(|sources| !sources.is_empty()))
        .collect();
    if known.is_empty() {
        return input_choices(None, current);
    }
    let mut intersection = known[0].to_vec();
    for sources in &known[1..] {
        intersection.retain(|source| sources.contains(source));
    }
    if intersection.is_empty() {
        return FeatureOptions {
            options: current.into_iter().collect(),
            no_shared_option: true,
        };
    }
    FeatureOptions {
        options: append_current(intersection, current),
        no_shared_option: false,
    }
}

/// Standard power modes, plus the current mode when it is not already listed.
pub(crate) fn power_options(current: Option<PowerMode>) -> FeatureOptions<PowerMode> {
    FeatureOptions {
        options: append_current(STANDARD_POWER_MODES.to_vec(), current),
        no_shared_option: false,
    }
}

fn append_current<T: Clone + PartialEq>(mut options: Vec<T>, current: Option<T>) -> Vec<T> {
    if let Some(current) = current
        && !options.contains(&current)
    {
        options.push(current);
    }
    dedupe(options)
}

fn dedupe<T: PartialEq>(options: Vec<T>) -> Vec<T> {
    let mut unique = Vec::new();
    for option in options {
        if !unique.contains(&option) {
            unique.push(option);
        }
    }
    unique
}

// ---------------------------------------------------------------------------
// Core DDC/CI functions
// ---------------------------------------------------------------------------

struct Endpoint<M> {
    info: MonitorInfo,
    monitor: M,
}

/// Fail closed: neither physical-handle order nor a model name proves which
/// CCD target belongs to a handle when a source has multiple targets/handles.
fn associate<'a>(
    source: &str,
    physical_count: usize,
    targets: &'a [ActiveTarget],
) -> Result<&'a ActiveTarget> {
    let matches: Vec<_> = targets
        .iter()
        .filter(|t| t.gdi_name.eq_ignore_ascii_case(source))
        .collect();
    if physical_count != 1 || matches.len() != 1 {
        return Err(DdcError::Ambiguous(format!(
            "{source}: {physical_count} physical handles, {} CCD targets",
            matches.len()
        )));
    }
    let target = matches[0];
    let key = MonitorKey::from_device_path(&target.device_path)
        .ok_or_else(|| DdcError::DisplayInfoError(format!("No usable device path for {source}")))?;
    if targets
        .iter()
        .filter(|t| MonitorKey::from_device_path(&t.device_path).as_ref() == Some(&key))
        .count()
        != 1
    {
        return Err(DdcError::Ambiguous(key.to_string()));
    }
    Ok(target)
}

fn discover() -> Result<Vec<Endpoint<Monitor>>> {
    let targets = ccd::active_targets().map_err(|e| DdcError::DisplayInfoError(e.to_string()))?;
    let parents = ddc_winapi::enumerate_monitors().map_err(communication)?;
    let mut endpoints = Vec::new();
    for parent in parents {
        let raw = ddc_winapi::get_physical_monitors_from_hmonitor(parent).map_err(communication)?;
        // SAFETY: each raw handle is transferred exactly once. Wrap the entire
        // batch before doing anything fallible; Monitor::drop owns its cleanup.
        let mut physical: Vec<_> = raw
            .into_iter()
            .map(|raw| unsafe { Monitor::new(raw) })
            .collect();
        if physical.is_empty() {
            continue;
        }
        let mut gdi = MONITORINFOEXW::default();
        gdi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: same parent HMONITOR, correctly sized extended output buffer.
        if unsafe { GetMonitorInfoW(parent.cast(), &mut gdi.monitorInfo) } == 0 {
            return Err(DdcError::DisplayInfoError(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let end = gdi
            .szDevice
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(gdi.szDevice.len());
        let source = String::from_utf16(&gdi.szDevice[..end])
            .map_err(|e| DdcError::DisplayInfoError(e.to_string()))?;
        let target = associate(&source, physical.len(), &targets)?;
        let key =
            MonitorKey::from_device_path(&target.device_path).expect("association validated path");
        if endpoints
            .iter()
            .any(|e: &Endpoint<Monitor>| e.info.key == key)
        {
            return Err(DdcError::Ambiguous(key.to_string()));
        }
        let rect = gdi.monitorInfo.rcMonitor;
        endpoints.push(Endpoint {
            info: MonitorInfo {
                id: endpoints.len() as u32 + 1,
                key,
                x: rect.left,
                y: rect.top,
                width: (rect.right - rect.left) as u32,
                height: (rect.bottom - rect.top) as u32,
                name: target.friendly_name.clone(),
                is_primary: gdi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            },
            monitor: physical.pop().expect("one physical handle"),
        });
    }
    // Detect topology changes across the GDI/physical/CCD calls. There is no
    // atomic Windows snapshot; never retry a write whose result is uncertain.
    let after = ccd::active_targets().map_err(|e| DdcError::DisplayInfoError(e.to_string()))?;
    if targets.len() != after.len() || targets.iter().any(|t| !after.contains(t)) {
        return Err(DdcError::DisplayInfoError(
            "Display topology changed during discovery; refresh and retry".into(),
        ));
    }
    if endpoints.is_empty() {
        return Err(DdcError::NoMonitorsFound);
    }
    Ok(endpoints)
}

fn communication(error: impl fmt::Display) -> DdcError {
    DdcError::DdcCommunication(error.to_string())
}

fn resolve_endpoint<'a, M>(
    endpoints: &'a mut [Endpoint<M>],
    key: &MonitorKey,
) -> Result<&'a mut M> {
    if !key.is_supported() {
        return Err(DdcError::MonitorNotFound(key.clone()));
    }
    let mut matches = endpoints.iter_mut().filter(|e| &e.info.key == key);
    let endpoint = matches
        .next()
        .ok_or_else(|| DdcError::MonitorNotFound(key.clone()))?;
    if matches.next().is_some() {
        return Err(DdcError::Ambiguous(key.to_string()));
    }
    Ok(&mut endpoint.monitor)
}

/// Narrow transport seam used by both real operations and deterministic tests.
pub(crate) trait Vcp {
    fn read(&mut self, code: u8) -> Result<(u16, u16)>;
    fn write(&mut self, code: u8, value: u16) -> Result<()>;
    fn capabilities(&mut self) -> Result<String> {
        Err(DdcError::DdcCommunication(
            "capabilities unavailable".into(),
        ))
    }
}

impl Vcp for Monitor {
    fn read(&mut self, code: u8) -> Result<(u16, u16)> {
        self.get_vcp_feature(code)
            .map(|v| (v.value(), v.maximum()))
            .map_err(communication)
    }
    fn write(&mut self, code: u8, value: u16) -> Result<()> {
        self.set_vcp_feature(code, value).map_err(communication)
    }
    fn capabilities(&mut self) -> Result<String> {
        let bytes = self.capabilities_string().map_err(communication)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }
}

fn write_resolved<M: Vcp>(
    endpoints: &mut [Endpoint<M>],
    key: &MonitorKey,
    code: u8,
    value: u16,
) -> Result<()> {
    resolve_endpoint(endpoints, key)?.write(code, value)
}

fn offset_resolved<M: Vcp>(
    endpoints: &mut [Endpoint<M>],
    key: &MonitorKey,
    code: u8,
    offset: i32,
) -> Result<u16> {
    let monitor = resolve_endpoint(endpoints, key)?;
    let (current, maximum) = monitor.read(code)?;
    if maximum == 0 || current > maximum {
        return Err(DdcError::DdcCommunication(format!(
            "Invalid monitor value/range: {current} / {maximum}"
        )));
    }
    let value = (i64::from(current) + i64::from(offset)).clamp(0, i64::from(maximum)) as u16;
    monitor.write(code, value)?;
    Ok(value)
}

pub fn detect_monitors() -> Result<Vec<MonitorInfo>> {
    Ok(discover()?
        .into_iter()
        .map(|endpoint| endpoint.info)
        .collect())
}

fn validate_targets_in<M>(endpoints: &mut [Endpoint<M>], keys: &[MonitorKey]) -> Result<()> {
    for key in keys {
        resolve_endpoint(endpoints, key)?;
    }
    Ok(())
}

/// Job-local ownership: raw handles never cross a task/thread boundary.
pub(crate) struct Session<M = Monitor> {
    endpoints: Vec<Endpoint<M>>,
}

impl Session {
    pub fn open(required: &[MonitorKey]) -> Result<Self> {
        Self::from_endpoints(discover()?, required)
    }
}

impl<M: Vcp> Session<M> {
    fn from_endpoints(mut endpoints: Vec<Endpoint<M>>, required: &[MonitorKey]) -> Result<Self> {
        validate_targets_in(&mut endpoints, required)?;
        Ok(Self { endpoints })
    }

    pub fn set_vcp(&mut self, key: &MonitorKey, code: u8, value: u16) -> Result<()> {
        write_resolved(&mut self.endpoints, key, code, value)
    }

    pub fn offset_vcp(&mut self, key: &MonitorKey, code: u8, offset: i32) -> Result<u16> {
        offset_resolved(&mut self.endpoints, key, code, offset)
    }
}

/// Read using the key captured during detection, never the display number.
pub fn read_monitor_state(info: MonitorInfo) -> Result<MonitorState> {
    let mut endpoints = discover()?;
    let mon = resolve_endpoint(&mut endpoints, &info.key)?;
    Ok(read_state(info, mon))
}

fn read_state(info: MonitorInfo, mon: &mut impl Vcp) -> MonitorState {
    let (brightness, brightness_max, brightness_read_error) =
        scalar_reading(mon.read(VCP_BRIGHTNESS).map_err(|error| error.to_string()));
    let (contrast, contrast_max, contrast_read_error) =
        scalar_reading(mon.read(VCP_CONTRAST).map_err(|error| error.to_string()));

    // Read input source
    let (input_source, input_source_read_error) = match mon.read(VCP_INPUT_SOURCE) {
        Ok((raw_value, _)) => {
            let input_source = decode_input_source(raw_value);
            log::debug!(
                "Input source read: monitor={info:?}, raw=0x{raw_value:04X}, reserved SH=0x{:02X}, selected SL=0x{:02X}, decoded={input_source:?}",
                raw_value >> 8,
                raw_value & 0xFF,
            );
            (input_source, None)
        }
        Err(error) => (InputSource::Custom(0), Some(error.to_string())),
    };
    let advertised_inputs = match mon.capabilities() {
        Ok(raw) => advertised_input_sources(&raw),
        Err(error) => {
            log::debug!("Input list unavailable: {error}");
            None
        }
    };

    MonitorState {
        info,
        brightness,
        brightness_max,
        brightness_read_error,
        contrast,
        contrast_max,
        contrast_read_error,
        input_source,
        input_source_read_error,
        advertised_inputs,
    }
}

/// Decoded `0x60` values from a capabilities string.
///
/// `None` when the string cannot be used or does not list discrete inputs.
fn advertised_input_sources(raw: &str) -> Option<Vec<InputSource>> {
    let capabilities = parse_capabilities(raw)?;
    if !capabilities.contains(VCP_INPUT_SOURCE) {
        return None;
    }
    let values = capabilities.discrete_values(VCP_INPUT_SOURCE)?;
    let sources = dedupe(
        values
            .iter()
            .map(|value| InputSource::from_vcp_value(*value))
            .collect(),
    );
    (!sources.is_empty()).then_some(sources)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn decode_input_source(raw_value: u16) -> InputSource {
    // MCCS 2.2a, Table 8-13: scalar VCP 0x60 uses SL; SH/MH/ML are reserved.
    // https://milek7.pl/ddcbacklight/mccs.pdf#page=81
    // The transport packs SH/SL into value(), so decode only SL for this feature.
    InputSource::from_vcp_value(raw_value & 0xFF)
}

fn scalar_reading(result: std::result::Result<(u16, u16), String>) -> (u16, u16, Option<String>) {
    let error = match result {
        Ok((value, maximum)) if maximum > 0 && value <= maximum => {
            return (value, maximum, None);
        }
        Ok((value, maximum)) => format!("Invalid monitor value/range: {value} / {maximum}"),
        Err(error) => error,
    };
    // These are storage defaults, not a reading or a capability verdict.
    (0, 100, Some(error))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn key(id: u32) -> MonitorKey {
        MonitorKey::from_device_path(&format!(r"\\?\DISPLAY#model#{id}")).unwrap()
    }

    pub(crate) fn monitor_state() -> MonitorState {
        MonitorState {
            info: MonitorInfo {
                id: 1,
                key: key(1),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                name: "Test monitor".into(),
                is_primary: true,
            },
            brightness: 40,
            brightness_max: 100,
            brightness_read_error: None,
            contrast: 120,
            contrast_max: 255,
            contrast_read_error: None,
            input_source: InputSource::Hdmi1,
            input_source_read_error: None,
            advertised_inputs: None,
        }
    }

    #[derive(Clone, Default)]
    pub(crate) struct FakeVcp {
        reading: (u16, u16),
        fail_read: bool,
        reads: Vec<u8>,
        pub(crate) writes: std::rc::Rc<std::cell::RefCell<Vec<(u8, u16)>>>,
    }

    impl Vcp for FakeVcp {
        fn read(&mut self, code: u8) -> Result<(u16, u16)> {
            self.reads.push(code);
            if self.fail_read {
                return Err(communication("read failed"));
            }
            Ok(self.reading)
        }
        fn write(&mut self, code: u8, value: u16) -> Result<()> {
            self.writes.borrow_mut().push((code, value));
            Ok(())
        }
    }

    pub(crate) fn fake_session(
        monitors: &[(MonitorInfo, FakeVcp)],
        required: &[MonitorKey],
    ) -> Result<Session<FakeVcp>> {
        Session::from_endpoints(
            monitors
                .iter()
                .map(|(info, monitor)| Endpoint {
                    info: info.clone(),
                    monitor: monitor.clone(),
                })
                .collect(),
            required,
        )
    }

    fn endpoint(id: u32) -> Endpoint<FakeVcp> {
        let mut info = monitor_state().info;
        info.id = id;
        info.key = key(id);
        Endpoint {
            info,
            monitor: FakeVcp {
                reading: (40, 100),
                ..Default::default()
            },
        }
    }

    fn target(source: &str, instance: u32) -> ActiveTarget {
        ActiveTarget {
            source_adapter: ccd::Luid { low: 1, high: 2 },
            source_id: instance,
            target_adapter: ccd::Luid { low: 1, high: 2 },
            target_id: instance,
            gdi_name: source.into(),
            device_path: format!(r"\\?\DISPLAY#MODEL#{instance}"),
            friendly_name: "Same model".into(),
        }
    }

    #[test]
    fn full_path_identity_is_normalized_not_model_or_display_number() {
        assert_eq!(
            MonitorKey::from_device_path(r"\\?\DISPLAY#MODEL#1"),
            Some(key(1))
        );
        assert_ne!(key(1), key(2));
        assert!(MonitorKey::from_device_path("").is_none());
        assert!(MonitorKey::from_device_path("Monitor 1").is_none());
        assert!(MonitorKey::from_device_path("\\\\?\\bad\0path").is_none());
        let targets = [target("source-b", 2), target("source-a", 1)];
        assert_eq!(
            associate("SOURCE-A", 1, &targets).unwrap().device_path,
            targets[1].device_path
        );
        assert_eq!(
            associate("source-b", 1, &targets).unwrap().friendly_name,
            targets[1].friendly_name
        );
    }

    #[test]
    fn ambiguous_physical_and_ccd_associations_are_refused() {
        let one = target("source-a", 1);
        assert!(associate("source-a", 2, std::slice::from_ref(&one)).is_err());
        assert!(associate("source-a", 0, std::slice::from_ref(&one)).is_err());
        assert!(associate("missing", 1, std::slice::from_ref(&one)).is_err());
        let clone = target("source-a", 2);
        assert!(associate("source-a", 1, &[one.clone(), clone]).is_err());
        let mut duplicate_path = target("source-b", 2);
        duplicate_path.device_path = one.device_path.to_ascii_lowercase();
        assert!(associate("source-a", 1, &[one.clone(), duplicate_path]).is_err());
        let mut unknown = target("source-a", 3);
        unknown.device_path.clear();
        assert!(associate("source-a", 1, &[unknown.clone()]).is_err());
        // An unnamed second target must not disappear and create a false 1:1 join.
        assert!(associate("source-a", 1, &[one, unknown]).is_err());
    }

    #[test]
    fn reordered_handles_and_display_numbers_write_only_the_saved_key() {
        let a = endpoint(1);
        let mut b = endpoint(2);
        let a_writes = a.monitor.writes.clone();
        let b_writes = b.monitor.writes.clone();
        b.info.id = 1; // The old number now labels another physical device.
        let mut session = Session::from_endpoints(vec![b, a], &[key(1)]).unwrap();
        session.set_vcp(&key(1), VCP_BRIGHTNESS, 80).unwrap();
        assert_eq!(*a_writes.borrow(), [(VCP_BRIGHTNESS, 80)]);
        assert!(b_writes.borrow().is_empty());
    }

    #[test]
    fn missing_ambiguous_and_unknown_keys_never_write_or_fall_back() {
        let endpoint = endpoint(2);
        let writes = endpoint.monitor.writes.clone();
        // Even an available first target cannot execute when another is missing.
        assert!(Session::from_endpoints(vec![endpoint], &[key(2), key(1)]).is_err());
        assert!(writes.borrow().is_empty());
        let a = self::endpoint(1);
        let duplicate = self::endpoint(1);
        let writes = a.monitor.writes.clone();
        let other_writes = duplicate.monitor.writes.clone();
        assert!(Session::from_endpoints(vec![a, duplicate], &[key(1)]).is_err());
        assert!(writes.borrow().is_empty());
        assert!(other_writes.borrow().is_empty());
        let mut session = Session::from_endpoints(vec![self::endpoint(2)], &[key(2)]).unwrap();
        assert!(session.set_vcp(&key(1), VCP_BRIGHTNESS, 80).is_err());
        let unknown: MonitorKey = serde_json::from_str("\"future-key:v2:1\"").unwrap();
        assert!(session.set_vcp(&unknown, VCP_BRIGHTNESS, 80).is_err());
        assert!(session.endpoints[0].monitor.writes.borrow().is_empty());
    }

    #[test]
    fn offsets_read_and_write_one_handle_and_clamp_without_overflow() {
        let a = endpoint(1);
        let b = endpoint(2);
        let other_writes = b.monitor.writes.clone();
        let mut session = Session::from_endpoints(vec![b, a], &[key(1)]).unwrap();
        assert_eq!(
            session
                .offset_vcp(&key(1), VCP_BRIGHTNESS, i32::MAX)
                .unwrap(),
            100
        );
        assert_eq!(
            session
                .offset_vcp(&key(1), VCP_BRIGHTNESS, i32::MIN)
                .unwrap(),
            0
        );
        assert_eq!(
            session.endpoints[1].monitor.reads,
            [VCP_BRIGHTNESS, VCP_BRIGHTNESS]
        );
        assert_eq!(
            *session.endpoints[1].monitor.writes.borrow(),
            [(VCP_BRIGHTNESS, 100), (VCP_BRIGHTNESS, 0)]
        );
        assert!(session.endpoints[0].monitor.reads.is_empty());
        assert!(other_writes.borrow().is_empty());
    }

    #[test]
    fn invalid_or_failed_offset_reads_never_write() {
        for (reading, fail_read) in [((0, 0), false), ((101, 100), false), ((40, 100), true)] {
            let mut a = endpoint(1);
            a.monitor.reading = reading;
            a.monitor.fail_read = fail_read;
            let writes = a.monitor.writes.clone();
            let mut session = Session::from_endpoints(vec![a], &[key(1)]).unwrap();
            assert!(session.offset_vcp(&key(1), VCP_CONTRAST, 1).is_err());
            assert!(writes.borrow().is_empty());
        }
    }

    struct ScriptedVcp {
        replies: std::collections::VecDeque<(u8, Result<(u16, u16)>)>,
        capabilities: Option<String>,
    }

    impl Vcp for ScriptedVcp {
        fn read(&mut self, code: u8) -> Result<(u16, u16)> {
            let (expected_code, reply) = self.replies.pop_front().expect("unexpected read");
            assert_eq!(code, expected_code);
            reply
        }

        fn write(&mut self, _code: u8, _value: u16) -> Result<()> {
            panic!("reading monitor state must not write");
        }

        fn capabilities(&mut self) -> Result<String> {
            self.capabilities
                .take()
                .ok_or_else(|| communication("capabilities unavailable"))
        }
    }

    fn state_reads(input: Result<(u16, u16)>) -> ScriptedVcp {
        ScriptedVcp {
            replies: [
                (VCP_BRIGHTNESS, Ok((0x0123, 0x0345))),
                (VCP_CONTRAST, Ok((0x0234, 0x0456))),
                (VCP_INPUT_SOURCE, input),
            ]
            .into(),
            capabilities: None,
        }
    }

    #[test]
    fn input_source_read_uses_sl_for_duplicate_and_unequal_bytes() {
        for (raw, expected) in [
            (0x0F0F, InputSource::Dp1),
            (0x1010, InputSource::Dp2),
            (0x1111, InputSource::Hdmi1),
            (0x1212, InputSource::Hdmi2),
            (0x0F11, InputSource::Hdmi1),
            (0x110F, InputSource::Dp1),
            (0xFF11, InputSource::Hdmi1),
            (0xAB0F, InputSource::Dp1),
        ] {
            assert_eq!(decode_input_source(raw), expected, "raw=0x{raw:04X}");
        }
    }

    #[test]
    fn input_source_read_preserves_standard_codes_with_any_reserved_sh() {
        for sh in 0..=0xFF {
            for (sl, expected) in [
                (0x01, InputSource::Vga1),
                (0x02, InputSource::Vga2),
                (0x03, InputSource::Dvi1),
                (0x04, InputSource::Dvi2),
                (0x0F, InputSource::Dp1),
                (0x10, InputSource::Dp2),
                (0x11, InputSource::Hdmi1),
                (0x12, InputSource::Hdmi2),
                (0x13, InputSource::UsbC1),
                (0x14, InputSource::UsbC2),
            ] {
                let raw = (sh << 8) | sl;
                assert_eq!(decode_input_source(raw), expected, "raw=0x{raw:04X}");
            }
        }
    }

    #[test]
    fn input_source_read_unknown_sl_never_falls_back_to_sh() {
        for sh in 0..=0xFF {
            for sl in [0x00, 0x05, 0xEE, 0xFF] {
                let raw = (sh << 8) | sl;
                assert_eq!(
                    decode_input_source(raw),
                    InputSource::Custom(sl),
                    "raw=0x{raw:04X}"
                );
            }
        }
    }

    #[test]
    fn input_source_read_state_decodes_only_input_and_ignores_model_and_reserved_maximum() {
        for model in ["AW2725QF", "DELL U2723QE", "U2723QE", "Other display", ""] {
            for (raw, expected) in [
                (0x0F11, InputSource::Hdmi1),
                (0x110F, InputSource::Dp1),
                (0x0F0F, InputSource::Dp1),
                (0x1010, InputSource::Dp2),
                (0x1111, InputSource::Hdmi1),
                (0xAB12, InputSource::Hdmi2),
                (0x1100, InputSource::Custom(0)),
                (0x0FEE, InputSource::Custom(0xEE)),
                (0x11FF, InputSource::Custom(0xFF)),
            ] {
                for maximum in [0, 0xFFFF] {
                    let mut info = monitor_state().info;
                    info.name = model.into();
                    let mut transport = state_reads(Ok((raw, maximum)));
                    let state = read_state(info.clone(), &mut transport);
                    assert!(transport.replies.is_empty());
                    assert_eq!(state.info.key, info.key);
                    assert_eq!(state.info.name, info.name);
                    assert_eq!((state.brightness, state.brightness_max), (0x0123, 0x0345));
                    assert_eq!((state.contrast, state.contrast_max), (0x0234, 0x0456));
                    assert_eq!(state.brightness_read_error, None);
                    assert_eq!(state.contrast_read_error, None);
                    assert_eq!(
                        state.input_source, expected,
                        "model={model:?}, raw=0x{raw:04X}"
                    );
                    assert_eq!(state.input_source_read_error, None);
                }
            }
        }
    }

    #[test]
    fn input_source_read_state_error_is_independent_and_can_recover() {
        let mut transport = state_reads(Err(communication("input timeout")));
        let state = read_state(monitor_state().info, &mut transport);
        assert!(transport.replies.is_empty());
        assert_eq!((state.brightness, state.brightness_max), (0x0123, 0x0345));
        assert_eq!((state.contrast, state.contrast_max), (0x0234, 0x0456));
        assert_eq!(state.brightness_read_error, None);
        assert_eq!(state.contrast_read_error, None);
        assert_eq!(state.input_source, InputSource::Custom(0));
        assert_eq!(
            state.input_source_read_error,
            Some(communication("input timeout").to_string())
        );

        let mut transport = state_reads(Ok((0x0F11, 0)));
        let recovered = read_state(state.info, &mut transport);
        assert!(transport.replies.is_empty());
        assert_eq!(recovered.input_source, InputSource::Hdmi1);
        assert_eq!(recovered.input_source_read_error, None);
    }

    #[test]
    fn input_source_read_state_survives_independent_scalar_errors_and_invalid_ranges() {
        for failed_code in [VCP_BRIGHTNESS, VCP_CONTRAST] {
            for invalid_range in [false, true] {
                let mut transport = state_reads(Ok((0x0F11, 0)));
                transport
                    .replies
                    .iter_mut()
                    .find(|(code, _)| *code == failed_code)
                    .unwrap()
                    .1 = if invalid_range {
                    Ok((0x0101, 0x0100))
                } else {
                    Err(communication("scalar timeout"))
                };
                let state = read_state(monitor_state().info, &mut transport);
                assert!(transport.replies.is_empty());
                assert_eq!(state.input_source, InputSource::Hdmi1);
                assert_eq!(state.input_source_read_error, None);
                assert_eq!(
                    state.brightness_read_error.is_some(),
                    failed_code == VCP_BRIGHTNESS
                );
                assert_eq!(
                    state.contrast_read_error.is_some(),
                    failed_code == VCP_CONTRAST
                );
                if failed_code == VCP_BRIGHTNESS {
                    assert_eq!((state.brightness, state.brightness_max), (0, 100));
                    assert_eq!((state.contrast, state.contrast_max), (0x0234, 0x0456));
                } else {
                    assert_eq!((state.brightness, state.brightness_max), (0x0123, 0x0345));
                    assert_eq!((state.contrast, state.contrast_max), (0, 100));
                }
            }
        }
    }

    #[test]
    fn input_source_read_does_not_change_generic_decoder_serialization_or_writes() {
        for raw in [0x0F0F, 0x1111, 0x0F11, 0x110F, 0xFFFF] {
            assert_eq!(InputSource::from_vcp_value(raw), InputSource::Custom(raw));
            let custom = InputSource::Custom(raw);
            let json = serde_json::to_string(&custom).unwrap();
            assert_eq!(json, format!("{{\"Custom\":{raw}}}"));
            assert_eq!(serde_json::from_str::<InputSource>(&json).unwrap(), custom);
        }
        let cases = [
            (InputSource::Vga1, 0x0001),
            (InputSource::Vga2, 0x0002),
            (InputSource::Dvi1, 0x0003),
            (InputSource::Dvi2, 0x0004),
            (decode_input_source(0x110F), 0x000F),
            (InputSource::Dp2, 0x0010),
            (decode_input_source(0x0F11), 0x0011),
            (InputSource::Hdmi2, 0x0012),
            (InputSource::UsbC1, 0x0013),
            (InputSource::UsbC2, 0x0014),
            (InputSource::Custom(0x0F0F), 0x0F0F),
            (InputSource::Custom(0x1111), 0x1111),
            (InputSource::Custom(0x0F11), 0x0F11),
            (InputSource::Custom(0x110F), 0x110F),
            (InputSource::Custom(0xFFFF), 0xFFFF),
        ];
        let mut session = Session::from_endpoints(vec![endpoint(1)], &[key(1)]).unwrap();
        for (source, expected) in cases {
            assert_eq!(source.vcp_value(), expected);
            session
                .set_vcp(&key(1), VCP_INPUT_SOURCE, source.vcp_value())
                .unwrap();
        }
        assert_eq!(
            *session.endpoints[0].monitor.writes.borrow(),
            cases.map(|(_, value)| (VCP_INPUT_SOURCE, value))
        );
    }

    #[test]
    fn scalar_read_failures_and_invalid_ranges_remain_unknown_until_valid_read() {
        assert_eq!(
            scalar_reading(Err("timeout".into())),
            (0, 100, Some("timeout".into()))
        );
        assert!(scalar_reading(Ok((0, 0))).2.is_some());
        assert!(scalar_reading(Ok((101, 100))).2.is_some());
        assert_eq!(scalar_reading(Ok((120, 255))), (120, 255, None));
        // A valid zero must remain distinguishable from a failed read's default.
        assert_eq!(scalar_reading(Ok((0, 100))), (0, 100, None));
    }

    const DELL_CAPABILITIES: &str = "(prot(monitor)type(LCD)model(DELL U2723QE)cmds(01 02 03 07 0C E3 F3)vcp(02 04 05 08 0B 0C 10 12 14(05 08 0B) 16 18 1A 60(11 12 0F 10) AC AE B6 C6 C8 C9 D6(01 04) DC(00 03) DF)mswhql(1)mccs_ver(2.2))";

    #[test]
    fn parses_dell_style_capabilities_into_advertised_controls() {
        let caps = parse_capabilities(DELL_CAPABILITIES).expect("vcp section");
        assert!(caps.contains(VCP_BRIGHTNESS));
        assert!(caps.contains(VCP_CONTRAST));
        assert_eq!(caps.discrete_values(VCP_BRIGHTNESS), None);
        assert_eq!(
            caps.discrete_values(VCP_INPUT_SOURCE),
            Some(&[0x11, 0x12, 0x0F, 0x10][..])
        );
        assert_eq!(
            caps.discrete_values(VCP_POWER_MODE),
            Some(&[0x01, 0x04][..])
        );
        assert!(!caps.contains(0x99));
    }

    #[test]
    fn capabilities_without_a_complete_vcp_section_are_unknown() {
        assert!(parse_capabilities("(prot(monitor)type(LCD))").is_none());
        assert!(parse_capabilities("vcp(10 12").is_none());
        assert!(parse_capabilities("").is_none());
    }

    #[test]
    fn bare_vcp_code_is_continuous_and_parentheses_are_the_value_list() {
        let caps = parse_capabilities("VCP(10 60 (11 12) 12(00 64))").unwrap();
        assert!(caps.contains(0x10));
        assert_eq!(caps.discrete_values(0x10), None);
        assert_eq!(caps.discrete_values(0x60), Some(&[0x11, 0x12][..]));
        assert_eq!(caps.discrete_values(0x12), Some(&[0x00, 0x64][..]));
    }

    #[test]
    fn read_state_leaves_inputs_unset_until_a_usable_list_is_read() {
        let mut transport = state_reads(Ok((0x11, 0)));
        let state = read_state(monitor_state().info, &mut transport);
        assert!(state.advertised_inputs.is_none());
    }

    #[test]
    fn read_state_stores_advertised_inputs_without_dropping_other_readings() {
        let mut transport = state_reads(Ok((0x11, 0)));
        transport.capabilities = Some("vcp(60(11 0F))".into());
        let state = read_state(monitor_state().info, &mut transport);
        assert_eq!(state.brightness, 0x0123);
        assert_eq!(state.input_source, InputSource::Hdmi1);
        assert_eq!(
            state.advertised_inputs,
            Some(vec![InputSource::Hdmi1, InputSource::Dp1])
        );
    }

    #[test]
    fn read_state_keeps_nonstandard_input_codes() {
        let mut transport = state_reads(Ok((0x11, 0)));
        transport.capabilities = Some("vcp(60(11 0F 1B))".into());
        let state = read_state(monitor_state().info, &mut transport);
        assert_eq!(
            state.advertised_inputs,
            Some(vec![
                InputSource::Hdmi1,
                InputSource::Dp1,
                InputSource::Custom(0x1B)
            ])
        );
    }

    #[test]
    fn read_state_keeps_other_readings_when_the_input_list_is_unavailable() {
        let mut transport = state_reads(Ok((0x11, 0)));
        let state = read_state(monitor_state().info, &mut transport);
        assert_eq!(state.brightness, 0x0123);
        assert_eq!(state.input_source, InputSource::Hdmi1);
        assert!(state.advertised_inputs.is_none());
    }
}
