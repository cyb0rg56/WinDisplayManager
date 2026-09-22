use crate::ddc::{InputSource, MonitorInfo, MonitorKey, PowerMode};
use crate::persistence::{self, LoadOutcome, SCHEMA_VERSION, WriteMode};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Hotkey binding descriptor  (serializable)
// ---------------------------------------------------------------------------

/// A serializable representation of a hotkey (modifier flags + key code string).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String, // e.g. "F1", "Digit1", "ArrowUp"
}

impl HotkeyBinding {
    /// An empty/unbound binding (no key assigned).
    pub fn unbound() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: String::new(),
        }
    }

    /// Build a `global_hotkey::HotKey` from this binding.
    pub fn to_hotkey(&self) -> Option<HotKey> {
        let code = string_to_code(&self.key)?;
        let mut mods = Modifiers::empty();
        if self.ctrl {
            mods |= Modifiers::CONTROL;
        }
        if self.alt {
            mods |= Modifiers::ALT;
        }
        if self.shift {
            mods |= Modifiers::SHIFT;
        }
        if self.win {
            mods |= Modifiers::SUPER;
        }
        let modifiers = if mods.is_empty() { None } else { Some(mods) };
        Some(HotKey::new(modifiers, code))
    }
}

impl std::fmt::Display for HotkeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.key.is_empty() {
            return write!(f, "(none)");
        }
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        parts.push(&self.key);
        write!(f, "{}", parts.join(" + "))
    }
}

// ---------------------------------------------------------------------------
// Action-based hotkey model
// ---------------------------------------------------------------------------

/// How an action applies its value to the target.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionType {
    /// Set the target to an absolute value.
    Set,
    /// Add a signed delta to the target's current value.
    Offset,
    /// Turn off the selected monitors (target is ignored).
    Off,
}

impl ActionType {
    pub const ALL: &'static [ActionType] = &[ActionType::Set, ActionType::Offset, ActionType::Off];
    pub const NO_OFFSET: &'static [ActionType] = &[ActionType::Set, ActionType::Off];

    pub fn label(self) -> &'static str {
        match self {
            ActionType::Set => "Set",
            ActionType::Offset => "Offset",
            ActionType::Off => "Turn Off",
        }
    }
}

/// What an action operates on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionTarget {
    Brightness,
    Contrast,
    InputSource,
    PowerMode,
    Profile,
    CustomVcp,
}

impl ActionTarget {
    pub const ALL: &'static [ActionTarget] = &[
        ActionTarget::Brightness,
        ActionTarget::Contrast,
        ActionTarget::InputSource,
        ActionTarget::PowerMode,
        ActionTarget::Profile,
        ActionTarget::CustomVcp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ActionTarget::Brightness => "Brightness",
            ActionTarget::Contrast => "Contrast",
            ActionTarget::InputSource => "Input Source",
            ActionTarget::PowerMode => "Power Mode",
            ActionTarget::Profile => "Apply Profile",
            ActionTarget::CustomVcp => "Custom VCP Code",
        }
    }

    /// Whether `ActionType::Offset` is a meaningful choice for this target.
    pub fn supports_offset(self) -> bool {
        matches!(
            self,
            ActionTarget::Brightness | ActionTarget::Contrast | ActionTarget::CustomVcp
        )
    }
}

/// A per-monitor input-source assignment. Lets a single Input Source action
/// switch different monitors to different inputs from one hotkey.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonitorInput {
    pub monitor_id: MonitorTarget,
    pub input_source: InputSource,
}

/// Numbers are historical enumeration positions, never current monitor IDs.
/// Unknown string keys remain round-trippable but cannot resolve to hardware.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MonitorTarget {
    Stable(MonitorKey),
    LegacyIndex(u32),
}

impl From<u32> for MonitorTarget {
    fn from(id: u32) -> Self {
        Self::LegacyIndex(id)
    }
}

impl std::fmt::Display for MonitorTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stable(key) => key.fmt(f),
            Self::LegacyIndex(id) => write!(f, "Legacy monitor {id} (rebind required)"),
        }
    }
}

impl MonitorTarget {
    pub fn resolve<'a>(&self, available: &'a [MonitorInfo]) -> Result<&'a MonitorInfo, String> {
        let Self::Stable(key) = self else {
            return Err(format!(
                "{self}: select Rebind or Remove in the hotkey editor"
            ));
        };
        let mut matches = available
            .iter()
            .filter(|m| &m.key == key && key.is_supported());
        let found = matches
            .next()
            .ok_or_else(|| format!("Monitor unavailable: {key}"))?;
        if matches.next().is_some() {
            return Err(format!("Ambiguous monitor: {key}"));
        }
        Ok(found)
    }
}

