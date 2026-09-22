//! Theme-colored icon buttons backed by bundled symbolic SVGs.

use std::sync::OnceLock;

use cosmic::Element;
use cosmic::widget::{self, icon};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppIcon {
    Edit,
    Collapse,
    Trash,
    Refresh,
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
    widget::button::icon(handle(icon))
        .class(class)
        .tooltip(label)
        .on_press(message)
        .into()
}

fn handle(app_icon: AppIcon) -> icon::Handle {
    match app_icon {
        AppIcon::Edit => cached(
            &EDIT,
            include_bytes!("../resources/icons/edit-symbolic.svg"),
        ),
        AppIcon::Collapse => cached(
            &COLLAPSE,
            include_bytes!("../resources/icons/collapse-symbolic.svg"),
        ),
        AppIcon::Trash => cached(
            &TRASH,
            include_bytes!("../resources/icons/trash-symbolic.svg"),
        ),
        AppIcon::Refresh => cached(
            &REFRESH,
            include_bytes!("../resources/icons/refresh-symbolic.svg"),
        ),
    }
}

fn cached(slot: &'static OnceLock<icon::Handle>, bytes: &'static [u8]) -> icon::Handle {
    slot.get_or_init(|| icon::from_svg_bytes(bytes).symbolic(true))
        .clone()
}

static EDIT: OnceLock<icon::Handle> = OnceLock::new();
static COLLAPSE: OnceLock<icon::Handle> = OnceLock::new();
static TRASH: OnceLock<icon::Handle> = OnceLock::new();
static REFRESH: OnceLock<icon::Handle> = OnceLock::new();
