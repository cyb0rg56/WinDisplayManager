//! System tray icon implementation for Windows Display Manager.
//!
//! Provides a tray icon with menu options to show the window or exit the application.

use cosmic::iced::{
    futures::{SinkExt, Stream},
    stream, Subscription,
};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
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

/// Async stream wrapper for receiving tray messages.
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
    /// Build an iced `Subscription` that yields tray messages.
    pub fn subscription(self) -> Subscription<TrayMessage> {
        Subscription::run_with(TrayId(self), |data| {
            data.0.clone().into_subscription_stream()
        })
    }

    /// Convert this into an async stream suitable for iced subscriptions.
    pub fn into_subscription_stream(self) -> impl Stream<Item = TrayMessage> {
        let receiver_arc = self.receiver.clone();

        stream::channel(
            1,
            |mut sender: cosmic::iced::futures::channel::mpsc::Sender<TrayMessage>| async move {
                loop {
                    let mut receiver = receiver_arc.lock().await;
                    if let Some(msg) = receiver.recv().await {
                        if sender.send(msg).await.is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            },
        )
    }
}

/// Identity wrapper so a `TrayStream` can be used as `Subscription` data.
/// There is only ever one tray, so the identity is constant.
struct TrayId(TrayStream);

impl Hash for TrayId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "system-tray".hash(state);
    }
}

/// Create the tray icon from the embedded ICO file.
fn create_tray_icon() -> Result<tray_icon::Icon, tray_icon::BadIcon> {
    // Embed the ICO file at compile time
    static ICON_DATA: &[u8] = include_bytes!("../icon.ico");
    
    log::debug!("Creating tray icon, embedded data size: {} bytes", ICON_DATA.len());
    
    // Write to temp file since tray_icon can load ICO from path
    let temp_path = std::env::temp_dir().join("windisplaymanager_icon.ico");
    std::fs::write(&temp_path, ICON_DATA)
        .map_err(|e| {
            log::error!("Failed to write temp icon file: {e}");
            tray_icon::BadIcon::OsError(e)
        })?;
    
    log::debug!("Wrote temp icon to: {}", temp_path.display());
    
    let result = tray_icon::Icon::from_path(&temp_path, None);
    match &result {
        Ok(_) => log::debug!("Tray icon loaded successfully"),
        Err(e) => log::error!("Failed to load tray icon: {e:?}"),
    }
    result
}
