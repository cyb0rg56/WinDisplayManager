use super::Message;
use cosmic::Element;
use cosmic::iced::widget::{center, container, mouse_area, opaque};
use cosmic::iced::{Color, Length};
use cosmic::widget;
use std::borrow::Cow;

/// Centered confirmation over a dimmed scrim.
///
/// The shell already places this above the window, so the returned element is
/// only the scrim layer: clicks on the dimmed area cancel, and the card is
/// opaque so its own clicks do not.
pub(super) fn confirm_dialog<'a>(
    title: impl Into<Cow<'a, str>>,
    body: impl Into<Cow<'a, str>>,
    confirm_label: impl Into<Cow<'a, str>>,
    on_cancel: Message,
    on_confirm: Message,
) -> Element<'a, Message> {
    let card: Element<'a, Message> = widget::dialog()
        .title(title)
        .body(body)
        .width(Length::Fixed(420.0))
        .secondary_action(widget::button::standard("Cancel").on_press(on_cancel.clone()))
        .primary_action(widget::button::destructive(confirm_label).on_press(on_confirm))
        .into();
    let scrim = center(opaque(card))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(
                Color {
                    a: 0.8,
                    ..Color::BLACK
                }
                .into(),
            ),
            ..container::Style::default()
        });
    opaque(mouse_area(scrim).on_press(on_cancel))
}
