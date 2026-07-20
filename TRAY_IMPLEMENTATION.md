# System Tray (Close-to-Tray) Implementation Guide

This documents the pattern used in fan-control to keep the app running in the system tray when the window is closed.

## Dependencies

```toml
# Cargo.toml
[dependencies]
tray-icon = "0.x"        # System tray icon and menu
tokio = { version = "1", features = ["sync"] }
resvg = "0.x"             # SVG rendering for tray icon (optional, can use PNG)
```

The app also uses `cosmic`/`iced` as the GUI framework, but the tray pattern itself is framework-agnostic.

---

## Step 1: Define Tray Messages

Create an enum for all actions the tray menu can trigger:

```rust
#[derive(Debug, Clone)]
pub enum SystemTrayMsg {
    Show,              // Show/restore the main window
    Config(String),    // Switch to a named config
    Inactive,          // Toggle inactive mode
    Exit,              // Actually quit the application
}
```

---

## Step 2: Create the Tray Icon and Event Stream

```rust
use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex, mpsc};
use tray_icon::{
    TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, MenuId, PredefinedMenuItem},
};

// Static menu IDs
static MENU_ID_SHOW: LazyLock<MenuId> = LazyLock::new(|| MenuId::new("SHOW"));
static MENU_ID_EXIT: LazyLock<MenuId> = LazyLock::new(|| MenuId::new("EXIT"));

pub struct SystemTray {
    tray_icon: TrayIcon,
}

#[derive(Clone)]
pub struct SystemTrayStream {
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<SystemTrayMsg>>>,
}

impl SystemTray {
    pub fn new() -> anyhow::Result<(Self, SystemTrayStream)> {
        // Build the tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("My App")
            .with_icon(load_tray_icon()?)
            .build()?;

        let (sender, receiver) = mpsc::unbounded_channel();

        // Handle menu item clicks
        let menu_sender = sender.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = match event.id {
                id if id == *MENU_ID_SHOW => menu_sender.send(SystemTrayMsg::Show),
                id if id == *MENU_ID_EXIT => menu_sender.send(SystemTrayMsg::Exit),
                _ => return,
            };
        }));

        // Handle double-click on tray icon → show window
        let tray_sender = sender;
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                let _ = tray_sender.send(SystemTrayMsg::Show);
            }
        }));

        Ok((
            Self { tray_icon },
            SystemTrayStream {
                receiver: Arc::new(Mutex::new(receiver)),
            },
        ))
    }

    /// Rebuild the context menu (call whenever state changes)
    pub fn update_menu(&self) -> anyhow::Result<()> {
        let menu = Menu::new();
        menu.append(&MenuItem::with_id(MENU_ID_SHOW.clone(), "Show", true, None))?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&MenuItem::with_id(MENU_ID_EXIT.clone(), "Exit", true, None))?;
        self.tray_icon.set_menu(Some(Box::new(menu)));
        Ok(())
    }
}
```

---

## Step 3: Expose Events as an Async Stream

This lets your GUI framework subscribe to tray events:

```rust
use cosmic::iced::futures::Stream;  // or futures::Stream
use cosmic::iced::stream;

impl SystemTrayStream {
    pub fn sub(self) -> impl Stream<Item = SystemTrayMsg> {
        stream::channel(1, |mut sender| async move {
            loop {
                let mut receiver = self.receiver.lock().await;
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
```

---

## Step 4: Load a Tray Icon

From an SVG (using resvg):

```rust
fn load_tray_icon() -> Result<tray_icon::Icon, tray_icon::BadIcon> {
    let svg = include_bytes!("path/to/icon.svg");
    let (width, height) = (32, 32);

    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg, &opt).unwrap();
    let viewbox = tree.size();

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            width as f32 / viewbox.width(),
            height as f32 / viewbox.height(),
        ),
        &mut pixmap.as_mut(),
    );

    tray_icon::Icon::from_rgba(pixmap.data().to_vec(), width, height)
}
```

