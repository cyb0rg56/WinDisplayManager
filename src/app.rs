mod actions;
mod coordination;
mod debounce;
mod hardware_queue;
mod hotkey_editor;
mod hotkey_views;
mod monitor_views;
mod monitors;
mod profile_handlers;
mod profile_views;
mod settings_views;
mod views;

use self::actions::HardwareJob;
use self::coordination::{HardwareCoordinator, HardwareResult};
use self::debounce::{DebounceToken, SliderDebounce};

use crate::config::{
    ActionTarget, ActionType, AppConfig, ConfigStore, HotkeyActionSpec, MonitorTarget,
    TurnOffBehavior,
};
use crate::ddc::{InputSource, MonitorInfo, MonitorKey, MonitorState, PowerMode};
use crate::hotkeys::{self, HotkeyManager};
use crate::persistence::LoadOutcome;
use crate::tray::{SystemTray, TrayMessage, TrayStream};
use cosmic::iced::event::{self, Event};
use cosmic::iced::keyboard::{Event as KeyboardEvent, Key, Modifiers};
use cosmic::iced::{Length, Subscription, window};
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

    SetBrightness(u32, u16),
    SetContrast(u32, u16),
    SelectInputSource(u32, usize), // monitor_id, index into INPUT_SOURCES
    SetInputSource(u32, InputSource),
    HardwareJobFinished(u64, Result<HardwareResult, String>),
    // Debounced slider changes
    BrightnessSliderChanged(u32, u16),
    ContrastSliderChanged(u32, u16),
    ApplyBrightnessDebounced(u32, DebounceToken),
    ApplyContrastDebounced(u32, DebounceToken),
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
    SetMonitorInput(String, usize, MonitorTarget, Option<InputSource>),
    SetActionPowerMode(String, usize, PowerMode),
    SetActionProfile(String, usize, String),
    ToggleActionAllMonitors(String, usize, bool),
    ToggleActionMonitor(String, usize, MonitorTarget, bool),
    RebindMonitorTarget(String, usize, MonitorTarget, MonitorKey),
    RemoveMonitorTarget(String, usize, MonitorTarget),
    SetTurnOffBehavior(TurnOffBehavior),
    SaveConfig,
    RetryConfig,
    RecoverConfigBackup,
    RequestResetConfig,
    CancelResetConfig,
    ConfirmResetConfig,
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
    config_store: ConfigStore,
    pending_config_reset: bool,
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
    slider_debounce: SliderDebounce,
    profiles: Vec<String>,
    profile_name_input: String,
    pending_profile_delete: Option<String>,
    pending_profile_replace: Option<String>,
    action_executor: HardwareCoordinator,
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
        let (config_store, outcome) = ConfigStore::open(AppConfig::config_path());
        let config = match outcome {
            LoadOutcome::Missing => AppConfig::default(),
            LoadOutcome::Loaded(config) => config,
            LoadOutcome::Failed(error) => {
                log::error!("Configuration requires recovery: {error}");
                AppConfig::recovery_placeholder()
            }
        };

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
            config_store,
            pending_config_reset: false,
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
            slider_debounce: SliderDebounce::default(),
            profiles: Vec::new(),
            profile_name_input: String::new(),
            pending_profile_delete: None,
            pending_profile_replace: None,
            action_executor: HardwareCoordinator::default(),
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
        // The store also guards saves; this gate prevents editing or executing
        // placeholder settings while a failed load awaits the user's decision.
        if self.config_store.recovery_error().is_some()
            && matches!(
                message,
                Message::HotkeyTriggered(_)
                    | Message::ToggleHotkeys(_)
                    | Message::SaveConfig
                    | Message::AddHotkey
                    | Message::DeleteHotkey(_)
                    | Message::StartRecording(_)
                    | Message::ClearBinding(_)
                    | Message::KeyPressed(_, _)
                    | Message::AddAction(_)
                    | Message::DeleteAction(_, _)
                    | Message::SetActionType(_, _, _)
                    | Message::SetActionTarget(_, _, _)
                    | Message::ActionValueDraftChanged(_, _, _)
                    | Message::ActionVcpDraftChanged(_, _, _)
                    | Message::SetActionInputSource(_, _, _)
                    | Message::SetMonitorInput(_, _, _, _)
                    | Message::SetActionPowerMode(_, _, _)
                    | Message::SetActionProfile(_, _, _)
                    | Message::ToggleActionAllMonitors(_, _, _)
                    | Message::ToggleActionMonitor(_, _, _, _)
                    | Message::RebindMonitorTarget(_, _, _, _)
                    | Message::RemoveMonitorTarget(_, _, _)
                    | Message::SetTurnOffBehavior(_)
                    | Message::AddProfileHotkey(_)
            )
        {
            self.status_message =
                "Configuration is read-only until you retry, recover a backup, or confirm a reset."
                    .into();
            return cosmic::app::Task::none();
        }
        match message {
            // -- Monitor detection ------------------------------------------
            Message::RefreshMonitors => return self.refresh_monitors(),
            Message::RetryMonitor(monitor_id) => return self.retry_monitor(monitor_id),

            // -- Brightness -------------------------------------------------
            Message::BrightnessSliderChanged(monitor_id, value) => {
                return self.brightness_slider_changed(monitor_id, value);
            }
            Message::ApplyBrightnessDebounced(monitor_id, token) => {
                return self.apply_brightness_debounced(monitor_id, token);
            }
            Message::SetBrightness(monitor_id, value) => {
                return self.set_brightness(monitor_id, value);
            }

            // -- Contrast ---------------------------------------------------
            Message::ContrastSliderChanged(monitor_id, value) => {
                return self.contrast_slider_changed(monitor_id, value);
            }
            Message::ApplyContrastDebounced(monitor_id, token) => {
                return self.apply_contrast_debounced(monitor_id, token);
            }
            Message::SetContrast(monitor_id, value) => return self.set_contrast(monitor_id, value),

            // -- Input source -----------------------------------------------
            Message::SelectInputSource(monitor_id, idx) => {
                return self.select_input_source(monitor_id, idx);
            }
            Message::SetInputSource(monitor_id, source) => {
                return self.set_input_source(monitor_id, source);
            }

            Message::HardwareJobFinished(id, result) => {
                return self.hardware_job_finished(id, result);
            }
            Message::RebindMonitorTarget(id, idx, old, key) => {
                self.rebind_monitor_target(id, idx, old, key)
            }
            Message::RemoveMonitorTarget(id, idx, target) => {
                self.remove_monitor_target(id, idx, target)
            }

            // -- Hotkey actions ---------------------------------------------
            Message::HotkeyTriggered(id) => return self.handle_hotkey_triggered(id),
            Message::AddHotkey => self.add_hotkey(),
            Message::ToggleHotkeyEditor(id) => self.toggle_hotkey_editor(id),
            Message::DeleteHotkey(id) => self.delete_hotkey(id),
            Message::StartRecording(hotkey_id) => self.start_recording(hotkey_id),
            Message::CancelRecording => self.cancel_recording(),
            Message::ClearBinding(id) => self.clear_binding(id),
            Message::KeyPressed(modifiers, key) => return self.key_pressed(modifiers, key),

            Message::AddAction(id) => self.add_action(id),
            Message::DeleteAction(id, idx) => self.delete_action(id, idx),
            Message::SetActionType(id, idx, action_type) => {
                self.set_action_type(id, idx, action_type)
            }
            Message::SetActionTarget(id, idx, target) => self.set_action_target(id, idx, target),
            Message::ActionValueDraftChanged(id, idx, draft) => {
                self.action_value_draft_changed(id, idx, draft)
            }
            Message::ActionVcpDraftChanged(id, idx, draft) => {
                self.action_vcp_draft_changed(id, idx, draft)
            }
            Message::SetActionInputSource(id, idx, source) => {
                self.set_action_input_source(id, idx, source)
            }
            Message::SetMonitorInput(id, idx, monitor_id, source) => {
                self.set_monitor_input(id, idx, monitor_id, source)
            }
            Message::SetActionPowerMode(id, idx, mode) => self.set_action_power_mode(id, idx, mode),
            Message::SetActionProfile(id, idx, name) => self.set_action_profile(id, idx, name),
            Message::ToggleActionAllMonitors(id, idx, checked) => {
                self.toggle_action_all_monitors(id, idx, checked)
            }
            Message::ToggleActionMonitor(id, idx, monitor_id, checked) => {
                self.toggle_action_monitor(id, idx, monitor_id, checked)
            }

            Message::SetTurnOffBehavior(behavior) => {
                self.config.turn_off_behavior = behavior;
                self.config_dirty = true;
            }

            Message::SaveConfig => return self.save_config(),
            Message::ToggleHotkeys(enabled) => return self.toggle_hotkeys(enabled),
            Message::RetryConfig => self.retry_config(),
            Message::RecoverConfigBackup => self.recover_config_backup(),
            Message::RequestResetConfig => {
                self.pending_config_reset = self.config_store.recovery_error().is_some();
            }
            Message::CancelResetConfig => self.pending_config_reset = false,
            Message::ConfirmResetConfig => self.confirm_reset_config(),

            // -- Profiles ---------------------------------------------------
            Message::RefreshProfiles => return self.refresh_profiles(),
            Message::ProfilesListed(profiles) => self.profiles_listed(profiles),
            Message::ProfileNameInput(value) => self.profile_name_input(value),
            Message::SaveCurrentProfile(name) => return self.save_current_profile(name),
            Message::ConfirmReplaceProfile => return self.confirm_replace_profile(),
            Message::CancelReplaceProfile => self.cancel_replace_profile(),
            Message::ApplyProfile(name) => return self.apply_profile(name),
            Message::RequestDeleteProfile(name) => self.request_delete_profile(name),
            Message::CancelDeleteProfile => self.cancel_delete_profile(),
            Message::DeleteProfile(name) => return self.delete_profile(name),
            Message::AddProfileHotkey(profile_name) => self.add_profile_hotkey(profile_name),
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
                        return self.enqueue_hardware_jobs([HardwareJob::SoftTurnOff {
                            monitor_ids: self.detected_monitors.iter().map(|m| m.id).collect(),
                        }]);
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

        let mut layout = widget::column::with_capacity(4);
        let recovering = self.config_store.recovery_error().is_some();
        if recovering {
            layout = layout.push(self.view_config_recovery());
        }
        if !recovering || !matches!(page, Page::Hotkeys | Page::Settings) {
            layout = layout.push(content);
        }
        let layout = layout
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

    fn handle_hotkey_actions(
        &mut self,
        actions: Vec<HotkeyActionSpec>,
    ) -> cosmic::app::Task<Message> {
        if let Err(error) = self
            .action_executor
            .enqueue_actions(actions, self.config.turn_off_behavior)
        {
            self.status_message = format!("Hotkey not dispatched: {error}");
            return cosmic::app::Task::none();
        }
        self.start_next_hardware_job()
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
