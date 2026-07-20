use crate::ddc::{InputSource, PowerMode};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
// Per-action hotkey configs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSwitchBinding {
    pub monitor_id: u32,
    pub input_source: InputSource,
    pub hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrightnessBinding {
    pub monitor_id: u32,
    pub direction: StepDirection,
    pub hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContrastBinding {
    pub monitor_id: u32,
    pub direction: StepDirection,
    pub hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerModeBinding {
    pub monitor_id: u32,
    pub power_mode: PowerMode,
    pub hotkey: HotkeyBinding,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepDirection {
    Up,
    Down,
}

// ---------------------------------------------------------------------------
// Top-level configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub input_switch_bindings: Vec<InputSwitchBinding>,
    pub brightness_bindings: Vec<BrightnessBinding>,
    pub contrast_bindings: Vec<ContrastBinding>,
    #[serde(default)]
    pub power_mode_bindings: Vec<PowerModeBinding>,
    /// Step size for brightness hotkey increments/decrements.
    pub brightness_step: u16,
    /// Step size for contrast hotkey increments/decrements.
    pub contrast_step: u16,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            input_switch_bindings: Vec::new(),
            brightness_bindings: Vec::new(),
            contrast_bindings: Vec::new(),
            power_mode_bindings: Vec::new(),
            brightness_step: 10,
            contrast_step: 10,
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
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(cfg) => return cfg,
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
// HotkeyAction  (runtime action tag, not serialized – used for ID mapping)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HotkeyAction {
    SwitchInput {
        monitor_id: u32,
        input_source: InputSource,
    },
    BrightnessUp {
        monitor_id: u32,
    },
    BrightnessDown {
        monitor_id: u32,
    },
    ContrastUp {
        monitor_id: u32,
    },
    ContrastDown {
        monitor_id: u32,
    },
    SetPowerMode {
        monitor_id: u32,
        power_mode: PowerMode,
    },
}

/// Build a mapping from global-hotkey ID → action from the loaded config.
pub fn build_hotkey_map(config: &HotkeyConfig) -> HashMap<u32, (HotKey, HotkeyAction)> {
    let mut map = HashMap::new();

    for binding in &config.input_switch_bindings {
        if let Some(hk) = binding.hotkey.to_hotkey() {
            let action = HotkeyAction::SwitchInput {
                monitor_id: binding.monitor_id,
                input_source: binding.input_source,
            };
            map.insert(hk.id(), (hk, action));
        }
    }

    for binding in &config.brightness_bindings {
        if let Some(hk) = binding.hotkey.to_hotkey() {
            let action = match binding.direction {
                StepDirection::Up => HotkeyAction::BrightnessUp {
                    monitor_id: binding.monitor_id,
                },
                StepDirection::Down => HotkeyAction::BrightnessDown {
                    monitor_id: binding.monitor_id,
                },
            };
            map.insert(hk.id(), (hk, action));
        }
    }

    for binding in &config.contrast_bindings {
        if let Some(hk) = binding.hotkey.to_hotkey() {
            let action = match binding.direction {
                StepDirection::Up => HotkeyAction::ContrastUp {
                    monitor_id: binding.monitor_id,
                },
                StepDirection::Down => HotkeyAction::ContrastDown {
                    monitor_id: binding.monitor_id,
                },
            };
            map.insert(hk.id(), (hk, action));
        }
    }

    for binding in &config.power_mode_bindings {
        if let Some(hk) = binding.hotkey.to_hotkey() {
            let action = HotkeyAction::SetPowerMode {
                monitor_id: binding.monitor_id,
                power_mode: binding.power_mode,
            };
            map.insert(hk.id(), (hk, action));
        }
    }

    map
}

// ---------------------------------------------------------------------------
// Key-code string ↔ global_hotkey::Code conversion
// ---------------------------------------------------------------------------

fn string_to_code(s: &str) -> Option<Code> {
    Some(match s {
        // Letters
        "KeyA" => Code::KeyA,
        "KeyB" => Code::KeyB,
        "KeyC" => Code::KeyC,
        "KeyD" => Code::KeyD,
        "KeyE" => Code::KeyE,
        "KeyF" => Code::KeyF,
        "KeyG" => Code::KeyG,
        "KeyH" => Code::KeyH,
        "KeyI" => Code::KeyI,
        "KeyJ" => Code::KeyJ,
        "KeyK" => Code::KeyK,
        "KeyL" => Code::KeyL,
        "KeyM" => Code::KeyM,
        "KeyN" => Code::KeyN,
        "KeyO" => Code::KeyO,
        "KeyP" => Code::KeyP,
        "KeyQ" => Code::KeyQ,
        "KeyR" => Code::KeyR,
        "KeyS" => Code::KeyS,
        "KeyT" => Code::KeyT,
        "KeyU" => Code::KeyU,
        "KeyV" => Code::KeyV,
        "KeyW" => Code::KeyW,
        "KeyX" => Code::KeyX,
        "KeyY" => Code::KeyY,
        "KeyZ" => Code::KeyZ,
        // Digits
        "Digit0" => Code::Digit0,
        "Digit1" => Code::Digit1,
        "Digit2" => Code::Digit2,
        "Digit3" => Code::Digit3,
        "Digit4" => Code::Digit4,
        "Digit5" => Code::Digit5,
        "Digit6" => Code::Digit6,
        "Digit7" => Code::Digit7,
        "Digit8" => Code::Digit8,
        "Digit9" => Code::Digit9,
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
