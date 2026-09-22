use super::actions::HardwareOutcome;
use super::coordination::{HardwareResult, QueueEvent, WindowsHardware};
use super::{AppModel, Message};
use crate::ddc::MonitorState;
use cosmic::Application;

/// Only completed, successful writes may change confirmed values. In particular,
/// a scalar write cannot establish the maximum that a failed read did not provide.
fn apply_monitor_outcome(monitors: &mut [MonitorState], result: &Result<HardwareOutcome, String>) {
    let Ok(outcome) = result else {
        return;
    };
    for monitor in monitors {
        match outcome {
            HardwareOutcome::BrightnessApplied { monitor_id, value }
                if &monitor.info.key == monitor_id =>
            {
                monitor.brightness = *value;
            }
            HardwareOutcome::ContrastApplied { monitor_id, value }
                if &monitor.info.key == monitor_id =>
            {
                monitor.contrast = *value;
            }
            HardwareOutcome::InputSourceApplied { monitor_id, source }
                if &monitor.info.key == monitor_id =>
            {
                monitor.input_source = *source;
                monitor.input_source_read_error = None;
            }
            _ => {}
        }
    }
}

fn apply_current_outcome(
    monitors: &mut [MonitorState],
    generation: u64,
    current: u64,
    result: &Result<HardwareOutcome, String>,
) {
    if generation == current {
        apply_monitor_outcome(monitors, result);
    }
}

impl AppModel {
    pub(super) fn enqueue_hardware_jobs(
        &mut self,
        jobs: impl IntoIterator<Item = super::HardwareJob>,
    ) -> cosmic::app::Task<Message> {
        if let Err(error) = self.action_executor.enqueue_ui(jobs.into_iter().collect()) {
            self.status_message = error;
            return cosmic::app::Task::none();
        }
        self.start_next_hardware_job()
    }

    pub(super) fn sync_hardware_generation(&mut self) {
        let generation = self.action_executor.generation();
        if self.monitor_generation != generation {
            self.monitor_generation = generation;
            self.slider_debounce.invalidate();
            self.monitors.clear();
            self.monitor_load_errors.clear();
        }
    }

    pub(super) fn start_next_hardware_job(&mut self) -> cosmic::app::Task<Message> {
        let work = self.action_executor.start_next(&self.monitors);
        self.sync_hardware_generation();
        let Some(work) = work else {
            return cosmic::app::Task::none();
        };
        let id = work.id;
        cosmic::app::Task::perform(
            async move { tokio::task::spawn_blocking(move || work.execute(&mut WindowsHardware)).await },
            move |result| {
                cosmic::Action::App(Message::HardwareJobFinished(
                    id,
                    match result {
                        Ok(result) => result,
                        Err(error) => Err(format!("Task join error: {error}")),
                    },
                ))
            },
        )
    }

    pub(super) fn hardware_job_finished(
        &mut self,
        id: u64,
        result: Result<HardwareResult, String>,
    ) -> cosmic::app::Task<Message> {
        let event = self.action_executor.finish(id, result);
        self.sync_hardware_generation();
        let mut follow_up = Vec::new();
        match event {
            QueueEvent::Ignored => {}
            QueueEvent::Discovered(Ok(infos)) => {
                follow_up.push(self.monitors_detected(self.monitor_generation, infos));
            }
            QueueEvent::Discovered(Err(error)) => {
                for info in &self.detected_monitors {
                    self.monitor_load_errors.insert(info.id, error.clone());
                }
                self.status_message =
                    format!("Discovery failed; dependent jobs canceled. Retry refresh: {error}");
            }
            QueueEvent::Read(key, Ok(state)) => {
                follow_up.push(self.monitor_state_loaded(self.monitor_generation, key, state));
            }
            QueueEvent::Read(key, Err(error)) => {
                follow_up.push(self.monitor_state_failed(self.monitor_generation, key, error));
            }
            QueueEvent::Applied(result) => {
                apply_current_outcome(
                    &mut self.monitors,
                    self.monitor_generation,
                    self.monitor_generation,
                    &result,
                );
                self.hardware_outcome_status(result);
            }
        }
        follow_up.push(self.start_next_hardware_job());
        if self.action_executor.is_idle() && std::mem::take(&mut self.refresh_profiles_after_jobs) {
            follow_up.push(self.update(Message::RefreshProfiles));
        }
        cosmic::app::Task::batch(follow_up)
    }

