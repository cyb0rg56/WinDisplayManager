//! UI-agnostic application state. The UI feeds [`Input`]s to
//! [`AppState::update`] and carries out the returned [`Effect`]s.

mod config_ops;
mod hardware_queue;
mod hotkey_editor;
mod monitors;
mod profiles;

pub use config_ops::{ConfigOp, ConfigOpDone};
pub use hotkey_editor::{
    action_master_selected, action_monitor_selected, parse_value_draft, parse_vcp_draft,
};

use crate::actions::HardwareJob;
use crate::config::{
    ActionTarget, ActionType, AppConfig, ConfigStore, HotkeyActionSpec, HotkeyBinding,
    MonitorTarget, TurnOffBehavior,
};
use crate::coordination::{HardwareCoordinator, HardwareResult, WindowsHardware, Work};
use crate::ddc::{InputSource, MonitorInfo, MonitorKey, MonitorState, PowerMode};
use crate::debounce::{DebounceToken, SliderDebounce, SliderFeature};
use crate::hotkeys::HotkeyManager;
use crate::persistence::LoadOutcome;
use crate::presentation::MonitorPending;
use crate::startup;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

const SLIDER_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Clone, Debug)]
pub enum Input {
    // Monitor controls
    RefreshMonitors,
    RetryMonitor(u32),
    SelectInputSource(u32, InputSource),
    HardwareJobFinished(u64, Result<HardwareResult, String>),
    // Debounced slider changes
    BrightnessSliderChanged(u32, u16),
    ContrastSliderChanged(u32, u16),
    ApplyBrightnessDebounced(u32, DebounceToken),
    ApplyContrastDebounced(u32, DebounceToken),
    TurnOffMonitors,
    // Hotkeys
    HotkeyTriggered(u32),
    ToggleHotkeys(bool),
    AddHotkey,
    SetHotkeyLabel(String, String),
    ToggleHotkeyEditor(String),
    RequestDeleteHotkey(String),
    CancelDeleteHotkey,
    DeleteHotkey(String),
    StartRecording(String),
    CancelRecording,
    ClearBinding(String),
    /// A key chord captured while recording; empty `key` means unsupported.
    KeyRecorded(HotkeyBinding),
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
    // Settings and configuration
    SetTurnOffBehavior(TurnOffBehavior),
    ToggleStartWithWindows(bool),
    ToggleStartMinimized(bool),
    SaveConfig,
    RetryConfig,
    RecoverConfigBackup,
    RequestResetConfig,
    CancelResetConfig,
    ConfirmResetConfig,
    ConfigOpFinished(ConfigOp, ConfigOpDone),
    // Profiles
    RefreshProfiles,
    ProfilesListed(u64, Result<Vec<String>, String>),
    ProfileNameInput(String),
    SaveCurrentProfile(String),
    ConfirmReplaceProfile,
    CancelReplaceProfile,
    ApplyProfile(String),
    RequestDeleteProfile(String),
    CancelDeleteProfile,
    DeleteProfile(String),
    AddProfileHotkey(String),
    /// The user copied the configuration path.
    ConfigPathCopied,
    /// Opening a link failed.
    OpenUrlFailed(String),
}

impl Input {
    /// Inputs that edit or act on configuration, which is read-only during recovery.
    fn needs_writable_config(&self) -> bool {
        matches!(
            self,
            Input::HotkeyTriggered(_)
                | Input::ToggleHotkeys(_)
                | Input::SaveConfig
                | Input::AddHotkey
                | Input::SetHotkeyLabel(_, _)
                | Input::RequestDeleteHotkey(_)
                | Input::DeleteHotkey(_)
                | Input::StartRecording(_)
                | Input::ClearBinding(_)
                | Input::KeyRecorded(_)
                | Input::AddAction(_)
                | Input::DeleteAction(_, _)
                | Input::SetActionType(_, _, _)
                | Input::SetActionTarget(_, _, _)
                | Input::ActionValueDraftChanged(_, _, _)
                | Input::ActionVcpDraftChanged(_, _, _)
                | Input::SetActionInputSource(_, _, _)
                | Input::SetMonitorInput(_, _, _, _)
                | Input::SetActionPowerMode(_, _, _)
                | Input::SetActionProfile(_, _, _)
                | Input::ToggleActionAllMonitors(_, _, _)
                | Input::ToggleActionMonitor(_, _, _, _)
                | Input::RebindMonitorTarget(_, _, _, _)
                | Input::RemoveMonitorTarget(_, _, _)
                | Input::SetTurnOffBehavior(_)
                | Input::ToggleStartWithWindows(_)
                | Input::ToggleStartMinimized(_)
                | Input::AddProfileHotkey(_)
        )
    }
}

