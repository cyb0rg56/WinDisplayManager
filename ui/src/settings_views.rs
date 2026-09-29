use crate::{AppModel, Message};
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;
use data::config::TurnOffBehavior;
use data::state::Input;

impl AppModel {
    /// Settings shown in the header context drawer.
    pub(super) fn view_settings(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let config = self.state.config();

        let description =
            widget::text::body("Configure startup, hotkeys, and how displays are turned off.");

        // --- Hotkeys enabled toggle ---
        let hotkeys_section = cosmic::widget::settings::section().title("Hotkeys").add(
            cosmic::widget::settings::item::builder("Enable global hotkeys")
                .description("When disabled, hotkeys will not trigger any actions")
                .control(
                    widget::toggler(config.hotkeys_enabled)
                        .on_toggle(|enabled| Message::Data(Input::ToggleHotkeys(enabled))),
                ),
        );

        let turn_off_labels: Vec<&str> = TurnOffBehavior::ALL
            .iter()
            .map(|behavior| behavior.label())
            .collect();
        let selected_turn_off = TurnOffBehavior::ALL
            .iter()
            .position(|behavior| behavior == &config.turn_off_behavior);
        let power_section = cosmic::widget::settings::section()
            .title("Turn Off Displays")
            .add(
                cosmic::widget::settings::item::builder("Power-off method").control(
                    widget::dropdown(turn_off_labels, selected_turn_off, |selected| {
                        Message::Data(Input::SetTurnOffBehavior(TurnOffBehavior::ALL[selected]))
                    }),
                ),
            );

        let mut startup_section = cosmic::widget::settings::section().title("Startup").add(
            cosmic::widget::settings::item::builder("Start with Windows")
                .description("Launch when you sign in")
                .control(
                    widget::toggler(config.start_with_windows)
                        .on_toggle(|enabled| Message::Data(Input::ToggleStartWithWindows(enabled))),
                ),
        );
        if config.start_with_windows {
            startup_section = startup_section.add(
                cosmic::widget::settings::item::builder("Start minimized")
                    .description("Stay in the system tray until you open it")
                    .control(
                        widget::toggler(config.start_minimized).on_toggle(|minimized| {
                            Message::Data(Input::ToggleStartMinimized(minimized))
                        }),
                    ),
            );
        }

        widget::column::with_capacity(4)
            .push(description)
            .push(startup_section)
            .push(hotkeys_section)
            .push(power_section)
            .spacing(space_s)
            .width(Length::Fill)
            .into()
    }
}