    fn hardware_outcome_status(&mut self, result: Result<HardwareOutcome, String>) {
        match result {
            Ok(
                HardwareOutcome::BrightnessApplied { .. }
                | HardwareOutcome::ContrastApplied { .. }
                | HardwareOutcome::InputSourceApplied { .. },
            ) => {}
            Ok(HardwareOutcome::PowerModeApplied { monitor_id, mode }) => {
                self.status_message = format!("Monitor {monitor_id} power mode set to {mode}.");
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
                self.status_message =
                    format!("Applied profile '{name}'; rediscovering monitors...");
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::{ActionExecutor, HardwareJob};
    use crate::app::debounce::SliderDebounce;
    use crate::app::views::{
        MonitorPending, PendingValue, input_presentation, scalar_presentation,
    };
    use crate::ddc::{
        InputSource,
        tests::{key, monitor_state},
    };

    #[test]
    fn async_success_requires_current_generation_and_matches_key_not_display_number() {
        let mut monitors = [monitor_state(), monitor_state()];
        monitors[0].info.id = 2;
        monitors[1].info.key = key(2);
        let result = Ok(HardwareOutcome::BrightnessApplied {
            monitor_id: key(1),
            value: 80,
        });
        apply_current_outcome(&mut monitors, 7, 8, &result);
        assert_eq!(monitors[0].brightness, 40);
        assert_eq!(monitors[1].brightness, 40);
        apply_current_outcome(&mut monitors, 8, 8, &result);
        assert_eq!(monitors[0].brightness, 80);
        assert_eq!(monitors[1].brightness, 40);
        let result = Ok(HardwareOutcome::BrightnessApplied {
            monitor_id: key(3),
            value: 90,
        });
        apply_current_outcome(&mut monitors, 8, 8, &result);
        assert_eq!(monitors[0].brightness, 80);
        assert_eq!(monitors[1].brightness, 40);
    }

    #[test]
    fn failed_writes_remove_previews_and_preserve_confirmed_values() {
        for job in [
            HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            },
            HardwareJob::SetContrast {
                monitor_id: 1,
                value: 200,
            },
            HardwareJob::SetInputSource {
                monitor_id: 1,
                source: InputSource::Dp1,
            },
        ] {
            let mut monitors = [monitor_state()];
            let mut executor = ActionExecutor::default();
            executor.extend([job]);
            executor.start_next().unwrap();
            let pending =
                MonitorPending::new(1, executor.outstanding(), &SliderDebounce::default());
            assert!(
                pending.brightness != PendingValue::None
                    || pending.contrast != PendingValue::None
                    || pending.input_source != PendingValue::None
            );

            executor.complete_active();
            apply_monitor_outcome(&mut monitors, &Err("write failed".into()));
            let pending =
                MonitorPending::new(1, executor.outstanding(), &SliderDebounce::default());
            let monitor = &monitors[0];
            let brightness = scalar_presentation(
                monitor.brightness,
                monitor.brightness_max,
                monitor.brightness_read_error.as_deref(),
                pending.brightness,
            );
            let contrast = scalar_presentation(
                monitor.contrast,
                monitor.contrast_max,
                monitor.contrast_read_error.as_deref(),
                pending.contrast,
            );
            let input = input_presentation(
                monitor.input_source,
                monitor.input_source_read_error.as_deref(),
                pending.input_source,
            );
            assert_eq!(brightness.value, Some(40));
            assert_eq!(brightness.label, "40 / 100");
            assert_eq!(contrast.value, Some(120));
            assert_eq!(contrast.label, "120 / 255");
            assert_eq!(input.value, Some(InputSource::Hdmi1));
            assert_eq!(input.label, "HDMI 1");
            assert!(executor.is_idle());
        }
    }

    #[test]
    fn older_success_does_not_confirm_or_hide_a_newer_pending_write() {
        let mut monitors = [monitor_state()];
        let mut executor = ActionExecutor::default();
        executor.extend([
            HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 50,
            },
            HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            },
        ]);
        executor.start_next().unwrap();
        executor.complete_active();
        apply_monitor_outcome(
            &mut monitors,
            &Ok(HardwareOutcome::BrightnessApplied {
                monitor_id: key(1),
                value: 50,
            }),
        );
        assert_eq!(monitors[0].brightness, 50);
        let pending = MonitorPending::new(1, executor.outstanding(), &SliderDebounce::default());
        let preview = scalar_presentation(monitors[0].brightness, 100, None, pending.brightness);
        assert_eq!(preview.value, Some(80));
        assert_eq!(preview.label, "80 / 100 (pending)");

        executor.start_next().unwrap();
        executor.complete_active();
        apply_monitor_outcome(&mut monitors, &Err("second write failed".into()));
        let pending = MonitorPending::new(1, executor.outstanding(), &SliderDebounce::default());
        let restored = scalar_presentation(monitors[0].brightness, 100, None, pending.brightness);
        assert_eq!(restored.value, Some(50));
        assert_eq!(restored.label, "50 / 100");
    }