/// Side effects the UI must carry out after an update.
pub enum Effect {
    /// Run blocking work off the UI thread, then feed back its completion input.
    Spawn(Job),
    /// Feed back the input after the delay.
    Delay(Duration, Input),
    /// Detected monitors changed; rebuild navigation.
    MonitorsDetected,
    /// The profile list changed; refresh the tray menu.
    ProfilesUpdated,
    /// Navigate to the hotkeys page.
    ShowHotkeys,
}

/// Blocking work produced by the state; opaque to the UI.
pub struct Job(JobKind);

enum JobKind {
    Hardware(Work),
    Config(ConfigOp, ConfigStore, AppConfig),
    ListProfiles(u64),
}

impl Job {
    /// Runs the work on the current thread. Call from a blocking-safe context.
    pub fn run(self) -> Input {
        match self.0 {
            JobKind::Hardware(work) => {
                let id = work.id;
                Input::HardwareJobFinished(id, work.execute(&mut WindowsHardware))
            }
            JobKind::Config(op, store, config) => {
                let done = config_ops::run(&op, store, config);
                Input::ConfigOpFinished(op, done)
            }
            JobKind::ListProfiles(request) => Input::ProfilesListed(
                request,
                crate::profiles::list_profiles()
                    .map_err(|error| format!("List profiles error: {error}")),
            ),
        }
    }

    /// Completion input reporting that the worker running this job failed.
    pub fn on_failure(&self) -> impl FnOnce(String) -> Input + Send + 'static {
        let failed: Box<dyn FnOnce(String) -> Input + Send> = match &self.0 {
            JobKind::Hardware(work) => {
                let id = work.id;
                Box::new(move |error| {
                    Input::HardwareJobFinished(id, Err(format!("Task join error: {error}")))
                })
            }
            JobKind::Config(op, _, _) => {
                let op = op.clone();
                Box::new(move |error| Input::ConfigOpFinished(op, ConfigOpDone::join_failed(error)))
            }
            JobKind::ListProfiles(request) => {
                let request = *request;
                Box::new(move |error| {
                    Input::ProfilesListed(request, Err(format!("Task join error: {error}")))
                })
            }
        };
        failed
    }
}

#[derive(Clone, Debug)]
enum RecordingState {
    NotRecording,
    Recording { hotkey_id: String },
}

pub struct AppState {
    monitor_generation: u64,
    detected_monitors: Vec<MonitorInfo>,
    monitors: Vec<MonitorState>,
    monitor_load_errors: HashMap<u32, String>,
    config: AppConfig,
    config_store: ConfigStore,
    config_ops: VecDeque<ConfigOp>,
    config_op_active: bool,
    pending_config_reset: bool,
    hotkey_manager: Option<HotkeyManager>,
    hotkey_action_map: Arc<HashMap<u32, Vec<HotkeyActionSpec>>>,
    hotkey_status: HashMap<String, bool>,
    hotkey_generation: u64,
    status_message: String,
    recording_state: RecordingState,
    expanded_hotkey: Option<String>,
    pending_hotkey_delete: Option<String>,
    value_drafts: HashMap<(String, usize), String>,
    vcp_drafts: HashMap<(String, usize), String>,
    config_dirty: bool,
    config_path: String,
    slider_debounce: SliderDebounce,
    profiles: Vec<String>,
    profiles_request: u64,
    profile_name_input: String,
    pending_profile_delete: Option<String>,
    pending_profile_replace: Option<String>,
    action_executor: HardwareCoordinator,
    refresh_profiles_after_jobs: bool,
    effects: Vec<Effect>,
}

