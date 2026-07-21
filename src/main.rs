#![windows_subsystem = "windows"]

mod app;
mod ccd;
mod config;
mod ddc;
mod hotkeys;
mod profiles;
mod tray;

fn main() -> cosmic::iced::Result {
    env_logger::init();

    // Use software rendering (tiny-skia) to avoid wgpu frame sync issues
    // during window drag/resize on Windows
    // SAFETY: No other threads are running at this point in main()
    unsafe {
        std::env::set_var("ICED_BACKEND", "tiny-skia");
    }

    let settings = cosmic::app::Settings::default()
        // Disable antialiasing for better performance
        .antialiasing(false)
        // Don't exit when the window is closed — keep running in the tray
        .exit_on_close(false)
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)
                .min_height(400.0),
        );

    cosmic::app::run::<app::AppModel>(settings, ())
}
