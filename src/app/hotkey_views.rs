use super::hotkey_editor::{
    action_master_selected, action_monitor_selected, parse_value_draft, parse_vcp_draft,
};
use super::{AppModel, Message, RecordingState};
use crate::config::{
    ActionTarget, ActionType, Hotkey, HotkeyActionSpec, HotkeyHeading, MonitorTarget,
    hotkey_headings,
};
use crate::ddc::{
    FeatureOptions, InputSource, MonitorInfo, NO_SHARED_OPTION_NOTE, input_choices, power_options,
    shared_input_choices,
};
use crate::icons::{self, AppIcon};
use cosmic::Element;
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Length};
use cosmic::theme;
use cosmic::widget;

fn option_picker<T>(
    choices: FeatureOptions<T>,
    selected: T,
    on_select: impl Fn(T) -> Message + Send + Sync + 'static,
) -> Element<'static, Message>
where
    T: Clone + PartialEq + std::fmt::Display + Send + Sync + 'static,
{
    let mut column = widget::column::with_capacity(3).spacing(4.0);
    let options = choices.options;
    if choices.no_shared_option {
        column = column.push(widget::text::caption(NO_SHARED_OPTION_NOTE));
    }
    if options.is_empty() {
        return column.into();
    }
    let selected_index = options.iter().position(|option| option == &selected);
    let labels: Vec<String> = options.iter().map(ToString::to_string).collect();
    column
        .push(widget::dropdown(labels, selected_index, move |index| {
            on_select(options[index].clone())
        }))
        .into()
}

impl AppModel {
    fn targeted_advertised_inputs(
        &self,
        action: &HotkeyActionSpec,
    ) -> Vec<Option<Vec<InputSource>>> {
        let infos: Vec<MonitorInfo> = if action.all_monitors {
            self.detected_monitors.clone()
        } else {
            action
                .explicit_targets()
                .iter()
                .filter_map(|target| target.resolve(&self.detected_monitors).ok().cloned())
                .collect()
        };
        infos
            .iter()
            .filter_map(|info| {
                self.monitors
                    .iter()
                    .find(|monitor| monitor.info.key == info.key)
                    .map(|monitor| monitor.advertised_inputs.clone())
            })
            .collect()
    }

