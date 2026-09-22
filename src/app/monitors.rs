use super::debounce::{DebounceToken, SliderFeature};
use super::{AppModel, Message, Page};
use crate::ddc::{InputSource, MonitorInfo, MonitorKey, MonitorState};
use cosmic::Application;
use cosmic::widget::nav_bar;

fn mark_monitor_read_failed(monitors: &mut [MonitorState], key: &MonitorKey, error: &str) {
    if let Some(monitor) = monitors.iter_mut().find(|monitor| &monitor.info.key == key) {
        monitor.brightness_read_error = Some(error.to_owned());
        monitor.contrast_read_error = Some(error.to_owned());
        monitor.input_source_read_error = Some(error.to_owned());
    }
}

impl AppModel {
    pub(super) fn refresh_monitors(&mut self) -> cosmic::app::Task<Message> {
        self.action_executor.refresh();
        self.sync_hardware_generation();
        self.status_message = "Detecting monitors (waiting drafts and jobs canceled)...".into();
        self.start_next_hardware_job()
    }

    pub(super) fn retry_monitor(&mut self, monitor_id: u32) -> cosmic::app::Task<Message> {
        if !self.action_executor.retry(monitor_id) {
            return self.update(Message::RefreshMonitors);
        }
        // Keep the previous error visible until a serialized read succeeds.
        self.start_next_hardware_job()
    }

    pub(super) fn monitors_detected(
        &mut self,
        generation: u64,
        infos: Vec<MonitorInfo>,
    ) -> cosmic::app::Task<Message> {
        if generation != self.monitor_generation {
            return cosmic::app::Task::none();
        }
        // Also discard edits made while detection was in progress: IDs may now
        // refer to different monitors. Invalidation never resets draft revisions.
        self.slider_debounce.invalidate();
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
        self.nav.insert().text("About").data::<Page>(Page::About);

        // Activate first monitor
        self.nav.activate_position(0);

        self.status_message = format!("{} monitor(s) detected", infos.len());

        // Initial reads were inserted into the FIFO by the coordinator.
        cosmic::app::Task::none()
    }

    pub(super) fn monitor_state_loaded(
        &mut self,
        generation: u64,
        key: MonitorKey,
        mut state: MonitorState,
    ) -> cosmic::app::Task<Message> {
        if generation != self.monitor_generation {
            return cosmic::app::Task::none();
        }
        let Some(info) = self.detected_monitors.iter().find(|m| m.key == key) else {
            return cosmic::app::Task::none();
        };
        if state.info.key != key {
            return cosmic::app::Task::none();
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
        cosmic::app::Task::none()
    }

    pub(super) fn monitor_state_failed(
        &mut self,
        generation: u64,
        key: MonitorKey,
        error: String,
    ) -> cosmic::app::Task<Message> {
        if generation != self.monitor_generation {
            return cosmic::app::Task::none();
        }
        let Some(info) = self.detected_monitors.iter().find(|m| m.key == key) else {
            return cosmic::app::Task::none();
        };
        let id = info.id;
        mark_monitor_read_failed(&mut self.monitors, &key, &error);
        self.monitor_load_errors.insert(id, error.clone());
        self.status_message = format!("Could not read monitor {id}: {error}");
        cosmic::app::Task::none()
    }

    pub(super) fn brightness_slider_changed(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        if !self.action_executor.accepts_monitor(monitor_id) {
            return cosmic::app::Task::none();
        }
        let token = self.slider_debounce.record(
            self.monitor_generation,
            monitor_id,
            SliderFeature::Brightness,
            value,
        );

        cosmic::app::Task::perform(
            async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                token
            },
            move |token| cosmic::Action::App(Message::ApplyBrightnessDebounced(monitor_id, token)),
        )
    }

    pub(super) fn apply_brightness_debounced(
        &mut self,
        monitor_id: u32,
        token: DebounceToken,
    ) -> cosmic::app::Task<Message> {
        if let Some(value) = self.slider_debounce.take_current(
            self.monitor_generation,
            monitor_id,
            SliderFeature::Brightness,
            token,
        ) {
            return self.update(Message::SetBrightness(monitor_id, value));
        }
        cosmic::app::Task::none()
    }

    pub(super) fn set_brightness(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        self.enqueue_hardware_jobs([super::HardwareJob::SetBrightness { monitor_id, value }])
    }

    pub(super) fn contrast_slider_changed(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        if !self.action_executor.accepts_monitor(monitor_id) {
            return cosmic::app::Task::none();
        }
        let token = self.slider_debounce.record(
            self.monitor_generation,
            monitor_id,
            SliderFeature::Contrast,
            value,
        );

        cosmic::app::Task::perform(
            async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                token
            },
            move |token| cosmic::Action::App(Message::ApplyContrastDebounced(monitor_id, token)),
        )
    }

    pub(super) fn apply_contrast_debounced(
        &mut self,
        monitor_id: u32,
        token: DebounceToken,
    ) -> cosmic::app::Task<Message> {
        if let Some(value) = self.slider_debounce.take_current(
            self.monitor_generation,
            monitor_id,
            SliderFeature::Contrast,
            token,
        ) {
            return self.update(Message::SetContrast(monitor_id, value));
        }
        cosmic::app::Task::none()
    }

    pub(super) fn set_contrast(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        self.enqueue_hardware_jobs([super::HardwareJob::SetContrast { monitor_id, value }])
    }

    pub(super) fn select_input_source(
        &mut self,
        monitor_id: u32,
        idx: usize,
    ) -> cosmic::app::Task<Message> {
        if let Some(&source) = super::INPUT_SOURCES.get(idx) {
            return self.update(Message::SetInputSource(monitor_id, source));
        }
        cosmic::app::Task::none()
    }

    pub(super) fn set_input_source(
        &mut self,
        monitor_id: u32,
        source: InputSource,
    ) -> cosmic::app::Task<Message> {
        self.enqueue_hardware_jobs([super::HardwareJob::SetInputSource { monitor_id, source }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::monitor_views::{PendingValue, input_presentation, scalar_presentation};
    use crate::ddc::tests::monitor_state;

    #[test]
    fn whole_read_failure_marks_only_the_affected_monitor_unknown() {
        let mut monitors = [monitor_state(), monitor_state()];
        monitors[1].info.id = 2;
        monitors[1].info.key = crate::ddc::tests::key(2);
        mark_monitor_read_failed(
            &mut monitors,
            &crate::ddc::tests::key(1),
            "monitor disconnected",
        );
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
