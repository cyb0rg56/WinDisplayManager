//! Theme-colored icon buttons from the symbolic icons bundled with libcosmic.

use std::sync::OnceLock;

use cosmic::Element;
use cosmic::widget::{self, icon};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppIcon {
    Edit,
    Collapse,
    Trash,
    Refresh,
    Save,
    Settings,
}

pub fn icon_button<'a, Message>(
    icon: AppIcon,
    label: &'a str,
    class: cosmic::theme::Button,
    message: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'static,
{
    with_tooltip(
        widget::button::icon(handle(icon))
            .class(class)
            .on_press(message),
        label,
        widget::tooltip::Position::Bottom,
    )
}

/// Header control matching the sidebar toggle: no fill until hover, or while `emphasized`.
pub fn header_icon_button<'a, Message>(
    icon: AppIcon,
    label: &'a str,
    emphasized: bool,
    message: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'static,
{
    let class = if emphasized {
        emphasized_header_class()
    } else {
        cosmic::theme::Button::NavToggle
    };
    with_tooltip(
        widget::button::icon(handle(icon))
            .padding([8, 16])
            .class(class)
            .on_press(message),
        label,
        widget::tooltip::Position::Bottom,
    )
}

fn emphasized_header_class() -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(|focused, theme| header_style(theme, focused, false)),
        hovered: Box::new(|focused, theme| header_style(theme, focused, true)),
        pressed: Box::new(|focused, theme| header_style(theme, focused, true)),
        disabled: Box::new(|theme| {
            let mut style = header_style(theme, false, false);
            if let Some(cosmic::iced::Background::Color(color)) = &mut style.background {
                color.a *= 0.5;
            }
            style
        }),
    }
}

fn header_style(
    theme: &cosmic::Theme,
    focused: bool,
    pressed: bool,
) -> cosmic::widget::button::Style {
    let cosmic = theme.cosmic();
    let component = &cosmic.icon_button;
    let fill = if pressed {
        component.pressed
    } else {
        component.hover
    };
    let mut style = cosmic::widget::button::Style::new();
    style.background = Some(cosmic::iced::Background::Color(fill.into()));
    style.border_radius = cosmic.corner_radii.radius_s.into();
    if focused {
        style.outline_width = 1.0;
        style.outline_color = cosmic.accent.base.into();
        style.border_width = 2.0;
        style.border_color = cosmic::iced::Color::TRANSPARENT;
    }
    style
}

fn with_tooltip<'a, Message>(
    button: impl Into<Element<'a, Message>>,
    label: &'a str,
    position: widget::tooltip::Position,
) -> Element<'a, Message>
where
    Message: Clone + 'static,
{
    widget::tooltip(button, widget::text::body(label), position).into()
}

fn handle(app_icon: AppIcon) -> icon::Handle {
    let (slot, name) = match app_icon {
        AppIcon::Edit => (&EDIT, "edit-symbolic"),
        AppIcon::Collapse => (&COLLAPSE, "pan-up-symbolic"),
        AppIcon::Trash => (&TRASH, "edit-delete-symbolic"),
        AppIcon::Refresh => (&REFRESH, "view-refresh-symbolic"),
        AppIcon::Save => (&SAVE, "media-floppy-symbolic"),
        AppIcon::Settings => (&SETTINGS, "preferences-system-symbolic"),
    };
    slot.get_or_init(|| icon::from_name(name).into()).clone()
}

static EDIT: OnceLock<icon::Handle> = OnceLock::new();
static COLLAPSE: OnceLock<icon::Handle> = OnceLock::new();
static TRASH: OnceLock<icon::Handle> = OnceLock::new();
static REFRESH: OnceLock<icon::Handle> = OnceLock::new();
static SAVE: OnceLock<icon::Handle> = OnceLock::new();
static SETTINGS: OnceLock<icon::Handle> = OnceLock::new();
