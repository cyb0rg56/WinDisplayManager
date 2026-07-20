//! System tray icon implementation for Windows Display Manager.
//!
//! Provides a tray icon with menu options to show the window or exit the application.

use cosmic::iced::{
    futures::{SinkExt, Stream},
    stream,
};
use std::sync::{Arc, LazyLock};
use tokio::sync::{mpsc, Mutex};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, MenuId},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

/// Messages emitted by the system tray.
#[derive(Debug, Clone)]
pub enum TrayMessage {
    /// User wants to show/focus the main window.
    ShowWindow,
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
    #[allow(dead_code)]
    tray_icon: TrayIcon,
}

// Static menu IDs
static MENU_ID_SHOW: LazyLock<MenuId> = LazyLock::new(|| MenuId::new("MENU_ID_SHOW"));
static MENU_ID_EXIT: LazyLock<MenuId> = LazyLock::new(|| MenuId::new("MENU_ID_EXIT"));

impl SystemTray {
    /// Create a new system tray icon with menu.
    ///
    /// Returns the tray and a stream for receiving tray events.
    pub fn new() -> anyhow::Result<(Self, TrayStream)> {
        // Build the menu
        let menu = Menu::new();
        
        let item_show = MenuItem::with_id(
            MENU_ID_SHOW.clone(),
            "Show Window",
            true,
            None,
        );
        menu.append(&item_show)?;
        
        let separator = PredefinedMenuItem::separator();
        menu.append(&separator)?;
        
        let item_exit = MenuItem::with_id(
            MENU_ID_EXIT.clone(),
            "Exit",
            true,
            None,
        );
        menu.append(&item_exit)?;

        // Build the tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("Windows Display Manager")
            .with_icon(create_tray_icon()?)
            .with_menu(Box::new(menu))
            .build()?;

        // Set up event channels
        let (sender, receiver) = mpsc::unbounded_channel();

        // Handle menu events
        let menu_sender = sender.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let msg = if event.id == *MENU_ID_SHOW {
                TrayMessage::ShowWindow
            } else if event.id == *MENU_ID_EXIT {
                TrayMessage::Exit
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
}

impl TrayStream {
    /// Convert this into an async stream suitable for iced subscriptions.
    pub fn into_subscription_stream(self) -> impl Stream<Item = TrayMessage> {
        let receiver_arc = self.receiver.clone();

        stream::channel(1, |mut sender| async move {
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
        })
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