/// A single step within a hotkey's action chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyActionSpec {
    pub action_type: ActionType,
    pub target: ActionTarget,
    /// Apply to every detected monitor instead of `monitors`.
    #[serde(default)]
    pub all_monitors: bool,
    /// Explicit monitor ids to apply to (ignored when `all_monitors` is set).
    #[serde(default)]
    pub monitors: Vec<MonitorTarget>,
    /// Set = absolute value; Offset = signed delta. Used for numeric targets.
    #[serde(default)]
    pub value: i32,
    /// VCP feature code, used when `target == CustomVcp`.
    #[serde(default)]
    pub vcp_code: u8,
    /// Used when `target == InputSource` and `all_monitors` is true (one input
    /// applied to every monitor).
    #[serde(default = "default_input_source")]
    pub input_source: InputSource,
    /// Used when `target == InputSource` and `all_monitors` is false: assigns a
    /// specific input per monitor, so one hotkey can switch different monitors
    /// to different inputs.
    #[serde(default)]
    pub monitor_inputs: Vec<MonitorInput>,
    /// Used when `target == PowerMode`.
    #[serde(default = "default_power_mode")]
    pub power_mode: PowerMode,
    /// Used when `target == Profile`.
    #[serde(default)]
    pub profile_name: String,
}

impl HotkeyActionSpec {
    pub fn explicit_targets(&self) -> Vec<MonitorTarget> {
        if self.action_type != ActionType::Off
            && self.target == ActionTarget::InputSource
            && !self.monitor_inputs.is_empty()
        {
            self.monitor_inputs
                .iter()
                .map(|input| input.monitor_id.clone())
                .collect()
        } else {
            self.monitors.clone()
        }
    }

    /// Validate the entire active selection before any job (including SoftOff)
    /// is queued. Duplicate assignments are rejected rather than last-wins.
    pub fn resolve_monitors(&self, available: &[MonitorInfo]) -> Result<Vec<u32>, String> {
        let targets = if self.all_monitors {
            available
                .iter()
                .map(|m| MonitorTarget::Stable(m.key.clone()))
                .collect()
        } else {
            self.explicit_targets()
        };
        let mut ids = Vec::new();
        for target in targets {
            let id = target.resolve(available)?.id;
            if ids.contains(&id) {
                return Err(format!("Duplicate monitor assignment: {target}"));
            }
            ids.push(id);
        }
        if ids.is_empty() {
            return Err("No target monitors selected or available".into());
        }
        Ok(ids)
    }

    pub fn rebind_target(&mut self, old: &MonitorTarget, new: MonitorKey) -> Result<(), String> {
        let new = MonitorTarget::Stable(new);
        if old != &new
            && (self.monitors.contains(&new)
                || self.monitor_inputs.iter().any(|i| i.monitor_id == new))
        {
            return Err(
                "That monitor is already assigned; remove its assignment before rebinding".into(),
            );
        }
        for target in &mut self.monitors {
            if target == old {
                *target = new.clone();
            }
        }
        for input in &mut self.monitor_inputs {
            if &input.monitor_id == old {
                input.monitor_id = new.clone();
            }
        }
        Ok(())
    }

    pub fn remove_target(&mut self, target: &MonitorTarget) {
        self.monitors.retain(|id| id != target);
        self.monitor_inputs
            .retain(|input| &input.monitor_id != target);
    }
}

fn default_input_source() -> InputSource {
    InputSource::Hdmi1
}

fn default_power_mode() -> PowerMode {
    PowerMode::On
}

impl Default for HotkeyActionSpec {
    fn default() -> Self {
        Self {
            action_type: ActionType::Set,
            target: ActionTarget::Brightness,
            all_monitors: true,
            monitors: Vec::new(),
            value: 10,
            vcp_code: 0x10,
            input_source: InputSource::Hdmi1,
            monitor_inputs: Vec::new(),
            power_mode: PowerMode::On,
            profile_name: String::new(),
        }
    }
}

/// A global hotkey bound to a chain of one or more actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkey {
    pub id: String,
    /// Optional name shown on the hotkey card. Blank labels use a numbered title.
    #[serde(default)]
    pub label: String,
    pub binding: HotkeyBinding,
    pub actions: Vec<HotkeyActionSpec>,
}

impl Hotkey {
    /// A new, unbound hotkey with a single default action.
    pub fn new_empty() -> Self {
        Self {
            id: new_id(),
            label: String::new(),
            binding: HotkeyBinding::unbound(),
            actions: vec![HotkeyActionSpec::default()],
        }
    }

    /// A new, unbound hotkey with a single "apply profile" action.
    pub fn new_for_profile(profile_name: String) -> Self {
        Self {
            id: new_id(),
            label: String::new(),
            binding: HotkeyBinding::unbound(),
            actions: vec![HotkeyActionSpec {
                action_type: ActionType::Set,
                target: ActionTarget::Profile,
                all_monitors: true,
                profile_name,
                ..Default::default()
            }],
        }
    }
}

/// Card heading for one hotkey: the visible title and the empty-field placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyHeading {
    /// Trimmed custom label, or [`Self::fallback`] when the label is blank.
    pub title: String,
    /// Numbered name (`Hotkey (n)`) used when the custom label is empty.
    pub fallback: String,
}

