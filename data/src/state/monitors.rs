use super::{AppState, Effect, Input, SLIDER_DEBOUNCE};
use crate::actions::HardwareJob;
use crate::ddc::{InputSource, MonitorInfo, MonitorKey, MonitorState};
use crate::debounce::{DebounceToken, SliderFeature};

fn mark_monitor_read_failed(monitors: &mut [MonitorState], key: &MonitorKey, error: &str) {
    if let Some(monitor) = monitors.iter_mut().find(|monitor| &monitor.info.key == key) {
        monitor.brightness_read_error = Some(error.to_owned());
        monitor.contrast_read_error = Some(error.to_owned());
        monitor.input_source_read_error = Some(error.to_owned());
    }
}

impl AppState {
    pub(super) fn refresh_monitors(&mut self) {
        self.action_executor.refresh();
        self.sync_hardware_generation();
        self.status_message = "Detecting monitors (waiting drafts and jobs canceled)...".into();
        self.start_next_hardware_job();
    }

    pub(super) fn retry_monitor(&mut self, monitor_id: u32) {
        if !self.action_executor.retry(monitor_id) {
            self.refresh_monitors();
            return;
        }
        // Keep the previous error visible until a serialized read succeeds.
        self.start_next_hardware_job();
    }

    pub(super) fn monitors_detected(&mut self, generation: u64, infos: Vec<MonitorInfo>) {
        if generation != self.monitor_generation {
            return;
        }
        // Also discard edits made while detection was in progress: IDs may now
        // refer to different monitors. Invalidation never resets draft revisions.
        self.slider_debounce.invalidate();
        self.status_message = format!("{} monitor(s) detected", infos.len());
        self.detected_monitors = infos;
        self.monitors.clear();
        self.monitor_load_errors.clear();
        self.emit(Effect::MonitorsDetected);
        // Initial reads were inserted into the FIFO by the coordinator.
    }

    pub(super) fn monitor_state_loaded(
        &mut self,
        generation: u64,
        key: MonitorKey,
        mut state: MonitorState,
    ) {
        if generation != self.monitor_generation {
            return;
        }
        let Some(info) = self.detected_monitors.iter().find(|m| m.key == key) else {
            return;
        };
        if state.info.key != key {
            return;
        }
        state.info = info.clone();
        self.monitor_load_errors.remove(&info.id);
        // Upsert
        if let Some(existing) = self.monitors.iter_mut().find(|m| m.info.key == key) {
            *existing = state;
        } else {
            self.monitors.push(state);
        }
        self.monitors.sort_by_key(|m| m.info.id);
    }

    pub(super) fn monitor_state_failed(&mut self, generation: u64, key: MonitorKey, error: String) {
        if generation != self.monitor_generation {
            return;
        }
        let Some(info) = self.detected_monitors.iter().find(|m| m.key == key) else {
            return;
        };
        let id = info.id;
        mark_monitor_read_failed(&mut self.monitors, &key, &error);
        self.monitor_load_errors.insert(id, error.clone());
        self.status_message = format!("Could not read monitor {id}: {error}");
    }

    pub(super) fn slider_changed(&mut self, monitor_id: u32, feature: SliderFeature, value: u16) {
        if !self.action_executor.accepts_monitor(monitor_id) {
            return;
        }
        let token =
            self.slider_debounce
                .record(self.monitor_generation, monitor_id, feature, value);
        let apply = match feature {
            SliderFeature::Brightness => Input::ApplyBrightnessDebounced(monitor_id, token),
            SliderFeature::Contrast => Input::ApplyContrastDebounced(monitor_id, token),
        };
        self.emit(Effect::Delay(SLIDER_DEBOUNCE, apply));
    }

    pub(super) fn apply_slider_debounced(
        &mut self,
        monitor_id: u32,
        feature: SliderFeature,
        token: DebounceToken,
    ) {
        let Some(value) =
            self.slider_debounce
                .take_current(self.monitor_generation, monitor_id, feature, token)
        else {
            return;
        };
        let job = match feature {
            SliderFeature::Brightness => HardwareJob::SetBrightness { monitor_id, value },
            SliderFeature::Contrast => HardwareJob::SetContrast { monitor_id, value },
        };
        self.enqueue_hardware_jobs([job]);
    }

    pub(super) fn set_input_source(&mut self, monitor_id: u32, source: InputSource) {
        self.enqueue_hardware_jobs([HardwareJob::SetInputSource { monitor_id, source }]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddc::test_util::{key, monitor_state};
    use crate::presentation::{PendingValue, input_presentation, scalar_presentation};

    #[test]
    fn whole_read_failure_marks_only_the_affected_monitor_unknown() {
        let mut monitors = [monitor_state(), monitor_state()];
        monitors[1].info.id = 2;
        monitors[1].info.key = key(2);
        mark_monitor_read_failed(&mut monitors, &key(1), "monitor disconnected");
        let monitor = &monitors[0];
        for (value, maximum, error) in [
            (
                monitor.brightness,
                monitor.brightness_max,
                monitor.brightness_read_error.as_deref(),
            ),
            (
                monitor.contrast,
                monitor.contrast_max,
                monitor.contrast_read_error.as_deref(),
            ),
        ] {
            let presentation = scalar_presentation(value, maximum, error, PendingValue::None);
            assert_eq!(presentation.value, None);
            assert_eq!(presentation.label, "Unknown");
            assert_eq!(presentation.read_error, Some("monitor disconnected"));
        }
        let input = input_presentation(
            monitor.input_source,
            monitor.input_source_read_error.as_deref(),
            PendingValue::None,
        );
        assert_eq!(input.value, None);
        assert_eq!(input.label, "Unknown");
        assert_eq!(input.read_error, Some("monitor disconnected"));
        assert!(monitors[1].brightness_read_error.is_none());
        assert!(monitors[1].contrast_read_error.is_none());
        assert!(monitors[1].input_source_read_error.is_none());
    }
}
