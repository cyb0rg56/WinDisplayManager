# WinDisplayManager

[![Build status](https://github.com/cyb0rg56/WinDisplayManager/actions/workflows/rust.yml/badge.svg)](https://github.com/cyb0rg56/WinDisplayManager/actions/workflows/rust.yml)
[![Latest release](https://img.shields.io/github/v/release/cyb0rg56/WinDisplayManager)](https://github.com/cyb0rg56/WinDisplayManager/releases/latest)
[![MIT license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

DDC/CI monitor control for Windows — brightness, contrast, input switching,
power mode, hotkeys, and display profiles, all from a native GUI.

📖 Full docs and download links: **https://cyb0rg56.github.io/WinDisplayManager/**

## Features

- **Brightness & contrast control** over DDC/CI, per monitor.
- **Input source switching** (HDMI, DisplayPort, etc.) with a click or a hotkey.
- **Power mode control** to put a monitor to sleep or wake it from the app.
- **Global hotkeys** for brightness/contrast steps, input switching, power
  mode and applying profiles. This is configurable entirely in-app.
- **Display profiles** to save and restore whole monitor layouts (resolution, position, orientation) via Windows CCD, switchable instantly or by hotkey.
- **System tray integration** which lives quietly in the tray, always one click away.
- **Native GUI** built with [libcosmic](https://github.com/pop-os/libcosmic)/[iced](https://github.com/iced-rs/iced).

## Installation

Download the latest `windisplaymanager_rs-*.exe` from the
[Releases page](https://github.com/cyb0rg56/WinDisplayManager/releases/latest)
and run it — it's a single portable executable, no installer needed.

> The executable isn't code-signed, so Windows SmartScreen may warn you on
> first run. Click **More info → Run anyway** to proceed.

## Building from source

Requires a recent stable Rust toolchain (edition 2024) on Windows.

```powershell
cargo build --release
```

The binary is produced at `target/release/windisplaymanager_rs.exe`.

## Documentation

- [Hotkeys guide](https://cyb0rg56.github.io/WinDisplayManager/docs/hotkeys/)
- [Profiles guide](https://cyb0rg56.github.io/WinDisplayManager/docs/profiles/)
- [Privacy policy](https://cyb0rg56.github.io/WinDisplayManager/privacy/)

## License

Licensed under the [MIT License](LICENSE).
