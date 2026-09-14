use super::actions::HardwareOutcome;
use super::{AppModel, Message};
use cosmic::Application;

impl AppModel {
    pub(super) fn enqueue_hardware_jobs(
        &mut self,
        jobs: impl IntoIterator<Item = super::HardwareJob>,
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

    pub(super) fn hardware_job_finished(
        &mut self,
        result: Result<HardwareOutcome, String>,
    ) -> cosmic::app::Task<Message> {
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
        next
    }
}
