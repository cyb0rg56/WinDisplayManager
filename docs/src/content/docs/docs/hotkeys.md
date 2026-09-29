---
title: Hotkeys
description: How WinDisplayManager hotkey recording, action chains, and config work.
---

WinDisplayManager binds **global hotkeys** (they work even when the app isn't
focused) to **actions**. Each hotkey is a key combination plus a *chain* of one
or more actions that all run when you press it. A single shortcut can, for
example, dim every display, switch each monitor to a different input, and apply
a profile all at once.

Hotkeys are managed on the **Hotkeys** page.

## Anatomy of a hotkey

Each hotkey card shows:

- Its **title**: the optional **Label** you type, or a numbered fallback such
  as `Hotkey (1)` when the label is blank.
- Its **key combination** (the accelerator), e.g. `Ctrl + Alt + F1`.
- Its registration status: **Active** (registered with Windows),
  **Inactive** (global hotkeys are disabled, another application already owns
  the combination, or it duplicates an earlier hotkey), or **Unbound** (no key
  recorded yet).
- One or more **actions**, which run in order when the hotkey is pressed.

### Action type

| Type | What it does |
|---|---|
| **Set** | Sets the target to an absolute value (e.g. Brightness = 50). |
| **Offset** | Adds a signed value to the target's *current* value (e.g. `+10` to brighten, `-10` to dim). Available for Brightness, Contrast and Custom VCP only. |
| **Turn Off** | Turns off the selected displays (see [Turn-off behavior](#turn-off-behavior)). |

### Action target

| Target | Notes |
|---|---|
| **Brightness** | An absolute level (Set) or a signed delta (Offset), clamped to the monitor's range. |
| **Contrast** | An absolute level (Set) or a signed delta (Offset), clamped to the monitor's range. |
| **Input Source** | Switch to HDMI, DisplayPort or USB-C. Can switch a **different input per monitor** (see below). |
| **Power Mode** | On / Standby / Suspend / Off, via the DDC/CI power state. |
| **Apply Profile** | Applies a saved [display profile](/docs/profiles/). |
| **Custom VCP Code** | Write any raw DDC/CI VCP feature code (in hex). Set writes the value directly; Offset reads the monitor's current value first and adds the delta. |

### Editing an action

Actions are edited vertically in four labeled sections:

- **Actions**: choose **Set**, **Offset**, or **Turn Off**.
- **Commands**: choose Brightness, Contrast, Input Source, Power Mode,
   Apply Profile, or Custom VCP Code.
- **Value**: type signed decimal values directly, from `-200` to `200`.
   Other commands show their appropriate dropdown. Custom VCP actions also
   provide a separate hexadecimal **VCP code** field (`00`–`FF`, with or
   without a `0x` prefix).
- **Displays**: use switches for **All displays** and each detected monitor.

When an Input Source action targets several monitors, the dropdown only lists
inputs that all of them advertise. If they share none, the editor says
*Selected displays do not share this option*.

Turn Off actions only show Actions and Displays because they do not need a
command or value.

### Which monitors

Every action applies either to **All displays** or to a specific set of
monitors selected with switches. Turning off one monitor while **All displays**
is enabled changes the action to an explicit selection containing the other
detected monitors. When every connected monitor is selected again, the **All
displays** switch is shown as enabled. Turning that switch off clears the
explicit monitor selection.

**Input Source** actions show an input dropdown for each selected monitor when
**All displays** is off, so one hotkey can switch different monitors to
different inputs. Selecting every monitor does not replace those distinct input
choices. An unselected monitor is left unchanged. Saved monitor selections
remain visible as unavailable if that display is temporarily disconnected.

**Apply Profile** always restores the entire saved display layout, so it does
not show per-display switches.

## Migrating monitor selections

Monitor selections now store the complete Windows monitor device-interface path,
not an enumeration number. Display numbers in the UI are labels only. Two monitors
with the same model name remain distinct when Windows gives them distinct paths.

Old numeric selections load as **Legacy monitor N (rebind required)** and are
preserved on save. They are never assigned automatically to today's Monitor N.
In each action's Displays section, choose **Rebind to Monitor …** or **Remove**
for each unresolved entry, then save. Rebinding retains that entry's input-source
choice. If the destination already has an assignment, remove that assignment
first; it is not silently overwritten. Disconnected saved selections can also
be rebound or removed. Selecting **All displays** explicitly uses the currently
identified monitors instead of the saved selection.

An explicit action with any unresolved, missing, or duplicate target is refused
before dispatch, including Windows sleep. An empty selection never means all.
The hotkey chain is validated before enqueueing. The first queued monitor
operation (not a profile operation) checks the complete monitor selection again
against live hardware. If this initial preflight fails, the batch's remaining
monitor operations are blocked. After it succeeds, each later operation checks
only its own target; Windows sleep checks its selected group. A monitor that
intentionally disconnects after switching inputs or powering off does not block
an operation on a different, still-connected monitor.

This is not an atomic transaction: a later operation can still fail if its own
target disappears, and earlier successful operations are not rolled back.
Profile operations and their topology-change behavior are unchanged.

Identity is scoped to a Windows device instance, not an immutable physical serial
number. Moving a monitor to another port/dock, reinstalling a driver, or replacing
hardware may change its path and require rebinding. The app does not guess by model
name, position, or enumeration order. Clone/tiled configurations with multiple
physical handles or CCD targets for one GDI source, missing identity information,
and duplicate paths are refused; detection/control may be unavailable for the
whole snapshot until the ambiguity is removed. Refresh after changing topology.

**Soft (Windows monitor sleep)** remains a global Windows operation even after
validating the selected targets; it cannot sleep just one selected monitor.
Display-layout profile remapping is unchanged by this identity migration.

## Creating a hotkey

1. Open the **Hotkeys** page and click **Add Hotkey**. The new hotkey has no
   key combination yet and starts with one action: **Set Brightness** to `10`
   on **All displays**. Change it to whatever you need.
2. Click **Record** and press the modifiers and key you want
   (`Ctrl`/`Alt`/`Shift`/`Win` + a key). The captured combination is shown back
   to you. Use **Record** again to change it, **Cancel** to stop recording, or
   **Clear** to unbind it. A combination already used by another hotkey is
   refused with *That key combination is already assigned*.
3. Optionally type a **Label** so the card is easy to find.
4. Configure the action (type, target, value, monitors). Click **Add Action**
   to chain additional actions onto the same hotkey.
5. Click the **Save** icon in the header to write everything to disk. The icon
   is highlighted while there are unsaved changes.

Recording, clearing, or deleting a binding updates the Windows registration
immediately, so a new combination works before you save. It is only kept
across restarts after you save. Action edits also take effect immediately for
already registered hotkeys.

Remove a single action with **Delete Action**, or the entire hotkey with
**Delete** (you are asked to confirm). All global hotkeys can be toggled
on/off from [Settings](/docs/settings/) without deleting your bindings.

## Turn-off behavior

The **Turn Off** action type follows the **Power-off method** in
[Settings](/docs/settings/). The default is **None**, so Turn Off actions do
nothing until you choose a method.

| Mode | Effect |
|---|---|
| **None** | Turn-off actions do nothing. |
| **Soft (Windows monitor sleep)** | Asks Windows to put the displays to sleep; they wake on input. |
| **DDC/CI power off** | Sends a DDC/CI power-off command to each selected monitor. |
| **Both** | Does both of the above. |

## Where hotkeys are stored

Hotkeys are saved as JSON alongside the rest of the app configuration at:

```
%APPDATA%\windisplaymanager\config.json
```

Each hotkey records its key combination and its list of actions (type, target,
value, target monitors, input source(s), power mode, VCP code or profile name,
as applicable). You normally never need to touch this file (the Hotkeys page
manages it for you), but it's plain, human-readable JSON if you ever want to
back it up or inspect it. Older configuration shapes are read automatically;
legacy numeric monitor selections require explicit rebinding as described above.
If you edit the file by hand and two hotkeys share a combination, only the
first one is registered. See [Configuration & recovery](/docs/configuration/)
for the full file layout and what happens if it cannot be read.
