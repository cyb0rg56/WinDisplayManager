use super::{AppModel, INPUT_SOURCES, Message, POWER_MODES};
use crate::config::TurnOffBehavior;
use crate::ddc::{InputSource, PowerMode};
use cosmic::Element;
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Length};
use cosmic::widget;

pub(super) fn input_source_index(source: &InputSource) -> Option<usize> {
    INPUT_SOURCES.iter().position(|s| s == source)
}

pub(super) fn power_mode_index(mode: &PowerMode) -> Option<usize> {
    POWER_MODES.iter().position(|m| m == mode)
}

impl AppModel {
    /// View for a single monitor page.
    pub(super) fn view_monitor(&self, monitor_id: u32) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let monitor = self.monitors.iter().find(|m| m.info.id == monitor_id);

        match monitor {
            None => {
                let content: Element<'_, Message> =
                    if let Some(error) = self.monitor_load_errors.get(&monitor_id) {
                        widget::column::with_capacity(3)
                            .push(widget::text::title4(format!(
                                "Monitor {monitor_id} unavailable"
                            )))
                            .push(widget::text::body(error))
                            .push(
                                widget::button::standard("Retry")
                                    .on_press(Message::RetryMonitor(monitor_id)),
                            )
                            .spacing(space_s)
                            .into()
                    } else {
                        widget::text::body("Loading monitor data...").into()
                    };
                widget::container(content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Center)
                    .into()
            }

            Some(mon) => {
                // Header
                let header_label = if mon.info.name.is_empty() {
                    format!("Monitor {}", mon.info.id)
                } else {
                    mon.info.name.clone()
                };
                let header = widget::text::title3(header_label);

                let resolution_text = format!(
                    "{}x{} at ({}, {}){}",
                    mon.info.width,
                    mon.info.height,
                    mon.info.x,
                    mon.info.y,
                    if mon.info.is_primary {
                        " [Primary]"
                    } else {
                        ""
                    }
                );
                let resolution_label = widget::text::caption(resolution_text);

                // Input source dropdown
                let selected_idx = input_source_index(&mon.input_source);
                let mid = mon.info.id;

                // Create labels as owned data
                static INPUT_SOURCE_LABELS: &[&str] = &[
                    "HDMI 1",
                    "HDMI 2",
                    "DisplayPort 1",
                    "DisplayPort 2",
                    "USB-C 1",
                    "USB-C 2",
                    "VGA 1",
                    "VGA 2",
                    "DVI 1",
                    "DVI 2",
                ];

                let input_section = cosmic::widget::settings::section()
                    .title("Input Source")
                    .add(
                        cosmic::widget::settings::item::builder("Active input").control(
                            widget::dropdown(INPUT_SOURCE_LABELS, selected_idx, move |idx| {
                                Message::SelectInputSource(mid, idx)
                            }),
                        ),
                    );

                // Brightness slider
                let brightness_val = mon.brightness as f64;
                let brightness_max = mon.brightness_max.max(1) as f64;
                let mid_b = mon.info.id;
                let brightness_section =
                    cosmic::widget::settings::section().title("Brightness").add(
                        cosmic::widget::settings::item::builder(format!(
                            "{} / {}",
                            mon.brightness, mon.brightness_max
                        ))
                        .control(
                            widget::slider(0.0..=brightness_max, brightness_val, move |v| {
                                Message::BrightnessSliderChanged(mid_b, v as u16)
                            })
                            .width(Length::Fixed(300.0)),
                        ),
                    );

                // Contrast slider
                let contrast_val = mon.contrast as f64;
                let contrast_max = mon.contrast_max.max(1) as f64;
                let mid_c = mon.info.id;
                let contrast_section = cosmic::widget::settings::section().title("Contrast").add(
                    cosmic::widget::settings::item::builder(format!(
                        "{} / {}",
                        mon.contrast, mon.contrast_max
                    ))
                    .control(
                        widget::slider(0.0..=contrast_max, contrast_val, move |v| {
                            Message::ContrastSliderChanged(mid_c, v as u16)
                        })
                        .width(Length::Fixed(300.0)),
                    ),
                );

                widget::column::with_capacity(6)
                    .push(header)
                    .push(resolution_label)
                    .push(input_section)
                    .push(brightness_section)
                    .push(contrast_section)
                    .spacing(space_s)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        }
    }

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

    pub(super) fn view_about(&self) -> Element<'_, Message> {
        widget::scrollable(cosmic::widget::about(&self.about, |url| {
            Message::OpenUrl(url.to_owned())
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// View for the settings page (step sizes and other config).
    pub(super) fn view_settings(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let header = widget::text::title3("Settings");
        let description =
            widget::text::body("Configure step sizes and other application settings.");

        // --- Hotkeys enabled toggle ---
        let hotkeys_section = cosmic::widget::settings::section().title("Hotkeys").add(
            cosmic::widget::settings::item::builder("Enable global hotkeys")
                .description("When disabled, hotkeys will not trigger any actions")
                .control(
                    widget::toggler(self.config.hotkeys_enabled).on_toggle(Message::ToggleHotkeys),
                ),
        );

        // --- Step size ---
        let step_section = cosmic::widget::settings::section()
            .title("Step Sizes")
            .add(
                cosmic::widget::settings::item::builder(format!(
                    "Brightness step: {}",
                    self.config.hotkeys.brightness_step
                ))
                .control(widget::text::body("")),
            )
            .add(
                cosmic::widget::settings::item::builder(format!(
                    "Contrast step: {}",
                    self.config.hotkeys.contrast_step
                ))
                .control(widget::text::body("")),
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

        let content = widget::column::with_capacity(5)
            .push(header)
            .push(description)
            .push(hotkeys_section)
            .push(step_section)
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