    pub(super) fn view_hotkeys_current(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let mut hotkeys = widget::column::with_capacity(self.config.hotkeys.hotkeys.len() + 1)
            .spacing(space_s)
            .width(Length::Fill);

        let headings = hotkey_headings(&self.config.hotkeys.hotkeys);
        for (hotkey, heading) in self.config.hotkeys.hotkeys.iter().zip(headings) {
            hotkeys = hotkeys.push(self.view_hotkey_card(hotkey, heading, space_s));
        }

        let add_row = widget::row::with_capacity(2)
            .push(widget::button::suggested("Add Hotkey").on_press(Message::AddHotkey))
            .push(widget::Space::new().width(Length::Fill))
            .spacing(space_s);

        let content = widget::column::with_capacity(5)
            .push(widget::text::title3("Hotkeys"))
            .push(widget::text::body(
                "Configure global hotkeys and ordered display actions.",
            ))
            .push(add_row)
            .push(hotkeys)
            .spacing(space_s)
            .width(Length::Fill);

        widget::scrollable(
            widget::container(content)
                .width(Length::Fill)
                .max_width(760.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_hotkey_card<'a>(
        &'a self,
        hotkey: &'a Hotkey,
        heading: HotkeyHeading,
        space_s: u16,
    ) -> Element<'a, Message> {
        let id = hotkey.id.clone();
        let expanded = self.expanded_hotkey.as_deref() == Some(id.as_str());
        let active = self.hotkey_status.get(&id).copied().unwrap_or(false);
        let status = if hotkey.binding.key.is_empty() {
            "Unbound"
        } else if active {
            "Active"
        } else {
            "Inactive"
        };
        let summary = widget::row::with_capacity(5)
            .push(widget::text::body(hotkey.binding.to_string()).width(Length::Fill))
            .push(widget::text::caption(status))
            .push(icons::icon_button(
                if expanded {
                    AppIcon::Collapse
                } else {
                    AppIcon::Edit
                },
                if expanded { "Collapse" } else { "Edit" },
                theme::Button::Standard,
                Message::ToggleHotkeyEditor(id.clone()),
            ))
            .push(icons::icon_button(
                AppIcon::Trash,
                "Delete",
                theme::Button::Destructive,
                Message::RequestDeleteHotkey(id.clone()),
            ))
            .spacing(space_s)
            .align_y(Alignment::Center);

        let mut content = widget::column::with_capacity(4)
            .push(summary)
            .spacing(space_s);
        if expanded {
            let recording_here = matches!(
                &self.recording_state,
                RecordingState::Recording { hotkey_id, .. } if hotkey_id == &id
            );
            let binding_row = if recording_here {
                widget::row::with_capacity(2)
                    .push(widget::text::body("Press a key combination...").width(Length::Fill))
                    .push(widget::button::standard("Cancel").on_press(Message::CancelRecording))
            } else {
                widget::row::with_capacity(3)
                    .push(
                        widget::button::standard("Record")
                            .on_press(Message::StartRecording(id.clone())),
                    )
                    .push(
                        widget::button::standard("Clear")
                            .on_press(Message::ClearBinding(id.clone())),
                    )
                    .push(widget::Space::new().width(Length::Fill))
            }
            .spacing(space_s)
            .align_y(Alignment::Center);
            let label_row = widget::row::with_capacity(2)
                .push(widget::text::body("Label").width(Length::Fixed(112.0)))
                .push(
                    widget::text_input(heading.fallback, &hotkey.label)
                        .on_input({
                            let id = id.clone();
                            move |value| Message::SetHotkeyLabel(id.clone(), value)
                        })
                        .width(Length::Fill),
                )
                .spacing(space_s)
                .align_y(Alignment::Center);
            content = content.push(label_row);
            content = content.push(binding_row);

            for (idx, action) in hotkey.actions.iter().enumerate() {
                content = content.push(self.view_action_editor(&id, idx, action, space_s));
            }
            content = content.push(
                widget::button::standard("Add Action").on_press(Message::AddAction(id.clone())),
            );
        }

        cosmic::widget::settings::section()
            .title(heading.title)
            .add(content)
            .into()
    }

    fn view_action_editor(
        &self,
        hotkey_id: &str,
        idx: usize,
        action: &HotkeyActionSpec,
        space_s: u16,
    ) -> Element<'_, Message> {
        let id = hotkey_id.to_string();
        let type_options = if action.target.supports_offset() {
            ActionType::ALL
        } else {
            ActionType::NO_OFFSET
        };
        let type_labels: Vec<&str> = type_options.iter().map(|value| value.label()).collect();
        let selected_type = type_options
            .iter()
            .position(|value| value == &action.action_type);
        let action_row = widget::row::with_capacity(3)
            .push(widget::dropdown(type_labels, selected_type, {
                let id = id.clone();
                move |selected| Message::SetActionType(id.clone(), idx, type_options[selected])
            }))
            .push(widget::Space::new().width(Length::Fill))
            .push(icons::icon_button(
                AppIcon::Trash,
                "Delete Action",
                theme::Button::Destructive,
                Message::DeleteAction(id.clone(), idx),
            ))
            .spacing(space_s)
            .align_y(Alignment::Center);
        let mut fields = widget::column::with_capacity(5)
            .push(
                widget::row::with_capacity(2)
                    .push(widget::text::body("Actions").width(Length::Fixed(112.0)))
                    .push(
                        widget::container(action_row)
                            .width(Length::Fill)
                            .align_x(Horizontal::Left),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            )
            .spacing(space_s);

        if action.action_type != ActionType::Off {
            let targets: Vec<&str> = ActionTarget::ALL
                .iter()
                .map(|value| value.label())
                .collect();
            let selected = ActionTarget::ALL
                .iter()
                .position(|value| value == &action.target);
            fields = fields.push(
                widget::row::with_capacity(2)
                    .push(widget::text::body("Commands").width(Length::Fixed(112.0)))
                    .push(
                        widget::container(widget::dropdown(targets, selected, {
                            let id = id.clone();
                            move |selected| {
                                Message::SetActionTarget(
                                    id.clone(),
                                    idx,
                                    ActionTarget::ALL[selected],
                                )
                            }
                        }))
                        .width(Length::Fill)
                        .align_x(Horizontal::Left),
                    )
                    .spacing(space_s)
                    .align_y(Alignment::Center),
            );

            if action.target == ActionTarget::CustomVcp {
                let draft = self
                    .vcp_drafts
                    .get(&(id.clone(), idx))
                    .map(String::as_str)
                    .unwrap_or("");
                let mut code = widget::column::with_capacity(2).push(
                    widget::text_input("0x10", draft)
                        .on_input({
                            let id = id.clone();
                            move |value| Message::ActionVcpDraftChanged(id.clone(), idx, value)
                        })
                        .width(Length::Fixed(96.0)),
                );
                if let Err(error) = parse_vcp_draft(draft) {
                    code = code.push(widget::text::caption(error));
                }
                fields = fields.push(
                    widget::row::with_capacity(2)
                        .push(widget::text::body("VCP code").width(Length::Fixed(112.0)))
                        .push(
                            widget::container(code)
                                .width(Length::Fill)
                                .align_x(Horizontal::Left),
                        )
                        .spacing(space_s)
                        .align_y(Alignment::Start),
                );
            }

            let value_control: Option<Element<'_, Message>> = match action.target {
                ActionTarget::Brightness | ActionTarget::Contrast | ActionTarget::CustomVcp => {
                    let draft = self
                        .value_drafts
                        .get(&(id.clone(), idx))
                        .map(String::as_str)
                        .unwrap_or("");
                    let mut input = widget::column::with_capacity(2).push(
                        widget::text_input("0", draft)
                            .on_input({
                                let id = id.clone();
                                move |value| {
                                    Message::ActionValueDraftChanged(id.clone(), idx, value)
                                }
                            })
                            .width(Length::Fixed(96.0)),
                    );
                    if let Err(error) = parse_value_draft(draft) {
                        input = input.push(widget::text::caption(error));
                    }
                    Some(input.into())
                }
                ActionTarget::InputSource if action.all_monitors => {
                    let choices = shared_input_choices(
                        &self.targeted_advertised_inputs(action),
                        Some(action.input_source),
                    );
                    let id = id.clone();
                    Some(option_picker(choices, action.input_source, move |source| {
                        Message::SetActionInputSource(id.clone(), idx, source)
                    }))
                }
                ActionTarget::PowerMode => {
                    let choices = power_options(Some(action.power_mode));
                    let id = id.clone();
                    Some(option_picker(choices, action.power_mode, move |mode| {
                        Message::SetActionPowerMode(id.clone(), idx, mode)
                    }))
                }
                ActionTarget::Profile => {
                    let mut profiles = self.profiles.clone();
                    if !action.profile_name.is_empty() && !profiles.contains(&action.profile_name) {
                        profiles.push(action.profile_name.clone());
                    }
                    if profiles.is_empty() {
                        Some(widget::text::body("No profiles saved yet.").into())
                    } else {
                        let selected = profiles
                            .iter()
                            .position(|value| value == &action.profile_name);
                        Some(
                            widget::dropdown(profiles.clone(), selected, {
                                let id = id.clone();
                                move |selected| {
                                    Message::SetActionProfile(
                                        id.clone(),
                                        idx,
                                        profiles[selected].clone(),
                                    )
                                }
                            })
                            .into(),
                        )
                    }
                }
                ActionTarget::InputSource => None,
            };
            if let Some(control) = value_control {
                fields = fields.push(
                    widget::row::with_capacity(2)
                        .push(widget::text::body("Value").width(Length::Fixed(112.0)))
                        .push(
                            widget::container(control)
                                .width(Length::Fill)
                                .align_x(Horizontal::Left),
                        )
                        .spacing(space_s)
                        .align_y(Alignment::Center),
                );
            }
        }

        let mut display_ids: Vec<MonitorTarget> = self
            .detected_monitors
            .iter()
            .map(|monitor| MonitorTarget::Stable(monitor.key.clone()))
            .collect();
        for monitor_id in action.monitors.iter().cloned().chain(
            action
                .monitor_inputs
                .iter()
                .map(|input| input.monitor_id.clone()),
        ) {
            if !display_ids.contains(&monitor_id) {
                display_ids.push(monitor_id);
            }
        }

        let mut displays = widget::column::with_capacity(display_ids.len() + 2).spacing(space_s);
        if action.target == ActionTarget::Profile && action.action_type != ActionType::Off {
            displays = displays.push(widget::text::body("Entire saved display layout"));
        } else {
            let master_selected = action_master_selected(action, &display_ids);
            displays = displays.push(
                widget::row::with_capacity(3)
                    .push(widget::text::body("All displays").width(Length::Fill))
                    .push(widget::Space::new().width(Length::Fixed(space_s as f32)))
                    .push(widget::toggler(master_selected).on_toggle({
                        let id = id.clone();
                        move |checked| Message::ToggleActionAllMonitors(id.clone(), idx, checked)
                    }))
                    .align_y(Alignment::Center),
            );

            for monitor_id in display_ids {
                let available = monitor_id.resolve(&self.detected_monitors).ok();
                let label = available
                    .map(|monitor| format!("Monitor {}: {}", monitor.id, monitor.name))
                    .unwrap_or_else(|| format!("{monitor_id} (unresolved)"));
                let selected = action_monitor_selected(action, &monitor_id);
                if available.is_some() {
                    displays = displays.push(
                        widget::row::with_capacity(2)
                            .push(widget::text::body(label).width(Length::Fill))
                            .push(widget::toggler(selected).on_toggle({
                                let id = id.clone();
                                let target = monitor_id.clone();
                                move |checked| {
                                    Message::ToggleActionMonitor(
                                        id.clone(),
                                        idx,
                                        target.clone(),
                                        checked,
                                    )
                                }
                            }))
                            .align_y(Alignment::Center),
                    );
                } else {
                    let labels: Vec<String> = self
                        .detected_monitors
                        .iter()
                        .map(|m| format!("Rebind to Monitor {}: {}", m.id, m.name))
                        .collect();
                    let keys: Vec<_> = self
                        .detected_monitors
                        .iter()
                        .map(|m| m.key.clone())
                        .collect();
                    let mut row = widget::row::with_capacity(3)
                        .push(widget::text::body(label).width(Length::Fill))
                        .push(widget::button::standard("Remove").on_press(
                            Message::RemoveMonitorTarget(id.clone(), idx, monitor_id.clone()),
                        ))
                        .spacing(space_s);
                    if !keys.is_empty() {
                        row = row.push(widget::dropdown(labels, None, {
                            let id = id.clone();
                            let old = monitor_id.clone();
                            move |selected| {
                                Message::RebindMonitorTarget(
                                    id.clone(),
                                    idx,
                                    old.clone(),
                                    keys[selected].clone(),
                                )
                            }
                        }));
                    }
                    displays = displays.push(row);
                }

                if action.action_type != ActionType::Off
                    && action.target == ActionTarget::InputSource
                    && !action.all_monitors
                    && selected
                {
                    let current = action
                        .monitor_inputs
                        .iter()
                        .find(|input| input.monitor_id == monitor_id)
                        .map(|input| input.input_source)
                        .unwrap_or(action.input_source);
                    let key = available.map(|info| info.key.clone());
                    let monitor = key.and_then(|key| {
                        self.monitors.iter().find(|monitor| monitor.info.key == key)
                    });
                    let choices = input_choices(
                        monitor.and_then(|monitor| monitor.advertised_inputs.as_deref()),
                        Some(current),
                    );
                    displays = displays.push(
                        widget::row::with_capacity(2)
                            .push(widget::text::caption("Input source").width(Length::Fill))
                            .push(option_picker(choices, current, {
                                let id = id.clone();
                                let target = monitor_id.clone();
                                move |source| {
                                    Message::SetMonitorInput(
                                        id.clone(),
                                        idx,
                                        target.clone(),
                                        Some(source),
                                    )
                                }
                            }))
                            .spacing(space_s)
                            .align_y(Alignment::Center),
                    );
                }
            }
        }
        fields = fields.push(
            cosmic::widget::settings::section()
                .title("Displays")
                .add(displays),
        );

        widget::container(fields)
            .class(cosmic::theme::Container::Card)
            .padding(space_s)
            .width(Length::Fill)
            .into()
    }
}
