---
layout: default
title: WinDisplayManager
---

{% include nav.html %}

<p align="center">{% include logo.svg %}</p>

<h1 align="center">WinDisplayManager</h1>
<p align="center"><strong>DDC/CI monitor control for Windows</strong> — brightness, contrast, input switching, power mode, hotkeys, and display profiles, all from a native GUI.</p>

<p align="center" class="badges">
  <a href="https://github.com/cyb0rg56/WinDisplayManager/actions/workflows/rust.yml"><img src="https://github.com/cyb0rg56/WinDisplayManager/actions/workflows/rust.yml/badge.svg" alt="Build status"></a>
  <a href="https://github.com/cyb0rg56/WinDisplayManager/releases/latest"><img src="https://img.shields.io/github/v/release/cyb0rg56/WinDisplayManager" alt="Latest release"></a>
  <a href="https://github.com/cyb0rg56/WinDisplayManager/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license"></a>
</p>

<p align="center">
  <a class="btn-download" href="https://github.com/cyb0rg56/WinDisplayManager/releases/latest">Download for Windows</a>
</p>

## Features

- **Brightness & contrast control** over DDC/CI, per monitor.
- **Input source switching** with a click or a hotkey.
- **Power mode control** to put a monitor to sleep or wake it from the app.
- **Global hotkeys** for brightness/contrast steps, input switching, power mode and applying profiles. This is configurable entirely in-app.
- **Display profiles** to save and restore whole monitor layouts (resolution, position, orientation) via Windows CCD, and switch between them instantly or with a hotkey.
- **System tray integration** that lives quietly in the tray, always one click away.
- **Native GUI** built with [libcosmic](https://github.com/pop-os/libcosmic)/[iced](https://github.com/iced-rs/iced).
## Screenshots

<div class="screenshot-grid">
  <div class="screenshot-placeholder">Monitor control page<br>screenshot coming soon</div>
  <div class="screenshot-placeholder">Hotkeys settings page<br>screenshot coming soon</div>
  <div class="screenshot-placeholder">Display profiles page<br>screenshot coming soon</div>
</div>

## Getting started

1. **Download** the latest `windisplaymanager_rs-*.exe` from the [Releases page](https://github.com/cyb0rg56/WinDisplayManager/releases/latest).
2. **Run it** — it's a single portable executable, no installer needed.
   > Since the executable isn't code-signed, Windows SmartScreen may warn you on first run. Click **More info → Run anyway** to proceed.
3. Your monitors are **detected automatically**. Adjust brightness, contrast, input, and power mode right from the Monitor page.
4. Head to **Settings** to record global hotkeys, or **Profiles** to save your current display layout for quick recall later.

## Documentation

- [Hotkeys guide]({{ site.baseurl }}/docs/hotkeys/) — how hotkey recording and config work.
- [Profiles guide]({{ site.baseurl }}/docs/profiles/) — how display profiles are stored and applied.

{% include footer.html %}
