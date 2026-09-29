use super::{AppModel, Message};
use crate::profiles;

impl AppModel {
    pub(super) fn refresh_profiles(&mut self) -> cosmic::app::Task<Message> {
        self.profiles_request = self.profiles_request.wrapping_add(1);
        let request = self.profiles_request;
        cosmic::app::Task::perform(
            async { tokio::task::spawn_blocking(profiles::list_profiles).await },
            move |result| {
                let listed = match result {
                    Ok(Ok(list)) => Ok(list),
                    Ok(Err(error)) => Err(format!("List profiles error: {error}")),
                    Err(error) => Err(format!("Task join error: {error}")),
                };
                cosmic::Action::App(Message::ProfilesListed(request, listed))
            },
        )
    }

    pub(super) fn profiles_listed(&mut self, request: u64, listed: Result<Vec<String>, String>) {
        if request != self.profiles_request {
            return;
        }
        match listed {
            Ok(profiles) => {
                self.profiles = profiles;
                if let Some((tray, _)) = &self.tray {
                    tray.update_menu(&self.profiles);
                }
            }
            Err(error) => {
                log::error!("{error}");
                self.status_message = error;
            }
        }
    }

    pub(super) fn profile_name_input(&mut self, value: String) {
        self.profile_name_input = value;
    }

    pub(super) fn save_current_profile(&mut self, name: String) -> cosmic::app::Task<Message> {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status_message = "Enter a profile name first.".into();
            return cosmic::app::Task::none();
        }
        // The worker reports an existing profile, which prompts for replacement.
        self.pending_profile_replace = None;
        self.status_message = format!("Saving profile '{name}'...");
        self.enqueue_hardware_jobs([super::HardwareJob::SaveProfile {
            name,
            replace: false,
        }])
    }

    pub(super) fn confirm_replace_profile(&mut self) -> cosmic::app::Task<Message> {
        let Some(name) = self.pending_profile_replace.take() else {
            return cosmic::app::Task::none();
        };
        self.status_message = format!("Replacing profile '{name}'...");
        self.enqueue_hardware_jobs([super::HardwareJob::SaveProfile {
            name,
            replace: true,
        }])
    }

    pub(super) fn cancel_replace_profile(&mut self) {
        self.pending_profile_replace = None;
    }

    pub(super) fn apply_profile(&mut self, name: String) -> cosmic::app::Task<Message> {
        self.status_message = format!("Applying profile '{name}'...");
        self.enqueue_hardware_jobs([super::HardwareJob::ApplyProfile { name }])
    }

    pub(super) fn request_delete_profile(&mut self, name: String) {
        self.pending_hotkey_delete = None;
        self.pending_profile_delete = Some(name);
    }

    pub(super) fn cancel_delete_profile(&mut self) {
        self.pending_profile_delete = None;
    }

    pub(super) fn delete_profile(&mut self, name: String) -> cosmic::app::Task<Message> {
        self.pending_profile_delete = None;
        self.status_message = format!("Deleting profile '{name}'...");
        self.enqueue_hardware_jobs([super::HardwareJob::DeleteProfile { name }])
    }
}
