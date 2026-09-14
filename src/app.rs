mod actions;

use self::actions::{ActionExecutor, HardwareJob, HardwareOutcome};
use crate::config::{
    ActionTarget, ActionType, AppConfig, Hotkey, HotkeyActionSpec, HotkeyBinding, MonitorInput,
    TurnOffBehavior,
};
use crate::ddc::{self, InputSource, MonitorInfo, MonitorState, PowerMode};
use crate::hotkeys::{self, HotkeyManager};
use crate::profiles;
use crate::tray::{SystemTray, TrayMessage, TrayStream};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::event::{self, Event};
use cosmic::iced::keyboard::{Event as KeyboardEvent, Key, Modifiers};
use cosmic::iced::{Alignment, Length, Subscription, window};
use cosmic::prelude::*;
use cosmic::widget::{self, nav_bar};
use cosmic::{Core, executor};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Simple inline SVG icon (monitor symbol)
const APP_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="100 100 320 280">
  <!-- Monitor -->
  <path d="M 365.047 120.547 L 158.791 120.547 C 143.497 120.554 131.102 132.953 131.101 148.247 L 131.101 276.344 C 131.102 291.636 143.498 304.033 158.791 304.034 L 229.066 304.034 L 229.066 326.233 L 197.294 326.233 C 189.148 326.233 184.056 335.051 188.129 342.106 C 190.02 345.38 193.513 347.397 197.294 347.397 L 326.547 347.397 C 334.693 347.397 339.784 338.579 335.711 331.524 C 333.821 328.25 330.327 326.233 326.547 326.233 L 294.771 326.233 L 294.771 304.035 L 365.046 304.035 C 380.342 304.038 392.744 291.64 392.746 276.344 L 392.746 148.242 C 392.742 132.946 380.342 120.548 365.046 120.547 L 365.047 120.547 Z M 368.507 276.34 C 368.502 278.25 366.956 279.797 365.047 279.804 L 158.791 279.804 C 156.881 279.798 155.336 278.249 155.336 276.34 L 155.336 148.242 C 155.337 146.334 156.883 144.789 158.791 144.788 L 365.047 144.788 C 366.954 144.79 368.501 146.334 368.507 148.242 L 368.507 276.34 Z" data-name="Monitor" style=""></path>
  
  <!-- Centered Lightning Bolt -->
  <path transform="translate(5, -10)" d="M 271 190 L 248.371 190 L 236.157 224.479 C 236.124 224.577 236.197 224.676 236.306 224.681 L 250.319 224.681 C 250.433 224.676 250.517 224.78 250.484 224.882 L 239.64 255.276 C 239.438 255.842 239.967 256.398 240.589 256.277 C 240.792 256.238 240.97 256.13 241.092 255.972 L 273.806 213.863 C 273.882 213.769 273.82 213.631 273.692 213.617 C 273.688 213.617 273.681 213.616 273.674 213.616 L 258.968 213.616 C 258.847 213.603 258.775 213.489 258.819 213.384 L 271.149 190.17 C 271.157 190.085 271.091 190.009 271 190 Z" style="fill: rgb(255, 221, 0);"></path>
</svg>"#;

// ---------------------------------------------------------------------------
// Navigation pages
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Monitor(u32), // 1-indexed monitor ID
    Hotkeys,
    Profiles,
    Settings,
    About,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Message {
    // Monitor controls
    RefreshMonitors,
    RetryMonitor(u32),
    MonitorsDetected(u64, Vec<MonitorInfo>),
    MonitorStateLoaded(u64, u32, Box<MonitorState>),
    MonitorStateFailed(u64, u32, String),
    SetBrightness(u32, u16),
    SetContrast(u32, u16),
    SelectInputSource(u32, usize), // monitor_id, index into INPUT_SOURCES
    SetInputSource(u32, InputSource),
    HardwareJobFinished(Result<HardwareOutcome, String>),
    // Debounced slider changes
    BrightnessSliderChanged(u32, u16),
    ContrastSliderChanged(u32, u16),
    ApplyBrightnessDebounced(u32, u16),
    ApplyContrastDebounced(u32, u16),
    // Hotkeys
    HotkeyTriggered(u32),
    ToggleHotkeys(bool),
    AddHotkey,
    ToggleHotkeyEditor(String),
    DeleteHotkey(String),
    StartRecording(String),
    CancelRecording,
    ClearBinding(String),
    KeyPressed(Modifiers, Key),
    AddAction(String),
    DeleteAction(String, usize),
    SetActionType(String, usize, ActionType),
    SetActionTarget(String, usize, ActionTarget),
    ActionValueDraftChanged(String, usize, String),
    ActionVcpDraftChanged(String, usize, String),
    SetActionInputSource(String, usize, InputSource),
    SetMonitorInput(String, usize, u32, Option<InputSource>),
    SetActionPowerMode(String, usize, PowerMode),
    SetActionProfile(String, usize, String),
    ToggleActionAllMonitors(String, usize, bool),
    ToggleActionMonitor(String, usize, u32, bool),
    SetTurnOffBehavior(TurnOffBehavior),
    SaveConfig,
    // Profiles
    RefreshProfiles,
    ProfilesListed(Vec<String>),
    ProfileNameInput(String),
    SaveCurrentProfile(String),
    ConfirmReplaceProfile,
    CancelReplaceProfile,
    ApplyProfile(String),
    RequestDeleteProfile(String),
    CancelDeleteProfile,
    DeleteProfile(String),
    AddProfileHotkey(String),
    // System tray
    Tray(TrayMessage),
    /// Hide window (close-to-tray)
    HideWindow,
    WindowClosed(window::Id),
    OpenUrl(String),
    // Errors
    Error(String),
}

// ---------------------------------------------------------------------------
// Hotkey recording state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum RecordingState {
    NotRecording,
    Recording { hotkey_id: String },
}

// ---------------------------------------------------------------------------
// Application model
// ---------------------------------------------------------------------------

pub struct AppModel {
    core: Core,
    nav: nav_bar::Model,
    monitor_generation: u64,
    detected_monitors: Vec<MonitorInfo>,
    monitors: Vec<MonitorState>,
    monitor_load_errors: HashMap<u32, String>,
    config: AppConfig,
    hotkey_manager: Option<HotkeyManager>,
    hotkey_action_map: Arc<HashMap<u32, Vec<HotkeyActionSpec>>>,
    hotkey_status: HashMap<String, bool>,
    hotkey_generation: u64,
    status_message: String,
    recording_state: RecordingState,
    expanded_hotkey: Option<String>,
    value_drafts: HashMap<(String, usize), String>,
    vcp_drafts: HashMap<(String, usize), String>,
    config_dirty: bool,
    about: widget::about::About,
    // Debounce state for sliders
    pending_brightness: Option<(u32, u16)>,
    pending_contrast: Option<(u32, u16)>,
    profiles: Vec<String>,
    profile_name_input: String,
    pending_profile_delete: Option<String>,
    pending_profile_replace: Option<String>,
    action_executor: ActionExecutor<HardwareJob>,
    refresh_monitors_after_jobs: bool,
    refresh_profiles_after_jobs: bool,
    // System tray
    tray: Option<(SystemTray, TrayStream)>,
}

// List of input sources shown in the dropdown
const INPUT_SOURCES: &[InputSource] = &[
    InputSource::Hdmi1,
    InputSource::Hdmi2,
    InputSource::Dp1,
    InputSource::Dp2,
    InputSource::UsbC1,
    InputSource::UsbC2,
    InputSource::Vga1,
    InputSource::Vga2,
    InputSource::Dvi1,
    InputSource::Dvi2,
];

const POWER_MODES: &[PowerMode] = &[
    PowerMode::On,
    PowerMode::Standby,
    PowerMode::Suspend,
    PowerMode::Off,
];

fn input_source_index(source: &InputSource) -> Option<usize> {
    INPUT_SOURCES.iter().position(|s| s == source)
}

fn power_mode_index(mode: &PowerMode) -> Option<usize> {
    POWER_MODES.iter().position(|m| m == mode)
}

fn parse_value_draft(value: &str) -> Result<Option<i32>, &'static str> {
    let value = value.trim();
    if value.is_empty() || value == "+" || value == "-" {
        return Ok(None);
    }
    let parsed = value
        .parse::<i32>()
        .map_err(|_| "Enter a signed decimal value")?;
    if !(-200..=200).contains(&parsed) {
        return Err("Value must be between -200 and 200");
    }
    Ok(Some(parsed))
}

fn parse_vcp_draft(value: &str) -> Result<Option<u8>, &'static str> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("0x") {
        return Ok(None);
    }
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let parsed = u16::from_str_radix(digits, 16).map_err(|_| "Enter a hexadecimal VCP code")?;
    u8::try_from(parsed)
        .map(Some)
        .map_err(|_| "VCP code must be between 00 and FF")
}

fn uses_monitor_inputs(action: &HotkeyActionSpec) -> bool {
    action.action_type != ActionType::Off && action.target == ActionTarget::InputSource
}

fn explicit_monitor_ids(action: &HotkeyActionSpec) -> Vec<u32> {
    if uses_monitor_inputs(action) && !action.monitor_inputs.is_empty() {
        action
            .monitor_inputs
            .iter()
            .map(|input| input.monitor_id)
            .collect()
    } else {
        action.monitors.clone()
    }
}

fn action_monitor_selected(action: &HotkeyActionSpec, monitor_id: u32) -> bool {
    action.all_monitors || explicit_monitor_ids(action).contains(&monitor_id)
}

