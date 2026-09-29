# WinDisplayManager

[![Build status](https://github.com/cyb0rg56/WinDisplayManager/actions/workflows/rust.yml/badge.svg)](https://github.com/cyb0rg56/WinDisplayManager/actions/workflows/rust.yml)
[![Latest release](https://img.shields.io/github/v/release/cyb0rg56/WinDisplayManager)](https://github.com/cyb0rg56/WinDisplayManager/releases/latest)
[![MIT license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

DDC/CI monitor control for Windows: brightness, contrast, input switching,
power mode, hotkeys, and display profiles, all from a native GUI.

📖 Full docs and download links: **https://cyb0rg56.github.io/WinDisplayManager/**

## Features

- **Brightness & contrast control** over DDC/CI, per monitor.
- **Input source switching** (HDMI, DisplayPort, etc.) from the monitor page
  or a hotkey, using the inputs each monitor advertises.
- **Power mode control** to put monitors into standby or turn them off with a
  hotkey, or sleep all displays from the tray.
- **Global hotkeys** with configurable *action chains*: one shortcut can run
  several actions at once (set or offset brightness/contrast, switch inputs,
  change power mode, write custom DDC/CI VCP codes, apply a profile, or turn
  displays off), each targeting all displays or specific monitors. Configurable
  entirely in-app.
- **Display profiles** to save and restore whole monitor layouts (resolution, position, orientation) via Windows CCD, switchable instantly or by hotkey.
- **System tray integration**: closing the window keeps the app in the tray,
  with quick access to profiles and turning displays off. Optionally starts
  with Windows, minimized to the tray.
- **Native GUI** built with [libcosmic](https://github.com/pop-os/libcosmic)/[iced](https://github.com/iced-rs/iced).

## Installation

Download the latest `windisplaymanager_rs-*.exe` from the
[Releases page](https://github.com/cyb0rg56/WinDisplayManager/releases/latest)
and run it. It's a single portable executable, no installer needed.

> The executable isn't code-signed, so Windows SmartScreen may warn you on
> first run. Click **More info → Run anyway** to proceed.

Monitors must have **DDC/CI** enabled in their on-screen menu. Settings are
stored in `%APPDATA%\windisplaymanager\config.json`; see
[Configuration & recovery](https://cyb0rg56.github.io/WinDisplayManager/docs/configuration/).

## Building from source

Requires a recent stable Rust toolchain (edition 2024) on Windows.

```powershell
cargo build --release
```

The binary is produced at `target/release/windisplaymanager_rs.exe`.

To publish a GitHub Release, tag `v` plus the `Cargo.toml` version and push that tag. The steps are in the [testing runbook](https://cyb0rg56.github.io/WinDisplayManager/docs/testing/).

## Documentation

- [Monitors guide](https://cyb0rg56.github.io/WinDisplayManager/docs/monitors/)
- [Hotkeys guide](https://cyb0rg56.github.io/WinDisplayManager/docs/hotkeys/)
- [Profiles guide](https://cyb0rg56.github.io/WinDisplayManager/docs/profiles/)
- [Settings & startup](https://cyb0rg56.github.io/WinDisplayManager/docs/settings/)
- [Configuration & recovery](https://cyb0rg56.github.io/WinDisplayManager/docs/configuration/)
- [Troubleshooting & FAQ](https://cyb0rg56.github.io/WinDisplayManager/docs/troubleshooting/)
- [Testing](https://cyb0rg56.github.io/WinDisplayManager/docs/testing/)
- [Privacy policy](https://cyb0rg56.github.io/WinDisplayManager/privacy/)

## License

Licensed under the [MIT License](LICENSE).
