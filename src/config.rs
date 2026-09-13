use crate::ddc::{InputSource, PowerMode};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
    pub monitor_id: u32,
    pub input_source: InputSource,
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
    pub monitors: Vec<u32>,
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
    pub binding: HotkeyBinding,
    pub actions: Vec<HotkeyActionSpec>,
}

impl Hotkey {
    /// A new, unbound hotkey with a single default action.
    pub fn new_empty() -> Self {
        Self {
            id: new_id(),
            binding: HotkeyBinding::unbound(),
            actions: vec![HotkeyActionSpec::default()],
        }
    }

    /// A new, unbound hotkey with a single "apply profile" action.
    pub fn new_for_profile(profile_name: String) -> Self {
        Self {
            id: new_id(),
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
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Set,
                    target: ActionTarget::InputSource,
                    monitors: vec![b.monitor_id],
                    input_source: b.input_source,
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
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Offset,
                    target: ActionTarget::Brightness,
                    monitors: vec![b.monitor_id],
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
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Offset,
                    target: ActionTarget::Contrast,
                    monitors: vec![b.monitor_id],
                    value,
                    ..Default::default()
                }],
            });
        }

        for b in self.power_mode_bindings.drain(..) {
            self.hotkeys.push(Hotkey {
                id: new_id(),
                binding: b.hotkey,
                actions: vec![HotkeyActionSpec {
                    action_type: ActionType::Set,
                    target: ActionTarget::PowerMode,
                    monitors: vec![b.monitor_id],
                    power_mode: b.power_mode,
                    ..Default::default()
                }],
            });
        }

        for b in self.profile_bindings.drain(..) {
            self.hotkeys.push(Hotkey {
                id: new_id(),
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
    pub hotkeys: HotkeyConfig,
    /// Refresh interval in seconds for polling monitor state (0 = disabled).
    pub refresh_interval_secs: u64,
    /// Whether global hotkeys are enabled.
    #[serde(default = "default_hotkeys_enabled")]
    pub hotkeys_enabled: bool,
    /// Behavior for `ActionType::Off` actions ("Turn Off Displays").
    #[serde(default)]
    pub turn_off_behavior: TurnOffBehavior,
}

fn default_hotkeys_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkeys: HotkeyConfig::default(),
            refresh_interval_secs: 0,
            hotkeys_enabled: true,
            turn_off_behavior: TurnOffBehavior::default(),
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

    /// Load configuration from disk, falling back to defaults.
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str::<Self>(&contents) {
                    Ok(mut cfg) => {
                        cfg.hotkeys.migrate_legacy();
                        return cfg;
                    }
                    Err(e) => {
                        log::warn!("Failed to parse config: {e}. Using defaults.");
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read config file: {e}. Using defaults.");
                }
            }
        }
        Self::default()
    }

    /// Save configuration to disk.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(&path, json)
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
