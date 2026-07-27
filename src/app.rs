use crate::config::{
    ActionTarget, ActionType, AppConfig, Hotkey, HotkeyActionSpec, HotkeyBinding, MonitorInput,
    TurnOffBehavior,
};
use crate::ccd;
use crate::ddc::{self, InputSource, MonitorInfo, MonitorState, PowerMode};
use crate::hotkeys::{self, HotkeyManager};
use crate::profiles;
use crate::tray::{SystemTray, TrayMessage, TrayStream};
use cosmic::app::Application;
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::event::{self, Event};
use cosmic::iced::keyboard::{Event as KeyboardEvent, Key, Modifiers};
use cosmic::iced::{Alignment, Length, Subscription, window};
use cosmic::prelude::*;
use cosmic::widget::{self, nav_bar};
use cosmic::{executor, Core};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Simple inline SVG icon (monitor symbol)
const APP_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="131.1 120.55 261.65 226.85">  <!-- Monitor -->  <path d="M 365.047 120.547 L 158.791 120.547 C 143.497 120.554 131.102 132.953 131.101 148.247 L 131.101 276.344 C 131.102 291.636 143.498 304.033 158.791 304.034 L 229.066 304.034 L 229.066 326.233 L 197.294 326.233 C 189.148 326.233 184.056 335.051 188.129 342.106 C 190.02 345.38 193.513 347.397 197.294 347.397 L 326.547 347.397 C 334.693 347.397 339.784 338.579 335.711 331.524 C 333.821 328.25 330.327 326.233 326.547 326.233 L 294.771 326.233 L 294.771 304.035 L 365.046 304.035 C 380.342 304.038 392.744 291.64 392.746 276.344 L 392.746 148.242 C 392.742 132.946 380.342 120.548 365.046 120.547 L 365.047 120.547 Z M 368.507 276.34 C 368.502 278.25 366.956 279.797 365.047 279.804 L 158.791 279.804 C 156.881 279.798 155.336 278.249 155.336 276.34 L 155.336 148.242 C 155.337 146.334 156.883 144.789 158.791 144.788 L 365.047 144.788 C 366.954 144.79 368.501 146.334 368.507 148.242 L 368.507 276.34 Z" data-name="Monitor" style=""></path>    <!-- Centered Lightning Bolt -->  <path transform="translate(5, -10)" d="M 271 190 L 248.371 190 L 236.157 224.479 C 236.124 224.577 236.197 224.676 236.306 224.681 L 250.319 224.681 C 250.433 224.676 250.517 224.78 250.484 224.882 L 239.64 255.276 C 239.438 255.842 239.967 256.398 240.589 256.277 C 240.792 256.238 240.97 256.13 241.092 255.972 L 273.806 213.863 C 273.882 213.769 273.82 213.631 273.692 213.617 C 273.688 213.617 273.681 213.616 273.674 213.616 L 258.968 213.616 C 258.847 213.603 258.775 213.489 258.819 213.384 L 271.149 190.17 C 271.157 190.085 271.091 190.009 271 190 Z" style="fill: rgb(255, 221, 0);"></path></svg>"#;

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
    MonitorsDetected(Vec<MonitorInfo>),
    MonitorStateLoaded(u32, Box<MonitorState>),
    SetBrightness(u32, u16),
    SetContrast(u32, u16),
    SelectInputSource(u32, usize),        // monitor_id, index into INPUT_SOURCES
    BrightnessApplied(u32, u16),
    ContrastApplied(u32, u16),
    InputSourceApplied(u32, InputSource),
    PowerModeApplied(u32, PowerMode),
    CustomVcpApplied(u32, u8, u16),
    // Debounced slider changes
    BrightnessSliderChanged(u32, u16),
    ContrastSliderChanged(u32, u16),
    ApplyBrightnessDebounced(u32, u16),
    ApplyContrastDebounced(u32, u16),
    // Hotkeys (action-chain model)
    HotkeyTriggered(Vec<HotkeyActionSpec>),
    ToggleHotkeys(bool),
    AddHotkey,
    DeleteHotkey(String),
    StartRecording(String),
    CancelRecording,
    ClearBinding(String),
    KeyPressed(Modifiers, Key),
    AddAction(String),
    DeleteAction(String, usize),
    SetActionType(String, usize, ActionType),
    SetActionTarget(String, usize, ActionTarget),
    SetActionValue(String, usize, i32),
    SetActionVcpCode(String, usize, u8),
    SetActionInputSource(String, usize, InputSource),
    /// Set (Some) or clear (None) the input for one monitor in a per-monitor
    /// Input Source action.
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
    ApplyProfile(String),
    DeleteProfile(String),
    ProfileApplied(String),
    AddProfileHotkey(String),
    MonitorsPoweredOff,
    // System tray
    Tray(TrayMessage),
    /// Hide window (close-to-tray)
    HideWindow,
    /// A window surface was closed (used to reset the tracked main window)
    WindowClosed(window::Id),
    /// Open a URL from the About page in the default browser
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
    Recording {
        hotkey_id: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        key: String,
    },
}


// ---------------------------------------------------------------------------
// Application model
// ---------------------------------------------------------------------------

