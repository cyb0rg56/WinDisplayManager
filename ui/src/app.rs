use crate::keys;
use crate::modal;
use crate::subscriptions;
use cosmic::iced::event::{self, Event};
use cosmic::iced::keyboard::{Event as KeyboardEvent, Key, Modifiers};
use cosmic::iced::{Length, Subscription, window};
use cosmic::prelude::*;
use cosmic::widget::{self, nav_bar};
use cosmic::{Core, executor};
use data::state::{AppState, Input};
use data::tray::{SystemTray, TrayMessage, TrayStream};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Simple inline SVG icon (monitor symbol)
const APP_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="100 100 320 280">
  <!-- Monitor -->
  <path d="M 365.047 120.547 L 158.791 120.547 C 143.497 120.554 131.102 132.953 131.101 148.247 L 131.101 276.344 C 131.102 291.636 143.498 304.033 158.791 304.034 L 229.066 304.034 L 229.066 326.233 L 197.294 326.233 C 189.148 326.233 184.056 335.051 188.129 342.106 C 190.02 345.38 193.513 347.397 197.294 347.397 L 326.547 347.397 C 334.693 347.397 339.784 338.579 335.711 331.524 C 333.821 328.25 330.327 326.233 326.547 326.233 L 294.771 326.233 L 294.771 304.035 L 365.046 304.035 C 380.342 304.038 392.744 291.64 392.746 276.344 L 392.746 148.242 C 392.742 132.946 380.342 120.548 365.046 120.547 L 365.047 120.547 Z M 368.507 276.34 C 368.502 278.25 366.956 279.797 365.047 279.804 L 158.791 279.804 C 156.881 279.798 155.336 278.249 155.336 276.34 L 155.336 148.242 C 155.337 146.334 156.883 144.789 158.791 144.788 L 365.047 144.788 C 366.954 144.79 368.501 146.334 368.507 148.242 L 368.507 276.34 Z" data-name="Monitor" style=""></path>

  <!-- Centered Lightning Bolt -->
  <path transform="translate(5, -10)" d="M 271 190 L 248.371 190 L 236.157 224.479 C 236.124 224.577 236.197 224.676 236.306 224.681 L 250.319 224.681 C 250.433 224.676 250.517 224.78 250.484 224.882 L 239.64 255.276 C 239.438 255.842 239.967 256.398 240.589 256.277 C 240.792 256.238 240.97 256.13 241.092 255.972 L 273.806 213.863 C 273.882 213.769 273.82 213.631 273.692 213.617 C 273.688 213.617 273.681 213.616 273.674 213.616 L 258.968 213.616 C 258.847 213.603 258.775 213.489 258.819 213.384 L 271.149 190.17 C 271.157 190.085 271.091 190.009 271 190 Z" style="fill: rgb(255, 221, 0);"></path>
</svg>"#;

// ---------------------------------------------------------------------------
// Navigation pages
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Monitor(u32), // 1-indexed monitor ID
    Hotkeys,
    Profiles,
    About,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Message {
    /// Handled by the UI-agnostic application state.
    Data(Input),
    /// Keyboard input while recording a hotkey.
    KeyPressed(Modifiers, Key),
    ToggleSettings,
    CloseSettings,
    // System tray
    Tray(TrayMessage),
    /// Hide window (close-to-tray)
    HideWindow,
    WindowClosed(window::Id),
    OpenUrl(String),
    /// Discard edits to the read-only configuration path field.
    ConfigPathInput,
    CopyConfigPath,
}

// ---------------------------------------------------------------------------
// Application model
// ---------------------------------------------------------------------------

pub struct AppModel {
    core: Core,
    pub(crate) nav: nav_bar::Model,
    pub(crate) about: widget::about::About,
    pub(crate) state: AppState,
    // System tray
    pub(crate) tray: Option<(SystemTray, TrayStream)>,
}

// ---------------------------------------------------------------------------
// cosmic::Application implementation
// ---------------------------------------------------------------------------

impl cosmic::Application for AppModel {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.windisplaymanager.app";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, cosmic::app::Task<Self::Message>) {
        let (state, effects) = AppState::load();

        // Build nav model with a placeholder; will be rebuilt after detection
        let mut nav = nav_bar::Model::default();
        nav.insert()
            .text("Detecting monitors...")
            .data::<Page>(Page::Hotkeys)
            .activate();

        let about = widget::about::About::default()
            .name("Windows Display Manager")
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .comments("DDC/CI monitor control with global hotkeys.");

        // Set up system tray
        let tray = match SystemTray::new() {
            Ok(t) => {
                log::info!("System tray created successfully");
                Some(t)
            }
            Err(e) => {
                log::warn!("Failed to create system tray: {e}");
                None
            }
        };

