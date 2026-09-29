use super::{AppState, Effect, Job, JobKind};
use crate::actions::HardwareJob;

impl AppState {
    pub(super) fn refresh_profiles(&mut self) {
        self.profiles_request = self.profiles_request.wrapping_add(1);
        let request = self.profiles_request;
        self.emit(Effect::Spawn(Job(JobKind::ListProfiles(request))));
    }

    pub(super) fn profiles_listed(&mut self, request: u64, listed: Result<Vec<String>, String>) {
        if request != self.profiles_request {
            return;
        }
        match listed {
            Ok(profiles) => {
                self.profiles = profiles;
                self.emit(Effect::ProfilesUpdated);
            }
            Err(error) => {
                log::error!("{error}");
                self.status_message = error;
            }
        }
    }

    pub(super) fn save_current_profile(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status_message = "Enter a profile name first.".into();
            return;
        }
        // The worker reports an existing profile, which prompts for replacement.
        self.pending_profile_replace = None;
        self.status_message = format!("Saving profile '{name}'...");
        self.enqueue_hardware_jobs([HardwareJob::SaveProfile {
            name,
            replace: false,
        }]);
    }

    pub(super) fn confirm_replace_profile(&mut self) {
        let Some(name) = self.pending_profile_replace.take() else {
            return;
        };
        self.status_message = format!("Replacing profile '{name}'...");
        self.enqueue_hardware_jobs([HardwareJob::SaveProfile {
            name,
            replace: true,
        }]);
    }

    pub(super) fn apply_profile(&mut self, name: String) {
        self.status_message = format!("Applying profile '{name}'...");
        self.enqueue_hardware_jobs([HardwareJob::ApplyProfile { name }]);
    }

    pub(super) fn request_delete_profile(&mut self, name: String) {
        self.pending_hotkey_delete = None;
        self.pending_profile_delete = Some(name);
    }

    pub(super) fn delete_profile(&mut self, name: String) {
        self.pending_profile_delete = None;
        self.status_message = format!("Deleting profile '{name}'...");
        self.enqueue_hardware_jobs([HardwareJob::DeleteProfile { name }]);
    }
}
