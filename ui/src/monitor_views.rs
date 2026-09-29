use crate::{AppModel, Message};
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced::alignment::Horizontal;
use cosmic::widget;
use data::ddc::input_choices;
use data::presentation::{input_presentation, scalar_presentation};
use data::state::Input;

impl AppModel {
    /// View for a single monitor page.
    pub(super) fn view_monitor(&self, monitor_id: u32) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let monitor = self.state.monitor(monitor_id);

        match monitor {
            None => {
                let content: Element<'_, Message> =
                    if let Some(error) = self.state.monitor_load_error(monitor_id) {
                        widget::column::with_capacity(3)
                            .push(widget::text::title4(format!(
                                "Monitor {monitor_id} unavailable"
                            )))
                            .push(widget::text::body(error))
                            .push(
                                widget::button::standard("Retry")
                                    .on_press(Message::Data(Input::RetryMonitor(monitor_id))),
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

                let pending = self.state.monitor_pending(&mon.info);
                let input = input_presentation(
                    mon.input_source,
                    mon.input_source_read_error.as_deref(),
                    pending.input_source,
                );
                let mid = mon.info.id;
                let inputs = input_choices(mon.advertised_inputs.as_deref(), input.value);
                let sources = inputs.options;
                let labels: Vec<String> = sources.iter().map(ToString::to_string).collect();
                let selected_idx = input
                    .value
                    .as_ref()
                    .and_then(|source| sources.iter().position(|candidate| candidate == source));
                let control: Element<'_, Message> = if sources.is_empty() {
                    widget::text::body("No input sources advertised").into()
                } else {
                    widget::dropdown(labels, selected_idx, move |idx| {
                        Message::Data(Input::SelectInputSource(mid, sources[idx]))
                    })
                    .into()
                };
                let mut section = cosmic::widget::settings::section()
                    .title("Input Source")
                    .add(cosmic::widget::settings::item::builder(input.label).control(control));
                if let Some(error) = input.read_error {
                    section = section.add(widget::text::body(format!(
                        "Could not read input source: {error}"
                    )));
                }
                let input_section = section;

                let mut content = widget::column::with_capacity(6)
                    .push(header)
                    .push(resolution_label)
                    .spacing(space_s)
                    .width(Length::Fill);
                content = content.push(input_section);
                let brightness = scalar_presentation(
                    mon.brightness,
                    mon.brightness_max,
                    mon.brightness_read_error.as_deref(),
                    pending.brightness,
                );
                let brightness_control: Element<'_, Message> = match brightness.value {
                    Some(value) => widget::slider(
                        0.0..=f64::from(mon.brightness_max),
                        f64::from(value),
                        move |v| Message::Data(Input::BrightnessSliderChanged(mid, v as u16)),
                    )
                    .width(Length::Fixed(300.0))
                    .into(),
                    None => widget::text::body("Refresh to adjust").into(),
                };
                let mut brightness_section =
                    cosmic::widget::settings::section().title("Brightness").add(
                        cosmic::widget::settings::item::builder(brightness.label)
                            .control(brightness_control),
                    );
                if let Some(error) = brightness.read_error {
                    brightness_section = brightness_section.add(widget::text::body(format!(
                        "Could not read brightness: {error}"
                    )));
                }
                content = content.push(brightness_section);
                let contrast = scalar_presentation(
                    mon.contrast,
                    mon.contrast_max,
                    mon.contrast_read_error.as_deref(),
                    pending.contrast,
                );
                let contrast_control: Element<'_, Message> = match contrast.value {
                    Some(value) => widget::slider(
                        0.0..=f64::from(mon.contrast_max),
                        f64::from(value),
                        move |v| Message::Data(Input::ContrastSliderChanged(mid, v as u16)),
                    )
                    .width(Length::Fixed(300.0))
                    .into(),
                    None => widget::text::body("Refresh to adjust").into(),
                };
                let mut contrast_section =
                    cosmic::widget::settings::section().title("Contrast").add(
                        cosmic::widget::settings::item::builder(contrast.label)
                            .control(contrast_control),
                    );
                if let Some(error) = contrast.read_error {
                    contrast_section = contrast_section.add(widget::text::body(format!(
                        "Could not read contrast: {error}"
                    )));
                }
                content = content.push(contrast_section);
                if let Some(error) = self.state.monitor_load_error(monitor_id) {
                    content = content.push(widget::text::body(format!(
                        "Could not refresh monitor: {error}"
                    )));
                }
                widget::scrollable(content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        }
    }
}
