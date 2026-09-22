use super::{AppModel, Message};
use crate::config::TurnOffBehavior;
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;

impl AppModel {
    /// View for the settings page.
    pub(super) fn view_settings(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Settings");
        let description = widget::text::body("Configure hotkeys and how displays are turned off.");

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

        let content = widget::column::with_capacity(4)
            .push(header)
            .push(description)
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