/// Headings in list order. A non-blank label is the title and does not consume
/// a number. A blank or whitespace-only label becomes `Hotkey (n)`, then `n`
/// increases by 1.
pub fn hotkey_headings(hotkeys: &[Hotkey]) -> Vec<HotkeyHeading> {
    let mut next = 1u32;
    hotkeys
        .iter()
        .map(|hotkey| {
            let fallback = format!("Hotkey ({next})");
            let label = hotkey.label.trim();
            if label.is_empty() {
                next += 1;
                HotkeyHeading {
                    title: fallback.clone(),
                    fallback,
                }
            } else {
                HotkeyHeading {
                    title: label.to_string(),
                    fallback,
                }
            }
        })
        .collect()
}

/// Generate a unique id for a new [`Hotkey`].
pub fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let counter = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("hk-{nanos}-{counter}")
}

/// The "Turn Off Displays" behavior for `ActionType::Off` actions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TurnOffBehavior {
    #[default]
    None,
    /// Soft off via broadcast `WM_SYSCOMMAND` / `SC_MONITORPOWER`.
    Soft,
    /// DDC/CI power-off command sent to each selected monitor.
    Ddc,
    /// Both soft-off and DDC/CI power-off.
    Both,
}

impl TurnOffBehavior {
    pub const ALL: &'static [TurnOffBehavior] = &[
        TurnOffBehavior::None,
        TurnOffBehavior::Soft,
        TurnOffBehavior::Ddc,
        TurnOffBehavior::Both,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TurnOffBehavior::None => "None",
            TurnOffBehavior::Soft => "Soft (Windows monitor sleep)",
            TurnOffBehavior::Ddc => "DDC/CI power off",
            TurnOffBehavior::Both => "Both",
        }
    }

    pub fn uses_soft(self) -> bool {
        matches!(self, TurnOffBehavior::Soft | TurnOffBehavior::Both)
    }

    pub fn uses_ddc(self) -> bool {
        matches!(self, TurnOffBehavior::Ddc | TurnOffBehavior::Both)
    }
}

// ---------------------------------------------------------------------------
// Legacy (pre-action-chain) binding shapes — kept only to migrate old
// config.json files into the new `Hotkey`/`HotkeyActionSpec` model.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct LegacyInputSwitchBinding {
    monitor_id: u32,
    input_source: InputSource,
    hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyBrightnessBinding {
    monitor_id: u32,
    direction: LegacyStepDirection,
    hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyContrastBinding {
    monitor_id: u32,
    direction: LegacyStepDirection,
    hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyPowerModeBinding {
    monitor_id: u32,
    power_mode: PowerMode,
    hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyProfileBinding {
    profile_name: String,
    hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
enum LegacyStepDirection {
    Up,
    Down,
}

// ---------------------------------------------------------------------------
// Top-level configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default)]
    pub hotkeys: Vec<Hotkey>,
    /// Step size for brightness hotkey increments/decrements (used by the
    /// legacy migration and as the default value for new offset actions).
    pub brightness_step: u16,
    /// Step size for contrast hotkey increments/decrements.
    pub contrast_step: u16,

    // -- Legacy fields, only populated when loading an old config.json --
    #[serde(default, skip_serializing)]
    input_switch_bindings: Vec<LegacyInputSwitchBinding>,
    #[serde(default, skip_serializing)]
    brightness_bindings: Vec<LegacyBrightnessBinding>,
    #[serde(default, skip_serializing)]
    contrast_bindings: Vec<LegacyContrastBinding>,
    #[serde(default, skip_serializing)]
    power_mode_bindings: Vec<LegacyPowerModeBinding>,
    #[serde(default, skip_serializing)]
    profile_bindings: Vec<LegacyProfileBinding>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            hotkeys: Vec::new(),
            brightness_step: 10,
            contrast_step: 10,
            input_switch_bindings: Vec::new(),
            brightness_bindings: Vec::new(),
            contrast_bindings: Vec::new(),
            power_mode_bindings: Vec::new(),
            profile_bindings: Vec::new(),
        }
    }
}

impl HotkeyConfig {
    /// Convert any legacy (pre-action-chain) bindings into the new
    /// `Hotkey`/`HotkeyActionSpec` model. No-op if there is nothing to
    /// migrate. Legacy fields are always drained so they never round-trip
    /// back into a saved config.
    fn migrate_legacy(&mut self) {
        let brightness_step = self.brightness_step as i32;
        let contrast_step = self.contrast_step as i32;

        for b in self.input_switch_bindings.drain(..) {
            self.hotkeys.push(Hotkey {
                id: new_id(),
                label: String::new(),
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Set,
                    target: ActionTarget::InputSource,
                    all_monitors: false,
                    monitors: vec![b.monitor_id.into()],
                    input_source: b.input_source,
                    monitor_inputs: vec![MonitorInput {
                        monitor_id: b.monitor_id.into(),
                        input_source: b.input_source,
                    }],
                    ..Default::default()
                }],
            });
        }

        for b in self.brightness_bindings.drain(..) {
            let value = match b.direction {
                LegacyStepDirection::Up => brightness_step,
                LegacyStepDirection::Down => -brightness_step,
            };
            self.hotkeys.push(Hotkey {
                id: new_id(),
                label: String::new(),
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Offset,
                    target: ActionTarget::Brightness,
                    all_monitors: false,
                    monitors: vec![b.monitor_id.into()],
                    value,
                    ..Default::default()
                }],
            });
        }

        for b in self.contrast_bindings.drain(..) {
            let value = match b.direction {
                LegacyStepDirection::Up => contrast_step,
                LegacyStepDirection::Down => -contrast_step,
            };
            self.hotkeys.push(Hotkey {
                id: new_id(),
                label: String::new(),
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Offset,
                    target: ActionTarget::Contrast,
                    all_monitors: false,
                    monitors: vec![b.monitor_id.into()],
                    value,
                    ..Default::default()
                }],
            });
        }

        for b in self.power_mode_bindings.drain(..) {
            self.hotkeys.push(Hotkey {
                id: new_id(),
                label: String::new(),
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Set,
                    target: ActionTarget::PowerMode,
                    all_monitors: false,
                    monitors: vec![b.monitor_id.into()],
                    power_mode: b.power_mode,
                    ..Default::default()
                }],
            });
        }

        for b in self.profile_bindings.drain(..) {
            self.hotkeys.push(Hotkey {
                id: new_id(),
                label: String::new(),
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Set,
                    target: ActionTarget::Profile,
                    all_monitors: true,
                    profile_name: b.profile_name,
                    ..Default::default()
                }],
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Unversioned files are schema 0 and migrate on load, without writing.
    #[serde(default)]
    schema_version: u32,
    pub hotkeys: HotkeyConfig,
    /// Refresh interval in seconds for polling monitor state (0 = disabled).
    pub refresh_interval_secs: u64,
    /// Whether global hotkeys are enabled.
    #[serde(default = "default_hotkeys_enabled")]
    pub hotkeys_enabled: bool,
    /// Behavior for `ActionType::Off` actions ("Turn Off Displays").
    #[serde(default)]
    pub turn_off_behavior: TurnOffBehavior,
    /// Register a per-user Run key so the app launches at sign-in.
    #[serde(default)]
    pub start_with_windows: bool,
    /// When starting with Windows, pass `--minimized` so the first window stays in the tray.
    /// Kept when `start_with_windows` is off so turning startup back on restores the choice.
    #[serde(default)]
    pub start_minimized: bool,
}

