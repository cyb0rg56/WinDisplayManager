use super::{AppModel, Message};
use crate::config::TurnOffBehavior;
use crate::startup;
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;

impl AppModel {
    /// Persist startup settings and keep the per-user Run key in step.
    ///
    /// The registry is updated first. If saving config fails, the previous
    /// registration is written back and the in-memory config is left unchanged.
    pub(super) fn set_windows_startup(
        &mut self,
        enabled: bool,
        minimized: bool,
    ) -> cosmic::app::Task<Message> {
        let previous_enabled = self.config.start_with_windows;
        let previous_minimized = self.config.start_minimized;
        if let Err(error) = startup::apply(enabled, minimized) {
            self.status_message = format!("Failed to update Windows startup: {error}");
            return cosmic::app::Task::none();
        }

        let mut updated = self.config.clone();
        updated.start_with_windows = enabled;
        updated.start_minimized = minimized;
        if let Err(error) = self.config_store.save(&updated) {
            self.status_message = format!("Failed to save config: {error}");
            if let Err(revert_error) = startup::apply(previous_enabled, previous_minimized) {
                log::error!("Failed to restore Windows startup registration: {revert_error}");
            }
            return cosmic::app::Task::none();
        }

        self.config = updated;
        self.status_message = if !enabled {
            "Will not start with Windows".into()
        } else if minimized {
            "Will start with Windows, minimized to the tray".into()
        } else {
            "Will start with Windows".into()
        };
        cosmic::app::Task::none()
    }

    /// View for the settings page.
    pub(super) fn view_settings(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Settings");
        let description =
            widget::text::body("Configure startup, hotkeys, and how displays are turned off.");

        // --- Hotkeys enabled toggle ---
        let hotkeys_section = cosmic::widget::settings::section().title("Hotkeys").add(
            cosmic::widget::settings::item::builder("Enable global hotkeys")
                .description("When disabled, hotkeys will not trigger any actions")
                .control(
                    widget::toggler(self.config.hotkeys_enabled).on_toggle(Message::ToggleHotkeys),
                ),
        );

        let turn_off_labels: Vec<&str> = TurnOffBehavior::ALL
            .iter()
            .map(|behavior| behavior.label())
            .collect();
        let selected_turn_off = TurnOffBehavior::ALL
            .iter()
            .position(|behavior| behavior == &self.config.turn_off_behavior);
        let power_section = cosmic::widget::settings::section()
            .title("Turn Off Displays")
            .add(
                cosmic::widget::settings::item::builder("Power-off method").control(
                    widget::dropdown(turn_off_labels, selected_turn_off, |selected| {
                        Message::SetTurnOffBehavior(TurnOffBehavior::ALL[selected])
                    }),
                ),
            );

        let mut startup_section = cosmic::widget::settings::section().title("Startup").add(
            cosmic::widget::settings::item::builder("Start with Windows")
                .description("Launch when you sign in")
                .control(
                    widget::toggler(self.config.start_with_windows)
                        .on_toggle(Message::ToggleStartWithWindows),
                ),
        );
        if self.config.start_with_windows {
            startup_section = startup_section.add(
                cosmic::widget::settings::item::builder("Start minimized")
                    .description("Stay in the system tray until you open it")
                    .control(
                        widget::toggler(self.config.start_minimized)
                            .on_toggle(Message::ToggleStartMinimized),
                    ),
            );
        }

        let content = widget::column::with_capacity(5)
            .push(header)
            .push(description)
            .push(startup_section)
            .push(hotkeys_section)
            .push(power_section)
            .spacing(space_s)
            .width(Length::Fill);

        // Wrap in scrollable to ensure all content is accessible
        widget::scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
