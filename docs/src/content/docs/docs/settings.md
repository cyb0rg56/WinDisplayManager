---
title: Settings & startup
description: Startup options, global hotkeys, the power-off method, and the system tray.
---

Open **Settings** with the gear icon at the right of the header. The panel has
three sections: **Startup**, **Hotkeys**, and **Turn Off Displays**.

## Startup

| Setting | Default | Effect |
|---|---|---|
| **Start with Windows** | Off | Launch WinDisplayManager when you sign in. |
| **Start minimized** | Off | Only shown while *Start with Windows* is on. At sign-in the app starts in the system tray without opening its window. |

Both toggles are saved as soon as you change them; you don't need to click
**Save**. They are stored per user; no administrator rights are needed.

Behind the scenes the app writes one value to the registry:

```
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
  WindowsDisplayManager = "C:\path\to\windisplaymanager_rs.exe" [--minimized]
```

- `--minimized` is added only when **Start minimized** is on. Launching the
  app yourself always opens the window.
- Turning **Start with Windows** off removes the value. Your *Start minimized*
  choice is remembered for next time.
- On every launch the value is rewritten to match your saved settings and the
  executable's current location, so moving the `.exe` is picked up the next
  time you start it manually.
- If the registry update succeeds but saving the configuration fails, the
  previous registration is restored.

## Hotkeys

**Enable global hotkeys** (default: on) registers or unregisters every hotkey
with Windows. When off, your bindings are kept but pressing them does nothing,
and every hotkey card shows **Inactive**. This toggle is also saved
immediately. See the [Hotkeys guide](/docs/hotkeys/).

## Turn Off Displays

**Power-off method** decides what a hotkey **Turn Off** action does. The
default is **None**. Unlike the toggles above, this choice is saved when you
click **Save** in the header.

| Method | Effect |
|---|---|
| **None** | Turn Off actions do nothing. |
| **Soft (Windows monitor sleep)** | Asks Windows to put the displays to sleep. This affects **all** displays, not just the selected ones; they wake on mouse or keyboard input. |
| **DDC/CI power off** | Sends a DDC/CI power-off command to each selected monitor. Many monitors then need their power button (or a DDC/CI *Power Mode → On* command, if still supported) to wake. |
| **Both** | Does both of the above. |

## System tray

Closing the window (the close button or `Alt+F4`) hides WinDisplayManager to
the notification area; it keeps running so hotkeys keep working. Double-click
the tray icon to open the window again, or right-click it for the menu:

| Menu item | What it does |
|---|---|
| **Show Window** | Opens or focuses the main window. |
| **Load Profile** | Submenu with every saved [profile](/docs/profiles/); pick one to apply it. Disabled when no profiles exist. |
| **Save Current Layout…** | Opens the Profiles page so you can name and save the current layout. |
| **Turn Off Monitors** | Puts all displays to sleep through Windows (the *Soft* method). It always uses Windows sleep, regardless of the Power-off method setting. |
| **Exit** | Quits the app and unregisters all hotkeys. |

If the tray icon cannot be created, closing the window exits the app instead.
