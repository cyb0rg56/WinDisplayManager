use super::{AppModel, Message};
use crate::config::{AppConfig, ConfigStore};
use crate::persistence::LoadOutcome;
use crate::startup;

/// Config and registry writes run one at a time off the UI thread. Each
/// operation reads `AppModel::config` when it starts, not when requested.
#[derive(Clone, Debug)]
pub enum ConfigOp {
    Save,
    SetHotkeysEnabled(bool),
    SetStartup { enabled: bool, minimized: bool },
    Retry,
    RecoverBackup,
    Reset,
}

#[derive(Clone, Debug)]
pub struct ConfigOpDone {
    /// `None` only if the blocking task itself failed.
    store: Option<ConfigStore>,
    /// Loaded or recovered config for recovery operations; `None` otherwise.
    outcome: Result<Option<AppConfig>, String>,
}

fn run(op: &ConfigOp, mut store: ConfigStore, mut config: AppConfig) -> ConfigOpDone {
    let outcome = match *op {
        ConfigOp::Save => store
            .save(&config)
            .map(|()| None)
            .map_err(|e| format!("Failed to save config: {e}")),
        ConfigOp::SetHotkeysEnabled(enabled) => store
            .set_hotkeys_enabled(&mut config, enabled)
            .map(|()| None)
            .map_err(|e| format!("Failed to save config: {e}")),
        ConfigOp::SetStartup { enabled, minimized } => {
            set_startup(&mut store, config, enabled, minimized).map(|()| None)
        }
        ConfigOp::Retry => match store.retry() {
            LoadOutcome::Loaded(config) => Ok(Some(config)),
            LoadOutcome::Missing => Ok(None),
            LoadOutcome::Failed(e) => Err(format!("Configuration still needs recovery: {e}")),
        },
        ConfigOp::RecoverBackup => store
            .recover_backup()
            .map(Some)
            .map_err(|e| format!("Could not recover backup: {e}")),
        ConfigOp::Reset => store
            .reset_confirmed()
            .map(Some)
            .map_err(|e| format!("Could not reset configuration: {e}")),
    };
    ConfigOpDone {
        store: Some(store),
        outcome,
    }
}

/// The registry is updated first. If saving config fails, the previous
/// registration is written back.
fn set_startup(
    store: &mut ConfigStore,
    mut config: AppConfig,
    enabled: bool,
    minimized: bool,
) -> Result<(), String> {
    let previous = (config.start_with_windows, config.start_minimized);
    startup::apply(enabled, minimized)
        .map_err(|e| format!("Failed to update Windows startup: {e}"))?;
    config.start_with_windows = enabled;
    config.start_minimized = minimized;
    store.save(&config).map_err(|error| {
        if let Err(revert_error) = startup::apply(previous.0, previous.1) {
            log::error!("Failed to restore Windows startup registration: {revert_error}");
        }
        format!("Failed to save config: {error}")
    })
}

impl AppModel {
    pub(super) fn enqueue_config_op(&mut self, op: ConfigOp) -> cosmic::app::Task<Message> {
        self.config_ops.push_back(op);
        self.start_next_config_op()
    }

    fn start_next_config_op(&mut self) -> cosmic::app::Task<Message> {
        if self.config_op_active {
            return cosmic::app::Task::none();
        }
        let Some(op) = self.config_ops.pop_front() else {
            return cosmic::app::Task::none();
        };
        self.config_op_active = true;
        if matches!(op, ConfigOp::Save) {
            // Edits made while the save is in flight mark the config dirty again.
            self.config_dirty = false;
        }
        let store = self.config_store.clone();
        let config = self.config.clone();
        let task_op = op.clone();
        cosmic::app::Task::perform(
            async move { tokio::task::spawn_blocking(move || run(&task_op, store, config)).await },
            move |result| {
                let done = result.unwrap_or_else(|error| ConfigOpDone {
                    store: None,
                    outcome: Err(format!("Task join error: {error}")),
                });
                cosmic::Action::App(Message::ConfigOpFinished(op.clone(), done))
            },
        )
    }

    pub(super) fn config_op_finished(
        &mut self,
        op: ConfigOp,
        done: ConfigOpDone,
    ) -> cosmic::app::Task<Message> {
        self.config_op_active = false;
        if let Some(store) = done.store {
            self.config_store = store;
        }
        match (op, done.outcome) {
            (ConfigOp::Save, Err(error)) => {
                self.config_dirty = true;
                self.status_message = error;
            }
            (_, Err(error)) => self.status_message = error,
            (ConfigOp::Save, Ok(_)) => {
                self.status_message = "Configuration saved and hotkeys activated.".into();
                self.refresh_hotkey_registration();
            }
            (ConfigOp::SetHotkeysEnabled(enabled), Ok(_)) => {
                self.config.hotkeys_enabled = enabled;
                self.status_message = if enabled {
                    "Hotkeys enabled"
                } else {
                    "Hotkeys disabled"
                }
                .into();
                self.refresh_hotkey_registration();
            }
            (ConfigOp::SetStartup { enabled, minimized }, Ok(_)) => {
                self.config.start_with_windows = enabled;
                self.config.start_minimized = minimized;
                self.status_message = if !enabled {
                    "Will not start with Windows".into()
                } else if minimized {
                    "Will start with Windows, minimized to the tray".into()
                } else {
                    "Will start with Windows".into()
                };
            }
            (ConfigOp::Retry, Ok(Some(config))) => {
                self.install_recovered_config(config, "Configuration reloaded.")
            }
            (ConfigOp::Retry, Ok(None)) => self.install_recovered_config(
                AppConfig::default(),
                "No configuration file found; using defaults.",
            ),
            (ConfigOp::RecoverBackup, Ok(Some(config))) => {
                self.install_recovered_config(config, "Configuration restored from backup.")
            }
            (ConfigOp::Reset, Ok(Some(config))) => self.install_recovered_config(
                config,
                "Configuration reset to defaults. Existing backup retained.",
            ),
            (ConfigOp::RecoverBackup | ConfigOp::Reset, Ok(None)) => {}
        }
        self.start_next_config_op()
    }
}