fn default_hotkeys_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            hotkeys: HotkeyConfig::default(),
            refresh_interval_secs: 0,
            hotkeys_enabled: true,
            turn_off_behavior: TurnOffBehavior::default(),
            start_with_windows: false,
            start_minimized: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

impl AppConfig {
    /// Path to the JSON configuration file.
    pub fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("windisplaymanager").join("config.json")
    }

    pub fn load_from(path: &Path) -> LoadOutcome<Self> {
        persistence::load(path, Self::decode)
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut config: Self = persistence::decode_json(bytes)?;
        config.hotkeys.migrate_legacy();
        config.schema_version = SCHEMA_VERSION;
        Ok(config)
    }

    /// Inert runtime state, not a successful load or permission to save defaults.
    pub fn recovery_placeholder() -> Self {
        Self {
            hotkeys_enabled: false,
            ..Self::default()
        }
    }
}

/// The write gate survives failed loads, including subsequent removal of the
/// bad file. Only an explicit retry, backup recovery, or confirmed reset clears it.
#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    recovery_error: Option<String>,
}

impl ConfigStore {
    pub fn open(path: PathBuf) -> (Self, LoadOutcome<AppConfig>) {
        let mut store = Self {
            path,
            recovery_error: None,
        };
        let outcome = store.retry();
        (store, outcome)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn recovery_error(&self) -> Option<&str> {
        self.recovery_error.as_deref()
    }

    pub fn retry(&mut self) -> LoadOutcome<AppConfig> {
        let outcome = AppConfig::load_from(&self.path);
        self.recovery_error = match &outcome {
            LoadOutcome::Failed(error) => Some(error.to_string()),
            _ => None,
        };
        outcome
    }

    fn ensure_writable(&self) -> io::Result<()> {
        if let Some(error) = &self.recovery_error {
            return Err(io::Error::other(format!(
                "Configuration recovery required: {error}. Retry, recover a backup, or explicitly reset first."
            )));
        }
        Ok(())
    }

    pub fn save(&mut self, config: &AppConfig) -> io::Result<()> {
        self.ensure_writable()?;
        let bytes = serde_json::to_vec_pretty(config).map_err(persistence::invalid_data)?;
        let result = persistence::save_with_backup(&self.path, &bytes, WriteMode::Replace, |old| {
            AppConfig::decode(old).map(|_| ())
        });
        if result.is_err() {
            // Detect corruption or an unreadable primary that appeared since load.
            if let LoadOutcome::Failed(error) = AppConfig::load_from(&self.path) {
                self.recovery_error = Some(error.to_string());
            }
        }
        result
    }

    pub fn set_hotkeys_enabled(&mut self, config: &mut AppConfig, enabled: bool) -> io::Result<()> {
        self.ensure_writable()?;
        let mut updated = config.clone();
        updated.hotkeys_enabled = enabled;
        self.save(&updated)?;
        *config = updated;
        Ok(())
    }

    pub fn recover_backup(&mut self) -> io::Result<AppConfig> {
        let backup = persistence::backup_path(&self.path);
        let config = match AppConfig::load_from(&backup) {
            LoadOutcome::Loaded(config) => config,
            LoadOutcome::Missing => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "No configuration backup exists",
                ));
            }
            LoadOutcome::Failed(error) => return Err(error),
        };
        self.replace_for_recovery(&config)?;
        Ok(config)
    }

    /// Call only after the user explicitly confirms discarding the primary file.
    pub fn reset_confirmed(&mut self) -> io::Result<AppConfig> {
        let config = AppConfig::default();
        self.replace_for_recovery(&config)?;
        Ok(config)
    }

    fn replace_for_recovery(&mut self, config: &AppConfig) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(config).map_err(persistence::invalid_data)?;
        persistence::atomic_write(&self.path, &bytes, WriteMode::Replace)?;
        self.recovery_error = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Runtime hotkey → action-chain mapping