fn action_master_selected(action: &HotkeyActionSpec, detected_ids: &[u32]) -> bool {
    action.all_monitors
        || (!detected_ids.is_empty()
            && detected_ids
                .iter()
                .all(|monitor_id| action_monitor_selected(action, *monitor_id)))
}

fn toggle_action_monitor(
    action: &mut HotkeyActionSpec,
    detected_ids: &[u32],
    monitor_id: u32,
    checked: bool,
) {
    if action.all_monitors {
        action.all_monitors = false;
        action.monitors = detected_ids.to_vec();
        if uses_monitor_inputs(action) {
            action.monitor_inputs = detected_ids
                .iter()
                .map(|monitor_id| MonitorInput {
                    monitor_id: *monitor_id,
                    input_source: action.input_source,
                })
                .collect();
        }
    }

    action.monitors.retain(|id| *id != monitor_id);
    action
        .monitor_inputs
        .retain(|input| input.monitor_id != monitor_id);
    if checked {
        action.monitors.push(monitor_id);
        if uses_monitor_inputs(action) {
            action.monitor_inputs.push(MonitorInput {
                monitor_id,
                input_source: action.input_source,
            });
        }
    }
}

fn resolve_triggered_actions(
    enabled: bool,
    recording: bool,
    action_map: &HashMap<u32, Vec<HotkeyActionSpec>>,
    id: u32,
) -> Option<Vec<HotkeyActionSpec>> {
    if !enabled || recording {
        return None;
    }
    action_map.get(&id).cloned()
}

/// Convert an Iced Key to our internal string format
fn key_to_string(key: &Key) -> String {
    match key {
        Key::Named(named_key) => {
            use cosmic::iced::keyboard::key::Named;
            match named_key {
                Named::F1 => "F1",
                Named::F2 => "F2",
                Named::F3 => "F3",
                Named::F4 => "F4",
                Named::F5 => "F5",
                Named::F6 => "F6",
                Named::F7 => "F7",
                Named::F8 => "F8",
                Named::F9 => "F9",
                Named::F10 => "F10",
                Named::F11 => "F11",
                Named::F12 => "F12",
                Named::ArrowUp => "ArrowUp",
                Named::ArrowDown => "ArrowDown",
                Named::ArrowLeft => "ArrowLeft",
                Named::ArrowRight => "ArrowRight",
                Named::Home => "Home",
                Named::End => "End",
                Named::PageUp => "PageUp",
                Named::PageDown => "PageDown",
                Named::Insert => "Insert",
                Named::Delete => "Delete",
                Named::Enter => "Enter",
                Named::Escape => "Escape",
                Named::Backspace => "Backspace",
                Named::Tab => "Tab",
                _ => return String::new(),
            }
            .to_string()
        }
        Key::Character(c) => {
            let ch = c.chars().next().unwrap_or('?');
            if ch == ' ' {
                "Space".to_string()
            } else if ch.is_ascii_alphabetic() {
                format!("Key{}", ch.to_uppercase())
            } else if ch.is_ascii_digit() {
                format!("Digit{}", ch)
            } else {
                String::new()
            }
        }
        Key::Unidentified => String::new(),
    }
}