pub struct AppModel {
    core: Core,
    nav: nav_bar::Model,
    monitors: Vec<MonitorState>,
    config: AppConfig,
    hotkey_manager: Option<HotkeyManager>,
    hotkey_action_map: Arc<HashMap<u32, Vec<HotkeyActionSpec>>>,
    // Per-hotkey (by `Hotkey::id`) OS registration status, for the active/inactive indicator.
    hotkey_status: HashMap<String, bool>,
    status_message: String,
    recording_state: RecordingState,
    about: widget::about::About,
    // Debounce state for sliders
    pending_brightness: Option<(u32, u16)>,
    pending_contrast: Option<(u32, u16)>,
    // Saved monitor-layout profiles (file stems)
    profiles: Vec<String>,
    // Text input for naming a new profile
    profile_name_input: String,
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


fn input_source_index(source: &InputSource) -> Option<usize> {
    INPUT_SOURCES.iter().position(|s| s == source)
}

// List of power modes shown in the hotkey action dropdown
const POWER_MODES: &[PowerMode] = &[
    PowerMode::On,
    PowerMode::Standby,
    PowerMode::Suspend,
    PowerMode::Off,
];

fn power_mode_index(mode: &PowerMode) -> Option<usize> {
    POWER_MODES.iter().position(|m| m == mode)
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
            }.to_string()
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
    if ctrl { parts.push("Ctrl"); }
    if alt { parts.push("Alt"); }
    if shift { parts.push("Shift"); }
    if win { parts.push("Win"); }
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
            monitors: Vec::new(),
            config,
            hotkey_manager,
            hotkey_action_map,
            hotkey_status,
            status_message: "Starting...".into(),
            recording_state: RecordingState::NotRecording,
            about,
            pending_brightness: None,
            pending_contrast: None,
            profiles: Vec::new(),
            profile_name_input: String::new(),
            tray,
        };

        // Fire initial monitor detection and profile listing
        let cmd = cosmic::app::Task::batch([
            app.update(Message::RefreshMonitors),
            app.update(Message::RefreshProfiles),
        ]);
        (app, cmd)
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(
        &mut self,
        id: nav_bar::Id,
    ) -> cosmic::app::Task<Self::Message> {
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
            subs.push(
                hotkeys::hotkey_subscription(Arc::clone(&self.hotkey_action_map))
                    .map(Message::HotkeyTriggered),
            );
        }

        // Keyboard event subscription when recording hotkeys
        if !matches!(self.recording_state, RecordingState::NotRecording) {
            subs.push(
                event::listen_with(|event, _status, _id| {
                    if let Event::Keyboard(KeyboardEvent::KeyPressed { key, modifiers, .. }) = event {
                        Some(Message::KeyPressed(modifiers, key))
                    } else {
                        None
                    }
                })
            );
        }

        // System tray subscription
        if let Some((_, ref tray_stream)) = self.tray {
            subs.push(
                tray_stream.clone().subscription().map(Message::Tray),
            );
        }

        // Track window close so the tray can reopen a fresh window later.
        // The tracked main window id must only be cleared after the surface is
        // actually gone (see the WindowClosed handler in `update`).
        subs.push(
            event::listen_with(|event, _status, id| {
                if let Event::Window(window::Event::Closed) = event {
                    Some(Message::WindowClosed(id))
                } else {
                    None
                }
            })
        );

        Subscription::batch(subs)
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    fn update(&mut self, message: Self::Message) -> cosmic::app::Task<Self::Message> {
        match message {
            // -- Monitor detection ------------------------------------------
            Message::RefreshMonitors => {
                self.status_message = "Detecting monitors...".into();
                return cosmic::app::Task::perform(
                    async { tokio::task::spawn_blocking(ddc::detect_monitors).await },
                    |result| match result {
                        Ok(Ok(monitors)) => cosmic::Action::App(Message::MonitorsDetected(monitors)),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("DDC detection error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::MonitorsDetected(infos) => {
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
                // Profiles page
                self.nav
                    .insert()
                    .text("Profiles")
                    .data::<Page>(Page::Profiles);
                // Settings page
                self.nav
                    .insert()
                    .text("Settings")
                    .data::<Page>(Page::Settings);
                // About page
                self.nav
                    .insert()
                    .text("About")
                    .data::<Page>(Page::About);

                // Activate first monitor
                self.nav.activate_position(0);

                self.status_message =
                    format!("{} monitor(s) detected", infos.len());

                // Kick off state reads for each monitor
                let mut tasks = Vec::new();
                for info in infos {
                    let mid = info.id;
                    tasks.push(cosmic::app::Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                ddc::read_monitor_state(mid, info)
                            })
                            .await
                        },
                        move |result| match result {
                            Ok(Ok(state)) => {
                                cosmic::Action::App(Message::MonitorStateLoaded(mid, Box::new(state)))
                            }
                            Ok(Err(e)) => {
                                cosmic::Action::App(Message::Error(format!("Monitor {mid} read error: {e}")))
                            }
                            Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                        },
                    ));
                }
                return cosmic::app::Task::batch(tasks);
            }

            Message::MonitorStateLoaded(id, state) => {
                // Upsert
                if let Some(existing) = self.monitors.iter_mut().find(|m| m.info.id == id)
                {
                    *existing = *state;
                } else {
                    self.monitors.push(*state);
                }
                self.monitors.sort_by_key(|m| m.info.id);
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
                    move |(mid, val)| cosmic::Action::App(Message::ApplyBrightnessDebounced(mid, val)),
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
                // Direct brightness set (used by hotkeys and debounced slider)
                if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                    m.brightness = value;
                }
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ddc::set_brightness(monitor_id, value)
                        })
                        .await
                    },
                    move |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::BrightnessApplied(monitor_id, value)),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Brightness error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::BrightnessApplied(_monitor_id, _value) => {
                // Already optimistically set
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
                    move |(mid, val)| cosmic::Action::App(Message::ApplyContrastDebounced(mid, val)),
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
                // Direct contrast set (used by hotkeys and debounced slider)
                if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                    m.contrast = value;
                }
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ddc::set_contrast(monitor_id, value)
                        })
                        .await
                    },
                    move |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::ContrastApplied(monitor_id, value)),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Contrast error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::ContrastApplied(_monitor_id, _value) => {}

            // -- Input source -----------------------------------------------
            Message::SelectInputSource(monitor_id, idx) => {
                if let Some(&source) = INPUT_SOURCES.get(idx) {
                    if let Some(m) =
                        self.monitors.iter_mut().find(|m| m.info.id == monitor_id)
                    {
                        m.input_source = source;
                    }
                    return cosmic::app::Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                ddc::set_input_source(monitor_id, source)
                            })
                            .await
                        },
                        move |result| match result {
                            Ok(Ok(())) => cosmic::Action::App(Message::InputSourceApplied(monitor_id, source)),
                            Ok(Err(e)) => {
                                cosmic::Action::App(Message::Error(format!("Input source error: {e}")))
                            }
                            Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                        },
                    );
                }
            }

            Message::InputSourceApplied(_monitor_id, _source) => {}

            Message::PowerModeApplied(_monitor_id, _power_mode) => {}

            Message::CustomVcpApplied(_monitor_id, _code, _value) => {}

            // -- Hotkey actions ---------------------------------------------
            Message::HotkeyTriggered(actions) => {
                return self.handle_hotkey_action(actions);
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
            }

            Message::AddHotkey => {
                let hotkey = Hotkey::new_empty();
                let id = hotkey.id.clone();
                self.config.hotkeys.hotkeys.push(hotkey);
                self.recording_state = RecordingState::Recording {
                    hotkey_id: id,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message =
                    "New hotkey added. Press a key combination to bind it.".into();
            }

            Message::DeleteHotkey(id) => {
                self.config.hotkeys.hotkeys.retain(|h| h.id != id);
                if matches!(&self.recording_state, RecordingState::Recording { hotkey_id, .. } if hotkey_id == &id)
                {
                    self.recording_state = RecordingState::NotRecording;
                }
                self.refresh_hotkey_registration();
            }

            Message::StartRecording(hotkey_id) => {
                self.recording_state = RecordingState::Recording {
                    hotkey_id,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message = "Recording hotkey... Press modifiers and key".into();
            }

            Message::CancelRecording => {
                self.recording_state = RecordingState::NotRecording;
                self.status_message = "Hotkey recording cancelled".into();
            }

            Message::ClearBinding(id) => {
                if let Some(hk) = self.hotkey_mut(&id) {
                    hk.binding = HotkeyBinding::unbound();
                }
                self.refresh_hotkey_registration();
                self.status_message = "Hotkey binding cleared.".into();
            }

            Message::KeyPressed(modifiers, key) => {
                // Convert the key to our internal format
                let key_string = key_to_string(&key);

                if key_string.is_empty() {
                    return cosmic::app::Task::none();
                }

                let ctrl = modifiers.control();
                let alt = modifiers.alt();
                let shift = modifiers.shift();
                let win = modifiers.logo();

                if let RecordingState::Recording { hotkey_id, .. } = &self.recording_state {
                    let hotkey_id = hotkey_id.clone();
                    if let Some(hk) = self.hotkey_mut(&hotkey_id) {
                        hk.binding = HotkeyBinding {
                            ctrl,
                            alt,
                            shift,
                            win,
                            key: key_string.clone(),
                        };
                    }
                    self.status_message = format!(
                        "Hotkey bound to {}. Remember to save configuration.",
                        format_hotkey(ctrl, alt, shift, win, &key_string)
                    );
                    self.recording_state = RecordingState::NotRecording;
                    self.refresh_hotkey_registration();
                }
            }

            Message::AddAction(id) => {
                if let Some(hk) = self.hotkey_mut(&id) {
                    hk.actions.push(HotkeyActionSpec::default());
                }
                self.refresh_hotkey_actions();
            }

            Message::DeleteAction(id, idx) => {
                if let Some(hk) = self.hotkey_mut(&id) {
                    if idx < hk.actions.len() {
                        hk.actions.remove(idx);
                    }
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionType(id, idx, action_type) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.action_type = action_type;
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionTarget(id, idx, target) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.target = target;
                    if !target.supports_offset() && a.action_type == ActionType::Offset {
                        a.action_type = ActionType::Set;
                    }
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionValue(id, idx, value) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.value = value;
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionVcpCode(id, idx, code) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.vcp_code = code;
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionInputSource(id, idx, source) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.input_source = source;
                }
                self.refresh_hotkey_actions();
            }

            Message::SetMonitorInput(id, idx, monitor_id, source) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    match source {
                        Some(src) => {
                            if let Some(mi) = a
                                .monitor_inputs
                                .iter_mut()
                                .find(|mi| mi.monitor_id == monitor_id)
                            {
                                mi.input_source = src;
                            } else {
                                a.monitor_inputs.push(MonitorInput {
                                    monitor_id,
                                    input_source: src,
                                });
                            }
                        }
                        None => a.monitor_inputs.retain(|mi| mi.monitor_id != monitor_id),
                    }
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionPowerMode(id, idx, mode) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.power_mode = mode;
                }
                self.refresh_hotkey_actions();
            }

            Message::SetActionProfile(id, idx, name) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.profile_name = name;
                }
                self.refresh_hotkey_actions();
            }

            Message::ToggleActionAllMonitors(id, idx, all) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    a.all_monitors = all;
                }
                self.refresh_hotkey_actions();
            }

            Message::ToggleActionMonitor(id, idx, monitor_id, checked) => {
                if let Some(a) = self.action_mut(&id, idx) {
                    if checked {
                        if !a.monitors.contains(&monitor_id) {
                            a.monitors.push(monitor_id);
                        }
                    } else {
                        a.monitors.retain(|m| *m != monitor_id);
                    }
                }
                self.refresh_hotkey_actions();
            }

            Message::SetTurnOffBehavior(behavior) => {
                self.config.turn_off_behavior = behavior;
            }

            Message::SaveConfig => {
                // Save and re-register hotkeys
                if let Err(e) = self.config.save() {
                    self.status_message = format!("Failed to save config: {e}");
                } else {
                    self.status_message = "Configuration saved and hotkeys activated.".into();
                    self.refresh_hotkey_registration();
                }
            }

            // -- Profiles ---------------------------------------------------
            Message::RefreshProfiles => {
                return cosmic::app::Task::perform(
                    async { tokio::task::spawn_blocking(profiles::list_profiles).await },
                    |result| match result {
                        Ok(list) => cosmic::Action::App(Message::ProfilesListed(list)),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::ProfilesListed(list) => {
                self.profiles = list;
                // Keep the tray menu in sync with the profile list.
                if let Some((ref tray, _)) = self.tray {
                    tray.update_menu(&self.profiles);
                }
            }

            Message::ProfileNameInput(text) => {
                self.profile_name_input = text;
            }

            Message::SaveCurrentProfile(name) => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Enter a profile name first.".into();
                    return cosmic::app::Task::none();
                }
                self.profile_name_input.clear();
                self.status_message = format!("Saving profile '{name}'...");
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || profiles::save_current(&name)).await
                    },
                    |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::RefreshProfiles),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Save profile error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::ApplyProfile(name) => {
                self.status_message = format!("Applying profile '{name}'...");
                let for_task = name.clone();
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || profiles::apply_profile(&for_task)).await
                    },
                    move |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::ProfileApplied(name.clone())),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Apply profile error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::ProfileApplied(name) => {
                self.status_message = format!("Applied profile '{name}'.");
                // Topology changed: re-detect monitors.
                return self.update(Message::RefreshMonitors);
            }

            Message::DeleteProfile(name) => {
                return cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || profiles::delete_profile(&name)).await
                    },
                    |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::RefreshProfiles),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Delete profile error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                );
            }

            Message::AddProfileHotkey(profile_name) => {
                let hotkey = Hotkey::new_for_profile(profile_name);
                let id = hotkey.id.clone();
                self.config.hotkeys.hotkeys.push(hotkey);
                if let Some(pos) = self.nav_position_of(Page::Hotkeys) {
                    self.nav.activate_position(pos);
                }
                self.recording_state = RecordingState::Recording {
                    hotkey_id: id,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message =
                    "New profile hotkey added on the Hotkeys page. Press a key combination to bind it."
                        .into();
            }

            Message::MonitorsPoweredOff => {
                self.status_message = "Monitors turned off.".into();
            }

            // -- Hide window (close-to-tray) --------------------------------
            Message::HideWindow => {
                log::info!("Hiding window to tray");
                if let Some(id) = self.core.main_window_id() {
                    return window::close(id);
                }
            }

            // -- Window surface closed --------------------------------------
            // Reset the tracked main window once its surface is gone so the
            // tray can open a fresh window on the next `ShowWindow`.
            Message::WindowClosed(id) => {
                if self.core.main_window_id() == Some(id) {
                    log::info!("Main window closed; clearing tracked window id");
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
                            // Window was closed — open a new one.
                            // `decorations: false` lets COSMIC draw its own header
                            // bar (client-side decorations) without the native
                            // Windows title bar appearing as well.
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
                        log::info!("Tray: load profile '{name}'");
                        return self.update(Message::ApplyProfile(name));
                    }
                    TrayMessage::SaveCurrentProfile => {
                        log::info!("Tray: save current profile requested");
                        // Show the window on the Profiles page so the user can name it.
                        if let Some(pos) = self.nav_position_of(Page::Profiles) {
                            self.nav.activate_position(pos);
                        }
                        return self.update(Message::Tray(TrayMessage::ShowWindow));
                    }
                    TrayMessage::TurnOffMonitors => {
                        log::info!("Tray: turn off monitors");
                        return cosmic::app::Task::perform(
                            async { tokio::task::spawn_blocking(ccd::turn_off_monitors).await },
                            |result| match result {
                                Ok(()) => cosmic::Action::App(Message::MonitorsPoweredOff),
                                Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                            },
                        );
                    }
                    TrayMessage::Exit => {
                        log::info!("Tray: Exit requested");
                        return cosmic::iced::exit();
                    }
                }
            }

            Message::OpenUrl(url) => {
                if let Err(e) = std::process::Command::new("rundll32.exe")
                    .args(["url.dll,FileProtocolHandler", &url])
                    .spawn()
                {
                    log::warn!("Failed to open URL {url}: {e}");
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
            Page::Hotkeys => self.view_hotkeys(),
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
                widget::container(
                    widget::text::body("Loading monitor data...")
                )
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
                    if mon.info.is_primary { " [Primary]" } else { "" }
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
                let brightness_section = cosmic::widget::settings::section()
                    .title("Brightness")
                    .add(
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
                let contrast_section = cosmic::widget::settings::section()
                    .title("Contrast")
                    .add(
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

                widget::container(
                    widget::column::with_capacity(6)
                        .push(header)
                        .push(resolution_label)
                        .push(input_section)
                        .push(brightness_section)
                        .push(contrast_section)
                        .spacing(space_s)
                        .width(Length::Fill)
                        .max_width(700.0),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
        }
    }

    /// View for the hotkeys configuration page.
    fn view_hotkeys(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Hotkeys");
        let description = widget::text::body(
            "Configure global hotkeys, each bound to a chain of one or more actions. \
             Changes take effect immediately; press Save to persist them.",
        );

        let mut cards =
            widget::column::with_capacity(self.config.hotkeys.hotkeys.len() + 1).spacing(space_s);

        if self.config.hotkeys.hotkeys.is_empty() {
            cards = cards.push(widget::text::body("No hotkeys configured yet."));
        }

        for hotkey in &self.config.hotkeys.hotkeys {
            cards = cards.push(self.view_hotkey_card(hotkey, space_s));
        }

        let add_button = widget::button::standard("+ Add Hotkey").on_press(Message::AddHotkey);

        let turn_off_labels: Vec<&str> =
            TurnOffBehavior::ALL.iter().map(|b| b.label()).collect();
        let turn_off_idx = TurnOffBehavior::ALL
            .iter()
            .position(|b| *b == self.config.turn_off_behavior);
        let turn_off_row = widget::row::with_capacity(2)
            .push(widget::text::body("\"Turn Off\" action behavior").width(Length::FillPortion(2)))
            .push(widget::dropdown(turn_off_labels, turn_off_idx, |idx| {
                Message::SetTurnOffBehavior(TurnOffBehavior::ALL[idx])
            }))
            .spacing(space_s)
            .align_y(Alignment::Center);

        let turn_off_section = cosmic::widget::settings::section()
            .title("Turn Off Behavior")
            .add(turn_off_row);

        let save_row = widget::row::with_capacity(1)
            .push(widget::button::suggested("Save Configuration").on_press(Message::SaveConfig));

        let content = widget::column::with_capacity(6)
            .push(header)
            .push(description)
            .push(cards)
            .push(add_button)
            .push(turn_off_section)
            .push(save_row)
            .spacing(space_s)
            .width(Length::Fill);

        widget::scrollable(
            widget::container(content).width(Length::Fill).max_width(800.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Render a single hotkey's card: accelerator controls + action chain.
    fn view_hotkey_card(&self, hotkey: &Hotkey, space_s: u16) -> Element<'_, Message> {
        let id = hotkey.id.clone();
        let is_active = self.hotkey_status.get(&hotkey.id).copied().unwrap_or(false);
        let recording_this =
            matches!(&self.recording_state, RecordingState::Recording { hotkey_id, .. } if hotkey_id == &hotkey.id);

        let status_dot = widget::text::body(if is_active { "●" } else { "○" });

        let accel_row: Element<'_, Message> = if recording_this {
            let (ctrl, alt, shift, win, key) = match &self.recording_state {
                RecordingState::Recording { ctrl, alt, shift, win, key, .. } => {
                    (*ctrl, *alt, *shift, *win, key.clone())
                }
                _ => unreachable!(),
            };
            let captured = if key.is_empty() {
                "Waiting for key press...".to_string()
            } else {
                format!("Captured: {}", format_hotkey(ctrl, alt, shift, win, &key))
            };
            widget::row::with_capacity(2)
                .push(widget::text::body(captured).width(Length::Fill))
                .push(widget::button::standard("Cancel").on_press(Message::CancelRecording))
                .spacing(space_s)
                .align_y(Alignment::Center)
                .into()
        } else {
            widget::row::with_capacity(5)
                .push(status_dot)
                .push(widget::text::body(hotkey.binding.to_string()).width(Length::Fill))
                .push(widget::button::standard("Record").on_press(Message::StartRecording(id.clone())))
                .push(widget::button::standard("Clear").on_press(Message::ClearBinding(id.clone())))
                .push(
                    widget::button::destructive("Delete Hotkey")
                        .on_press(Message::DeleteHotkey(id.clone())),
                )
                .spacing(space_s)
                .align_y(Alignment::Center)
                .into()
        };

        let mut actions_col =
            widget::column::with_capacity(hotkey.actions.len() + 1).spacing(space_s);

        for (idx, action) in hotkey.actions.iter().enumerate() {
            actions_col = actions_col.push(self.view_action_row(&id, idx, action, space_s));
        }

        actions_col = actions_col.push(
            widget::button::standard("+ Add Action").on_press(Message::AddAction(id.clone())),
        );

        let content = widget::column::with_capacity(2)
            // Match the action cards' horizontal padding so the accelerator row's
            // trailing buttons (Cancel / Delete Hotkey) line up with each action
            // row's Delete button below.
            .push(widget::container(accel_row).padding([0, space_s]))
            .push(actions_col)
            .spacing(space_s);

        cosmic::widget::settings::section()
            .title(format!("Hotkey: {}", hotkey.binding))
            .add(content)
            .into()
    }

    /// Render a single action row within a hotkey's action chain.
    fn view_action_row(
        &self,
        hotkey_id: &str,
        idx: usize,
        action: &HotkeyActionSpec,
        space_s: u16,
    ) -> Element<'_, Message> {
        let id = hotkey_id.to_string();

        let type_options: &[ActionType] = if action.target.supports_offset() {
            ActionType::ALL
        } else {
            ActionType::NO_OFFSET
        };
        let type_labels: Vec<&str> = type_options.iter().map(|t| t.label()).collect();
        let type_idx = type_options.iter().position(|t| *t == action.action_type);
        let type_dropdown = widget::dropdown(type_labels, type_idx, {
            let id = id.clone();
            move |i| Message::SetActionType(id.clone(), idx, type_options[i])
        });

        let target_labels: Vec<&str> = ActionTarget::ALL.iter().map(|t| t.label()).collect();
        let target_idx = ActionTarget::ALL.iter().position(|t| *t == action.target);
        let target_dropdown = widget::dropdown(target_labels, target_idx, {
            let id = id.clone();
            move |i| Message::SetActionTarget(id.clone(), idx, ActionTarget::ALL[i])
        });

        let mut header_row = widget::row::with_capacity(4)
            .push(type_dropdown)
            .spacing(space_s)
            .align_y(Alignment::Center);

        if action.action_type != ActionType::Off {
            header_row = header_row.push(target_dropdown);
        }

        header_row = header_row.push(widget::Space::new().width(Length::Fill)).push(
            widget::button::destructive("Delete")
                .on_press(Message::DeleteAction(id.clone(), idx)),
        );

        let mut rows = widget::column::with_capacity(3).push(header_row).spacing(space_s);

        if action.action_type != ActionType::Off {
            let value_control: Option<Element<'_, Message>> = match action.target {
                ActionTarget::Brightness | ActionTarget::Contrast => {
                    let id = id.clone();
                    Some(
                        widget::spin_button(
                            action.value.to_string(),
                            action.value,
                            1i32,
                            -200i32,
                            200i32,
                            move |v| Message::SetActionValue(id.clone(), idx, v),
                        )
                        .into(),
                    )
                }
                ActionTarget::CustomVcp => {
                    let id_val = id.clone();
                    let id_code = id.clone();
                    let value_row = widget::row::with_capacity(4)
                        .push(widget::text::body("VCP code"))
                        .push(widget::spin_button(
                            format!("0x{:02X}", action.vcp_code),
                            action.vcp_code,
                            1u8,
                            0u8,
                            255u8,
                            move |v| Message::SetActionVcpCode(id_code.clone(), idx, v),
                        ))
                        .push(widget::text::body("Value"))
                        .push(widget::spin_button(
                            action.value.to_string(),
                            action.value,
                            1i32,
                            -200i32,
                            200i32,
                            move |v| Message::SetActionValue(id_val.clone(), idx, v),
                        ))
                        .spacing(space_s)
                        .align_y(Alignment::Center);
                    Some(value_row.into())
                }
                ActionTarget::InputSource => {
                    if action.all_monitors {
                        let labels: Vec<String> =
                            INPUT_SOURCES.iter().map(|s| s.to_string()).collect();
                        let sel = input_source_index(&action.input_source);
                        let id = id.clone();
                        Some(
                            widget::dropdown(labels, sel, move |i| {
                                Message::SetActionInputSource(id.clone(), idx, INPUT_SOURCES[i])
                            })
                            .into(),
                        )
                    } else {
                        // Per-monitor input pickers are rendered in the monitor
                        // section below.
                        None
                    }
                }
                ActionTarget::PowerMode => {
                    let labels: Vec<String> = POWER_MODES.iter().map(|m| m.to_string()).collect();
                    let sel = power_mode_index(&action.power_mode);
                    let id = id.clone();
                    Some(
                        widget::dropdown(labels, sel, move |i| {
                            Message::SetActionPowerMode(id.clone(), idx, POWER_MODES[i])
                        })
                        .into(),
                    )
                }
                ActionTarget::Profile => {
                    let id = id.clone();
                    if self.profiles.is_empty() {
                        Some(widget::text::body("No profiles saved yet.").into())
                    } else {
                        let sel = self.profiles.iter().position(|p| p == &action.profile_name);
                        let profiles = self.profiles.clone();
                        Some(
                            widget::dropdown(self.profiles.clone(), sel, move |i| {
                                Message::SetActionProfile(id.clone(), idx, profiles[i].clone())
                            })
                            .into(),
                        )
                    }
                }
            };

            if let Some(control) = value_control {
                rows = rows.push(control);
            }
        }

        // Monitor selection. Input Source actions get per-monitor input pickers
        // so a single hotkey can switch different monitors to different inputs;
        // other targets get simple monitor checkboxes.
        if action.target == ActionTarget::InputSource {
            let all_checkbox = {
                let id = id.clone();
                widget::checkbox(action.all_monitors)
                    .label("All displays use the same input")
                    .on_toggle(move |checked| {
                        Message::ToggleActionAllMonitors(id.clone(), idx, checked)
                    })
            };
            rows = rows.push(all_checkbox);

            if !action.all_monitors {
                for mon in &self.monitors {
                    let mid = mon.info.id;
                    let label = if mon.info.name.is_empty() {
                        format!("Monitor {mid}")
                    } else {
                        mon.info.name.clone()
                    };

                    // Options: "Don't change" (index 0) then every input source.
                    let mut labels: Vec<String> = Vec::with_capacity(INPUT_SOURCES.len() + 1);
                    labels.push("— Don't change —".to_string());
                    labels.extend(INPUT_SOURCES.iter().map(|s| s.to_string()));

                    let current = action
                        .monitor_inputs
                        .iter()
                        .find(|mi| mi.monitor_id == mid)
                        .map(|mi| mi.input_source);
                    let sel = match current {
                        Some(src) => input_source_index(&src).map(|i| i + 1),
                        None => Some(0),
                    };

                    let id = id.clone();
                    let row = widget::row::with_capacity(2)
                        .push(widget::text::body(label).width(Length::FillPortion(2)))
                        .push(widget::dropdown(labels, sel, move |i| {
                            let source = if i == 0 { None } else { Some(INPUT_SOURCES[i - 1]) };
                            Message::SetMonitorInput(id.clone(), idx, mid, source)
                        }))
                        .spacing(space_s)
                        .align_y(Alignment::Center);
                    rows = rows.push(row);
                }
            }
        } else {
            let all_checkbox = {
                let id = id.clone();
                widget::checkbox(action.all_monitors)
                    .label("All Displays")
                    .on_toggle(move |checked| {
                        Message::ToggleActionAllMonitors(id.clone(), idx, checked)
                    })
            };

            let mut monitors_row = widget::row::with_capacity(self.monitors.len() + 1)
                .push(all_checkbox)
                .spacing(space_s)
                .align_y(Alignment::Center);

            if !action.all_monitors {
                for mon in &self.monitors {
                    let mid = mon.info.id;
                    let checked = action.monitors.contains(&mid);
                    let label = if mon.info.name.is_empty() {
                        format!("Monitor {mid}")
                    } else {
                        mon.info.name.clone()
                    };
                    let id = id.clone();
                    monitors_row = monitors_row.push(
                        widget::checkbox(checked)
                            .label(label)
                            .on_toggle(move |c| Message::ToggleActionMonitor(id.clone(), idx, mid, c)),
                    );
                }
            }

            rows = rows.push(monitors_row);
        }

        widget::container(rows)
            .class(cosmic::theme::Container::Card)
            .padding(space_s)
            .into()
    }


    /// View for the settings page (step sizes and other config).
    fn view_settings(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Settings");
        let description = widget::text::body(
            "Configure step sizes and other application settings.",
        );

        // --- Hotkeys enabled toggle ---
        let hotkeys_section = cosmic::widget::settings::section()
            .title("Hotkeys")
            .add(
                cosmic::widget::settings::item::builder("Enable global hotkeys")
                    .description("When disabled, hotkeys will not trigger any actions")
                    .control(
                        widget::toggler(self.config.hotkeys_enabled)
                            .on_toggle(Message::ToggleHotkeys),
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

        let content = widget::column::with_capacity(4)
            .push(header)
            .push(description)
            .push(hotkeys_section)
            .push(step_section)
            .spacing(space_s)
            .width(Length::Fill);

        // Wrap in scrollable to ensure all content is accessible
        widget::scrollable(
            widget::container(content).width(Length::Fill).max_width(700.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// View for the About page.
    fn view_about(&self) -> Element<'_, Message> {
        widget::scrollable(
            cosmic::widget::about(&self.about, |url| Message::OpenUrl(url.to_owned())),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// View for the monitor-layout profiles page.
    fn view_profiles(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Monitor Layout Profiles");
        let description = widget::text::body(
            "Save the current monitor arrangement (resolution, position, primary, \
             refresh rate, rotation, scaling) as a profile and restore it later \
             with one click, a hotkey, or from the tray.",
        );

        // Saved profiles list.
        let mut items =
            widget::column::with_capacity(self.profiles.len() + 1).spacing(4);
        if self.profiles.is_empty() {
            items = items.push(widget::text::body("No profiles saved yet."));
        } else {
            for name in &self.profiles {
                let row = widget::row::with_capacity(4)
                    .push(widget::text::body(name.clone()).width(Length::Fill))
                    .push(
                        widget::button::standard("Apply")
                            .on_press(Message::ApplyProfile(name.clone())),
                    )
                    .push(
                        widget::button::standard("Set Hotkey")
                            .on_press(Message::AddProfileHotkey(name.clone())),
                    )
                    .push(
                        widget::button::destructive("Delete")
                            .on_press(Message::DeleteProfile(name.clone())),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center);
                items = items.push(row);
            }
        }
        let profiles_section = cosmic::widget::settings::section()
            .title("Saved Profiles")
            .add(items);

        let mut content = widget::column::with_capacity(4)
            .push(header)
            .push(description)
            .spacing(space_s)
            .width(Length::Fill);

        {
            let name_input =
                widget::text_input("New profile name", &self.profile_name_input)
                    .on_input(Message::ProfileNameInput)
                    .on_submit(Message::SaveCurrentProfile)
                    .width(Length::Fill);
            let save_button = widget::button::suggested("Save Current Layout")
                .on_press(Message::SaveCurrentProfile(self.profile_name_input.clone()));
            let save_row = widget::row::with_capacity(2)
                .push(name_input)
                .push(save_button)
                .spacing(space_s)
                .align_y(Alignment::Center);
            let save_section = cosmic::widget::settings::section()
                .title("Save Current Layout")
                .add(save_row);
            content = content.push(save_section);
        }

        content = content.push(profiles_section);

        widget::scrollable(
            widget::container(content).width(Length::Fill).max_width(700.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Find the nav position of a page, if present.
    fn nav_position_of(&self, target: Page) -> Option<u16> {
        self.nav.iter().find_map(|id| {
            if self.nav.data::<Page>(id) == Some(&target) {
                self.nav.position(id)
            } else {
                None
            }
        })
    }

    /// Find a hotkey by id, mutably.
    fn hotkey_mut(&mut self, id: &str) -> Option<&mut Hotkey> {
        self.config.hotkeys.hotkeys.iter_mut().find(|h| h.id == id)
    }

    /// Find an action within a hotkey by id + index, mutably.
    fn action_mut(&mut self, id: &str, idx: usize) -> Option<&mut HotkeyActionSpec> {
        self.hotkey_mut(id).and_then(|h| h.actions.get_mut(idx))
    }

    /// Re-register global hotkeys from the current config, recreating the
    /// manager if needed, and refresh the cached action map + status.
    fn refresh_hotkey_registration(&mut self) {
        if let Some(ref mut manager) = self.hotkey_manager {
            manager.update(&self.config);
            self.hotkey_action_map = manager.action_map();
            self.hotkey_status = manager.status();
        } else {
            self.hotkey_manager = HotkeyManager::new(&self.config);
            self.hotkey_action_map = self
                .hotkey_manager
                .as_ref()
                .map(|m| m.action_map())
                .unwrap_or_else(|| Arc::new(HashMap::new()));
            self.hotkey_status = self
                .hotkey_manager
                .as_ref()
                .map(|m| m.status())
                .unwrap_or_default();
        }
    }

    /// Cheaply refresh only the cached hotkey action-chain payloads, without
    /// re-registering hotkeys with the OS. Use for edits that change an
    /// action's fields but not its key binding.
    fn refresh_hotkey_actions(&mut self) {
        if let Some(ref mut manager) = self.hotkey_manager {
            manager.rebuild_action_map(&self.config);
            self.hotkey_action_map = manager.action_map();
        }
    }

    // -----------------------------------------------------------------------
    // Hotkey action handler
    // -----------------------------------------------------------------------

    /// Turn off displays per the configured `TurnOffBehavior`, for the given
    /// resolved monitor id list (used only by the Ddc variant).
    fn dispatch_turn_off(&self, monitors: &[u32]) -> cosmic::app::Task<Message> {
        let behavior = self.config.turn_off_behavior;
        let mut tasks: Vec<cosmic::app::Task<Message>> = Vec::new();

        if behavior.uses_soft() {
            tasks.push(cosmic::app::Task::perform(
                async { tokio::task::spawn_blocking(ccd::turn_off_monitors).await },
                |result| match result {
                    Ok(()) => cosmic::Action::App(Message::MonitorsPoweredOff),
                    Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                },
            ));
        }

        if behavior.uses_ddc() {
            for &monitor_id in monitors {
                tasks.push(cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ddc::set_power_mode(monitor_id, PowerMode::Off)
                        })
                        .await
                    },
                    move |result| match result {
                        Ok(Ok(())) => {
                            cosmic::Action::App(Message::PowerModeApplied(monitor_id, PowerMode::Off))
                        }
                        Ok(Err(e)) => {
                            cosmic::Action::App(Message::Error(format!("Power off error: {e}")))
                        }
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                ));
            }
        }

        cosmic::app::Task::batch(tasks)
    }

    /// Resolve which monitor ids an action applies to.
    fn resolve_monitors(&self, action: &HotkeyActionSpec) -> Vec<u32> {
        if action.all_monitors {
            self.monitors.iter().map(|m| m.info.id).collect()
        } else {
            action.monitors.clone()
        }
    }

    fn handle_hotkey_action(&mut self, actions: Vec<HotkeyActionSpec>) -> cosmic::app::Task<Message> {
        let mut tasks: Vec<cosmic::app::Task<Message>> = Vec::new();

        for action in &actions {
            if action.action_type == ActionType::Off {
                let monitors = self.resolve_monitors(action);
                tasks.push(self.dispatch_turn_off(&monitors));
                continue;
            }

            if action.target == ActionTarget::Profile {
                tasks.push(self.update(Message::ApplyProfile(action.profile_name.clone())));
                continue;
            }

            let monitors = self.resolve_monitors(action);

            match action.target {
                ActionTarget::Brightness => {
                    for monitor_id in monitors {
                        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                            let new_val = match action.action_type {
                                ActionType::Set => (action.value.max(0) as u16).min(m.brightness_max),
                                ActionType::Offset => {
                                    let v = i32::from(m.brightness) + action.value;
                                    (v.max(0) as u16).min(m.brightness_max)
                                }
                                ActionType::Off => unreachable!(),
                            };
                            m.brightness = new_val;
                            tasks.push(self.update(Message::SetBrightness(monitor_id, new_val)));
                        }
                    }
                }
                ActionTarget::Contrast => {
                    for monitor_id in monitors {
                        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                            let new_val = match action.action_type {
                                ActionType::Set => (action.value.max(0) as u16).min(m.contrast_max),
                                ActionType::Offset => {
                                    let v = i32::from(m.contrast) + action.value;
                                    (v.max(0) as u16).min(m.contrast_max)
                                }
                                ActionType::Off => unreachable!(),
                            };
                            m.contrast = new_val;
                            tasks.push(self.update(Message::SetContrast(monitor_id, new_val)));
                        }
                    }
                }
                ActionTarget::InputSource => {
                    // In "all displays" mode every monitor gets the same input;
                    // otherwise each monitor uses its own configured input.
                    let pairs: Vec<(u32, InputSource)> = if action.all_monitors {
                        monitors
                            .iter()
                            .map(|&mid| (mid, action.input_source))
                            .collect()
                    } else {
                        action
                            .monitor_inputs
                            .iter()
                            .map(|mi| (mi.monitor_id, mi.input_source))
                            .collect()
                    };
                    for (monitor_id, input_source) in pairs {
                        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
                            m.input_source = input_source;
                        }
                        tasks.push(cosmic::app::Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    ddc::set_input_source(monitor_id, input_source)
                                })
                                .await
                            },
                            move |result| match result {
                                Ok(Ok(())) => cosmic::Action::App(Message::InputSourceApplied(
                                    monitor_id,
                                    input_source,
                                )),
                                Ok(Err(e)) => cosmic::Action::App(Message::Error(format!(
                                    "Input switch error: {e}"
                                ))),
                                Err(e) => {
                                    cosmic::Action::App(Message::Error(format!("Task join error: {e}")))
                                }
                            },
                        ));
                    }
                }
                ActionTarget::PowerMode => {
                    for monitor_id in monitors {
                        let power_mode = action.power_mode;
                        tasks.push(cosmic::app::Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    ddc::set_power_mode(monitor_id, power_mode)
                                })
                                .await
                            },
                            move |result| match result {
                                Ok(Ok(())) => {
                                    cosmic::Action::App(Message::PowerModeApplied(monitor_id, power_mode))
                                }
                                Ok(Err(e)) => {
                                    cosmic::Action::App(Message::Error(format!("Power mode error: {e}")))
                                }
                                Err(e) => {
                                    cosmic::Action::App(Message::Error(format!("Task join error: {e}")))
                                }
                            },
                        ));
                    }
                }
                ActionTarget::CustomVcp => {
                    for monitor_id in monitors {
                        let code = action.vcp_code;
                        let offset = action.value;
                        let is_offset = action.action_type == ActionType::Offset;
                        tasks.push(cosmic::app::Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || -> anyhow::Result<u16> {
                                    let value = if is_offset {
                                        let (current, max) = ddc::get_vcp(monitor_id, code)?;
                                        let v = i32::from(current) + offset;
                                        v.max(0).min(i32::from(max)) as u16
                                    } else {
                                        offset.max(0) as u16
                                    };
                                    ddc::set_vcp(monitor_id, code, value)?;
                                    Ok(value)
                                })
                                .await
                            },
                            move |result| match result {
                                Ok(Ok(value)) => {
                                    cosmic::Action::App(Message::CustomVcpApplied(monitor_id, code, value))
                                }
                                Ok(Err(e)) => {
                                    cosmic::Action::App(Message::Error(format!("Custom VCP error: {e}")))
                                }
                                Err(e) => {
                                    cosmic::Action::App(Message::Error(format!("Task join error: {e}")))
                                }
                            },
                        ));
                    }
                }
                ActionTarget::Profile => unreachable!(),
            }
        }

        cosmic::app::Task::batch(tasks)
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
