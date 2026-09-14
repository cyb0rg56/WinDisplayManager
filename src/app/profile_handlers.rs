use super::{AppModel, Message};
use crate::profiles;

impl AppModel {
    pub(super) fn refresh_profiles(&mut self) -> cosmic::app::Task<Message> {
        cosmic::app::Task::perform(
            async { tokio::task::spawn_blocking(profiles::list_profiles).await },
            |result| match result {
                Ok(list) => cosmic::Action::App(Message::ProfilesListed(list)),
                Err(error) => {
                    cosmic::Action::App(Message::Error(format!("Task join error: {error}")))
                }
            },
        )
    }

    pub(super) fn profiles_listed(&mut self, profiles: Vec<String>) {
        self.profiles = profiles;
        if let Some((tray, _)) = &self.tray {
            tray.update_menu(&self.profiles);
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
        match profiles::profile_exists(&name) {
            Ok(true) => {
                self.status_message =
                    format!("Profile '{name}' already exists. Confirm replacement.");
                self.pending_profile_replace = Some(name);
                cosmic::app::Task::none()
            }
            Ok(false) => {
                self.status_message = format!("Saving profile '{name}'...");
                self.enqueue_hardware_jobs([super::HardwareJob::SaveProfile {
                    name,
                    replace: false,
                }])
            }
            Err(error) => {
                self.status_message = format!("Invalid profile name: {error}");
                cosmic::app::Task::none()
            }
        }
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
        self.pending_profile_delete = Some(name);
    }

    pub(super) fn cancel_delete_profile(&mut self) {
        self.pending_profile_delete = None;
    }

    pub(super) fn delete_profile(&mut self, name: String) -> cosmic::app::Task<Message> {
        self.pending_profile_delete = None;
        cosmic::app::Task::perform(
            async move { tokio::task::spawn_blocking(move || profiles::delete_profile(&name)).await },
            |result| match result {
                Ok(Ok(())) => cosmic::Action::App(Message::RefreshProfiles),
                Ok(Err(error)) => {
                    cosmic::Action::App(Message::Error(format!("Delete profile error: {error}")))
                }
                Err(error) => {
                    cosmic::Action::App(Message::Error(format!("Task join error: {error}")))
                }
            },
        )
    }
}
