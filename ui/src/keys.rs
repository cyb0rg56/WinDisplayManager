use cosmic::iced::keyboard::key::Named;
use cosmic::iced::keyboard::{Key, Modifiers};
use data::config::HotkeyBinding;

/// Binding for a pressed chord; `key` is empty when the key is unsupported.
pub(crate) fn binding(modifiers: Modifiers, key: &Key) -> HotkeyBinding {
    HotkeyBinding {
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
        shift: modifiers.shift(),
        win: modifiers.logo(),
        key: key_to_string(key),
    }
}

/// Convert an Iced Key to our internal string format
fn key_to_string(key: &Key) -> String {
    match key {
        Key::Named(named_key) => match named_key {
            Named::F1 => "F1",
            Named::F2 => "F2",
            Named::F3 => "F3",
            Named::F4 => "F4",
            Named::F5 => "F5",
            Named::F6 => "F6",
            Named::F7 => "F7",
            Named::F8 => "F8",
            Named::F9 => "F9",
            Named::F10 => "F10",
            Named::F11 => "F11",
            Named::F12 => "F12",
            Named::ArrowUp => "ArrowUp",
            Named::ArrowDown => "ArrowDown",
            Named::ArrowLeft => "ArrowLeft",
            Named::ArrowRight => "ArrowRight",
            Named::Home => "Home",
            Named::End => "End",
            Named::PageUp => "PageUp",
            Named::PageDown => "PageDown",
            Named::Insert => "Insert",
            Named::Delete => "Delete",
            Named::Enter => "Enter",
            Named::Escape => "Escape",
            Named::Backspace => "Backspace",
            Named::Tab => "Tab",
            _ => return String::new(),
        }
        .to_string(),
        Key::Character(c) => {
            let ch = c.chars().next().unwrap_or('?');
            if ch == ' ' {
                "Space".to_string()
            } else if ch.is_ascii_alphabetic() {
                format!("Key{}", ch.to_uppercase())
            } else if ch.is_ascii_digit() {
                format!("Digit{}", ch)
            } else {
                String::new()
            }
        }
        Key::Unidentified => String::new(),
    }
}