// ---------------------------------------------------------------------------

/// Build a mapping from global-hotkey OS id → the hotkey's action chain.
pub fn build_hotkey_map(config: &HotkeyConfig) -> HashMap<u32, (HotKey, Vec<HotkeyActionSpec>)> {
    let mut map = HashMap::new();
    for hotkey in &config.hotkeys {
        if let Some(hk) = hotkey.binding.to_hotkey() {
            map.insert(hk.id(), (hk, hotkey.actions.clone()));
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Key-code string ↔ global_hotkey::Code conversion
// ---------------------------------------------------------------------------

pub fn string_to_code(s: &str) -> Option<Code> {
    Some(match s {
        // Letters
        "A" => Code::KeyA,
        "B" => Code::KeyB,
        "C" => Code::KeyC,
        "D" => Code::KeyD,
        "E" => Code::KeyE,
        "F" => Code::KeyF,
        "G" => Code::KeyG,
        "H" => Code::KeyH,
        "I" => Code::KeyI,
        "J" => Code::KeyJ,
        "K" => Code::KeyK,
        "L" => Code::KeyL,
        "M" => Code::KeyM,
        "N" => Code::KeyN,
        "O" => Code::KeyO,
        "P" => Code::KeyP,
        "Q" => Code::KeyQ,
        "R" => Code::KeyR,
        "S" => Code::KeyS,
        "T" => Code::KeyT,
        "U" => Code::KeyU,
        "V" => Code::KeyV,
        "W" => Code::KeyW,
        "X" => Code::KeyX,
        "Y" => Code::KeyY,
        "Z" => Code::KeyZ,
        // Digits
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        // Function keys
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "F13" => Code::F13,
        "F14" => Code::F14,
        "F15" => Code::F15,
        "F16" => Code::F16,
        "F17" => Code::F17,
        "F18" => Code::F18,
        "F19" => Code::F19,
        "F20" => Code::F20,
        "F21" => Code::F21,
        "F22" => Code::F22,
        "F23" => Code::F23,
        "F24" => Code::F24,
        // Arrows
        "ArrowUp" => Code::ArrowUp,
        "ArrowDown" => Code::ArrowDown,
        "ArrowLeft" => Code::ArrowLeft,
        "ArrowRight" => Code::ArrowRight,
        // Numpad
        "Numpad0" => Code::Numpad0,
        "Numpad1" => Code::Numpad1,
        "Numpad2" => Code::Numpad2,
        "Numpad3" => Code::Numpad3,
        "Numpad4" => Code::Numpad4,
        "Numpad5" => Code::Numpad5,
        "Numpad6" => Code::Numpad6,
        "Numpad7" => Code::Numpad7,
        "Numpad8" => Code::Numpad8,
        "Numpad9" => Code::Numpad9,
        "NumpadAdd" => Code::NumpadAdd,
        "NumpadSubtract" => Code::NumpadSubtract,
        // Misc
        "Space" => Code::Space,
        "Enter" => Code::Enter,
        "Escape" => Code::Escape,
        "Backspace" => Code::Backspace,
        "Tab" => Code::Tab,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "Insert" => Code::Insert,
        "Delete" => Code::Delete,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{backup_path, tests::TestDir};
    use std::fs;

    #[test]
    fn startup_flags_default_off_when_missing_from_saved_config() {
        let defaults = AppConfig::default();
        assert!(!defaults.start_with_windows);
        assert!(!defaults.start_minimized);

        let mut value = serde_json::to_value(&defaults).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("start_with_windows");
        object.remove("start_minimized");
        let config = AppConfig::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(!config.start_with_windows);
        assert!(!config.start_minimized);
    }

    #[test]
    fn load_outcomes_distinguish_missing_loaded_and_io_failure() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        let (mut store, outcome) = ConfigStore::open(path.clone());
        assert!(matches!(outcome, LoadOutcome::Missing));
        assert!(!path.exists());
        store.save(&AppConfig::default()).unwrap();
        assert!(matches!(store.retry(), LoadOutcome::Loaded(_)));
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(matches!(store.retry(), LoadOutcome::Failed(_)));
        assert!(store.save(&AppConfig::default()).is_err());
        assert!(store.reset_confirmed().is_err());
        assert!(store.recovery_error().is_some());
        assert!(path.is_dir());
        dir.assert_no_temps();
    }

    #[test]
    fn malformed_and_future_config_block_toggle_and_save_until_explicit_retry() {
        let mut future = serde_json::to_value(AppConfig::default()).unwrap();
        future["schema_version"] = serde_json::json!(SCHEMA_VERSION + 1);
        let future = serde_json::to_vec(&future).unwrap();
        for original in [b"{bad json".as_slice(), future.as_slice()] {
            let dir = TestDir::new();
            let path = dir.0.join("config.json");
            let backup = serde_json::to_vec(&AppConfig::default()).unwrap();
            fs::write(&path, original).unwrap();
            fs::write(backup_path(&path), &backup).unwrap();
            let (mut store, outcome) = ConfigStore::open(path.clone());
            assert!(matches!(outcome, LoadOutcome::Failed(_)));
            let mut config = AppConfig::recovery_placeholder();
            for enabled in [true, false] {
                assert!(store.set_hotkeys_enabled(&mut config, enabled).is_err());
                assert!(!config.hotkeys_enabled);
                assert!(store.save(&config).is_err());
            }
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(fs::read(backup_path(&path)).unwrap(), backup);
            fs::remove_file(&path).unwrap();
            assert!(store.save(&config).is_err());
            assert!(!path.exists());
            assert!(matches!(store.retry(), LoadOutcome::Missing));
            store.save(&AppConfig::default()).unwrap();
            dir.assert_no_temps();
        }
    }

    #[test]
    fn corruption_after_load_also_blocks_save_and_preserves_backup() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        let (mut store, _) = ConfigStore::open(path.clone());
        let mut config = AppConfig::default();
        store.save(&config).unwrap();
        store.set_hotkeys_enabled(&mut config, false).unwrap();
        let backup = fs::read(backup_path(&path)).unwrap();
        fs::write(&path, b"corrupted externally").unwrap();
        assert!(store.set_hotkeys_enabled(&mut config, true).is_err());
        assert!(!config.hotkeys_enabled);
        assert!(store.recovery_error().is_some());
        assert!(store.save(&config).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"corrupted externally");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), backup);
        dir.assert_no_temps();
    }

    #[test]
    fn explicit_backup_recovery_and_reset_keep_the_good_backup() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        let good = AppConfig {
            hotkeys_enabled: false,
            ..AppConfig::default()
        };
        let backup = serde_json::to_vec(&good).unwrap();
        fs::write(&path, b"bad").unwrap();
        fs::write(backup_path(&path), &backup).unwrap();
        let (mut store, _) = ConfigStore::open(path.clone());
        let recovered = store.recover_backup().unwrap();
        assert!(!recovered.hotkeys_enabled);
        assert!(store.recovery_error().is_none());
        assert!(matches!(
            AppConfig::load_from(&path),
            LoadOutcome::Loaded(_)
        ));
        assert_eq!(fs::read(backup_path(&path)).unwrap(), backup);
        fs::write(&path, br#"{"schema_version":999}"#).unwrap();
        assert!(matches!(store.retry(), LoadOutcome::Failed(_)));
        let reset = store.reset_confirmed().unwrap();
        assert!(reset.hotkeys_enabled);
        assert!(store.recovery_error().is_none());
        assert_eq!(fs::read(backup_path(&path)).unwrap(), backup);
        assert!(matches!(
            AppConfig::load_from(&path),
            LoadOutcome::Loaded(_)
        ));
        dir.assert_no_temps();
    }

    #[test]
    fn missing_invalid_or_future_backup_cannot_exit_recovery() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        fs::write(&path, b"original invalid file").unwrap();
        let (mut store, _) = ConfigStore::open(path.clone());
        assert!(store.recover_backup().is_err());
        for bytes in [b"bad backup".as_slice(), br#"{"schema_version":999}"#] {
            fs::write(backup_path(&path), bytes).unwrap();
            assert!(store.recover_backup().is_err());
            assert!(store.recovery_error().is_some());
            assert_eq!(fs::read(&path).unwrap(), b"original invalid file");
            assert_eq!(fs::read(backup_path(&path)).unwrap(), bytes);
        }
        dir.assert_no_temps();
    }

    #[test]
    fn stable_numeric_and_unknown_targets_roundtrip_through_schema1_store() {
        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        let mut config = AppConfig::default();
        let mut hotkey = Hotkey::new_empty();
        let key = crate::ddc::tests::key(1);
        let unknown: MonitorTarget = serde_json::from_str("\"future-monitor:v2:abc\"").unwrap();
        hotkey.actions[0] = HotkeyActionSpec {
            target: ActionTarget::InputSource,
            all_monitors: false,
            monitors: vec![
                1.into(),
                MonitorTarget::Stable(key.clone()),
                unknown.clone(),
            ],
            monitor_inputs: vec![MonitorInput {
                monitor_id: 1.into(),
                input_source: InputSource::Hdmi2,
            }],
            ..Default::default()
        };
        config.hotkeys.hotkeys.push(hotkey);
        let (mut store, _) = ConfigStore::open(path.clone());
        store.save(&config).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["schema_version"], 1);
        assert_eq!(
            saved["hotkeys"]["hotkeys"][0]["actions"][0]["monitors"][0],
            1
        );
        assert_eq!(
            saved["hotkeys"]["hotkeys"][0]["actions"][0]["monitor_inputs"][0]["monitor_id"],
            1
        );
        let LoadOutcome::Loaded(reloaded) = store.retry() else {
            panic!("reload failed")
        };
        let action = &reloaded.hotkeys.hotkeys[0].actions[0];
        assert_eq!(
            action.monitors,
            vec![1.into(), MonitorTarget::Stable(key), unknown]
        );
        assert_eq!(action.monitor_inputs[0].input_source, InputSource::Hdmi2);
        assert!(
            action
                .resolve_monitors(&[crate::ddc::tests::monitor_state().info])
                .is_err()
        );
    }

    #[test]
    fn explicit_rebind_preserves_input_values_and_rejects_collisions() {
        let mut action = HotkeyActionSpec {
            target: ActionTarget::InputSource,
            all_monitors: false,
            monitors: vec![1.into(), 2.into()],
            monitor_inputs: vec![
                MonitorInput {
                    monitor_id: 1.into(),
                    input_source: InputSource::Hdmi2,
                },
                MonitorInput {
                    monitor_id: 2.into(),
                    input_source: InputSource::Dp1,
                },
            ],
            ..Default::default()
        };
        let live = crate::ddc::tests::monitor_state().info;
        assert!(
            action
                .resolve_monitors(std::slice::from_ref(&live))
                .is_err()
        );
        action.rebind_target(&1.into(), live.key.clone()).unwrap();
        assert_eq!(action.monitor_inputs[0].input_source, InputSource::Hdmi2);
        assert_eq!(action.monitors[0], MonitorTarget::Stable(live.key.clone()));
        assert!(action.rebind_target(&2.into(), live.key.clone()).is_err());
        assert_eq!(action.monitor_inputs[1].monitor_id, 2.into());
        assert_eq!(action.monitor_inputs[1].input_source, InputSource::Dp1);
        // The still-unresolved second entry blocks the entire action.
        assert!(
            action
                .resolve_monitors(std::slice::from_ref(&live))
                .is_err()
        );
        action.remove_target(&2.into());
        assert_eq!(action.resolve_monitors(&[live]).unwrap(), [1]);
        assert_eq!(action.monitor_inputs[0].input_source, InputSource::Hdmi2);
    }

    #[test]
    fn whole_action_resolution_rejects_legacy_missing_duplicate_and_empty_targets_including_off() {
        let live = crate::ddc::tests::monitor_state().info;
        let stable = MonitorTarget::Stable(live.key.clone());
        for action_type in [ActionType::Set, ActionType::Offset, ActionType::Off] {
            for targets in [
                vec![stable.clone(), 1.into()],
                vec![
                    stable.clone(),
                    MonitorTarget::Stable(crate::ddc::tests::key(2)),
                ],
                vec![stable.clone(), stable.clone()],
                Vec::new(),
            ] {
                let action = HotkeyActionSpec {
                    action_type,
                    all_monitors: false,
                    monitors: targets,
                    ..Default::default()
                };
                assert!(
                    action
                        .resolve_monitors(std::slice::from_ref(&live))
                        .is_err()
                );
            }
        }
        let action = HotkeyActionSpec {
            all_monitors: true,
            monitors: vec![99.into()],
            ..Default::default()
        };
        assert_eq!(
            action
                .resolve_monitors(std::slice::from_ref(&live))
                .unwrap(),
            [1]
        );
        assert!(action.resolve_monitors(&[]).is_err());
        assert!(action.resolve_monitors(&[live.clone(), live]).is_err());
    }

    #[test]
    fn explicit_resolution_uses_keys_after_numbers_change() {
        let mut live = crate::ddc::tests::monitor_state().info;
        let action = HotkeyActionSpec {
            all_monitors: false,
            monitors: vec![MonitorTarget::Stable(live.key.clone())],
            ..Default::default()
        };
        live.id = 27;
        assert_eq!(action.resolve_monitors(&[live]).unwrap(), [27]);
    }

    #[test]
    fn legacy_bindings_migrate_to_explicit_action_targets() {
        let json = r#"
                {
                    "hotkeys": {
                        "hotkeys": [],
                        "brightness_step": 7,
                        "contrast_step": 9,
                        "input_switch_bindings": [{
                            "monitor_id": 4,
                            "input_source": "Hdmi2",
                            "hotkey": {"ctrl": true, "alt": false, "shift": false, "win": false, "key": "F1"}
                        }],
                        "brightness_bindings": [{
                            "monitor_id": 5,
                            "direction": "Down",
                            "hotkey": {"ctrl": true, "alt": false, "shift": false, "win": false, "key": "F2"}
                        }],
                        "contrast_bindings": [{
                            "monitor_id": 6,
                            "direction": "Up",
                            "hotkey": {"ctrl": true, "alt": false, "shift": false, "win": false, "key": "F3"}
                        }],
                        "power_mode_bindings": [{
                            "monitor_id": 7,
                            "power_mode": "Off",
                            "hotkey": {"ctrl": true, "alt": false, "shift": false, "win": false, "key": "F4"}
                        }],
                        "profile_bindings": []
                    },
                    "refresh_interval_secs": 0,
                    "hotkeys_enabled": true,
                    "turn_off_behavior": "None"
                }
                "#;

        let dir = TestDir::new();
        let path = dir.0.join("config.json");
        fs::write(&path, json).unwrap();
        let (mut store, outcome) = ConfigStore::open(path.clone());
        let LoadOutcome::Loaded(config) = outcome else {
            panic!("Legacy config did not load")
        };
        assert_eq!(config.schema_version, SCHEMA_VERSION);
        assert_eq!(fs::read_to_string(&path).unwrap(), json);
        store.save(&config).unwrap();
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), json);
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"schema_version\": 1"));
        assert!(!saved.contains("input_switch_bindings"));
        let LoadOutcome::Loaded(reloaded) = store.retry() else {
            panic!("Migrated config did not reload")
        };
        assert_eq!(reloaded.hotkeys.hotkeys.len(), 4);
        assert_eq!(reloaded.hotkeys.hotkeys[0].id, config.hotkeys.hotkeys[0].id);
        dir.assert_no_temps();

        assert_eq!(config.hotkeys.hotkeys.len(), 4);
        let input = &config.hotkeys.hotkeys[0].actions[0];
        assert!(!input.all_monitors);
        assert_eq!(input.monitors, vec![4.into()]);
        assert_eq!(input.monitor_inputs.len(), 1);
        assert_eq!(input.monitor_inputs[0].monitor_id, 4.into());
        assert_eq!(input.monitor_inputs[0].input_source, InputSource::Hdmi2);

        let brightness = &config.hotkeys.hotkeys[1].actions[0];
        assert!(!brightness.all_monitors);
        assert_eq!(brightness.monitors, vec![5.into()]);
        assert_eq!(brightness.value, -7);

        let contrast = &config.hotkeys.hotkeys[2].actions[0];
        assert!(!contrast.all_monitors);
        assert_eq!(contrast.monitors, vec![6.into()]);
        assert_eq!(contrast.value, 9);

        let power = &config.hotkeys.hotkeys[3].actions[0];
        assert!(!power.all_monitors);
        assert_eq!(power.monitors, vec![7.into()]);
        assert_eq!(power.power_mode, PowerMode::Off);
    }

    #[test]
    fn missing_hotkey_label_deserializes_as_empty() {
        let hotkey = Hotkey::new_empty();
        let mut value = serde_json::to_value(&hotkey).unwrap();
        value.as_object_mut().unwrap().remove("label");
        let loaded: Hotkey = serde_json::from_value(value).unwrap();
        assert!(loaded.label.is_empty());
    }

    #[test]
    fn blank_hotkey_labels_are_numbered_and_custom_labels_skip_the_count() {
        let mut hotkeys = vec![
            Hotkey::new_empty(),
            Hotkey::new_empty(),
            Hotkey::new_empty(),
            Hotkey::new_empty(),
        ];
        hotkeys[1].label = "  Work  ".into();
        hotkeys[2].label = "   ".into();

        let headings = hotkey_headings(&hotkeys);
        assert_eq!(headings[0].title, "Hotkey (1)");
        assert_eq!(headings[0].fallback, "Hotkey (1)");
        assert_eq!(headings[1].title, "Work");
        assert_eq!(headings[1].fallback, "Hotkey (2)");
        assert_eq!(headings[2].title, "Hotkey (2)");
        assert_eq!(headings[2].fallback, "Hotkey (2)");
        assert_eq!(headings[3].title, "Hotkey (3)");
        assert_eq!(headings[3].fallback, "Hotkey (3)");
    }
}