    #[test]
    fn only_valid_knowledge_clears_existing_read_errors() {
        let mut monitor = monitor_state();
        monitor.brightness_read_error = Some("brightness timeout".into());
        monitor.contrast_read_error = Some("contrast timeout".into());
        monitor.input_source_read_error = Some("input timeout".into());
        let mut monitors = [monitor];
        apply_monitor_outcome(&mut monitors, &Err("write failed".into()));
        assert_eq!(monitors[0].input_source, InputSource::Hdmi1);
        assert_eq!(
            monitors[0].input_source_read_error.as_deref(),
            Some("input timeout")
        );

        apply_monitor_outcome(
            &mut monitors,
            &Ok(HardwareOutcome::BrightnessApplied {
                monitor_id: key(1),
                value: 80,
            }),
        );
        apply_monitor_outcome(
            &mut monitors,
            &Ok(HardwareOutcome::ContrastApplied {
                monitor_id: key(1),
                value: 200,
            }),
        );
        let monitor = &monitors[0];
        assert_eq!(
            monitor.brightness_read_error.as_deref(),
            Some("brightness timeout")
        );
        assert_eq!(
            monitor.contrast_read_error.as_deref(),
            Some("contrast timeout")
        );
        assert_eq!(
            scalar_presentation(
                monitor.brightness,
                monitor.brightness_max,
                monitor.brightness_read_error.as_deref(),
                PendingValue::None
            )
            .value,
            None
        );
        assert_eq!(
            scalar_presentation(
                monitor.contrast,
                monitor.contrast_max,
                monitor.contrast_read_error.as_deref(),
                PendingValue::None
            )
            .value,
            None
        );

        apply_monitor_outcome(
            &mut monitors,
            &Ok(HardwareOutcome::InputSourceApplied {
                monitor_id: key(2),
                source: InputSource::Dp1,
            }),
        );
        assert!(monitors[0].input_source_read_error.is_some());
        apply_monitor_outcome(
            &mut monitors,
            &Ok(HardwareOutcome::InputSourceApplied {
                monitor_id: key(1),
                source: InputSource::Dp1,
            }),
        );
        assert_eq!(monitors[0].input_source, InputSource::Dp1);
        assert_eq!(monitors[0].input_source_read_error, None);
    }
}