/// Format a hotkey combination for display
fn format_hotkey(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> String {
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if win {
        parts.push("Win");
    }
    if !key.is_empty() {
        parts.push(key);
    }
    parts.join(" + ")
}

// ---------------------------------------------------------------------------
// cosmic::Application implementation
// ---------------------------------------------------------------------------

impl cosmic::Application for AppModel {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.windisplaymanager.app";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, cosmic::app::Task<Self::Message>) {
        // Load persistent config
        let config = AppConfig::load();

        // Set up hotkey manager
        let hotkey_manager = HotkeyManager::new(&config);
        let hotkey_action_map = hotkey_manager
            .as_ref()
            .map(|m| m.action_map())
            .unwrap_or_else(|| Arc::new(HashMap::new()));
        let hotkey_status = hotkey_manager
            .as_ref()
            .map(|m| m.status())
            .unwrap_or_default();

        let mut value_drafts = HashMap::new();
        let mut vcp_drafts = HashMap::new();
        for hotkey in &config.hotkeys.hotkeys {
            for (idx, action) in hotkey.actions.iter().enumerate() {
                value_drafts.insert((hotkey.id.clone(), idx), action.value.to_string());
                vcp_drafts.insert(
                    (hotkey.id.clone(), idx),
                    format!("0x{:02X}", action.vcp_code),
                );
            }
        }

        // Build nav model with a placeholder; will be rebuilt after detection
        let mut nav = nav_bar::Model::default();
        nav.insert()
            .text("Detecting monitors...")
            .data::<Page>(Page::Hotkeys)
            .activate();

        let about = widget::about::About::default()
            .name("Windows Display Manager")
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .comments("DDC/CI monitor control with global hotkeys.");

        // Set up system tray
        let tray = match SystemTray::new() {
            Ok(t) => {
                log::info!("System tray created successfully");
                Some(t)
            }
            Err(e) => {
                log::warn!("Failed to create system tray: {e}");
                None
            }
        };

        let mut app = AppModel {
            core,
            nav,
            monitor_generation: 0,
            detected_monitors: Vec::new(),
            monitors: Vec::new(),
            monitor_load_errors: HashMap::new(),
            config,
            hotkey_manager,
            hotkey_action_map,
            hotkey_status,
            hotkey_generation: 0,
            status_message: "Starting...".into(),
            recording_state: RecordingState::NotRecording,
            expanded_hotkey: None,
            value_drafts,
            vcp_drafts,
            config_dirty: false,
            about,
            pending_brightness: None,
            pending_contrast: None,
            profiles: Vec::new(),
            profile_name_input: String::new(),
            pending_profile_delete: None,
            pending_profile_replace: None,
            action_executor: ActionExecutor::default(),
            refresh_monitors_after_jobs: false,
            refresh_profiles_after_jobs: false,
            tray,
        };

        let cmd = cosmic::app::Task::batch([
            app.update(Message::RefreshMonitors),
            app.update(Message::RefreshProfiles),
        ]);
        (app, cmd)
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> cosmic::app::Task<Self::Message> {
        self.nav.activate(id);
        self.update_title()
    }

    // Intercept the header-bar close button → hide to tray instead of exiting
    fn on_app_exit(&mut self) -> Option<Self::Message> {
        if self.tray.is_some() {
            Some(Message::HideWindow)
        } else {
            None // no tray → exit normally
        }
    }

    // Intercept window surface close (e.g. Alt+F4) → hide to tray
    fn on_close_requested(&self, id: window::Id) -> Option<Self::Message> {
        if self.tray.is_some() {
            if self.core.main_window_id().is_some_and(|main| main == id) {
                return Some(Message::HideWindow);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Subscriptions
    // -----------------------------------------------------------------------

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs: Vec<Subscription<Self::Message>> = Vec::new();

        // Global hotkey polling subscription (only when enabled)
        if self.config.hotkeys_enabled && !self.hotkey_action_map.is_empty() {
            let registered_ids = self.hotkey_action_map.keys().copied().collect();
            subs.push(
                hotkeys::hotkey_subscription(registered_ids, self.hotkey_generation)
                    .map(Message::HotkeyTriggered),
            );
        }

        // Keyboard event subscription when recording hotkeys
        if !matches!(self.recording_state, RecordingState::NotRecording) {
            subs.push(event::listen_with(|event, _status, _id| {
                if let Event::Keyboard(KeyboardEvent::KeyPressed { key, modifiers, .. }) = event {
                    Some(Message::KeyPressed(modifiers, key))
                } else {
                    None
                }
            }));
        }

        // System tray subscription
        if let Some((_, ref tray_stream)) = self.tray {
            subs.push(tray_stream.clone().subscription().map(Message::Tray));
        }

        subs.push(event::listen_with(|event, _status, id| {
            if let Event::Window(window::Event::Closed) = event {
                Some(Message::WindowClosed(id))
            } else {
                None
            }
        }));

        Subscription::batch(subs)
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    fn update(&mut self, message: Self::Message) -> cosmic::app::Task<Self::Message> {
        match message {
            // -- Monitor detection ------------------------------------------
            Message::RefreshMonitors => {
                self.monitor_generation = self.monitor_generation.wrapping_add(1);
                let generation = self.monitor_generation;
                self.status_message = "Detecting monitors...".into();
                return cosmic::app::Task::perform(
                    async { tokio::task::spawn_blocking(ddc::detect_monitors).await },
                    move |result| match result {
                        Ok(Ok(monitors)) => {
                            cosmic::Action::App(Message::MonitorsDetected(generation, monitors))
                        }
                        Ok(Err(e)) => {
                            cosmic::Action::App(Message::Error(format!("DDC detection error: {e}")))
                        }
                        Err(e) => {
                            cosmic::Action::App(Message::Error(format!("Task join error: {e}")))
                        }
                    },
                );
            }

            Message::RetryMonitor(monitor_id) => {
                let Some(info) = self
                    .detected_monitors
                    .iter()
                    .find(|monitor| monitor.id == monitor_id)
                    .cloned()
                else {
                    return self.update(Message::RefreshMonitors);
                };
                self.monitor_load_errors.remove(&monitor_id);
                let generation = self.monitor_generation;
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ddc::read_monitor_state(monitor_id, info)
                        })
                        .await
                    },
                    move |result| match result {
                        Ok(Ok(state)) => cosmic::Action::App(Message::MonitorStateLoaded(
                            generation,
                            monitor_id,
                            Box::new(state),
                        )),
                        Ok(Err(error)) => cosmic::Action::App(Message::MonitorStateFailed(
                            generation,
                            monitor_id,
                            error.to_string(),
                        )),
                        Err(error) => cosmic::Action::App(Message::MonitorStateFailed(
                            generation,
                            monitor_id,
                            format!("Task join error: {error}"),
                        )),
                    },
                );
            }

            Message::MonitorsDetected(generation, infos) => {
                if generation != self.monitor_generation {
                    return cosmic::app::Task::none();
                }
                self.detected_monitors = infos.clone();
                self.monitors.clear();
                self.monitor_load_errors.clear();
                // Rebuild nav bar
                self.nav = nav_bar::Model::default();
                for info in &infos {
                    let label = if info.name.is_empty() {
                        format!("Monitor {}", info.id)
                    } else {
                        format!("{} ({}x{})", info.name, info.width, info.height)
                    };
                    self.nav
                        .insert()
                        .text(label)
                        .data::<Page>(Page::Monitor(info.id));
                }
                // Hotkeys page
                self.nav
                    .insert()
                    .text("Hotkeys")
                    .data::<Page>(Page::Hotkeys);
                self.nav
                    .insert()
                    .text("Profiles")
                    .data::<Page>(Page::Profiles);
                // Settings page
                self.nav
                    .insert()
                    .text("Settings")
                    .data::<Page>(Page::Settings);
                self.nav.insert().text("About").data::<Page>(Page::About);

                // Activate first monitor
                self.nav.activate_position(0);

                self.status_message = format!("{} monitor(s) detected", infos.len());

                // Kick off state reads for each monitor
                let mut tasks = Vec::new();
                for info in infos {
                    let mid = info.id;
                    tasks.push(cosmic::app::Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || ddc::read_monitor_state(mid, info))
                                .await
                        },
                        move |result| match result {
                            Ok(Ok(state)) => cosmic::Action::App(Message::MonitorStateLoaded(
                                generation,
                                mid,
                                Box::new(state),
                            )),
                            Ok(Err(e)) => cosmic::Action::App(Message::MonitorStateFailed(
                                generation,
                                mid,
                                e.to_string(),
                            )),
                            Err(e) => cosmic::Action::App(Message::MonitorStateFailed(
                                generation,
                                mid,
                                format!("Task join error: {e}"),
                            )),
                        },
                    ));
                }
                return cosmic::app::Task::batch(tasks);
            }

            Message::MonitorStateLoaded(generation, id, state) => {
                if generation != self.monitor_generation {
                    return cosmic::app::Task::none();
                }
                self.monitor_load_errors.remove(&id);
                // Upsert
                if let Some(existing) = self.monitors.iter_mut().find(|m| m.info.id == id) {
                    *existing = *state;
                } else {
                    self.monitors.push(*state);
                }
                self.monitors.sort_by_key(|m| m.info.id);
            }

            Message::MonitorStateFailed(generation, id, error) => {
                if generation != self.monitor_generation {
                    return cosmic::app::Task::none();
                }
                self.monitor_load_errors.insert(id, error.clone());
                self.status_message = format!("Could not read monitor {id}: {error}");
            }

            // -- Brightness -------------------------------------------------
            Message::BrightnessSliderChanged(monitor_id, value) => {
                // Update UI immediately for smooth feedback
                if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                    m.brightness = value;
                }
                // Store pending change and debounce
                self.pending_brightness = Some((monitor_id, value));

                // Schedule debounced application after 150ms
                return cosmic::app::Task::perform(
                    async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                        (monitor_id, value)
                    },
                    move |(mid, val)| {
                        cosmic::Action::App(Message::ApplyBrightnessDebounced(mid, val))
                    },
                );
            }

            Message::ApplyBrightnessDebounced(monitor_id, value) => {
                // Only apply if this is still the pending value
                if let Some((pending_id, pending_val)) = self.pending_brightness {
                    if pending_id == monitor_id && pending_val == value {
                        self.pending_brightness = None;
                        return self.update(Message::SetBrightness(monitor_id, value));
                    }
                }
            }

            Message::SetBrightness(monitor_id, value) => {
                if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                    m.brightness = value;
                }
                return self
                    .enqueue_hardware_jobs([HardwareJob::SetBrightness { monitor_id, value }]);
            }

            // -- Contrast ---------------------------------------------------
            Message::ContrastSliderChanged(monitor_id, value) => {
                // Update UI immediately for smooth feedback
                if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                    m.contrast = value;
                }
                // Store pending change and debounce
                self.pending_contrast = Some((monitor_id, value));

                // Schedule debounced application after 150ms
                return cosmic::app::Task::perform(
                    async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                        (monitor_id, value)
                    },
                    move |(mid, val)| {
                        cosmic::Action::App(Message::ApplyContrastDebounced(mid, val))
                    },
                );
            }

            Message::ApplyContrastDebounced(monitor_id, value) => {
                // Only apply if this is still the pending value
                if let Some((pending_id, pending_val)) = self.pending_contrast {
                    if pending_id == monitor_id && pending_val == value {
                        self.pending_contrast = None;
                        return self.update(Message::SetContrast(monitor_id, value));
                    }
                }
            }

            Message::SetContrast(monitor_id, value) => {
                if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                    m.contrast = value;
                }
                return self
                    .enqueue_hardware_jobs([HardwareJob::SetContrast { monitor_id, value }]);
            }

            // -- Input source -----------------------------------------------
            Message::SelectInputSource(monitor_id, idx) => {
                if let Some(&source) = INPUT_SOURCES.get(idx) {
                    return self.update(Message::SetInputSource(monitor_id, source));
                }
            }

            Message::SetInputSource(monitor_id, source) => {
                if let Some(monitor) = self
                    .monitors
                    .iter_mut()
                    .find(|monitor| monitor.info.id == monitor_id)
                {
                    monitor.input_source = source;
                }
                return self
                    .enqueue_hardware_jobs([HardwareJob::SetInputSource { monitor_id, source }]);
            }

            Message::HardwareJobFinished(result) => {
                self.action_executor.complete_active();
                match result {
                    Ok(HardwareOutcome::BrightnessApplied { monitor_id, value }) => {
                        if let Some(monitor) = self
                            .monitors
                            .iter_mut()
                            .find(|monitor| monitor.info.id == monitor_id)
                        {
                            monitor.brightness = value;
                        }
                    }
                    Ok(HardwareOutcome::ContrastApplied { monitor_id, value }) => {
                        if let Some(monitor) = self
                            .monitors
                            .iter_mut()
                            .find(|monitor| monitor.info.id == monitor_id)
                        {
                            monitor.contrast = value;
                        }
                    }
                    Ok(HardwareOutcome::InputSourceApplied { monitor_id, source }) => {
                        if let Some(monitor) = self
                            .monitors
                            .iter_mut()
                            .find(|monitor| monitor.info.id == monitor_id)
                        {
                            monitor.input_source = source;
                        }
                    }
                    Ok(HardwareOutcome::PowerModeApplied { monitor_id, mode }) => {
                        self.status_message =
                            format!("Monitor {monitor_id} power mode set to {mode}.");
                    }
                    Ok(HardwareOutcome::CustomVcpApplied {
                        monitor_id,
                        code,
                        value,
                    }) => {
                        self.status_message =
                            format!("Monitor {monitor_id} VCP 0x{code:02X} set to {value}.");
                    }
                    Ok(HardwareOutcome::ProfileApplied { name }) => {
                        self.status_message = format!("Applied profile '{name}'.");
                        self.refresh_monitors_after_jobs = true;
                    }
                    Ok(HardwareOutcome::ProfileSaved { name }) => {
                        self.status_message = format!("Saved profile '{name}'.");
                        self.profile_name_input.clear();
                        self.refresh_profiles_after_jobs = true;
                    }
                    Ok(HardwareOutcome::MonitorsPoweredOff) => {
                        self.status_message = "Monitors turned off.".into();
                    }
                    Err(error) => self.status_message = error,
                }

                let next = self.start_next_hardware_job();
                if self.action_executor.is_idle() {
                    let mut follow_up = vec![next];
                    if std::mem::take(&mut self.refresh_monitors_after_jobs) {
                        follow_up.push(self.update(Message::RefreshMonitors));
                    }
                    if std::mem::take(&mut self.refresh_profiles_after_jobs) {
                        follow_up.push(self.update(Message::RefreshProfiles));
                    }
                    return cosmic::app::Task::batch(follow_up);
                }
                return next;
            }

            // -- Hotkey actions ---------------------------------------------
            Message::HotkeyTriggered(id) => {
                if let Some(actions) = resolve_triggered_actions(
                    self.config.hotkeys_enabled,
                    !matches!(self.recording_state, RecordingState::NotRecording),
                    self.hotkey_action_map.as_ref(),
                    id,
                ) {
                    return self.handle_hotkey_actions(actions);
                }
            }

            Message::AddHotkey => {
                let hotkey = Hotkey::new_empty();
                let id = hotkey.id.clone();
                self.config.hotkeys.hotkeys.push(hotkey);
                self.expanded_hotkey = Some(id.clone());
                self.initialize_hotkey_drafts(&id);
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::ToggleHotkeyEditor(id) => {
                if self.expanded_hotkey.as_deref() == Some(&id) {
                    self.expanded_hotkey = None;
                } else {
                    self.initialize_hotkey_drafts(&id);
                    self.expanded_hotkey = Some(id);
                }
            }

            Message::DeleteHotkey(id) => {
                self.config.hotkeys.hotkeys.retain(|hotkey| hotkey.id != id);
                self.clear_hotkey_drafts(&id);
                if self.expanded_hotkey.as_deref() == Some(&id) {
                    self.expanded_hotkey = None;
                }
                self.recording_state = RecordingState::NotRecording;
                self.config_dirty = true;
                self.refresh_hotkey_registration();
            }

            Message::StartRecording(hotkey_id) => {
                self.recording_state = RecordingState::Recording { hotkey_id };
                self.status_message = "Press a key combination to bind the hotkey.".into();
            }

            Message::CancelRecording => {
                self.recording_state = RecordingState::NotRecording;
                self.status_message = "Hotkey recording cancelled".into();
            }

            Message::ClearBinding(id) => {
                if let Some(hotkey) = self.hotkey_mut(&id) {
                    hotkey.binding = HotkeyBinding::unbound();
                }
                self.recording_state = RecordingState::NotRecording;
                self.config_dirty = true;
                self.refresh_hotkey_registration();
            }

            Message::KeyPressed(modifiers, key) => {
                let key_string = key_to_string(&key);
                if key_string.is_empty() {
                    return cosmic::app::Task::none();
                }

                let ctrl = modifiers.control();
                let alt = modifiers.alt();
                let shift = modifiers.shift();
                let win = modifiers.logo();
                let binding = HotkeyBinding {
                    ctrl,
                    alt,
                    shift,
                    win,
                    key: key_string.clone(),
                };
                if let RecordingState::Recording { hotkey_id, .. } = &self.recording_state {
                    let hotkey_id = hotkey_id.clone();
                    let duplicate = binding.to_hotkey().is_some_and(|candidate| {
                        self.config.hotkeys.hotkeys.iter().any(|hotkey| {
                            hotkey.id != hotkey_id
                                && hotkey
                                    .binding
                                    .to_hotkey()
                                    .is_some_and(|other| other.id() == candidate.id())
                        })
                    });
                    if duplicate {
                        self.status_message = "That key combination is already assigned.".into();
                        return cosmic::app::Task::none();
                    }
                    if let Some(hotkey) = self.hotkey_mut(&hotkey_id) {
                        hotkey.binding = binding;
                    }
                    self.status_message = format!(
                        "Hotkey bound to {}. Remember to save configuration.",
                        format_hotkey(ctrl, alt, shift, win, &key_string)
                    );
                    self.recording_state = RecordingState::NotRecording;
                    self.config_dirty = true;
                    self.refresh_hotkey_registration();
                }
            }

            Message::AddAction(id) => {
                if let Some(hotkey) = self.hotkey_mut(&id) {
                    hotkey.actions.push(HotkeyActionSpec::default());
                }
                self.initialize_hotkey_drafts(&id);
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::DeleteAction(id, idx) => {
                if let Some(hotkey) = self.hotkey_mut(&id) {
                    if idx < hotkey.actions.len() {
                        hotkey.actions.remove(idx);
                    }
                }
                self.clear_hotkey_drafts(&id);
                self.initialize_hotkey_drafts(&id);
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::SetActionType(id, idx, action_type) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    action.action_type = action_type;
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::SetActionTarget(id, idx, target) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    if action.target == ActionTarget::InputSource {
                        action.monitors = explicit_monitor_ids(action);
                    }
                    action.target = target;
                    if !target.supports_offset() && action.action_type == ActionType::Offset {
                        action.action_type = ActionType::Set;
                    }
                    if target == ActionTarget::InputSource
                        && !action.all_monitors
                        && action.monitor_inputs.is_empty()
                    {
                        action.monitor_inputs = action
                            .monitors
                            .iter()
                            .map(|monitor_id| MonitorInput {
                                monitor_id: *monitor_id,
                                input_source: action.input_source,
                            })
                            .collect();
                    }
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::ActionValueDraftChanged(id, idx, draft) => {
                self.value_drafts.insert((id.clone(), idx), draft.clone());
                if let Ok(Some(value)) = parse_value_draft(&draft) {
                    if let Some(action) = self.action_mut(&id, idx) {
                        action.value = value;
                    }
                    self.config_dirty = true;
                    self.refresh_hotkey_actions();
                }
            }

            Message::ActionVcpDraftChanged(id, idx, draft) => {
                self.vcp_drafts.insert((id.clone(), idx), draft.clone());
                if let Ok(Some(code)) = parse_vcp_draft(&draft) {
                    if let Some(action) = self.action_mut(&id, idx) {
                        action.vcp_code = code;
                    }
                    self.config_dirty = true;
                    self.refresh_hotkey_actions();
                }
            }

            Message::SetActionInputSource(id, idx, source) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    action.input_source = source;
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::SetMonitorInput(id, idx, monitor_id, source) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    action.monitors.retain(|id| *id != monitor_id);
                    action
                        .monitor_inputs
                        .retain(|input| input.monitor_id != monitor_id);
                    if let Some(input_source) = source {
                        action.monitors.push(monitor_id);
                        action.monitor_inputs.push(MonitorInput {
                            monitor_id,
                            input_source,
                        });
                    }
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::SetActionPowerMode(id, idx, mode) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    action.power_mode = mode;
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::SetActionProfile(id, idx, name) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    action.profile_name = name;
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::ToggleActionAllMonitors(id, idx, checked) => {
                if let Some(action) = self.action_mut(&id, idx) {
                    action.all_monitors = checked;
                    if !checked {
                        action.monitors.clear();
                        action.monitor_inputs.clear();
                    }
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::ToggleActionMonitor(id, idx, monitor_id, checked) => {
                let detected_ids: Vec<u32> = self.detected_monitors.iter().map(|m| m.id).collect();
                if let Some(action) = self.action_mut(&id, idx) {
                    toggle_action_monitor(action, &detected_ids, monitor_id, checked);
                }
                self.config_dirty = true;
                self.refresh_hotkey_actions();
            }

            Message::SetTurnOffBehavior(behavior) => {
                self.config.turn_off_behavior = behavior;
                self.config_dirty = true;
            }

            Message::SaveConfig => {
                if let Some(error) = self.validate_action_drafts() {
                    self.status_message = error;
                    return cosmic::app::Task::none();
                }
                if let Err(e) = self.config.save() {
                    self.status_message = format!("Failed to save config: {e}");
                } else {
                    self.status_message = "Configuration saved and hotkeys activated.".into();
                    self.config_dirty = false;
                    self.refresh_hotkey_registration();
                }
            }

            Message::ToggleHotkeys(enabled) => {
                self.config.hotkeys_enabled = enabled;
                if enabled {
                    self.status_message = "Hotkeys enabled".into();
                } else {
                    self.status_message = "Hotkeys disabled".into();
                }
                // Auto-save the preference
                if let Err(e) = self.config.save() {
                    self.status_message = format!("Failed to save config: {e}");
                }
                self.refresh_hotkey_registration();
            }

            // -- Profiles ---------------------------------------------------
            Message::RefreshProfiles => {
                return cosmic::app::Task::perform(
                    async { tokio::task::spawn_blocking(profiles::list_profiles).await },
                    |result| match result {
                        Ok(list) => cosmic::Action::App(Message::ProfilesListed(list)),
                        Err(error) => {
                            cosmic::Action::App(Message::Error(format!("Task join error: {error}")))
                        }
                    },
                );
            }
            Message::ProfilesListed(profiles) => {
                self.profiles = profiles;
                if let Some((tray, _)) = &self.tray {
                    tray.update_menu(&self.profiles);
                }
            }
            Message::ProfileNameInput(value) => self.profile_name_input = value,
            Message::SaveCurrentProfile(name) => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Enter a profile name first.".into();
                    return cosmic::app::Task::none();
                }
                match profiles::profile_exists(&name) {
                    Ok(true) => {
                        self.status_message =
                            format!("Profile '{name}' already exists. Confirm replacement.");
                        self.pending_profile_replace = Some(name);
                        return cosmic::app::Task::none();
                    }
                    Ok(false) => {}
                    Err(error) => {
                        self.status_message = format!("Invalid profile name: {error}");
                        return cosmic::app::Task::none();
                    }
                }
                self.status_message = format!("Saving profile '{name}'...");
                return self.enqueue_hardware_jobs([HardwareJob::SaveProfile {
                    name,
                    replace: false,
                }]);
            }
            Message::ConfirmReplaceProfile => {
                let Some(name) = self.pending_profile_replace.take() else {
                    return cosmic::app::Task::none();
                };
                self.status_message = format!("Replacing profile '{name}'...");
                return self.enqueue_hardware_jobs([HardwareJob::SaveProfile {
                    name,
                    replace: true,
                }]);
            }
            Message::CancelReplaceProfile => self.pending_profile_replace = None,
            Message::ApplyProfile(name) => {
                self.status_message = format!("Applying profile '{name}'...");
                return self.enqueue_hardware_jobs([HardwareJob::ApplyProfile { name }]);
            }
            Message::RequestDeleteProfile(name) => self.pending_profile_delete = Some(name),
            Message::CancelDeleteProfile => self.pending_profile_delete = None,
            Message::DeleteProfile(name) => {
                self.pending_profile_delete = None;
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || profiles::delete_profile(&name)).await
                    },
                    |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::RefreshProfiles),
                        Ok(Err(error)) => cosmic::Action::App(Message::Error(format!(
                            "Delete profile error: {error}"
                        ))),
                        Err(error) => {
                            cosmic::Action::App(Message::Error(format!("Task join error: {error}")))
                        }
                    },
                );
            }
            Message::AddProfileHotkey(profile_name) => {
                let hotkey = Hotkey::new_for_profile(profile_name);
                let id = hotkey.id.clone();
                self.config.hotkeys.hotkeys.push(hotkey);
                self.initialize_hotkey_drafts(&id);
                self.expanded_hotkey = Some(id.clone());
                self.recording_state = RecordingState::Recording { hotkey_id: id };
                if let Some(position) = self.nav_position_of(Page::Hotkeys) {
                    self.nav.activate_position(position);
                }
                self.config_dirty = true;
            }
            // -- Hide window (close-to-tray) --------------------------------
            Message::HideWindow => {
                log::info!("Hiding window to tray");
                if let Some(id) = self.core.main_window_id() {
                    return window::close(id);
                }
            }

            Message::WindowClosed(id) => {
                if self.core.main_window_id() == Some(id) {
                    self.core_mut().set_main_window_id(None);
                }
            }

            // -- System tray ------------------------------------------------
            Message::Tray(tray_msg) => {
                match tray_msg {
                    TrayMessage::ShowWindow => {
                        log::info!("Tray: Show window requested");
                        if let Some(id) = self.core.main_window_id() {
                            // Window still exists — try to focus it
                            return window::gain_focus(id);
                        } else {
                            // Window was closed — open a new one
                            let (new_id, open_task) = window::open(window::Settings {
                                min_size: Some(cosmic::iced::Size::new(600.0, 400.0)),
                                decorations: false,
                                ..window::Settings::default()
                            });
                            self.core_mut().set_main_window_id(Some(new_id));
                            let title_task = self.update_title();
                            return cosmic::app::Task::batch([open_task.discard(), title_task]);
                        }
                    }
                    TrayMessage::LoadProfile(name) => {
                        return self.update(Message::ApplyProfile(name));
                    }
                    TrayMessage::SaveCurrentProfile => {
                        if let Some(position) = self.nav_position_of(Page::Profiles) {
                            self.nav.activate_position(position);
                        }
                        return self.update(Message::Tray(TrayMessage::ShowWindow));
                    }
                    TrayMessage::TurnOffMonitors => {
                        return self.enqueue_hardware_jobs([HardwareJob::SoftTurnOff]);
                    }
                    TrayMessage::Exit => {
                        log::info!("Tray: Exit requested");
                        return cosmic::iced::exit();
                    }
                }
            }

            Message::OpenUrl(url) => {
                if let Err(error) = std::process::Command::new("rundll32.exe")
                    .args(["url.dll,FileProtocolHandler", &url])
                    .spawn()
                {
                    self.status_message = format!("Failed to open URL: {error}");
                }
            }

            // -- Errors / misc ----------------------------------------------
            Message::Error(msg) => {
                log::error!("{msg}");
                self.status_message = msg;
            }
        }

        cosmic::app::Task::none()
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    fn view(&self) -> Element<'_, Self::Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        // Determine which page is active
        let page = self
            .nav
            .active_data::<Page>()
            .cloned()
            .unwrap_or(Page::Hotkeys);

        let content: Element<_> = match page {
            Page::Monitor(monitor_id) => self.view_monitor(monitor_id),
            Page::Hotkeys => self.view_hotkeys_current(),
            Page::Profiles => self.view_profiles(),
            Page::Settings => self.view_settings(),
            Page::About => self.view_about(),
        };

        // Wrap in a container with status bar at the bottom
        let status_bar = widget::text::caption(&self.status_message);

        let layout = widget::column::with_capacity(3)
            .push(content)
            .push(widget::divider::horizontal::default())
            .push(
                widget::container(status_bar)
                    .padding([4, 12])
                    .width(Length::Fill),
            )
            .spacing(space_s)
            .height(Length::Fill)
            .width(Length::Fill);

        widget::container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(space_m)
            .into()
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![
            widget::button::text("Refresh")
                .on_press(Message::RefreshMonitors)
                .into(),
        ]
    }
}

