#![windows_subsystem = "windows"]

fn main() -> cosmic::iced::Result {
    env_logger::init();

    // Use software rendering (tiny-skia) to avoid wgpu frame sync issues
    // during window drag/resize on Windows
    // SAFETY: No other threads are running at this point in main()
    unsafe {
        std::env::set_var("ICED_BACKEND", "tiny-skia");
    }

    // Tray-only launch is reserved for the Windows sign-in command.
    ui::run(data::startup::launched_minimized())
}
