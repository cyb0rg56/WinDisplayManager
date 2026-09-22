use super::AppModel;
use super::{Message, Page, RecordingState};
use crate::config::{
    ActionTarget, ActionType, AppConfig, Hotkey, HotkeyActionSpec, HotkeyBinding, MonitorInput,
    MonitorTarget,
};
use crate::ddc::{InputSource, MonitorKey, PowerMode};
use crate::hotkeys::HotkeyManager;
use crate::persistence::LoadOutcome;
use cosmic::iced::keyboard::{Key, Modifiers};
use std::collections::HashMap;
use std::sync::Arc;

pub(super) fn parse_value_draft(value: &str) -> Result<Option<i32>, &'static str> {
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

pub(super) fn parse_vcp_draft(value: &str) -> Result<Option<u8>, &'static str> {
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

pub(super) fn uses_monitor_inputs(action: &HotkeyActionSpec) -> bool {
    action.action_type != ActionType::Off && action.target == ActionTarget::InputSource
}

pub(super) fn explicit_monitor_ids(action: &HotkeyActionSpec) -> Vec<MonitorTarget> {
    action.explicit_targets()
}

pub(super) fn action_monitor_selected(
    action: &HotkeyActionSpec,
    monitor_id: &MonitorTarget,
) -> bool {
    action.all_monitors || explicit_monitor_ids(action).contains(monitor_id)
}

pub(super) fn action_master_selected(
    action: &HotkeyActionSpec,
    detected_ids: &[MonitorTarget],
) -> bool {
    action.all_monitors
        || (!detected_ids.is_empty()
            && detected_ids
                .iter()
                .all(|monitor_id| action_monitor_selected(action, monitor_id)))
}

