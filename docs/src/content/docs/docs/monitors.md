---
title: Monitors
description: Monitor pages, DDC/CI requirements, brightness, contrast, and input switching.
---

WinDisplayManager talks to your monitors over **DDC/CI**, the control channel
built into HDMI, DisplayPort and USB-C cables. Changes go straight to
the monitor's own settings, the same ones you'd change with its on-screen
menu buttons.

## Before you start

- **Enable DDC/CI on the monitor.** Most monitors have a *DDC/CI* option in
  their on-screen menu (often under *System*, *Others*, or *Setup*). Some ship
  with it off.
- **Direct connections work best.** Docks, KVM switches, HDMI splitters and
  some USB-C adapters don't pass DDC/CI through.
- **Built-in laptop panels don't support DDC/CI.** They don't appear as
  controllable monitors.

If no monitor responds, the app reports *No monitors with DDC/CI support
found*.

## Monitor pages

Each detected monitor gets its own entry at the top of the navigation list,
named after the monitor with its resolution, e.g. `DELL U2723QE (3840x2160)`,
or `Monitor N` if Windows reports no name. The **Hotkeys**, **Profiles** and
**About** pages follow.

A monitor page shows:

- The monitor name, its resolution and desktop position (e.g.
  `3840x2160 at (0, 0)`), and **[Primary]** for the primary display.
- **Input Source**: the current input and a dropdown to switch.
- **Brightness** and **Contrast**: the current value against the monitor's
  own maximum (e.g. `70 / 100`) and a slider.

Monitor numbers such as *Monitor 2* are labels for the current session only.
Hotkeys and profiles store the monitor's full Windows device path instead, so
reordering or reconnecting displays doesn't send a command to the wrong
monitor.

### Brightness and contrast

The slider range comes from the maximum each monitor reports, which is
usually 100 but not always. While you drag, the label shows
*(pending)*; the final value is sent once you stop moving the slider, so
rapid changes don't flood the monitor.

### Input source

The dropdown lists the inputs the monitor advertises in its DDC/CI
capabilities. If a monitor doesn't advertise a list, the app offers a
standard set instead: HDMI 1/2, DisplayPort 1/2, and USB-C 1/2. Not every
entry in that standard list will exist on your monitor.

Inputs the app doesn't recognize are shown by their raw code, e.g.
`Custom (0x42)`.

After switching to another input, the monitor may stop answering DDC/CI on
this PC's cable (it's now showing a different source). That's expected; use
a hotkey or the monitor's buttons to switch back.

## Refresh and Retry

- **Refresh** (the icon at the left of the header) detects monitors again and
  re-reads every value. It cancels changes that are still waiting to be sent.
  Use it after plugging in, unplugging, or rearranging displays.
- If a monitor can't be read at all, its page shows *Monitor N unavailable*,
  the error, and a **Retry** button that re-reads just that monitor.
- If only one value fails, its section shows *Could not read …* with the
  error and the label reads *Unknown*. Other controls keep working. The
  brightness or contrast slider is replaced by *Refresh to adjust*, because
  the app won't guess the monitor's range.

## Power mode and custom VCP codes

The monitor page doesn't include power controls. To put a monitor into
standby or turn it off, create a [hotkey](/docs/hotkeys/) with a **Power
Mode** or **Turn Off** action, or use **Turn Off Monitors** in the tray menu
(see [Settings & startup](/docs/settings/)). Hotkeys can also write any raw
VCP code with a **Custom VCP Code** action.

## DDC/CI codes used

| Feature | VCP code |
|---|---|
| Brightness | `0x10` |
| Contrast | `0x12` |
| Input source | `0x60` |
| Power mode | `0xD6` (On `1`, Standby `2`, Suspend `3`, Off `4`) |

Standard input values: DisplayPort 1/2 `0x0F`/`0x10`, HDMI 1/2
`0x11`/`0x12`, USB-C 1/2 `0x13`/`0x14`. Many monitors use their own values;
the app uses whatever the monitor advertises.
