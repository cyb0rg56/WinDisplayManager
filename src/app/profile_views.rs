use super::{AppModel, Message};
use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget;

impl AppModel {
    pub(super) fn view_profiles(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let save_row = widget::row::with_capacity(2)
            .push(
                widget::text_input("New profile name", &self.profile_name_input)
                    .on_input(Message::ProfileNameInput)
                    .on_submit(Message::SaveCurrentProfile)
                    .width(Length::Fill),
            )
            .push(
                widget::button::suggested("Save Current Layout")
                    .on_press(Message::SaveCurrentProfile(self.profile_name_input.clone())),
            )
            .spacing(space_s)
            .align_y(Alignment::Center);
        let mut save = widget::column::with_capacity(2)
            .push(save_row)
            .spacing(space_s);
        if let Some(name) = &self.pending_profile_replace {
            save = save.push(
                widget::row::with_capacity(3)
                    .push(
                        widget::text::body(format!("Replace existing profile '{name}'?"))
                            .width(Length::Fill),
                    )
                    .push(
                        widget::button::standard("Cancel").on_press(Message::CancelReplaceProfile),
                    )
                    .push(
                        widget::button::destructive("Replace")
                            .on_press(Message::ConfirmReplaceProfile),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            );
        }
        let mut profiles = widget::column::with_capacity(self.profiles.len() + 1).spacing(space_s);
        if self.profiles.is_empty() {
            profiles = profiles.push(widget::text::body("No profiles saved yet."));
        }
        for name in &self.profiles {
            let deleting = self.pending_profile_delete.as_deref() == Some(name.as_str());
            let mut row = widget::row::with_capacity(5)
                .push(widget::text::body(name.clone()).width(Length::Fill))
                .push(
                    widget::button::suggested("Apply")
                        .on_press(Message::ApplyProfile(name.clone())),
                )
                .push(
                    widget::button::standard("Hotkey")
                        .on_press(Message::AddProfileHotkey(name.clone())),
                )
                .spacing(space_s)
                .align_y(Alignment::Center);
            if deleting {
                row = row
                    .push(widget::button::standard("Cancel").on_press(Message::CancelDeleteProfile))
                    .push(
                        widget::button::destructive("Confirm Delete")
                            .on_press(Message::DeleteProfile(name.clone())),
                    );
            } else {
                row = row.push(
                    widget::button::destructive("Delete")
                        .on_press(Message::RequestDeleteProfile(name.clone())),
                );
            }
            profiles = profiles.push(row);
        }
        let content = widget::column::with_capacity(4)
            .push(widget::text::title3("Monitor Layout Profiles"))
            .push(widget::text::body(
                "Save and restore complete Windows display layouts.",
            ))
            .push(
                cosmic::widget::settings::section()
                    .title("Save Current Layout")
                    .add(save),
            )
            .push(
                cosmic::widget::settings::section()
                    .title("Saved Profiles")
                    .add(profiles),
            )
            .spacing(space_s)
            .width(Length::Fill);
        widget::scrollable(
            widget::container(content)
                .width(Length::Fill)
                .max_width(700.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