// ---------------------------------------------------------------------------
// View helpers
// ---------------------------------------------------------------------------

impl AppModel {
    /// View for a single monitor page.
    fn view_monitor(&self, monitor_id: u32) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let monitor = self.monitors.iter().find(|m| m.info.id == monitor_id);

        match monitor {
            None => {
                let content: Element<'_, Message> =
                    if let Some(error) = self.monitor_load_errors.get(&monitor_id) {
                        widget::column::with_capacity(3)
                            .push(widget::text::title4(format!(
                                "Monitor {monitor_id} unavailable"
                            )))
                            .push(widget::text::body(error))
                            .push(
                                widget::button::standard("Retry")
                                    .on_press(Message::RetryMonitor(monitor_id)),
                            )
                            .spacing(space_s)
                            .into()
                    } else {
                        widget::text::body("Loading monitor data...").into()
                    };
                widget::container(content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Center)
                    .into()
            }

            Some(mon) => {
                // Header
                let header_label = if mon.info.name.is_empty() {
                    format!("Monitor {}", mon.info.id)
                } else {
                    mon.info.name.clone()
                };
                let header = widget::text::title3(header_label);

                let resolution_text = format!(
                    "{}x{} at ({}, {}){}",
                    mon.info.width,
                    mon.info.height,
                    mon.info.x,
                    mon.info.y,
                    if mon.info.is_primary {
                        " [Primary]"
                    } else {
                        ""
                    }
                );
                let resolution_label = widget::text::caption(resolution_text);

                // Input source dropdown
                let selected_idx = input_source_index(&mon.input_source);
                let mid = mon.info.id;

                // Create labels as owned data
                static INPUT_SOURCE_LABELS: &[&str] = &[
                    "HDMI 1",
                    "HDMI 2",
                    "DisplayPort 1",
                    "DisplayPort 2",
                    "USB-C 1",
                    "USB-C 2",
                    "VGA 1",
                    "VGA 2",
                    "DVI 1",
                    "DVI 2",
                ];

                let input_section = cosmic::widget::settings::section()
                    .title("Input Source")
                    .add(
                        cosmic::widget::settings::item::builder("Active input").control(
                            widget::dropdown(INPUT_SOURCE_LABELS, selected_idx, move |idx| {
                                Message::SelectInputSource(mid, idx)
                            }),
                        ),
                    );

                // Brightness slider
                let brightness_val = mon.brightness as f64;
                let brightness_max = mon.brightness_max.max(1) as f64;
                let mid_b = mon.info.id;
                let brightness_section =
                    cosmic::widget::settings::section().title("Brightness").add(
                        cosmic::widget::settings::item::builder(format!(
                            "{} / {}",
                            mon.brightness, mon.brightness_max
                        ))
                        .control(
                            widget::slider(0.0..=brightness_max, brightness_val, move |v| {
                                Message::BrightnessSliderChanged(mid_b, v as u16)
                            })
                            .width(Length::Fixed(300.0)),
                        ),
                    );

                // Contrast slider
                let contrast_val = mon.contrast as f64;
                let contrast_max = mon.contrast_max.max(1) as f64;
                let mid_c = mon.info.id;
                let contrast_section = cosmic::widget::settings::section().title("Contrast").add(
                    cosmic::widget::settings::item::builder(format!(
                        "{} / {}",
                        mon.contrast, mon.contrast_max
                    ))
                    .control(
                        widget::slider(0.0..=contrast_max, contrast_val, move |v| {
                            Message::ContrastSliderChanged(mid_c, v as u16)
                        })
                        .width(Length::Fixed(300.0)),
                    ),
                );

                widget::column::with_capacity(6)
                    .push(header)
                    .push(resolution_label)
                    .push(input_section)
                    .push(brightness_section)
                    .push(contrast_section)
                    .spacing(space_s)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        }
    }

