mod app;
mod effects;
mod hotkey_views;
mod icons;
mod keys;
mod modal;
mod monitor_views;
mod profile_views;
mod settings_views;
mod subscriptions;
mod views;

use app::{AppModel, Message};

/// Runs the UI until exit. `minimized` starts in the tray without a window.
pub fn run(minimized: bool) -> cosmic::iced::Result {
    let mut settings = cosmic::app::Settings::default()
        // Disable antialiasing for better performance
        .antialiasing(false)
        // Don't exit when the window is closed — keep running in the tray
        .exit_on_close(false)
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)
                .min_height(400.0),
        );
    if minimized {
        settings = settings.no_main_window(true);
    }
    cosmic::app::run::<AppModel>(settings, ())
}
