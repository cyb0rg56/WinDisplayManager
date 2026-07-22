use crate::config::{
    AppConfig, HotkeyAction, HotkeyBinding, BrightnessBinding, ContrastBinding,
    InputSwitchBinding, PowerModeBinding, ProfileBinding, StepDirection,
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
    // Debounced slider changes
    BrightnessSliderChanged(u32, u16),
    ContrastSliderChanged(u32, u16),
    ApplyBrightnessDebounced(u32, u16),
    ApplyContrastDebounced(u32, u16),
    // Hotkeys
    HotkeyTriggered(HotkeyAction),
    ToggleHotkeys(bool),
    // Settings page - New binding workflow
    StartRecordingBrightness(u32, StepDirection),
    StartRecordingContrast(u32, StepDirection),
    StartRecordingInputSwitch(u32, InputSource),
    StartRecordingPowerMode(u32, PowerMode),
    CancelRecording,
    KeyPressed(Modifiers, Key),
    // Hotkey management
    RemoveHotkeyBinding(usize, BindingCategory),
    SelectAddBindingType(usize),
    SaveConfig,
    // Profiles
    RefreshProfiles,
    ProfilesListed(Vec<String>),
    ProfileNameInput(String),
    SaveCurrentProfile(String),
    ApplyProfile(String),
    DeleteProfile(String),
    ProfileApplied(String),
    StartRecordingProfile(String),
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

#[derive(Clone, Debug)]
pub enum BindingCategory {
    InputSwitch,
    Brightness,
    Contrast,
    PowerMode,
    Profile,
}

// ---------------------------------------------------------------------------
// Hotkey recording state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum RecordingState {
    NotRecording,
    RecordingBrightness {
        monitor_id: u32,
        direction: StepDirection,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        key: String,
    },
    RecordingContrast {
        monitor_id: u32,
        direction: StepDirection,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        key: String,
    },
    RecordingInputSwitch {
        monitor_id: u32,
        input_source: InputSource,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        key: String,
    },
    RecordingPowerMode {
        monitor_id: u32,
        power_mode: PowerMode,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        key: String,
    },
    RecordingProfile {
        profile_name: String,
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
    hotkey_action_map: Arc<HashMap<u32, HotkeyAction>>,
    status_message: String,
    recording_state: RecordingState,
    about: widget::about::About,
    // Debounce state for sliders
    pending_brightness: Option<(u32, u16)>,
    pending_contrast: Option<(u32, u16)>,
    // Selected action type in the "add hotkey" dropdown (0=brightness, 1=contrast, 2=input)
    add_binding_type: usize,
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
            status_message: "Starting...".into(),
            recording_state: RecordingState::NotRecording,
            about,
            pending_brightness: None,
            pending_contrast: None,
            add_binding_type: 0,
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

            // -- Hotkey actions ---------------------------------------------
            Message::HotkeyTriggered(action) => {
                return self.handle_hotkey_action(action);
            }

            // -- Settings: hotkey recording workflow ------------------------
            Message::StartRecordingBrightness(monitor_id, direction) => {
                self.recording_state = RecordingState::RecordingBrightness {
                    monitor_id,
                    direction,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message = "Recording hotkey... Press modifiers and key".into();
            }

            Message::StartRecordingContrast(monitor_id, direction) => {
                self.recording_state = RecordingState::RecordingContrast {
                    monitor_id,
                    direction,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message = "Recording hotkey... Press modifiers and key".into();
            }

            Message::StartRecordingInputSwitch(monitor_id, input_source) => {
                self.recording_state = RecordingState::RecordingInputSwitch {
                    monitor_id,
                    input_source,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message = "Recording hotkey... Press modifiers and key".into();
            }

            Message::StartRecordingPowerMode(monitor_id, power_mode) => {
                self.recording_state = RecordingState::RecordingPowerMode {
                    monitor_id,
                    power_mode,
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

                // Auto-save the hotkey
                match &self.recording_state {
                    RecordingState::RecordingBrightness { monitor_id, direction, .. } => {
                        self.config.hotkeys.brightness_bindings.push(BrightnessBinding {
                            monitor_id: *monitor_id,
                            direction: *direction,
                            hotkey: HotkeyBinding {
                                ctrl,
                                alt,
                                shift,
                                win,
                                key: key_string.clone(),
                            },
                        });
                        self.status_message = format!("Brightness hotkey added: {}. Remember to save configuration.", 
                            format_hotkey(ctrl, alt, shift, win, &key_string));
                        self.recording_state = RecordingState::NotRecording;
                        
                        // Re-register hotkeys immediately
                        if let Some(ref mut manager) = self.hotkey_manager {
                            manager.update(&self.config);
                            self.hotkey_action_map = manager.action_map();
                        } else {
                            self.hotkey_manager = HotkeyManager::new(&self.config);
                            self.hotkey_action_map = self
                                .hotkey_manager
                                .as_ref()
                                .map(|m| m.action_map())
                                .unwrap_or_else(|| Arc::new(HashMap::new()));
                        }
                    }
                    RecordingState::RecordingContrast { monitor_id, direction, .. } => {
                        self.config.hotkeys.contrast_bindings.push(ContrastBinding {
                            monitor_id: *monitor_id,
                            direction: *direction,
                            hotkey: HotkeyBinding {
                                ctrl,
                                alt,
                                shift,
                                win,
                                key: key_string.clone(),
                            },
                        });
                        self.status_message = format!("Contrast hotkey added: {}. Remember to save configuration.", 
                            format_hotkey(ctrl, alt, shift, win, &key_string));
                        self.recording_state = RecordingState::NotRecording;
                        
                        // Re-register hotkeys immediately
                        if let Some(ref mut manager) = self.hotkey_manager {
                            manager.update(&self.config);
                            self.hotkey_action_map = manager.action_map();
                        } else {
                            self.hotkey_manager = HotkeyManager::new(&self.config);
                            self.hotkey_action_map = self
                                .hotkey_manager
                                .as_ref()
                                .map(|m| m.action_map())
                                .unwrap_or_else(|| Arc::new(HashMap::new()));
                        }
                    }
                    RecordingState::RecordingInputSwitch { monitor_id, input_source, .. } => {
                        self.config.hotkeys.input_switch_bindings.push(InputSwitchBinding {
                            monitor_id: *monitor_id,
                            input_source: *input_source,
                            hotkey: HotkeyBinding {
                                ctrl,
                                alt,
                                shift,
                                win,
                                key: key_string.clone(),
                            },
                        });
                        self.status_message = format!("Input switch hotkey added: {}. Remember to save configuration.", 
                            format_hotkey(ctrl, alt, shift, win, &key_string));
                        self.recording_state = RecordingState::NotRecording;
                        
                        // Re-register hotkeys immediately
                        if let Some(ref mut manager) = self.hotkey_manager {
                            manager.update(&self.config);
                            self.hotkey_action_map = manager.action_map();
                        } else {
                            self.hotkey_manager = HotkeyManager::new(&self.config);
                            self.hotkey_action_map = self
                                .hotkey_manager
                                .as_ref()
                                .map(|m| m.action_map())
                                .unwrap_or_else(|| Arc::new(HashMap::new()));
                        }
                    }
                    RecordingState::RecordingPowerMode { monitor_id, power_mode, .. } => {
                        self.config.hotkeys.power_mode_bindings.push(PowerModeBinding {
                            monitor_id: *monitor_id,
                            power_mode: *power_mode,
                            hotkey: HotkeyBinding {
                                ctrl,
                                alt,
                                shift,
                                win,
                                key: key_string.clone(),
                            },
                        });
                        self.status_message = format!("Power mode hotkey added: {}. Remember to save configuration.",
                            format_hotkey(ctrl, alt, shift, win, &key_string));
                        self.recording_state = RecordingState::NotRecording;

                        // Re-register hotkeys immediately
                        if let Some(ref mut manager) = self.hotkey_manager {
                            manager.update(&self.config);
                            self.hotkey_action_map = manager.action_map();
                        } else {
                            self.hotkey_manager = HotkeyManager::new(&self.config);
                            self.hotkey_action_map = self
                                .hotkey_manager
                                .as_ref()
                                .map(|m| m.action_map())
                                .unwrap_or_else(|| Arc::new(HashMap::new()));
                        }
                    }
                    RecordingState::RecordingProfile { profile_name, .. } => {
                        self.config.hotkeys.profile_bindings.push(ProfileBinding {
                            profile_name: profile_name.clone(),
                            hotkey: HotkeyBinding {
                                ctrl,
                                alt,
                                shift,
                                win,
                                key: key_string.clone(),
                            },
                        });
                        self.status_message = format!("Profile hotkey added: {}. Remember to save configuration.",
                            format_hotkey(ctrl, alt, shift, win, &key_string));
                        self.recording_state = RecordingState::NotRecording;
                        self.refresh_hotkey_registration();
                    }
                    RecordingState::NotRecording => {}
                }
            }

            Message::SelectAddBindingType(idx) => {
                self.add_binding_type = idx;
            }

            Message::RemoveHotkeyBinding(idx, category) => {
                match category {
                    BindingCategory::InputSwitch => {
                        if idx < self.config.hotkeys.input_switch_bindings.len() {
                            self.config.hotkeys.input_switch_bindings.remove(idx);
                        }
                    }
                    BindingCategory::Brightness => {
                        if idx < self.config.hotkeys.brightness_bindings.len() {
                            self.config.hotkeys.brightness_bindings.remove(idx);
                        }
                    }
                    BindingCategory::Contrast => {
                        if idx < self.config.hotkeys.contrast_bindings.len() {
                            self.config.hotkeys.contrast_bindings.remove(idx);
                        }
                    }
                    BindingCategory::PowerMode => {
                        if idx < self.config.hotkeys.power_mode_bindings.len() {
                            self.config.hotkeys.power_mode_bindings.remove(idx);
                        }
                    }
                    BindingCategory::Profile => {
                        if idx < self.config.hotkeys.profile_bindings.len() {
                            self.config.hotkeys.profile_bindings.remove(idx);
                        }
                    }
                }
                
                // Re-register hotkeys after removal
                if let Some(ref mut manager) = self.hotkey_manager {
                    manager.update(&self.config);
                    self.hotkey_action_map = manager.action_map();
                } else {
                    self.hotkey_manager = HotkeyManager::new(&self.config);
                    self.hotkey_action_map = self
                        .hotkey_manager
                        .as_ref()
                        .map(|m| m.action_map())
                        .unwrap_or_else(|| Arc::new(HashMap::new()));
                }
            }

            Message::SaveConfig => {
                // Save and re-register hotkeys
                if let Err(e) = self.config.save() {
                    self.status_message = format!("Failed to save config: {e}");
                } else {
                    self.status_message = "Configuration saved and hotkeys activated.".into();
                    // Re-register hotkeys
                    if let Some(ref mut manager) = self.hotkey_manager {
                        manager.update(&self.config);
                        self.hotkey_action_map = manager.action_map();
                    } else {
                        self.hotkey_manager = HotkeyManager::new(&self.config);
                        self.hotkey_action_map = self
                            .hotkey_manager
                            .as_ref()
                            .map(|m| m.action_map())
                            .unwrap_or_else(|| Arc::new(HashMap::new()));
                    }
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

            Message::StartRecordingProfile(profile_name) => {
                self.recording_state = RecordingState::RecordingProfile {
                    profile_name,
                    ctrl: false,
                    alt: false,
                    shift: false,
                    win: false,
                    key: String::new(),
                };
                self.status_message = "Recording hotkey... Press modifiers and key".into();
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
            "Configure global hotkeys for monitor control. \
             Changes take effect immediately after recording.",
        );

        // --- Brightness bindings ---
        let mut brightness_items = widget::column::with_capacity(
            self.config.hotkeys.brightness_bindings.len() + 1,
        )
        .spacing(4);

        for (i, binding) in self.config.hotkeys.brightness_bindings.iter().enumerate() {
            let dir = match binding.direction {
                StepDirection::Up => "Up",
                StepDirection::Down => "Down",
            };
            let label = format!(
                "Monitor {} Brightness {} : {}",
                binding.monitor_id, dir, binding.hotkey
            );
            brightness_items = brightness_items.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body(label).width(Length::Fill))
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemoveHotkeyBinding(
                                i,
                                BindingCategory::Brightness,
                            )),
                    )
                    .align_y(Alignment::Center)
                    .spacing(space_s),
            );
        }

        let brightness_section = cosmic::widget::settings::section()
            .title("Brightness Hotkeys")
            .add(brightness_items);

        // --- Contrast bindings ---
        let mut contrast_items = widget::column::with_capacity(
            self.config.hotkeys.contrast_bindings.len() + 1,
        )
        .spacing(4);

        for (i, binding) in self.config.hotkeys.contrast_bindings.iter().enumerate() {
            let dir = match binding.direction {
                StepDirection::Up => "Up",
                StepDirection::Down => "Down",
            };
            let label = format!(
                "Monitor {} Contrast {} : {}",
                binding.monitor_id, dir, binding.hotkey
            );
            contrast_items = contrast_items.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body(label).width(Length::Fill))
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemoveHotkeyBinding(
                                i,
                                BindingCategory::Contrast,
                            )),
                    )
                    .align_y(Alignment::Center)
                    .spacing(space_s),
            );
        }

        let contrast_section = cosmic::widget::settings::section()
            .title("Contrast Hotkeys")
            .add(contrast_items);

        // --- Input switch bindings ---
        let mut input_items = widget::column::with_capacity(
            self.config.hotkeys.input_switch_bindings.len() + 1,
        )
        .spacing(4);

        for (i, binding) in self.config.hotkeys.input_switch_bindings.iter().enumerate()
        {
            let label = format!(
                "Monitor {} -> {} : {}",
                binding.monitor_id, binding.input_source, binding.hotkey
            );
            input_items = input_items.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body(label).width(Length::Fill))
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemoveHotkeyBinding(
                                i,
                                BindingCategory::InputSwitch,
                            )),
                    )
                    .align_y(Alignment::Center)
                    .spacing(space_s),
            );
        }

        let input_section = cosmic::widget::settings::section()
            .title("Input Switch Hotkeys")
            .add(input_items);

        // --- Power mode bindings ---
        let mut power_items = widget::column::with_capacity(
            self.config.hotkeys.power_mode_bindings.len() + 1,
        )
        .spacing(4);

        for (i, binding) in self.config.hotkeys.power_mode_bindings.iter().enumerate() {
            let label = format!(
                "Monitor {} Power -> {} : {}",
                binding.monitor_id, binding.power_mode, binding.hotkey
            );
            power_items = power_items.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body(label).width(Length::Fill))
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemoveHotkeyBinding(
                                i,
                                BindingCategory::PowerMode,
                            )),
                    )
                    .align_y(Alignment::Center)
                    .spacing(space_s),
            );
        }

        let power_section = cosmic::widget::settings::section()
            .title("Power Mode Hotkeys")
            .add(power_items);

        // --- Profile switch bindings ---
        let mut profile_items = widget::column::with_capacity(
            self.config.hotkeys.profile_bindings.len() + 1,
        )
        .spacing(4);

        for (i, binding) in self.config.hotkeys.profile_bindings.iter().enumerate() {
            let label = format!("Apply '{}' : {}", binding.profile_name, binding.hotkey);
            profile_items = profile_items.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body(label).width(Length::Fill))
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemoveHotkeyBinding(i, BindingCategory::Profile)),
                    )
                    .align_y(Alignment::Center)
                    .spacing(space_s),
            );
        }

        let profile_section = cosmic::widget::settings::section()
            .title("Profile Switch Hotkeys")
            .add(profile_items);

        // --- Hotkey recording UI or Quick-add buttons ---
        let add_section = match &self.recording_state {
            RecordingState::NotRecording => {
                // Dropdown to choose which kind of hotkey to add.
                static ADD_TYPE_LABELS: &[&str] =
                    &["Brightness", "Contrast", "Input Switching", "Power Mode"];
                let type_row = widget::row::with_capacity(2)
                    .push(widget::text::body("Binding type").width(Length::FillPortion(2)))
                    .push(widget::dropdown(
                        ADD_TYPE_LABELS,
                        Some(self.add_binding_type),
                        Message::SelectAddBindingType,
                    ))
                    .spacing(space_s)
                    .align_y(Alignment::Center);

                // Per-monitor controls for the selected binding type.
                let mut add_buttons =
                    widget::column::with_capacity(self.monitors.len() + 1).spacing(space_s);
                add_buttons = add_buttons.push(type_row);

                for mon in &self.monitors {
                    let mid = mon.info.id;
                    let monitor_label = if mon.info.name.is_empty() {
                        format!("Monitor {}", mid)
                    } else {
                        mon.info.name.clone()
                    };

                    let controls = match self.add_binding_type {
                        0 => widget::row::with_capacity(2)
                            .push(widget::button::standard("+ Up").on_press(
                                Message::StartRecordingBrightness(mid, StepDirection::Up),
                            ))
                            .push(widget::button::standard("+ Down").on_press(
                                Message::StartRecordingBrightness(mid, StepDirection::Down),
                            ))
                            .spacing(space_s)
                            .align_y(Alignment::Center),
                        1 => widget::row::with_capacity(2)
                            .push(widget::button::standard("+ Up").on_press(
                                Message::StartRecordingContrast(mid, StepDirection::Up),
                            ))
                            .push(widget::button::standard("+ Down").on_press(
                                Message::StartRecordingContrast(mid, StepDirection::Down),
                            ))
                            .spacing(space_s)
                            .align_y(Alignment::Center),
                        2 => widget::row::with_capacity(5)
                            .push(widget::button::standard("+ HDMI1").on_press(
                                Message::StartRecordingInputSwitch(mid, InputSource::Hdmi1),
                            ))
                            .push(widget::button::standard("+ HDMI2").on_press(
                                Message::StartRecordingInputSwitch(mid, InputSource::Hdmi2),
                            ))
                            .push(widget::button::standard("+ DP1").on_press(
                                Message::StartRecordingInputSwitch(mid, InputSource::Dp1),
                            ))
                            .push(widget::button::standard("+ DP2").on_press(
                                Message::StartRecordingInputSwitch(mid, InputSource::Dp2),
                            ))
                            .push(widget::button::standard("+ USB-C1").on_press(
                                Message::StartRecordingInputSwitch(mid, InputSource::UsbC1),
                            ))
                            .spacing(space_s)
                            .align_y(Alignment::Center),
                        _ => widget::row::with_capacity(4)
                            .push(widget::button::standard("+ On").on_press(
                                Message::StartRecordingPowerMode(mid, PowerMode::On),
                            ))
                            .push(widget::button::standard("+ Standby").on_press(
                                Message::StartRecordingPowerMode(mid, PowerMode::Standby),
                            ))
                            .push(widget::button::standard("+ Suspend").on_press(
                                Message::StartRecordingPowerMode(mid, PowerMode::Suspend),
                            ))
                            .push(widget::button::standard("+ Off").on_press(
                                Message::StartRecordingPowerMode(mid, PowerMode::Off),
                            ))
                            .spacing(space_s)
                            .align_y(Alignment::Center),
                    };

                    let row = widget::row::with_capacity(2)
                        .push(widget::text::body(monitor_label).width(Length::FillPortion(2)))
                        .push(controls)
                        .spacing(space_s)
                        .align_y(Alignment::Center);
                    add_buttons = add_buttons.push(row);
                }

                cosmic::widget::settings::section()
                    .title("Add Hotkey Binding")
                    .add(add_buttons)
            }
            recording_state => {
                // Show recording panel
                self.view_recording_panel(recording_state, space_s)
            }
        };

        // --- Save button ---
        let save_row = widget::row::with_capacity(1).push(
            widget::button::suggested("Save Configuration")
                .on_press(Message::SaveConfig),
        );

        let content = widget::column::with_capacity(9)
            .push(header)
            .push(description)
            .push(brightness_section)
            .push(contrast_section)
            .push(input_section)
            .push(power_section)
            .push(profile_section)
            .push(add_section)
            .push(save_row)
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
                            .on_press(Message::StartRecordingProfile(name.clone())),
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

        if let RecordingState::RecordingProfile { .. } = &self.recording_state {
            content = content.push(self.view_recording_panel(&self.recording_state, space_s));
        } else {
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

    /// Re-register global hotkeys from the current config, recreating the
    /// manager if needed, and refresh the cached action map.
    fn refresh_hotkey_registration(&mut self) {
        if let Some(ref mut manager) = self.hotkey_manager {
            manager.update(&self.config);
            self.hotkey_action_map = manager.action_map();
        } else {
            self.hotkey_manager = HotkeyManager::new(&self.config);
            self.hotkey_action_map = self
                .hotkey_manager
                .as_ref()
                .map(|m| m.action_map())
                .unwrap_or_else(|| Arc::new(HashMap::new()));
        }
    }

    /// View for the hotkey recording panel.
    fn view_recording_panel(&self, recording_state: &RecordingState, space_s: u16) -> cosmic::widget::settings::Section<'_, Message> {
        let (action_desc, _monitor_id, ctrl, alt, shift, win, key) = match recording_state {
            RecordingState::RecordingBrightness { monitor_id, direction, ctrl, alt, shift, win, key } => {
                let dir = match direction {
                    StepDirection::Up => "Up",
                    StepDirection::Down => "Down",
                };
                (format!("Monitor {} Brightness {}", monitor_id, dir), *monitor_id, *ctrl, *alt, *shift, *win, key.clone())
            }
            RecordingState::RecordingContrast { monitor_id, direction, ctrl, alt, shift, win, key } => {
                let dir = match direction {
                    StepDirection::Up => "Up",
                    StepDirection::Down => "Down",
                };
                (format!("Monitor {} Contrast {}", monitor_id, dir), *monitor_id, *ctrl, *alt, *shift, *win, key.clone())
            }
            RecordingState::RecordingInputSwitch { monitor_id, input_source, ctrl, alt, shift, win, key } => {
                (format!("Monitor {} Switch to {}", monitor_id, input_source), *monitor_id, *ctrl, *alt, *shift, *win, key.clone())
            }
            RecordingState::RecordingPowerMode { monitor_id, power_mode, ctrl, alt, shift, win, key } => {
                (format!("Monitor {} Power {}", monitor_id, power_mode), *monitor_id, *ctrl, *alt, *shift, *win, key.clone())
            }
            RecordingState::RecordingProfile { profile_name, ctrl, alt, shift, win, key } => {
                (format!("Apply profile '{}'", profile_name), 0, *ctrl, *alt, *shift, *win, key.clone())
            }
            RecordingState::NotRecording => {
                return cosmic::widget::settings::section()
                    .title("Recording")
                    .add(widget::text::body("Not recording"));
            }
        };

        let title = widget::text::title4(format!("Recording: {}", action_desc));
        
        // Build current display
        let current_display = format_hotkey(ctrl, alt, shift, win, &key);
        
        // Instruction text
        let instruction = widget::text::title4("Press your desired key combination now...");
        
        let current_text = if !key.is_empty() {
            widget::text::title3(format!("Captured: {}", current_display))
        } else {
            widget::text::body("Waiting for key press...")
        };

        // Help text
        let help_text = widget::text::caption(
            "Press any combination like: Ctrl+Alt+F1, Ctrl+Shift+A, Win+ArrowUp, etc.\nThe hotkey will be saved automatically when you press it."
        );

        // Action buttons
        let buttons_row = widget::row::with_capacity(1)
            .push(widget::button::standard("Cancel").on_press(Message::CancelRecording))
            .spacing(space_s);

        let content = widget::column::with_capacity(6)
            .push(title)
            .push(instruction)
            .push(current_text)
            .push(help_text)
            .push(buttons_row)
            .spacing(space_s * 2);

        cosmic::widget::settings::section()
            .title("Recording Hotkey")
            .add(content)
    }

    // -----------------------------------------------------------------------
    // Hotkey action handler
    // -----------------------------------------------------------------------

    fn handle_hotkey_action(
        &mut self,
        action: HotkeyAction,
    ) -> cosmic::app::Task<Message> {
        match action {
            HotkeyAction::SwitchInput {
                monitor_id,
                input_source,
            } => {
                if let Some(m) =
                    self.monitors.iter_mut().find(|m| m.info.id == monitor_id)
                {
                    m.input_source = input_source;
                }
                cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ddc::set_input_source(monitor_id, input_source)
                        })
                        .await
                    },
                    move |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::InputSourceApplied(monitor_id, input_source)),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Input switch error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                )
            }

            HotkeyAction::BrightnessUp { monitor_id } => {
                let step = self.config.hotkeys.brightness_step;
                if let Some(m) =
                    self.monitors.iter_mut().find(|m| m.info.id == monitor_id)
                {
                    let new_val = (m.brightness + step).min(m.brightness_max);
                    m.brightness = new_val;
                    return self.update(Message::SetBrightness(monitor_id, new_val));
                }
                cosmic::app::Task::none()
            }

            HotkeyAction::BrightnessDown { monitor_id } => {
                let step = self.config.hotkeys.brightness_step;
                if let Some(m) =
                    self.monitors.iter_mut().find(|m| m.info.id == monitor_id)
                {
                    let new_val = m.brightness.saturating_sub(step);
                    m.brightness = new_val;
                    return self.update(Message::SetBrightness(monitor_id, new_val));
                }
                cosmic::app::Task::none()
            }

            HotkeyAction::ContrastUp { monitor_id } => {
                let step = self.config.hotkeys.contrast_step;
                if let Some(m) =
                    self.monitors.iter_mut().find(|m| m.info.id == monitor_id)
                {
                    let new_val = (m.contrast + step).min(m.contrast_max);
                    m.contrast = new_val;
                    return self.update(Message::SetContrast(monitor_id, new_val));
                }
                cosmic::app::Task::none()
            }

            HotkeyAction::ContrastDown { monitor_id } => {
                let step = self.config.hotkeys.contrast_step;
                if let Some(m) =
                    self.monitors.iter_mut().find(|m| m.info.id == monitor_id)
                {
                    let new_val = m.contrast.saturating_sub(step);
                    m.contrast = new_val;
                    return self.update(Message::SetContrast(monitor_id, new_val));
                }
                cosmic::app::Task::none()
            }

            HotkeyAction::SetPowerMode { monitor_id, power_mode } => {
                cosmic::app::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ddc::set_power_mode(monitor_id, power_mode)
                        })
                        .await
                    },
                    move |result| match result {
                        Ok(Ok(())) => cosmic::Action::App(Message::PowerModeApplied(monitor_id, power_mode)),
                        Ok(Err(e)) => cosmic::Action::App(Message::Error(format!("Power mode error: {e}"))),
                        Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
                    },
                )
            }

            HotkeyAction::ApplyProfile { profile_name } => {
                self.update(Message::ApplyProfile(profile_name))
            }
        }
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