    fn view_hotkeys_current(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let mut hotkeys = widget::column::with_capacity(self.config.hotkeys.hotkeys.len() + 1)
            .spacing(space_s)
            .width(Length::Fill);

        for hotkey in &self.config.hotkeys.hotkeys {
            hotkeys = hotkeys.push(self.view_hotkey_card(hotkey, space_s));
        }

        let add_row = widget::row::with_capacity(2)
            .push(widget::button::suggested("Add Hotkey").on_press(Message::AddHotkey))
            .push(widget::Space::new().width(Length::Fill))
            .spacing(space_s);

        let save_label = if self.config_dirty {
            "Save Configuration *"
        } else {
            "Save Configuration"
        };
        let content = widget::column::with_capacity(6)
            .push(widget::text::title3("Hotkeys"))
            .push(widget::text::body(
                "Configure global hotkeys and ordered display actions.",
            ))
            .push(add_row)
            .push(hotkeys)
            .push(widget::button::suggested(save_label).on_press(Message::SaveConfig))
            .spacing(space_s)
            .width(Length::Fill);

        widget::scrollable(
            widget::container(content)
                .width(Length::Fill)
                .max_width(760.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_hotkey_card(&self, hotkey: &Hotkey, space_s: u16) -> Element<'_, Message> {
        let id = hotkey.id.clone();
        let expanded = self.expanded_hotkey.as_deref() == Some(id.as_str());
        let active = self.hotkey_status.get(&id).copied().unwrap_or(false);
        let status = if hotkey.binding.key.is_empty() {
            "Unbound"
        } else if active {
            "Active"
        } else {
            "Inactive"
        };
        let summary = widget::row::with_capacity(5)
            .push(widget::text::body(hotkey.binding.to_string()).width(Length::Fill))
            .push(widget::text::caption(status))
            .push(
                widget::button::standard(if expanded { "Collapse" } else { "Edit" })
                    .on_press(Message::ToggleHotkeyEditor(id.clone())),
            )
            .push(widget::button::destructive("Delete").on_press(Message::DeleteHotkey(id.clone())))
            .spacing(space_s)
            .align_y(Alignment::Center);

        let mut content = widget::column::with_capacity(4)
            .push(summary)
            .spacing(space_s);
        if expanded {
            let recording_here = matches!(
                &self.recording_state,
                RecordingState::Recording { hotkey_id, .. } if hotkey_id == &id
            );
            let binding_row = if recording_here {
                widget::row::with_capacity(2)
                    .push(widget::text::body("Press a key combination...").width(Length::Fill))
                    .push(widget::button::standard("Cancel").on_press(Message::CancelRecording))
            } else {
                widget::row::with_capacity(3)
                    .push(
                        widget::button::standard("Record")
                            .on_press(Message::StartRecording(id.clone())),
                    )
                    .push(
                        widget::button::standard("Clear")
                            .on_press(Message::ClearBinding(id.clone())),
                    )
                    .push(widget::Space::new().width(Length::Fill))
            }
            .spacing(space_s)
            .align_y(Alignment::Center);
            content = content.push(binding_row);

            for (idx, action) in hotkey.actions.iter().enumerate() {
                content = content.push(self.view_action_editor(&id, idx, action, space_s));
            }
            content = content.push(
                widget::button::standard("Add Action").on_press(Message::AddAction(id.clone())),
            );
        }

        cosmic::widget::settings::section()
            .title(format!("Hotkey ({})", hotkey.actions.len()))
            .add(content)
            .into()
    }

    fn view_action_editor(
        &self,
        hotkey_id: &str,
        idx: usize,
        action: &HotkeyActionSpec,
        space_s: u16,
    ) -> Element<'_, Message> {
        let id = hotkey_id.to_string();
        let type_options = if action.target.supports_offset() {
            ActionType::ALL
        } else {
            ActionType::NO_OFFSET
        };
        let type_labels: Vec<&str> = type_options.iter().map(|value| value.label()).collect();
        let selected_type = type_options
            .iter()
            .position(|value| value == &action.action_type);
        let action_row = widget::row::with_capacity(3)
            .push(widget::dropdown(type_labels, selected_type, {
                let id = id.clone();
                move |selected| Message::SetActionType(id.clone(), idx, type_options[selected])
            }))
            .push(widget::Space::new().width(Length::Fill))
            .push(
                widget::button::destructive("Delete Action")
                    .on_press(Message::DeleteAction(id.clone(), idx)),
            )
            .spacing(space_s)
            .align_y(Alignment::Center);
        let mut fields = widget::column::with_capacity(5)
            .push(
                widget::row::with_capacity(2)
                    .push(widget::text::body("Actions").width(Length::Fixed(112.0)))
                    .push(
                        widget::container(action_row)
                            .width(Length::Fill)
                            .align_x(Horizontal::Left),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            )
            .spacing(space_s);

        if action.action_type != ActionType::Off {
            let targets: Vec<&str> = ActionTarget::ALL
                .iter()
                .map(|value| value.label())
                .collect();
            let selected = ActionTarget::ALL
                .iter()
                .position(|value| value == &action.target);
            fields = fields.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body("Commands").width(Length::Fixed(112.0)))
                    .push(
                        widget::container(widget::dropdown(targets, selected, {
                            let id = id.clone();
                            move |selected| {
                                Message::SetActionTarget(id.clone(), idx, ActionTarget::ALL[selected])
                            }
                        }))
                        .width(Length::Fill)
                        .align_x(Horizontal::Left),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            );

            if action.target == ActionTarget::CustomVcp {
                let draft = self
                    .vcp_drafts
                    .get(&(id.clone(), idx))
                    .map(String::as_str)
                    .unwrap_or("");
                let mut code = widget::column::with_capacity(2).push(
                    widget::text_input("0x10", draft)
                        .on_input({
                            let id = id.clone();
                            move |value| Message::ActionVcpDraftChanged(id.clone(), idx, value)
                        })
                        .width(Length::Fixed(96.0)),
                );
                if let Err(error) = parse_vcp_draft(draft) {
                    code = code.push(widget::text::caption(error));
                }
                fields = fields.push(
                    widget::row::with_capacity(2)
                        .push(widget::text::body("VCP code").width(Length::Fixed(112.0)))
                        .push(
                            widget::container(code)
                                .width(Length::Fill)
                                .align_x(Horizontal::Left),
                        )
                        .spacing(space_s)
                        .align_y(Alignment::Start),
                );
            }

            let value_control: Option<Element<'_, Message>> = match action.target {
                ActionTarget::Brightness | ActionTarget::Contrast | ActionTarget::CustomVcp => {
                    let draft = self
                        .value_drafts
                        .get(&(id.clone(), idx))
                        .map(String::as_str)
                        .unwrap_or("");
                    let mut input = widget::column::with_capacity(2).push(
                        widget::text_input("0", draft)
                            .on_input({
                                let id = id.clone();
                                move |value| {
                                    Message::ActionValueDraftChanged(id.clone(), idx, value)
                                }
                            })
                            .width(Length::Fixed(96.0)),
                    );
                    if let Err(error) = parse_value_draft(draft) {
                        input = input.push(widget::text::caption(error));
                    }
                    Some(input.into())
                }
                ActionTarget::InputSource if action.all_monitors => {
                    let mut sources = INPUT_SOURCES.to_vec();
                    if !sources.contains(&action.input_source) {
                        sources.push(action.input_source);
                    }
                    let labels: Vec<String> = sources.iter().map(ToString::to_string).collect();
                    let selected = sources
                        .iter()
                        .position(|value| value == &action.input_source);
                    Some(
                        widget::dropdown(labels, selected, {
                            let id = id.clone();
                            move |selected| {
                                Message::SetActionInputSource(id.clone(), idx, sources[selected])
                            }
                        })
                        .into(),
                    )
                }
                ActionTarget::PowerMode => {
                    let labels: Vec<String> = POWER_MODES.iter().map(ToString::to_string).collect();
                    let selected = power_mode_index(&action.power_mode);
                    Some(
                        widget::dropdown(labels, selected, {
                            let id = id.clone();
                            move |selected| {
                                Message::SetActionPowerMode(id.clone(), idx, POWER_MODES[selected])
                            }
                        })
                        .into(),
                    )
                }
                ActionTarget::Profile => {
                    let mut profiles = self.profiles.clone();
                    if !action.profile_name.is_empty() && !profiles.contains(&action.profile_name) {
                        profiles.push(action.profile_name.clone());
                    }
                    if profiles.is_empty() {
                        Some(widget::text::body("No profiles saved yet.").into())
                    } else {
                        let selected = profiles
                            .iter()
                            .position(|value| value == &action.profile_name);
                        Some(
                            widget::dropdown(profiles.clone(), selected, {
                                let id = id.clone();
                                move |selected| {
                                    Message::SetActionProfile(
                                        id.clone(),
                                        idx,
                                        profiles[selected].clone(),
                                    )
                                }
                            })
                            .into(),
                        )
                    }
                }
                ActionTarget::InputSource => None,
            };
            if let Some(control) = value_control {
                fields = fields.push(
                    widget::row::with_capacity(2)
                        .push(widget::text::body("Value").width(Length::Fixed(112.0)))
                        .push(
                            widget::container(control)
                                .width(Length::Fill)
                                .align_x(Horizontal::Left),
                        )
                        .spacing(space_s)
                        .align_y(Alignment::Center),
                );
            }
        }

        let mut display_ids: Vec<u32> = self
            .detected_monitors
            .iter()
            .map(|monitor| monitor.id)
            .collect();
        for monitor_id in action
            .monitors
            .iter()
            .copied()
            .chain(action.monitor_inputs.iter().map(|input| input.monitor_id))
        {
            if !display_ids.contains(&monitor_id) {
                display_ids.push(monitor_id);
            }
        }

        let mut displays = widget::column::with_capacity(display_ids.len() + 2).spacing(space_s);
        if action.target == ActionTarget::Profile && action.action_type != ActionType::Off {
            displays = displays.push(widget::text::body("Entire saved display layout"));
        } else {
            let master_selected = action_master_selected(action, &display_ids);
            displays = displays.push(
                widget::row::with_capacity(3)
                    .push(widget::text::body("All displays").width(Length::Fill))
                    .push(widget::Space::new().width(Length::Fixed(space_s as f32)))
                    .push(widget::toggler(master_selected).on_toggle({
                        let id = id.clone();
                        move |checked| Message::ToggleActionAllMonitors(id.clone(), idx, checked)
                    }))
                    .align_y(Alignment::Center),
            );

            for monitor_id in display_ids {
                let label = self
                    .detected_monitors
                    .iter()
                    .find(|monitor| monitor.id == monitor_id)
                    .filter(|monitor| !monitor.name.is_empty())
                    .map(|monitor| monitor.name.clone())
                    .unwrap_or_else(|| format!("Monitor {monitor_id} (unavailable)"));
                let selected = action_monitor_selected(action, monitor_id);
                displays = displays.push(
                    widget::row::with_capacity(3)
                        .push(widget::text::body(label).width(Length::Fill))
                        .push(widget::Space::new().width(Length::Fixed(space_s as f32)))
                        .push(widget::toggler(selected).on_toggle({
                            let id = id.clone();
                            move |checked| {
                                Message::ToggleActionMonitor(id.clone(), idx, monitor_id, checked)
                            }
                        }))
                        .align_y(Alignment::Center),
                );

                if action.action_type != ActionType::Off
                    && action.target == ActionTarget::InputSource
                    && !action.all_monitors
                    && selected
                {
                    let current = action
                        .monitor_inputs
                        .iter()
                        .find(|input| input.monitor_id == monitor_id)
                        .map(|input| input.input_source)
                        .unwrap_or(action.input_source);
                    let mut sources = INPUT_SOURCES.to_vec();
                    if !sources.contains(&current) {
                        sources.push(current);
                    }
                    let labels: Vec<String> = sources.iter().map(ToString::to_string).collect();
                    let selected_source = sources.iter().position(|value| value == &current);
                    displays = displays.push(
                        widget::row::with_capacity(2)
                            .push(widget::text::caption("Input source").width(Length::Fill))
                            .push(widget::dropdown(labels, selected_source, {
                                let id = id.clone();
                                move |selected| {
                                    Message::SetMonitorInput(
                                        id.clone(),
                                        idx,
                                        monitor_id,
                                        Some(sources[selected]),
                                    )
                                }
                            }))
                            .spacing(space_s)
                            .align_y(Alignment::Center),
                    );
                }
            }
        }
        fields = fields.push(
            cosmic::widget::settings::section()
                .title("Displays")
                .add(displays),
        );

        widget::container(fields)
            .class(cosmic::theme::Container::Card)
            .padding(space_s)
            .width(Length::Fill)
            .into()
    }

