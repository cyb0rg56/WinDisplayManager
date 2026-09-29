use crate::icons::{self, AppIcon};
use crate::{AppModel, Message};
use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::theme;
use cosmic::widget;
use data::state::Input;

impl AppModel {
    pub(super) fn view_profiles(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let name_input = self.state.profile_name_input();
        let save_row = widget::row::with_capacity(2)
            .push(
                widget::text_input("New profile name", name_input)
                    .on_input(|value| Message::Data(Input::ProfileNameInput(value)))
                    .on_submit(|value| Message::Data(Input::SaveCurrentProfile(value)))
                    .width(Length::Fill),
            )
            .push(
                widget::button::suggested("Save Current Layout").on_press(Message::Data(
                    Input::SaveCurrentProfile(name_input.to_string()),
                )),
            )
            .spacing(space_s)
            .align_y(Alignment::Center);
        let mut save = widget::column::with_capacity(2)
            .push(save_row)
            .spacing(space_s);
        if let Some(name) = self.state.pending_profile_replace() {
            save = save.push(
                widget::row::with_capacity(3)
                    .push(
                        widget::text::body(format!("Replace existing profile '{name}'?"))
                            .width(Length::Fill),
                    )
                    .push(
                        widget::button::standard("Cancel")
                            .on_press(Message::Data(Input::CancelReplaceProfile)),
                    )
                    .push(
                        widget::button::destructive("Replace")
                            .on_press(Message::Data(Input::ConfirmReplaceProfile)),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            );
        }
        let saved = self.state.profiles();
        let mut profiles = widget::column::with_capacity(saved.len() + 1).spacing(space_s);
        if saved.is_empty() {
            profiles = profiles.push(widget::text::body("No profiles saved yet."));
        }
        for name in saved {
            let row = widget::row::with_capacity(4)
                .push(widget::text::body(name.clone()).width(Length::Fill))
                .push(
                    widget::button::suggested("Apply")
                        .on_press(Message::Data(Input::ApplyProfile(name.clone()))),
                )
                .push(
                    widget::button::standard("Hotkey")
                        .on_press(Message::Data(Input::AddProfileHotkey(name.clone()))),
                )
                .push(icons::icon_button(
                    AppIcon::Trash,
                    "Delete",
                    theme::Button::Destructive,
                    Message::Data(Input::RequestDeleteProfile(name.clone())),
                ))
                .spacing(space_s)
                .align_y(Alignment::Center);
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
