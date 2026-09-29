---
title: Troubleshooting
description: Fixes for common problems with monitor detection, hotkeys, profiles, and startup, plus a short FAQ.
---

Start with **Refresh** (the icon at the left of the header). It detects
monitors again and clears most temporary errors. The status line at the
bottom of the window shows the most recent message or error.

## Monitors

### "No monitors with DDC/CI support found"

- Turn on **DDC/CI** in each monitor's on-screen menu.
- Connect the monitor directly to the PC. Docks, KVMs, splitters and some
  USB-C adapters block DDC/CI.
- Try a different cable or port. DisplayPort and HDMI are usually more
  reliable than adapters.
- Laptop built-in panels aren't controllable over DDC/CI.

### A monitor page shows "Unknown" or "Could not read …"

That value couldn't be read. Other controls on the page still work. Click
**Retry** if shown, or **Refresh**. Some monitors respond slowly right after
waking or switching inputs; wait a few seconds and refresh. If brightness or
contrast says *Refresh to adjust*, the app is waiting for a successful read
before offering a slider.

### Switching input doesn't work, or the input list is wrong

- The list comes from the monitor's own capabilities. If the monitor doesn't
  advertise any, a standard HDMI/DisplayPort/USB-C list is shown, and some
  entries may not exist on your model.
- Some monitors use non-standard input codes. Check what the monitor reports
  after switching with its buttons (it may show as `Custom (0x..)`), then use
  a hotkey with that **Input Source** or a **Custom VCP Code** action on code
  `60`.
- Once a monitor shows another input, it may stop answering this PC. Switch
  back with a hotkey or the monitor's buttons.
- The input shown is what the monitor *reports*. It doesn't prove that the
  source is actually showing a picture.

### "Ambiguous monitor association" or "cannot be identified safely"

The app refuses to guess which physical monitor a command is for. This
happens with **cloned (duplicated) displays**, **tiled** or multi-stream
monitors, or when Windows reports incomplete identity information. Switch
Windows to *Extend these displays*, then click **Refresh**.

### A hotkey or profile stopped targeting a monitor after I changed ports

Monitors are identified by their Windows device path, which can change when
you move a monitor to another port or dock, reinstall a GPU driver, or
replace hardware. Open the hotkey's **Displays** section, then **Rebind** or
**Remove** the missing entry and save. For profiles, arrange the layout again
and re-save it with **Replace**.

## Hotkeys

### A hotkey shows "Inactive"

- **Enable global hotkeys** is off in [Settings](/docs/settings/).
- Another application (or Windows itself) already uses that key combination.
  Record a different combination.
- Two hotkeys in a hand-edited `config.json` share a combination; only the
  first is registered.

### A hotkey shows "Unbound"

No key has been recorded yet. Click **Record** and press the combination.

### "That key combination is already assigned"

Another hotkey in the app already uses it. Change or clear that hotkey first.

### "Legacy monitor N (rebind required)"

The hotkey was created by an older version that stored monitor numbers. See
[Migrating monitor selections](/docs/hotkeys/#migrating-monitor-selections).

### A Turn Off action does nothing

The **Power-off method** in Settings defaults to **None**. Choose *Soft*,
*DDC/CI power off*, or *Both*, then click **Save**.

### A hotkey worked until I restarted the app

Recording a combination takes effect immediately, but it's only kept after
you click **Save** in the header.

## Profiles

See [If applying fails](/docs/profiles/#safe-matching-and-limitations) in the
Profiles guide. In short: reconnect every monitor in the profile, switch
cloned displays to extended, and re-save the profile after changing ports,
docks, drivers or GPUs.

## Startup

### The app doesn't start at sign-in

- Check **Start with Windows** in Settings.
- Check that *WindowsDisplayManager* is enabled in **Task Manager → Startup
  apps** (or **Settings → Apps → Startup**). Windows can disable it there
  independently of the app.
- If you moved the `.exe`, start it once manually so the startup entry picks
  up the new location.

### The window doesn't appear at sign-in

**Start minimized** is on. The app is in the notification area; double-click
its icon, or turn *Start minimized* off.

## Configuration

### "Configuration recovery required"

`config.json` couldn't be read, so hotkeys are paused and nothing is
overwritten. See [Recovering a damaged configuration](/docs/configuration/#recovering-a-damaged-configuration).

## FAQ

### Windows SmartScreen warns about the download

Releases aren't code-signed yet. Click **More info → Run anyway**. You can
also [build from source](https://github.com/cyb0rg56/WinDisplayManager#building-from-source).

### Does it need administrator rights?

No. Settings, profiles, and the startup entry are all per user.

### Does it work with laptop screens?

Built-in laptop panels don't support DDC/CI, so brightness, contrast and input
controls don't apply to them. Display profiles still include them.

### Can I copy profiles to another PC?

Not reliably. Profiles identify monitors by Windows device path, which is
specific to each PC and connection. Recreate the layout on the other PC and
save a new profile.

### Can I turn off just one monitor?

Yes, with a **Power Mode → Off** action, or a **Turn Off** action with the
*DDC/CI power off* method, targeting only that monitor. The *Soft* method
and the tray's **Turn Off Monitors** always sleep every display.

### Where are my settings stored?

See [Configuration & recovery](/docs/configuration/).

### Does the app send any data anywhere?

No. See the [privacy policy](/privacy/).