impl AppState {
    /// Loads configuration and registers hotkeys. Call on the UI thread, which
    /// owns the hotkey message window.
    pub fn load() -> (Self, Vec<Effect>) {
        let (config_store, outcome) = match AppConfig::config_path() {
            Ok(path) => ConfigStore::open(path),
            Err(error) => ConfigStore::unavailable(error),
        };
        let config = match outcome {
            LoadOutcome::Missing => AppConfig::default(),
            LoadOutcome::Loaded(config) => config,
            LoadOutcome::Failed(error) => {
                log::error!("Configuration requires recovery: {error}");
                AppConfig::recovery_placeholder()
            }
        };
        // Keep the Run key aligned with saved settings. Skip recovery placeholders
        // so a failed load cannot delete a registration the user already chose.
        if config_store.recovery_error().is_none()
            && let Err(error) = startup::apply(config.start_with_windows, config.start_minimized)
        {
            log::warn!("Failed to sync Windows startup registration: {error}");
        }

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

        let config_path = config_store.path().display().to_string();
        let mut state = Self {
            monitor_generation: 0,
            detected_monitors: Vec::new(),
            monitors: Vec::new(),
            monitor_load_errors: HashMap::new(),
            config,
            config_store,
            config_ops: VecDeque::new(),
            config_op_active: false,
            pending_config_reset: false,
            hotkey_manager,
            hotkey_action_map,
            hotkey_status,
            hotkey_generation: 0,
            status_message: "Starting...".into(),
            recording_state: RecordingState::NotRecording,
            expanded_hotkey: None,
            pending_hotkey_delete: None,
            value_drafts,
            vcp_drafts,
            config_dirty: false,
            config_path,
            slider_debounce: SliderDebounce::default(),
            profiles: Vec::new(),
            profiles_request: 0,
            profile_name_input: String::new(),
            pending_profile_delete: None,
            pending_profile_replace: None,
            action_executor: HardwareCoordinator::default(),
            refresh_profiles_after_jobs: false,
            effects: Vec::new(),
        };
        state.refresh_monitors();
        state.refresh_profiles();
        let effects = std::mem::take(&mut state.effects);
        (state, effects)
    }

