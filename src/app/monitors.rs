use super::{AppModel, Message, Page};
use crate::ddc::{self, InputSource, MonitorInfo, MonitorState};
use cosmic::Application;
use cosmic::widget::nav_bar;

impl AppModel {
    pub(super) fn refresh_monitors(&mut self) -> cosmic::app::Task<Message> {
        self.monitor_generation = self.monitor_generation.wrapping_add(1);
        let generation = self.monitor_generation;
        self.status_message = "Detecting monitors...".into();
        cosmic::app::Task::perform(
            async { tokio::task::spawn_blocking(ddc::detect_monitors).await },
            move |result| match result {
                Ok(Ok(monitors)) => {
                    cosmic::Action::App(Message::MonitorsDetected(generation, monitors))
                }
                Ok(Err(e)) => {
                    cosmic::Action::App(Message::Error(format!("DDC detection error: {e}")))
                }
                Err(e) => cosmic::Action::App(Message::Error(format!("Task join error: {e}"))),
            },
        )
    }

    pub(super) fn retry_monitor(&mut self, monitor_id: u32) -> cosmic::app::Task<Message> {
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
        cosmic::app::Task::perform(
            async move {
                tokio::task::spawn_blocking(move || ddc::read_monitor_state(monitor_id, info)).await
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
        )
    }

    pub(super) fn monitors_detected(
        &mut self,
        generation: u64,
        infos: Vec<MonitorInfo>,
    ) -> cosmic::app::Task<Message> {
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
                    tokio::task::spawn_blocking(move || ddc::read_monitor_state(mid, info)).await
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
        cosmic::app::Task::batch(tasks)
    }

    pub(super) fn monitor_state_loaded(
        &mut self,
        generation: u64,
        id: u32,
        state: Box<MonitorState>,
    ) -> cosmic::app::Task<Message> {
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
        cosmic::app::Task::none()
    }

    pub(super) fn monitor_state_failed(
        &mut self,
        generation: u64,
        id: u32,
        error: String,
    ) -> cosmic::app::Task<Message> {
        if generation != self.monitor_generation {
            return cosmic::app::Task::none();
        }
        self.monitor_load_errors.insert(id, error.clone());
        self.status_message = format!("Could not read monitor {id}: {error}");
        cosmic::app::Task::none()
    }

    pub(super) fn brightness_slider_changed(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        // Update UI immediately for smooth feedback
        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
            m.brightness = value;
        }
        // Store pending change and debounce
        self.pending_brightness = Some((monitor_id, value));

        // Schedule debounced application after 150ms
        cosmic::app::Task::perform(
            async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                (monitor_id, value)
            },
            move |(mid, val)| cosmic::Action::App(Message::ApplyBrightnessDebounced(mid, val)),
        )
    }

    pub(super) fn apply_brightness_debounced(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        // Only apply if this is still the pending value
        if let Some((pending_id, pending_val)) = self.pending_brightness {
            if pending_id == monitor_id && pending_val == value {
                self.pending_brightness = None;
                return self.update(Message::SetBrightness(monitor_id, value));
            }
        }
        cosmic::app::Task::none()
    }

    pub(super) fn set_brightness(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
            m.brightness = value;
        }
        self.enqueue_hardware_jobs([super::HardwareJob::SetBrightness { monitor_id, value }])
    }

    pub(super) fn contrast_slider_changed(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        // Update UI immediately for smooth feedback
        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
            m.contrast = value;
        }
        // Store pending change and debounce
        self.pending_contrast = Some((monitor_id, value));

        // Schedule debounced application after 150ms
        cosmic::app::Task::perform(
            async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                (monitor_id, value)
            },
            move |(mid, val)| cosmic::Action::App(Message::ApplyContrastDebounced(mid, val)),
        )
    }

    pub(super) fn apply_contrast_debounced(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        // Only apply if this is still the pending value
        if let Some((pending_id, pending_val)) = self.pending_contrast {
            if pending_id == monitor_id && pending_val == value {
                self.pending_contrast = None;
                return self.update(Message::SetContrast(monitor_id, value));
            }
        }
        cosmic::app::Task::none()
    }

    pub(super) fn set_contrast(
        &mut self,
        monitor_id: u32,
        value: u16,
    ) -> cosmic::app::Task<Message> {
        if let Some(m) = self.monitors.iter_mut().find(|m| m.info.id == monitor_id) {
            m.contrast = value;
        }
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
        if let Some(monitor) = self
            .monitors
            .iter_mut()
            .find(|monitor| monitor.info.id == monitor_id)
        {
            monitor.input_source = source;
        }
        self.enqueue_hardware_jobs([super::HardwareJob::SetInputSource { monitor_id, source }])
    }
}
