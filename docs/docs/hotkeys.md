---
layout: default
title: Hotkeys — WinDisplayManager
permalink: /docs/hotkeys/
---

# Hotkeys

WinDisplayManager can bind global hotkeys (they work even when the app is not
focused) to five kinds of actions:

| Action | Description |
|---|---|
| Brightness step | Increase/decrease a monitor's brightness by a configurable step |
| Contrast step | Increase/decrease a monitor's contrast by a configurable step |
| Input switch | Switch a monitor directly to a given input source (HDMI, DisplayPort, ...) |
| Power mode | Put a monitor to sleep or wake it |
| Apply profile | Instantly switch to a saved [display profile]({{ site.baseurl }}/docs/profiles/) |

## Recording a hotkey

Hotkeys are recorded entirely from the **Settings** page — there's no config
file to hand-edit:

1. Open **Settings** and choose the action you want to bind (e.g. "Brightness
   Up" for a specific monitor).
2. Click **Start Recording**, then press the key combination you want to use.
3. The app captures your modifiers (`Ctrl`/`Alt`/`Shift`/`Win`) plus the key
   and shows it back to you (e.g. `Ctrl + Alt + F1`).
4. Save — the binding takes effect immediately and is written to your config.

You can remove any existing binding from the same page, and toggle **all**
global hotkeys on/off without deleting your bindings.

## Where hotkeys are stored

Bindings are saved as JSON alongside the rest of the app configuration at:

```
%APPDATA%\windisplaymanager\config.json
```

Each binding records the modifier flags and a key name (e.g. `"F1"`,
`"Digit1"`, `"ArrowUp"`) plus whatever is specific to that action type (monitor
ID, step direction, input source, power mode, or profile name). You normally
never need to touch this file directly — the Settings page manages it for
you — but it's plain, human-readable JSON if you ever want to back it up or
inspect it.

## Step size

The amount each "brightness up/down" or "contrast up/down" hotkey press
changes is controlled by a single step value (10 by default) shared across
all brightness/contrast hotkeys, configurable from the Settings page.

<footer class="site-footer">
  <p>
    <a href="{{ site.baseurl }}/">Home</a> ·
    <a href="https://github.com/cyb0rg56/WinDisplayManager">Source on GitHub</a> ·
    <a href="{{ site.baseurl }}/privacy/">Privacy Policy</a>
  </p>
</footer>