    pub fn update(&mut self, input: Input) -> Vec<Effect> {
        if self
            .pending_hotkey_delete
            .as_ref()
            .is_some_and(|id| self.hotkey_delete_title(id).is_none())
        {
            self.pending_hotkey_delete = None;
        }

        // The store also guards saves; this gate prevents editing or executing
        // placeholder settings while a failed load awaits the user's decision.
        if self.config_store.recovery_error().is_some() && input.needs_writable_config() {
            self.status_message =
                "Configuration is read-only until you retry, recover a backup, or confirm a reset."
                    .into();
            return Vec::new();
        }

        match input {
            Input::RefreshMonitors => self.refresh_monitors(),
            Input::RetryMonitor(monitor_id) => self.retry_monitor(monitor_id),
            Input::SelectInputSource(monitor_id, source) => {
                self.set_input_source(monitor_id, source)
            }
            Input::HardwareJobFinished(id, result) => self.hardware_job_finished(id, result),
            Input::BrightnessSliderChanged(monitor_id, value) => {
                self.slider_changed(monitor_id, SliderFeature::Brightness, value)
            }
            Input::ContrastSliderChanged(monitor_id, value) => {
                self.slider_changed(monitor_id, SliderFeature::Contrast, value)
            }
            Input::ApplyBrightnessDebounced(monitor_id, token) => {
                self.apply_slider_debounced(monitor_id, SliderFeature::Brightness, token)
            }
            Input::ApplyContrastDebounced(monitor_id, token) => {
                self.apply_slider_debounced(monitor_id, SliderFeature::Contrast, token)
            }
            Input::TurnOffMonitors => {
                let monitor_ids = self.detected_monitors.iter().map(|m| m.id).collect();
                self.enqueue_hardware_jobs([HardwareJob::SoftTurnOff { monitor_ids }]);
            }

            Input::HotkeyTriggered(id) => self.handle_hotkey_triggered(id),
            Input::ToggleHotkeys(enabled) => self.toggle_hotkeys(enabled),
            Input::AddHotkey => self.add_hotkey(),
            Input::SetHotkeyLabel(id, label) => self.set_hotkey_label(id, label),
            Input::ToggleHotkeyEditor(id) => self.toggle_hotkey_editor(id),
            Input::RequestDeleteHotkey(id) => self.request_delete_hotkey(id),
            Input::CancelDeleteHotkey => self.pending_hotkey_delete = None,
            Input::DeleteHotkey(id) => self.delete_hotkey(id),
            Input::StartRecording(hotkey_id) => self.start_recording(hotkey_id),
            Input::CancelRecording => self.cancel_recording(),
            Input::ClearBinding(id) => self.clear_binding(id),
            Input::KeyRecorded(binding) => self.key_recorded(binding),
            Input::AddAction(id) => self.add_action(id),
            Input::DeleteAction(id, idx) => self.delete_action(id, idx),
            Input::SetActionType(id, idx, action_type) => {
                self.set_action_type(id, idx, action_type)
            }
            Input::SetActionTarget(id, idx, target) => self.set_action_target(id, idx, target),
            Input::ActionValueDraftChanged(id, idx, draft) => {
                self.action_value_draft_changed(id, idx, draft)
            }
            Input::ActionVcpDraftChanged(id, idx, draft) => {
                self.action_vcp_draft_changed(id, idx, draft)
            }
            Input::SetActionInputSource(id, idx, source) => {
                self.set_action_input_source(id, idx, source)
            }
            Input::SetMonitorInput(id, idx, monitor_id, source) => {
                self.set_monitor_input(id, idx, monitor_id, source)
            }
            Input::SetActionPowerMode(id, idx, mode) => self.set_action_power_mode(id, idx, mode),
            Input::SetActionProfile(id, idx, name) => self.set_action_profile(id, idx, name),
            Input::ToggleActionAllMonitors(id, idx, checked) => {
                self.toggle_action_all_monitors(id, idx, checked)
            }
            Input::ToggleActionMonitor(id, idx, monitor_id, checked) => {
                self.toggle_action_monitor(id, idx, monitor_id, checked)
            }
            Input::RebindMonitorTarget(id, idx, old, key) => {
                self.rebind_monitor_target(id, idx, old, key)
            }
            Input::RemoveMonitorTarget(id, idx, target) => {
                self.remove_monitor_target(id, idx, target)
            }

            Input::SetTurnOffBehavior(behavior) => {
                self.config.turn_off_behavior = behavior;
                self.config_dirty = true;
            }
            Input::ToggleStartWithWindows(enabled) => {
                self.set_windows_startup(enabled, self.config.start_minimized)
            }
            Input::ToggleStartMinimized(minimized) => {
                self.set_windows_startup(self.config.start_with_windows, minimized)
            }
            Input::SaveConfig => self.save_config(),
            Input::RetryConfig => self.retry_config(),
            Input::RecoverConfigBackup => self.recover_config_backup(),
            Input::RequestResetConfig => {
                self.pending_config_reset = self.config_store.recovery_error().is_some();
            }
            Input::CancelResetConfig => self.pending_config_reset = false,
            Input::ConfirmResetConfig => self.confirm_reset_config(),
            Input::ConfigOpFinished(op, done) => self.config_op_finished(op, done),

            Input::RefreshProfiles => self.refresh_profiles(),
            Input::ProfilesListed(request, listed) => self.profiles_listed(request, listed),
            Input::ProfileNameInput(value) => self.profile_name_input = value,
            Input::SaveCurrentProfile(name) => self.save_current_profile(name),
            Input::ConfirmReplaceProfile => self.confirm_replace_profile(),
            Input::CancelReplaceProfile => self.pending_profile_replace = None,
            Input::ApplyProfile(name) => self.apply_profile(name),
            Input::RequestDeleteProfile(name) => self.request_delete_profile(name),
            Input::CancelDeleteProfile => self.pending_profile_delete = None,
            Input::DeleteProfile(name) => self.delete_profile(name),
            Input::AddProfileHotkey(profile_name) => self.add_profile_hotkey(profile_name),

            Input::ConfigPathCopied => self.status_message = "Copied configuration path".into(),
            Input::OpenUrlFailed(error) => {
                self.status_message = format!("Failed to open URL: {error}");
            }
        }
        std::mem::take(&mut self.effects)
    }

