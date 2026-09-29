//! System tray icon implementation for Windows Display Manager.
//!
//! Provides a tray icon with menu options to show the window or exit the application.

use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tray_icon::{
    TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};

/// Messages emitted by the system tray.
#[derive(Debug, Clone)]
pub enum TrayMessage {
    /// User wants to show/focus the main window.
    ShowWindow,
    /// User wants to load (apply) a named profile.
    LoadProfile(String),
    /// User wants to save the current layout as a new profile.
    SaveCurrentProfile,
    /// User wants to turn off all monitors.
    TurnOffMonitors,
    /// User wants to exit the application.
    Exit,
}

/// Async receiver for tray messages, shared so a restarted listener can resume.
#[derive(Clone)]
pub struct TrayStream {
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<TrayMessage>>>,
}

/// The system tray icon and its associated resources.
pub struct SystemTray {
    tray_icon: TrayIcon,
}

// Menu ID constants and the profile-load prefix used to encode profile names.
const MENU_ID_SHOW: &str = "MENU_ID_SHOW";
const MENU_ID_EXIT: &str = "MENU_ID_EXIT";
const MENU_ID_SAVE_CURRENT: &str = "MENU_ID_SAVE_CURRENT";
const MENU_ID_TURN_OFF: &str = "MENU_ID_TURN_OFF";
const PROFILE_LOAD_PREFIX: &str = "PROFILE_LOAD::";

/// Build the tray context menu for the given profile names.
fn build_menu(profiles: &[String]) -> anyhow::Result<Menu> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id(MENU_ID_SHOW, "Show Window", true, None))?;
    menu.append(&PredefinedMenuItem::separator())?;

    // Load Profile submenu (one item per profile).
    let load_submenu = Submenu::new("Load Profile", !profiles.is_empty());
    if profiles.is_empty() {
        load_submenu.append(&MenuItem::with_id(
            "PROFILE_NONE",
            "(no profiles)",
            false,
            None,
        ))?;
    } else {
        for name in profiles {
            load_submenu.append(&MenuItem::with_id(
                format!("{PROFILE_LOAD_PREFIX}{name}"),
                name.as_str(),
                true,
                None,
            ))?;
        }
    }
    menu.append(&load_submenu)?;

    menu.append(&MenuItem::with_id(
        MENU_ID_SAVE_CURRENT,
        "Save Current Layout\u{2026}",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        MENU_ID_TURN_OFF,
        "Turn Off Monitors",
        true,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(MENU_ID_EXIT, "Exit", true, None))?;
    Ok(menu)
}

impl SystemTray {
    /// Create a new system tray icon with menu.
    ///
    /// Returns the tray and a stream for receiving tray events.
    pub fn new() -> anyhow::Result<(Self, TrayStream)> {
        let menu = build_menu(&[])?;

        // Build the tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("Windows Display Manager")
            .with_icon(create_tray_icon()?)
            .with_menu(Box::new(menu))
            // Only show the context menu on right-click; left/double click is
            // reserved for showing the window.
            .with_menu_on_left_click(false)
            .build()?;

        // Set up event channels
        let (sender, receiver) = mpsc::unbounded_channel();

        // Handle menu events. IDs are parsed by string so the single global
        // handler keeps working after the menu is rebuilt.
        let menu_sender = sender.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let id = event.id.0.as_str();
            let msg = if id == MENU_ID_SHOW {
                TrayMessage::ShowWindow
            } else if id == MENU_ID_EXIT {
                TrayMessage::Exit
            } else if id == MENU_ID_SAVE_CURRENT {
                TrayMessage::SaveCurrentProfile
            } else if id == MENU_ID_TURN_OFF {
                TrayMessage::TurnOffMonitors
            } else if let Some(name) = id.strip_prefix(PROFILE_LOAD_PREFIX) {
                TrayMessage::LoadProfile(name.to_string())
            } else {
                return;
            };
            let _ = menu_sender.send(msg);
        }));

        // Handle tray icon events (double-click to show)
        let tray_sender = sender;
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                let _ = tray_sender.send(TrayMessage::ShowWindow);
            }
        }));

        Ok((
            Self { tray_icon },
            TrayStream {
                receiver: Arc::new(Mutex::new(receiver)),
            },
        ))
    }

    /// Rebuild the tray menu with the current set of profile names.
    pub fn update_menu(&self, profiles: &[String]) {
        match build_menu(profiles) {
            Ok(menu) => self.tray_icon.set_menu(Some(Box::new(menu))),
            Err(e) => log::warn!("Failed to rebuild tray menu: {e}"),
        }
    }
}

impl TrayStream {
    /// Next tray message; `None` once the tray is gone.
    pub async fn recv(&self) -> Option<TrayMessage> {
        self.receiver.lock().await.recv().await
    }
}

/// Load the tray icon from the executable's icon resource (ID 1, embedded by build.rs).
fn create_tray_icon() -> Result<tray_icon::Icon, tray_icon::BadIcon> {
    let result = tray_icon::Icon::from_resource(1, None);
    if let Err(e) = &result {
        log::error!("Failed to load tray icon: {e:?}");
    }
    result
}