    fn view_profiles(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let save_row = widget::row::with_capacity(2)
            .push(
                widget::text_input("New profile name", &self.profile_name_input)
                    .on_input(Message::ProfileNameInput)
                    .on_submit(Message::SaveCurrentProfile)
                    .width(Length::Fill),
            )
            .push(
                widget::button::suggested("Save Current Layout")
                    .on_press(Message::SaveCurrentProfile(self.profile_name_input.clone())),
            )
            .spacing(space_s)
            .align_y(Alignment::Center);
        let mut save = widget::column::with_capacity(2)
            .push(save_row)
            .spacing(space_s);
        if let Some(name) = &self.pending_profile_replace {
            save = save.push(
                widget::row::with_capacity(3)
                    .push(
                        widget::text::body(format!("Replace existing profile '{name}'?"))
                            .width(Length::Fill),
                    )
                    .push(
                        widget::button::standard("Cancel").on_press(Message::CancelReplaceProfile),
                    )
                    .push(
                        widget::button::destructive("Replace")
                            .on_press(Message::ConfirmReplaceProfile),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            );
        }
        let mut profiles = widget::column::with_capacity(self.profiles.len() + 1).spacing(space_s);
        if self.profiles.is_empty() {
            profiles = profiles.push(widget::text::body("No profiles saved yet."));
        }
        for name in &self.profiles {
            let deleting = self.pending_profile_delete.as_deref() == Some(name.as_str());
            let mut row = widget::row::with_capacity(5)
                .push(widget::text::body(name.clone()).width(Length::Fill))
                .push(
                    widget::button::suggested("Apply")
                        .on_press(Message::ApplyProfile(name.clone())),
                )
                .push(
                    widget::button::standard("Hotkey")
                        .on_press(Message::AddProfileHotkey(name.clone())),
                )
                .spacing(space_s)
                .align_y(Alignment::Center);
            if deleting {
                row = row
                    .push(widget::button::standard("Cancel").on_press(Message::CancelDeleteProfile))
                    .push(
                        widget::button::destructive("Confirm Delete")
                            .on_press(Message::DeleteProfile(name.clone())),
                    );
            } else {
                row = row.push(
                    widget::button::destructive("Delete")
                        .on_press(Message::RequestDeleteProfile(name.clone())),
                );
            }
            profiles = profiles.push(row);
        }
        let content = widget::column::with_capacity(4)
            .push(widget::text::title3("Monitor Layout Profiles"))
            .push(widget::text::body(
                "Save and restore complete Windows display layouts.",
            ))
            .push(
                cosmic::widget::settings::section()
                    .title("Save Current Layout")
                    .add(save),
            )
            .push(
                cosmic::widget::settings::section()
                    .title("Saved Profiles")
                    .add(profiles),
            )
            .spacing(space_s)
            .width(Length::Fill);
        widget::scrollable(
            widget::container(content)
                .width(Length::Fill)
                .max_width(700.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_about(&self) -> Element<'_, Message> {
        widget::scrollable(cosmic::widget::about(&self.about, |url| {
            Message::OpenUrl(url.to_owned())
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// View for the settings page (step sizes and other config).
    fn view_settings(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Settings");
        let description =
            widget::text::body("Configure step sizes and other application settings.");

        // --- Hotkeys enabled toggle ---
        let hotkeys_section = cosmic::widget::settings::section().title("Hotkeys").add(
            cosmic::widget::settings::item::builder("Enable global hotkeys")
                .description("When disabled, hotkeys will not trigger any actions")
                .control(
                    widget::toggler(self.config.hotkeys_enabled).on_toggle(Message::ToggleHotkeys),
                ),
        );

        // --- Step size ---
        let step_section = cosmic::widget::settings::section()
            .title("Step Sizes")
            .add(
                cosmic::widget::settings::item::builder(format!(
                    "Brightness step: {}",
                    self.config.hotkeys.brightness_step
                ))
                .control(widget::text::body("")),
            )
            .add(
                cosmic::widget::settings::item::builder(format!(
                    "Contrast step: {}",
                    self.config.hotkeys.contrast_step
                ))
                .control(widget::text::body("")),
            );

        let turn_off_labels: Vec<&str> = TurnOffBehavior::ALL
            .iter()
            .map(|behavior| behavior.label())
            .collect();
        let selected_turn_off = TurnOffBehavior::ALL
            .iter()
            .position(|behavior| behavior == &self.config.turn_off_behavior);
        let power_section = cosmic::widget::settings::section()
            .title("Turn Off Displays")
            .add(
                cosmic::widget::settings::item::builder("Power-off method").control(
                    widget::dropdown(turn_off_labels, selected_turn_off, |selected| {
                        Message::SetTurnOffBehavior(TurnOffBehavior::ALL[selected])
                    }),
                ),
            );

        let content = widget::column::with_capacity(5)
            .push(header)
            .push(description)
            .push(hotkeys_section)
            .push(step_section)
            .push(power_section)
            .spacing(space_s)
            .width(Length::Fill);

        // Wrap in scrollable to ensure all content is accessible
        widget::scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // -----------------------------------------------------------------------
    // Hotkey action handler
    // -----------------------------------------------------------------------

    fn nav_position_of(&self, target: Page) -> Option<u16> {
        self.nav.iter().find_map(|id| {
            if self.nav.data::<Page>(id) == Some(&target) {
                self.nav.position(id)
            } else {
                None
            }
        })
    }

    fn hotkey_mut(&mut self, id: &str) -> Option<&mut Hotkey> {
        self.config
            .hotkeys
            .hotkeys
            .iter_mut()
            .find(|hotkey| hotkey.id == id)
    }

    fn action_mut(&mut self, id: &str, idx: usize) -> Option<&mut HotkeyActionSpec> {
        self.hotkey_mut(id)
            .and_then(|hotkey| hotkey.actions.get_mut(idx))
    }

    fn initialize_hotkey_drafts(&mut self, id: &str) {
        let Some(hotkey) = self
            .config
            .hotkeys
            .hotkeys
            .iter()
            .find(|hotkey| hotkey.id == id)
        else {
            return;
        };
        for (idx, action) in hotkey.actions.iter().enumerate() {
            self.value_drafts
                .entry((id.to_string(), idx))
                .or_insert_with(|| action.value.to_string());
            self.vcp_drafts
                .entry((id.to_string(), idx))
                .or_insert_with(|| format!("0x{:02X}", action.vcp_code));
        }
    }

    fn clear_hotkey_drafts(&mut self, id: &str) {
        self.value_drafts
            .retain(|(hotkey_id, _), _| hotkey_id != id);
        self.vcp_drafts.retain(|(hotkey_id, _), _| hotkey_id != id);
    }

    fn validate_action_drafts(&self) -> Option<String> {
        for hotkey in &self.config.hotkeys.hotkeys {
            for (idx, action) in hotkey.actions.iter().enumerate() {
                if matches!(
                    action.target,
                    ActionTarget::Brightness | ActionTarget::Contrast | ActionTarget::CustomVcp
                ) {
                    let draft = self
                        .value_drafts
                        .get(&(hotkey.id.clone(), idx))
                        .map(String::as_str)
                        .unwrap_or("");
                    if !matches!(parse_value_draft(draft), Ok(Some(_))) {
                        return Some(format!(
                            "Hotkey {} action {} has an invalid Value.",
                            hotkey.binding,
                            idx + 1
                        ));
                    }
                }
                if action.target == ActionTarget::CustomVcp {
                    let draft = self
                        .vcp_drafts
                        .get(&(hotkey.id.clone(), idx))
                        .map(String::as_str)
                        .unwrap_or("");
                    if !matches!(parse_vcp_draft(draft), Ok(Some(_))) {
                        return Some(format!(
                            "Hotkey {} action {} has an invalid VCP code.",
                            hotkey.binding,
                            idx + 1
                        ));
                    }
                }
            }
        }
        None
    }

    fn refresh_hotkey_registration(&mut self) {
        self.hotkey_generation = self.hotkey_generation.wrapping_add(1);
        if let Some(manager) = &mut self.hotkey_manager {
            manager.update(&self.config);
            self.hotkey_action_map = manager.action_map();
            self.hotkey_status = manager.status();
        } else {
            self.hotkey_manager = HotkeyManager::new(&self.config);
            self.hotkey_action_map = self
                .hotkey_manager
                .as_ref()
                .map(HotkeyManager::action_map)
                .unwrap_or_else(|| Arc::new(HashMap::new()));
            self.hotkey_status = self
                .hotkey_manager
                .as_ref()
                .map(HotkeyManager::status)
                .unwrap_or_default();
        }
    }

    fn refresh_hotkey_actions(&mut self) {
        if let Some(manager) = &mut self.hotkey_manager {
            manager.rebuild_action_map(&self.config);
            self.hotkey_action_map = manager.action_map();
        }
    }

    fn enqueue_hardware_jobs(
        &mut self,
        jobs: impl IntoIterator<Item = HardwareJob>,
    ) -> cosmic::app::Task<Message> {
        self.action_executor.extend(jobs);
        self.start_next_hardware_job()
    }

    fn start_next_hardware_job(&mut self) -> cosmic::app::Task<Message> {
        let Some(job) = self.action_executor.start_next() else {
            return cosmic::app::Task::none();
        };
        cosmic::app::Task::perform(
            async move { tokio::task::spawn_blocking(move || job.execute()).await },
            |result| {
                cosmic::Action::App(Message::HardwareJobFinished(match result {
                    Ok(result) => result,
                    Err(error) => Err(format!("Task join error: {error}")),
                }))
            },
        )
    }

    fn resolve_monitors(&self, action: &HotkeyActionSpec) -> Vec<u32> {
        if action.all_monitors {
            self.detected_monitors
                .iter()
                .map(|monitor| monitor.id)
                .collect()
        } else {
            explicit_monitor_ids(action)
        }
    }

    fn turn_off_jobs(&self, monitor_ids: &[u32]) -> Vec<HardwareJob> {
        let mut jobs = Vec::new();
        if self.config.turn_off_behavior.uses_soft() {
            jobs.push(HardwareJob::SoftTurnOff);
        }
        if self.config.turn_off_behavior.uses_ddc() {
            for &monitor_id in monitor_ids {
                jobs.push(HardwareJob::SetPowerMode {
                    monitor_id,
                    mode: PowerMode::Off,
                });
            }
        }
        jobs
    }

    fn handle_hotkey_actions(
        &mut self,
        actions: Vec<HotkeyActionSpec>,
    ) -> cosmic::app::Task<Message> {
        let mut jobs = Vec::new();
        for action in actions {
            if action.action_type == ActionType::Off {
                jobs.extend(self.turn_off_jobs(&self.resolve_monitors(&action)));
                continue;
            }
            if action.target == ActionTarget::Profile {
                jobs.push(HardwareJob::ApplyProfile {
                    name: action.profile_name,
                });
                continue;
            }
            let monitor_ids = self.resolve_monitors(&action);
            match action.target {
                ActionTarget::Brightness => {
                    for monitor_id in monitor_ids {
                        match action.action_type {
                            ActionType::Set => {
                                let maximum = self
                                    .monitors
                                    .iter()
                                    .find(|monitor| monitor.info.id == monitor_id)
                                    .map(|monitor| monitor.brightness_max)
                                    .unwrap_or(100);
                                jobs.push(HardwareJob::SetBrightness {
                                    monitor_id,
                                    value: action.value.clamp(0, i32::from(maximum)) as u16,
                                });
                            }
                            ActionType::Offset => jobs.push(HardwareJob::OffsetBrightness {
                                monitor_id,
                                offset: action.value,
                            }),
                            ActionType::Off => unreachable!(),
                        }
                    }
                }
                ActionTarget::Contrast => {
                    for monitor_id in monitor_ids {
                        match action.action_type {
                            ActionType::Set => {
                                let maximum = self
                                    .monitors
                                    .iter()
                                    .find(|monitor| monitor.info.id == monitor_id)
                                    .map(|monitor| monitor.contrast_max)
                                    .unwrap_or(100);
                                jobs.push(HardwareJob::SetContrast {
                                    monitor_id,
                                    value: action.value.clamp(0, i32::from(maximum)) as u16,
                                });
                            }
                            ActionType::Offset => jobs.push(HardwareJob::OffsetContrast {
                                monitor_id,
                                offset: action.value,
                            }),
                            ActionType::Off => unreachable!(),
                        }
                    }
                }
                ActionTarget::InputSource => {
                    let inputs: Vec<(u32, InputSource)> = if action.all_monitors {
                        monitor_ids
                            .into_iter()
                            .map(|id| (id, action.input_source))
                            .collect()
                    } else if action.monitor_inputs.is_empty() {
                        action
                            .monitors
                            .iter()
                            .map(|id| (*id, action.input_source))
                            .collect()
                    } else {
                        action
                            .monitor_inputs
                            .iter()
                            .map(|input| (input.monitor_id, input.input_source))
                            .collect()
                    };
                    for (monitor_id, input_source) in inputs {
                        jobs.push(HardwareJob::SetInputSource {
                            monitor_id,
                            source: input_source,
                        });
                    }
                }
                ActionTarget::PowerMode => {
                    for monitor_id in monitor_ids {
                        jobs.push(HardwareJob::SetPowerMode {
                            monitor_id,
                            mode: action.power_mode,
                        });
                    }
                }
                ActionTarget::CustomVcp => {
                    for monitor_id in monitor_ids {
                        match action.action_type {
                            ActionType::Set => jobs.push(HardwareJob::SetCustomVcp {
                                monitor_id,
                                code: action.vcp_code,
                                value: action.value.clamp(0, i32::from(u16::MAX)) as u16,
                            }),
                            ActionType::Offset => jobs.push(HardwareJob::OffsetCustomVcp {
                                monitor_id,
                                code: action.vcp_code,
                                offset: action.value,
                            }),
                            ActionType::Off => unreachable!(),
                        }
                    }
                }
                ActionTarget::Profile => unreachable!(),
            }
        }
        self.enqueue_hardware_jobs(jobs)
    }

    // -----------------------------------------------------------------------
    // Title helper
    // -----------------------------------------------------------------------

    pub fn update_title(&mut self) -> cosmic::app::Task<Message> {
        let mut title = String::from("Windows Display Manager");
        if let Some(text) = self.nav.text(self.nav.active()) {
            title.push_str(" - ");
            title.push_str(text);
        }
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            cosmic::app::Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_drafts_allow_incomplete_text_and_validate_bounds() {
        assert_eq!(parse_value_draft(""), Ok(None));
        assert_eq!(parse_value_draft("-"), Ok(None));
        assert_eq!(parse_value_draft("+"), Ok(None));
        assert_eq!(parse_value_draft("-200"), Ok(Some(-200)));
        assert_eq!(parse_value_draft("200"), Ok(Some(200)));
        assert!(parse_value_draft("201").is_err());
        assert!(parse_value_draft("1.5").is_err());
    }

    #[test]
    fn vcp_drafts_accept_optional_prefix_and_reject_overflow() {
        assert_eq!(parse_vcp_draft(""), Ok(None));
        assert_eq!(parse_vcp_draft("0x"), Ok(None));
        assert_eq!(parse_vcp_draft("ff"), Ok(Some(0xff)));
        assert_eq!(parse_vcp_draft("0X10"), Ok(Some(0x10)));
        assert!(parse_vcp_draft("100").is_err());
        assert!(parse_vcp_draft("-1").is_err());
        assert!(parse_vcp_draft("xy").is_err());
    }

    #[test]
    fn individual_toggle_materializes_all_mode_without_forcing_it_back() {
        let mut action = HotkeyActionSpec {
            target: ActionTarget::InputSource,
            all_monitors: true,
            input_source: InputSource::Hdmi1,
            ..Default::default()
        };

        toggle_action_monitor(&mut action, &[1, 2], 1, false);
        assert!(!action.all_monitors);
        assert_eq!(action.monitors, vec![2]);
        assert_eq!(action.monitor_inputs.len(), 1);
        assert_eq!(action.monitor_inputs[0].monitor_id, 2);

        action.monitor_inputs[0].input_source = InputSource::Dp1;
        toggle_action_monitor(&mut action, &[1, 2], 1, true);
        assert!(action_master_selected(&action, &[1, 2]));
        assert!(!action.all_monitors);
        assert_eq!(action.monitor_inputs[0].input_source, InputSource::Dp1);
    }

    #[test]
    fn empty_detection_is_not_aggregate_all_and_off_uses_monitor_ids() {
        let mut action = HotkeyActionSpec {
            action_type: ActionType::Off,
            target: ActionTarget::InputSource,
            all_monitors: false,
            monitors: vec![7],
            monitor_inputs: vec![MonitorInput {
                monitor_id: 8,
                input_source: InputSource::Dp1,
            }],
            ..Default::default()
        };
        assert!(!action_master_selected(&action, &[]));
        assert_eq!(explicit_monitor_ids(&action), vec![7]);

        toggle_action_monitor(&mut action, &[7], 7, true);
        assert_eq!(action.monitors, vec![7]);
    }

    #[test]
    fn trigger_resolves_latest_payload_for_same_os_id() {
        let id = 42;
        let mut action_map = HashMap::new();
        action_map.insert(id, vec![HotkeyActionSpec::default()]);
        action_map.insert(
            id,
            vec![HotkeyActionSpec {
                action_type: ActionType::Offset,
                target: ActionTarget::Contrast,
                all_monitors: false,
                monitors: vec![3],
                value: -17,
                ..Default::default()
            }],
        );

        let actions = resolve_triggered_actions(true, false, &action_map, id).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target, ActionTarget::Contrast);
        assert_eq!(actions[0].value, -17);
        assert_eq!(actions[0].monitors, vec![3]);
        assert!(resolve_triggered_actions(false, false, &action_map, id).is_none());
        assert!(resolve_triggered_actions(true, true, &action_map, id).is_none());
        assert!(resolve_triggered_actions(true, false, &action_map, 999).is_none());
    }
}