Or from a PNG:

```rust
fn load_tray_icon() -> Result<tray_icon::Icon, tray_icon::BadIcon> {
    let bytes = include_bytes!("path/to/icon.png");
    let image = image::load_from_memory(bytes).unwrap().to_rgba8();
    let (w, h) = image.dimensions();
    tray_icon::Icon::from_rgba(image.into_raw(), w, h)
}
```

---

## Step 5: Wire It Into Your App

### App state

```rust
struct MyApp {
    tray: Option<(SystemTray, SystemTrayStream)>,
    main_window: Option<window::Id>,  // None when hidden
    // ... other fields
}
```

### Initialization

```rust
fn init() -> MyApp {
    let tray = match SystemTray::new() {
        Ok(tray) => {
            tray.0.update_menu().ok();
            Some(tray)
        }
        Err(e) => {
            eprintln!("Failed to create tray: {e}");
            None
        }
    };

    let mut app = MyApp {
        tray,
        main_window: None,
    };

    // Optionally start minimized (no window)
    if !settings.start_minimized {
        app.open_main_window();
    }

    app
}
```

### Subscribe to tray events

```rust
fn subscription(&self) -> Subscription<AppMsg> {
    let mut subs = vec![];

    if let Some(tray) = &self.tray {
        subs.push(
            Subscription::run_with_id("system-tray", tray.1.clone().sub())
                .map(AppMsg::SystemTray),
        );
    }

    Subscription::batch(subs)
}
```

---

## Step 6: The Key Behavior – Intercept Window Close

This is what makes close-to-tray work. Instead of exiting when the user clicks X, hide the window:

```rust
fn on_close_requested(&self, id: window::Id) -> Option<AppMsg> {
    if let Some(window) = &self.main_window {
        if window == &id {
            // DON'T exit — hide instead
            return Some(AppMsg::HideWindow);
        }
    }
    None
}
```

---

## Step 7: Handle Tray Messages

```rust
fn update(&mut self, msg: AppMsg) -> Task {
    match msg {
        AppMsg::SystemTray(tray_msg) => match tray_msg {
            SystemTrayMsg::Show => {
                if let Some(window) = &self.main_window {
                    // Window exists → bring to front
                    return focus_window(*window);
                } else {
                    // Window was closed → create a new one
                    return self.open_main_window();
                }
            }
            SystemTrayMsg::Exit => {
                self.cleanup();
                return exit_app();
            }
            // ... handle other variants
        },

        AppMsg::HideWindow => {
            // Close the window but keep the process alive
            if let Some(window) = self.main_window.take() {
                self.main_window = None;
                return close_window(window);
            }
        }

        // ... other messages
    }
}
```

---

## How It Works (Summary)

```
User clicks X
    │
    ▼
on_close_requested() intercepts
    │
    ▼
Returns AppMsg::HideWindow (NOT exit)
    │
    ▼
Window is closed, main_window = None
Process keeps running (tray icon + event loop still alive)
    │
    ├── Tray double-click / "Show" → open_main_window() creates new window
    │
    └── Tray "Exit" → actually calls process exit
```

### Key principles:

1. **Decouple window from process** — The `main_window` is an `Option`. When `None`, the app is "hidden" but still running.
2. **Intercept close** — `on_close_requested` returns a "hide" message instead of letting the framework exit.
3. **Tray keeps event loop alive** — The `SystemTray` struct (and its subscription) prevent the app from shutting down.
4. **Re-create windows on demand** — "Show" creates a fresh window if none exists, or focuses the existing one.

### Platform notes:

- In the fan-control source, this behavior is gated behind `#[cfg(not(target_os = "linux"))]` because Linux uses a different approach. You can remove those gates if you want tray-to-close on all platforms.
- On Windows, the tray icon appears in the notification area automatically.
- You may need to call `update_menu()` whenever your app state changes to reflect the current state in the tray context menu.