pub(super) fn toggle_action_monitor(
    action: &mut HotkeyActionSpec,
    detected_ids: &[MonitorTarget],
    monitor_id: MonitorTarget,
    checked: bool,
) {
    if action.all_monitors {
        action.all_monitors = false;
        action.monitors = detected_ids.to_vec();
        if uses_monitor_inputs(action) {
            action.monitor_inputs = detected_ids
                .iter()
                .map(|monitor_id| MonitorInput {
                    monitor_id: monitor_id.clone(),
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
        action.monitors.push(monitor_id.clone());
        if uses_monitor_inputs(action) {
            action.monitor_inputs.push(MonitorInput {
                monitor_id,
                input_source: action.input_source,
            });
        }
    }
}

pub(super) fn resolve_triggered_actions(
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
pub(super) fn key_to_string(key: &Key) -> String {
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
pub(super) fn format_hotkey(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> String {
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

impl AppModel {
    pub(super) fn hotkey_mut(&mut self, id: &str) -> Option<&mut Hotkey> {
        self.config
            .hotkeys
            .hotkeys
            .iter_mut()
            .find(|hotkey| hotkey.id == id)
    }

    pub(super) fn action_mut(&mut self, id: &str, idx: usize) -> Option<&mut HotkeyActionSpec> {
        self.hotkey_mut(id)
            .and_then(|hotkey| hotkey.actions.get_mut(idx))
    }

    pub(super) fn initialize_hotkey_drafts(&mut self, id: &str) {
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

    pub(super) fn clear_hotkey_drafts(&mut self, id: &str) {
        self.value_drafts
            .retain(|(hotkey_id, _), _| hotkey_id != id);
        self.vcp_drafts.retain(|(hotkey_id, _), _| hotkey_id != id);
    }

    pub(super) fn validate_action_drafts(&self) -> Option<String> {
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

    pub(super) fn refresh_hotkey_registration(&mut self) {
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

    pub(super) fn refresh_hotkey_actions(&mut self) {
        if let Some(manager) = &mut self.hotkey_manager {
            manager.rebuild_action_map(&self.config);
            self.hotkey_action_map = manager.action_map();
        }
    }

    pub(super) fn handle_hotkey_triggered(&mut self, id: u32) -> cosmic::app::Task<Message> {
        if let Some(actions) = resolve_triggered_actions(
            self.config.hotkeys_enabled,
            !matches!(self.recording_state, RecordingState::NotRecording),
            self.hotkey_action_map.as_ref(),
            id,
        ) {
            return self.handle_hotkey_actions(actions);
        }
        cosmic::app::Task::none()
    }

    pub(super) fn add_hotkey(&mut self) {
        let hotkey = Hotkey::new_empty();
        let id = hotkey.id.clone();
        self.config.hotkeys.hotkeys.push(hotkey);
        self.expanded_hotkey = Some(id.clone());
        self.initialize_hotkey_drafts(&id);
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn toggle_hotkey_editor(&mut self, id: String) {
        if self.expanded_hotkey.as_deref() == Some(&id) {
            self.expanded_hotkey = None;
        } else {
            self.initialize_hotkey_drafts(&id);
            self.expanded_hotkey = Some(id);
        }
    }

    pub(super) fn delete_hotkey(&mut self, id: String) {
        self.config.hotkeys.hotkeys.retain(|hotkey| hotkey.id != id);
        self.clear_hotkey_drafts(&id);
        if self.expanded_hotkey.as_deref() == Some(&id) {
            self.expanded_hotkey = None;
        }
        self.recording_state = RecordingState::NotRecording;
        self.config_dirty = true;
        self.refresh_hotkey_registration();
    }

    pub(super) fn start_recording(&mut self, hotkey_id: String) {
        self.recording_state = RecordingState::Recording { hotkey_id };
        self.status_message = "Press a key combination to bind the hotkey.".into();
    }

    pub(super) fn cancel_recording(&mut self) {
        self.recording_state = RecordingState::NotRecording;
        self.status_message = "Hotkey recording cancelled".into();
    }

    pub(super) fn clear_binding(&mut self, id: String) {
        if let Some(hotkey) = self.hotkey_mut(&id) {
            hotkey.binding = HotkeyBinding::unbound();
        }
        self.recording_state = RecordingState::NotRecording;
        self.config_dirty = true;
        self.refresh_hotkey_registration();
    }

    pub(super) fn key_pressed(
        &mut self,
        modifiers: Modifiers,
        key: Key,
    ) -> cosmic::app::Task<Message> {
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
        cosmic::app::Task::none()
    }

    pub(super) fn add_action(&mut self, id: String) {
        if let Some(hotkey) = self.hotkey_mut(&id) {
            hotkey.actions.push(HotkeyActionSpec::default());
        }
        self.initialize_hotkey_drafts(&id);
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn delete_action(&mut self, id: String, idx: usize) {
        if let Some(hotkey) = self.hotkey_mut(&id)
            && idx < hotkey.actions.len()
        {
            hotkey.actions.remove(idx);
        }
        self.clear_hotkey_drafts(&id);
        self.initialize_hotkey_drafts(&id);
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn set_action_type(&mut self, id: String, idx: usize, action_type: ActionType) {
        if let Some(action) = self.action_mut(&id, idx) {
            action.action_type = action_type;
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn set_action_target(&mut self, id: String, idx: usize, target: ActionTarget) {
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
                        monitor_id: monitor_id.clone(),
                        input_source: action.input_source,
                    })
                    .collect();
            }
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn action_value_draft_changed(&mut self, id: String, idx: usize, draft: String) {
        self.value_drafts.insert((id.clone(), idx), draft.clone());
        if let Ok(Some(value)) = parse_value_draft(&draft) {
            if let Some(action) = self.action_mut(&id, idx) {
                action.value = value;
            }
            self.config_dirty = true;
            self.refresh_hotkey_actions();
        }
    }

    pub(super) fn action_vcp_draft_changed(&mut self, id: String, idx: usize, draft: String) {
        self.vcp_drafts.insert((id.clone(), idx), draft.clone());
        if let Ok(Some(code)) = parse_vcp_draft(&draft) {
            if let Some(action) = self.action_mut(&id, idx) {
                action.vcp_code = code;
            }
            self.config_dirty = true;
            self.refresh_hotkey_actions();
        }
    }

    pub(super) fn set_action_input_source(&mut self, id: String, idx: usize, source: InputSource) {
        if let Some(action) = self.action_mut(&id, idx) {
            action.input_source = source;
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn set_monitor_input(
        &mut self,
        id: String,
        idx: usize,
        monitor_id: MonitorTarget,
        source: Option<InputSource>,
    ) {
        if let Some(action) = self.action_mut(&id, idx) {
            action.monitors.retain(|id| *id != monitor_id);
            action
                .monitor_inputs
                .retain(|input| input.monitor_id != monitor_id);
            if let Some(input_source) = source {
                action.monitors.push(monitor_id.clone());
                action.monitor_inputs.push(MonitorInput {
                    monitor_id,
                    input_source,
                });
            }
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn set_action_power_mode(&mut self, id: String, idx: usize, mode: PowerMode) {
        if let Some(action) = self.action_mut(&id, idx) {
            action.power_mode = mode;
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn set_action_profile(&mut self, id: String, idx: usize, name: String) {
        if let Some(action) = self.action_mut(&id, idx) {
            action.profile_name = name;
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn toggle_action_all_monitors(&mut self, id: String, idx: usize, checked: bool) {
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

    pub(super) fn toggle_action_monitor(
        &mut self,
        id: String,
        idx: usize,
        monitor_id: MonitorTarget,
        checked: bool,
    ) {
        let detected_ids: Vec<_> = self
            .detected_monitors
            .iter()
            .map(|m| MonitorTarget::Stable(m.key.clone()))
            .collect();
        if let Some(action) = self.action_mut(&id, idx) {
            toggle_action_monitor(action, &detected_ids, monitor_id, checked);
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn rebind_monitor_target(
        &mut self,
        id: String,
        idx: usize,
        old: MonitorTarget,
        key: MonitorKey,
    ) {
        if let Err(error) = MonitorTarget::Stable(key.clone()).resolve(&self.detected_monitors) {
            self.status_message = error;
            return;
        }
        if let Some(action) = self.action_mut(&id, idx)
            && let Err(error) = action.rebind_target(&old, key)
        {
            self.status_message = error;
            return;
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn remove_monitor_target(&mut self, id: String, idx: usize, target: MonitorTarget) {
        if let Some(action) = self.action_mut(&id, idx) {
            action.remove_target(&target);
        }
        self.config_dirty = true;
        self.refresh_hotkey_actions();
    }

    pub(super) fn save_config(&mut self) -> cosmic::app::Task<Message> {
        if let Some(error) = self.validate_action_drafts() {
            self.status_message = error;
            return cosmic::app::Task::none();
        }
        if let Err(e) = self.config_store.save(&self.config) {
            self.status_message = format!("Failed to save config: {e}");
        } else {
            self.status_message = "Configuration saved and hotkeys activated.".into();
            self.config_dirty = false;
            self.refresh_hotkey_registration();
        }
        cosmic::app::Task::none()
    }

    pub(super) fn toggle_hotkeys(&mut self, enabled: bool) -> cosmic::app::Task<Message> {
        match self
            .config_store
            .set_hotkeys_enabled(&mut self.config, enabled)
        {
            Ok(()) => {
                self.status_message = if enabled {
                    "Hotkeys enabled"
                } else {
                    "Hotkeys disabled"
                }
                .into();
                self.refresh_hotkey_registration();
            }
            Err(error) => self.status_message = format!("Failed to save config: {error}"),
        }
        cosmic::app::Task::none()
    }

    pub(super) fn retry_config(&mut self) {
        self.pending_config_reset = false;
        match self.config_store.retry() {
            LoadOutcome::Loaded(config) => {
                self.install_recovered_config(config, "Configuration reloaded.")
            }
            LoadOutcome::Missing => self.install_recovered_config(
                AppConfig::default(),
                "No configuration file found; using defaults.",
            ),
            LoadOutcome::Failed(error) => {
                self.status_message = format!("Configuration still needs recovery: {error}")
            }
        }
    }

    pub(super) fn recover_config_backup(&mut self) {
        self.pending_config_reset = false;
        match self.config_store.recover_backup() {
            Ok(config) => {
                self.install_recovered_config(config, "Configuration restored from backup.")
            }
            Err(error) => self.status_message = format!("Could not recover backup: {error}"),
        }
    }

    pub(super) fn confirm_reset_config(&mut self) {
        if !std::mem::take(&mut self.pending_config_reset)
            || self.config_store.recovery_error().is_none()
        {
            return;
        }
        match self.config_store.reset_confirmed() {
            Ok(config) => self.install_recovered_config(
                config,
                "Configuration reset to defaults. Existing backup retained.",
            ),
            Err(error) => self.status_message = format!("Could not reset configuration: {error}"),
        }
    }

    fn install_recovered_config(&mut self, config: AppConfig, status: &str) {
        self.config = config;
        self.config_dirty = false;
        self.recording_state = RecordingState::NotRecording;
        self.expanded_hotkey = None;
        self.value_drafts.clear();
        self.vcp_drafts.clear();
        let ids: Vec<_> = self
            .config
            .hotkeys
            .hotkeys
            .iter()
            .map(|hotkey| hotkey.id.clone())
            .collect();
        for id in ids {
            self.initialize_hotkey_drafts(&id);
        }
        self.refresh_hotkey_registration();
        self.status_message = status.into();
    }

    pub(super) fn add_profile_hotkey(&mut self, profile_name: String) {
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

        toggle_action_monitor(&mut action, &[1.into(), 2.into()], 1.into(), false);
        assert!(!action.all_monitors);
        assert_eq!(action.monitors, vec![2.into()]);
        assert_eq!(action.monitor_inputs.len(), 1);
        assert_eq!(action.monitor_inputs[0].monitor_id, 2.into());

        action.monitor_inputs[0].input_source = InputSource::Dp1;
        toggle_action_monitor(&mut action, &[1.into(), 2.into()], 1.into(), true);
        assert!(action_master_selected(&action, &[1.into(), 2.into()]));
        assert!(!action.all_monitors);
        assert_eq!(action.monitor_inputs[0].input_source, InputSource::Dp1);
    }

    #[test]
    fn empty_detection_is_not_aggregate_all_and_off_uses_monitor_ids() {
        let mut action = HotkeyActionSpec {
            action_type: ActionType::Off,
            target: ActionTarget::InputSource,
            all_monitors: false,
            monitors: vec![7.into()],
            monitor_inputs: vec![MonitorInput {
                monitor_id: 8.into(),
                input_source: InputSource::Dp1,
            }],
            ..Default::default()
        };
        assert!(!action_master_selected(&action, &[]));
        assert_eq!(explicit_monitor_ids(&action), vec![7.into()]);

        toggle_action_monitor(&mut action, &[7.into()], 7.into(), true);
        assert_eq!(action.monitors, vec![7.into()]);
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
                monitors: vec![3.into()],
                value: -17,
                ..Default::default()
            }],
        );

        let actions = resolve_triggered_actions(true, false, &action_map, id).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target, ActionTarget::Contrast);
        assert_eq!(actions[0].value, -17);
        assert_eq!(actions[0].monitors, vec![3.into()]);
        assert!(resolve_triggered_actions(false, false, &action_map, id).is_none());
        assert!(resolve_triggered_actions(true, true, &action_map, id).is_none());
        assert!(resolve_triggered_actions(true, false, &action_map, 999).is_none());
    }
}