    fn emit(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    // -----------------------------------------------------------------------
    // Read-only accessors for views
    // -----------------------------------------------------------------------

    pub fn detected_monitors(&self) -> &[MonitorInfo] {
        &self.detected_monitors
    }

    pub fn monitor(&self, monitor_id: u32) -> Option<&MonitorState> {
        self.monitors.iter().find(|m| m.info.id == monitor_id)
    }

    pub fn monitor_state(&self, key: &MonitorKey) -> Option<&MonitorState> {
        self.monitors.iter().find(|m| &m.info.key == key)
    }

    pub fn monitor_load_error(&self, monitor_id: u32) -> Option<&str> {
        self.monitor_load_errors
            .get(&monitor_id)
            .map(String::as_str)
    }

    /// Queued writes and slider drafts to preview on a monitor's controls.
    pub fn monitor_pending(&self, info: &MonitorInfo) -> MonitorPending {
        MonitorPending::new(
            info.id,
            self.action_executor
                .outstanding()
                .filter_map(|job| job.preview_for(info, self.monitor_generation)),
            &self.slider_debounce,
        )
    }

    /// Advertised input lists of the monitors an action targets.
    pub fn targeted_advertised_inputs(
        &self,
        action: &HotkeyActionSpec,
    ) -> Vec<Option<Vec<InputSource>>> {
        let infos: Vec<MonitorInfo> = if action.all_monitors {
            self.detected_monitors.clone()
        } else {
            action
                .explicit_targets()
                .iter()
                .filter_map(|target| target.resolve(&self.detected_monitors).ok().cloned())
                .collect()
        };
        infos
            .iter()
            .filter_map(|info| {
                self.monitor_state(&info.key)
                    .map(|monitor| monitor.advertised_inputs.clone())
            })
            .collect()
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn config_dirty(&self) -> bool {
        self.config_dirty
    }

    pub fn config_path(&self) -> &str {
        &self.config_path
    }

    pub fn recovery_error(&self) -> Option<&str> {
        self.config_store.recovery_error()
    }

    pub fn store_path(&self) -> &std::path::Path {
        self.config_store.path()
    }

    pub fn pending_config_reset(&self) -> bool {
        self.pending_config_reset
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn is_hotkey_active(&self, id: &str) -> bool {
        self.hotkey_status.get(id).copied().unwrap_or(false)
    }

    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded_hotkey.as_deref() == Some(id)
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.recording_state, RecordingState::Recording { .. })
    }

    pub fn is_recording_hotkey(&self, id: &str) -> bool {
        matches!(&self.recording_state, RecordingState::Recording { hotkey_id } if hotkey_id == id)
    }

    /// OS ids to listen for, or `None` while hotkeys are off or unregistered.
    pub fn listened_hotkey_ids(&self) -> Option<Vec<u32>> {
        (self.config.hotkeys_enabled && !self.hotkey_action_map.is_empty())
            .then(|| self.hotkey_action_map.keys().copied().collect())
    }

    /// Changes whenever registrations change, so listeners restart.
    pub fn hotkey_generation(&self) -> u64 {
        self.hotkey_generation
    }

    pub fn value_draft(&self, id: &str, idx: usize) -> &str {
        self.value_drafts
            .get(&(id.to_string(), idx))
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn vcp_draft(&self, id: &str, idx: usize) -> &str {
        self.vcp_drafts
            .get(&(id.to_string(), idx))
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn pending_hotkey_delete(&self) -> Option<&str> {
        self.pending_hotkey_delete.as_deref()
    }

    pub fn profiles(&self) -> &[String] {
        &self.profiles
    }

    pub fn profile_name_input(&self) -> &str {
        &self.profile_name_input
    }

    pub fn pending_profile_delete(&self) -> Option<&str> {
        self.pending_profile_delete.as_deref()
    }

    pub fn pending_profile_replace(&self) -> Option<&str> {
        self.pending_profile_replace.as_deref()
    }
}
