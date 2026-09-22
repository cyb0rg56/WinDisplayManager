use super::{AppModel, Message};
use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget;

impl AppModel {
    pub(super) fn view_about(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let config_path = widget::column::with_capacity(2)
            .push(widget::text::caption("Configuration file"))
            .push(
                widget::row::with_capacity(2)
                    .push(
                        widget::text_input("", &self.config_path)
                            .on_input(|_| Message::ConfigPathInput)
                            .width(Length::Fill),
                    )
                    .push(widget::button::standard("Copy").on_press(Message::CopyConfigPath))
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            )
            .spacing(space_s)
            .width(Length::Fill);
        let content = widget::column::with_capacity(2)
            .push(cosmic::widget::about(&self.about, |url| {
                Message::OpenUrl(url.to_owned())
            }))
            .push(config_path)
            .spacing(space_s)
            .width(Length::Fill);
        widget::scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(super) fn view_config_recovery(&self) -> Element<'_, Message> {
        let error = self
            .config_store
            .recovery_error()
            .unwrap_or("Unknown load error");
        let mut content = widget::column::with_capacity(6)
            .push(widget::text::title3("Configuration recovery required"))
            .push(widget::text::body(format!("{}: {error}", self.config_store.path().display())))
            .push(widget::text::body("The file has not been replaced. Configuration edits, saves, and hotkeys are blocked until recovery."))
            .push(widget::row::with_capacity(3)
                .push(widget::button::standard("Retry loading").on_press(Message::RetryConfig))
                .push(widget::button::standard("Recover backup").on_press(Message::RecoverConfigBackup))
                .push(widget::button::standard("Reset to defaults...").on_press(Message::RequestResetConfig))
                .spacing(8))
            .spacing(8);
        if self.pending_config_reset {
            content = content
                .push(widget::text::body("Discard the configuration file and write defaults? This cannot be undone. Any existing backup will be kept."))
                .push(widget::row::with_capacity(2)
                    .push(widget::button::destructive("Confirm reset").on_press(Message::ConfirmResetConfig))
                    .push(widget::button::standard("Cancel").on_press(Message::CancelResetConfig))
                    .spacing(8));
        }
        widget::container(content)
            .padding(12)
            .width(Length::Fill)
            .into()
    }
}