        let mut app = AppModel {
            core,
            nav,
            about,
            state,
            tray,
        };
        let task = app.run_effects(effects);
        (app, task)
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> cosmic::app::Task<Self::Message> {
        self.nav.activate(id);
        self.update_title()
    }

    fn dialog(&self) -> Option<Element<'_, Self::Message>> {
        if let Some(name) = self.state.pending_profile_delete() {
            return Some(modal::confirm_dialog(
                "Delete profile?",
                format!("Delete \"{name}\"? This removes the saved layout."),
                "Delete",
                Message::Data(Input::CancelDeleteProfile),
                Message::Data(Input::DeleteProfile(name.to_string())),
            ));
        }
        let id = self.state.pending_hotkey_delete()?;
        let title = self.state.hotkey_delete_title(id)?;
        Some(modal::confirm_dialog(
            "Delete hotkey?",
            format!("Delete \"{title}\"? This removes the hotkey and its actions."),
            "Delete",
            Message::Data(Input::CancelDeleteHotkey),
            Message::Data(Input::DeleteHotkey(id.to_string())),
        ))
    }

    fn on_escape(&mut self) -> cosmic::app::Task<Self::Message> {
        let mut effects = Vec::new();
        if self.state.pending_profile_delete().is_some() {
            effects.extend(self.state.update(Input::CancelDeleteProfile));
        }
        if self.state.pending_hotkey_delete().is_some() {
            effects.extend(self.state.update(Input::CancelDeleteHotkey));
        }
        self.run_effects(effects)
    }

    // Intercept the header-bar close button → hide to tray instead of exiting
    fn on_app_exit(&mut self) -> Option<Self::Message> {
        if self.tray.is_some() {
            Some(Message::HideWindow)
        } else {
            None // no tray → exit normally
        }
    }

    // Intercept window surface close (e.g. Alt+F4) → hide to tray
    fn on_close_requested(&self, id: window::Id) -> Option<Self::Message> {
        if self.tray.is_some() && self.core.main_window_id().is_some_and(|main| main == id) {
            return Some(Message::HideWindow);
        }
        None
    }

    // -----------------------------------------------------------------------
    // Subscriptions
    // -----------------------------------------------------------------------

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs: Vec<Subscription<Self::Message>> = Vec::new();

        // Global hotkey polling subscription (only when enabled)
        if let Some(registered_ids) = self.state.listened_hotkey_ids() {
            subs.push(
                subscriptions::hotkeys(registered_ids, self.state.hotkey_generation())
                    .map(|id| Message::Data(Input::HotkeyTriggered(id))),
            );
        }

        // Keyboard event subscription when recording hotkeys
        if self.state.is_recording() {
            subs.push(event::listen_with(|event, _status, _id| {
                if let Event::Keyboard(KeyboardEvent::KeyPressed { key, modifiers, .. }) = event {
                    Some(Message::KeyPressed(modifiers, key))
                } else {
                    None
                }
            }));
        }

        // System tray subscription
        if let Some((_, ref tray_stream)) = self.tray {
            subs.push(subscriptions::tray(tray_stream.clone()).map(Message::Tray));
        }

        subs.push(event::listen_with(|event, _status, id| {
            if let Event::Window(window::Event::Closed) = event {
                Some(Message::WindowClosed(id))
            } else {
                None
            }
        }));

        Subscription::batch(subs)
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    fn update(&mut self, message: Self::Message) -> cosmic::app::Task<Self::Message> {
        match message {
            Message::Data(input) => {
                let effects = self.state.update(input);
                return self.run_effects(effects);
            }
            Message::KeyPressed(modifiers, key) => {
                let binding = keys::binding(modifiers, &key);
                let effects = self.state.update(Input::KeyRecorded(binding));
                return self.run_effects(effects);
            }
            Message::ToggleSettings => {
                self.set_show_context(!self.core.window.show_context);
            }
            Message::CloseSettings => self.set_show_context(false),

            // -- Hide window (close-to-tray) --------------------------------
            Message::HideWindow => {
                log::info!("Hiding window to tray");
                if let Some(id) = self.core.main_window_id() {
                    return window::close(id);
                }
            }

            Message::WindowClosed(id) => {
                if self.core.main_window_id() == Some(id) {
                    self.core_mut().set_main_window_id(None);
                }
            }

            // -- System tray ------------------------------------------------
            Message::Tray(tray_msg) => {
                match tray_msg {
                    TrayMessage::ShowWindow => {
                        log::info!("Tray: Show window requested");
                        if let Some(id) = self.core.main_window_id() {
                            // Window still exists — try to focus it
                            return window::gain_focus(id);
                        } else {
                            // Window was closed — open a new one
                            let (new_id, open_task) = window::open(window::Settings {
                                size: cosmic::iced::Size::new(1920.0, 1080.0),
                                min_size: Some(cosmic::iced::Size::new(600.0, 400.0)),
                                decorations: false,
                                ..window::Settings::default()
                            });
                            self.core_mut().set_main_window_id(Some(new_id));
                            let title_task = self.update_title();
                            return cosmic::app::Task::batch([open_task.discard(), title_task]);
                        }
                    }
                    TrayMessage::LoadProfile(name) => {
                        return self.update(Message::Data(Input::ApplyProfile(name)));
                    }
                    TrayMessage::SaveCurrentProfile => {
                        self.activate_page(Page::Profiles);
                        return self.update(Message::Tray(TrayMessage::ShowWindow));
                    }
                    TrayMessage::TurnOffMonitors => {
                        return self.update(Message::Data(Input::TurnOffMonitors));
                    }
                    TrayMessage::Exit => {
                        log::info!("Tray: Exit requested");
                        return cosmic::iced::exit();
                    }
                }
            }

            Message::ConfigPathInput => {}
            Message::CopyConfigPath => {
                let effects = self.state.update(Input::ConfigPathCopied);
                return cosmic::app::Task::batch([
                    self.run_effects(effects),
                    cosmic::iced::clipboard::write(self.state.config_path().to_string()),
                ]);
            }
            Message::OpenUrl(url) => {
                if let Err(error) = std::process::Command::new("rundll32.exe")
                    .args(["url.dll,FileProtocolHandler", &url])
                    .spawn()
                {
                    let effects = self.state.update(Input::OpenUrlFailed(error.to_string()));
                    return self.run_effects(effects);
                }
            }
        }

        cosmic::app::Task::none()
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    fn view(&self) -> Element<'_, Self::Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        // Determine which page is active
        let page = self
            .nav
            .active_data::<Page>()
            .cloned()
            .unwrap_or(Page::Hotkeys);

        let content: Element<_> = match page {
            Page::Monitor(monitor_id) => self.view_monitor(monitor_id),
            Page::Hotkeys => self.view_hotkeys_current(),
            Page::Profiles => self.view_profiles(),
            Page::About => self.view_about(),
        };

        // Wrap in a container with status bar at the bottom
        let status_bar = widget::text::caption(self.state.status_message());

        let mut layout = widget::column::with_capacity(4);
        let recovering = self.state.recovery_error().is_some();
        if recovering {
            layout = layout.push(self.view_config_recovery());
        }
        if !recovering || !matches!(page, Page::Hotkeys) {
            layout = layout.push(content);
        }
        let layout = layout
            .push(widget::divider::horizontal::default())
            .push(
                widget::container(status_bar)
                    .padding([4, 12])
                    .width(Length::Fill),
            )
            .spacing(space_s)
            .height(Length::Fill)
            .width(Length::Fill);

        widget::container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(space_m)
            .into()
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![crate::icons::header_icon_button(
            crate::icons::AppIcon::Refresh,
            "Refresh",
            false,
            Message::Data(Input::RefreshMonitors),
        )]
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        let dirty = self.state.config_dirty();
        let save_tip = if dirty {
            "Save configuration (unsaved changes)"
        } else {
            "Save configuration"
        };
        vec![
            crate::icons::header_icon_button(
                crate::icons::AppIcon::Save,
                save_tip,
                dirty,
                Message::Data(Input::SaveConfig),
            ),
            crate::icons::header_icon_button(
                crate::icons::AppIcon::Settings,
                "Settings",
                self.core.window.show_context,
                Message::ToggleSettings,
            ),
        ]
    }

    fn context_drawer(&self) -> Option<cosmic::app::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }
        Some(
            cosmic::app::context_drawer(self.view_settings(), Message::CloseSettings)
                .title("Settings"),
        )
    }
}

// ---------------------------------------------------------------------------
// View helpers
// ---------------------------------------------------------------------------

impl AppModel {
    pub(crate) fn activate_page(&mut self, target: Page) {
        let position = self.nav.iter().find_map(|id| {
            if self.nav.data::<Page>(id) == Some(&target) {
                self.nav.position(id)
            } else {
                None
            }
        });
        if let Some(position) = position {
            self.nav.activate_position(position);
        }
    }

    // -----------------------------------------------------------------------
    // Title helper
    // -----------------------------------------------------------------------

    pub fn update_title(&mut self) -> cosmic::app::Task<Message> {
        let mut title = String::from("Windows Display Manager");
        if let Some(text) = self.nav.text(self.nav.active()) {
            title.push_str(" - ");
            title.push_str(text);
        }
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            cosmic::app::Task::none()
        }
    }
}
