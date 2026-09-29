---
title: Configuration & recovery
description: Where WinDisplayManager stores its settings, the file format, backups, and how to recover a damaged configuration.
---

All settings are stored per user under `%APPDATA%` (your roaming application
data folder). Nothing is written next to the executable.

| What | Location |
|---|---|
| Settings and hotkeys | `%APPDATA%\windisplaymanager\config.json` |
| Previous settings | `%APPDATA%\windisplaymanager\config.json.bak` |
| Display profiles | `%APPDATA%\MonitorSwitcher\Profiles\<name>.json` |
| Start with Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `WindowsDisplayManager` |

The **About** page shows the full path of the configuration file, with a
**Copy** button.

## What gets saved when

- The **Save** icon in the header writes hotkeys, labels, actions and the
  power-off method. It is highlighted while there are unsaved changes.
- **Start with Windows**, **Start minimized** and **Enable global hotkeys** are
  saved as soon as you toggle them. Saving writes the whole configuration, so
  any unsaved hotkey edits are written at the same time.
- Profiles are written when you save or replace them, independently of
  `config.json`.

If `config.json` doesn't exist yet, the app starts with defaults and creates
the file the first time something is saved.

## File format

`config.json` is plain, indented JSON:

```json
{
  "schema_version": 1,
  "hotkeys": {
    "hotkeys": [
      {
        "id": "hk-…",
        "label": "Dim everything",
        "binding": { "ctrl": true, "alt": true, "shift": false, "win": false, "key": "F1" },
        "actions": [
          {
            "action_type": "Offset",
            "target": "Brightness",
            "all_monitors": true,
            "monitors": [],
            "value": -10,
            "vcp_code": 16,
            "input_source": "Hdmi1",
            "monitor_inputs": [],
            "power_mode": "On",
            "profile_name": ""
          }
        ]
      }
    ],
    "brightness_step": 10,
    "contrast_step": 10
  },
  "hotkeys_enabled": true,
  "turn_off_behavior": "None",
  "start_with_windows": false,
  "start_minimized": false
}
```

| Field | Default | Notes |
|---|---|---|
| `schema_version` | `1` | Files without it are treated as older versions and upgraded when loaded. |
| `hotkeys.hotkeys` | `[]` | See the [Hotkeys guide](/docs/hotkeys/). |
| `hotkeys_enabled` | `true` | *Enable global hotkeys* in Settings. |
| `turn_off_behavior` | `"None"` | `"None"`, `"Soft"`, `"Ddc"` or `"Both"`. |
| `start_with_windows` | `false` | Mirrors the registry `Run` value. |
| `start_minimized` | `false` | Kept even when startup is off. |

Monitor selections in `monitors` and `monitor_inputs` are Windows device paths
(`windows-device-path:v1:\\?\display#…`). A plain number is an old,
[legacy selection](/docs/hotkeys/#migrating-monitor-selections) that needs
rebinding. You normally don't need to edit the file by hand; if you do, exit
the app first so your edits aren't overwritten.

## Backups

Every save first copies the current, valid `config.json` to
`config.json.bak`, then replaces the file in one step, so a crash or power
loss mid-save leaves either the old or the new file, never a half-written
one. Profiles work the same way: replacing a profile keeps the previous
version as `<name>.json.bak`.

The app never overwrites a file it can't read. If `config.json` is damaged,
ordinary saves are refused rather than replacing it.

## Recovering a damaged configuration

If `config.json` can't be read (invalid JSON, a newer `schema_version` from a
later release, or a permissions problem), the app shows
**Configuration recovery required** at the top of every page, with the file
path and the error. Until you resolve it:

- Global hotkeys are disabled, and the Hotkeys page shows only the recovery
  panel.
- Settings and hotkey edits are blocked, and nothing is written to
  `config.json`.
- Your Start with Windows registration is left as it was.
- Monitor controls and profiles keep working.

Choose one of:

| Button | What it does |
|---|---|
| **Retry loading** | Reads `config.json` again. Use this after fixing the file by hand or restoring a copy. |
| **Recover backup** | Loads `config.json.bak` and writes it back as `config.json`. The backup is kept. |
| **Reset to defaults…** | After you click **Confirm reset**, replaces `config.json` with defaults. This discards your hotkeys; the backup is kept. |

Before resetting, consider copying the damaged file somewhere safe: it may
still contain hotkeys you can re-create.

A file with a newer `schema_version` usually means you ran a newer release and
then went back to an older one. Upgrade again instead of resetting, or use
**Recover backup** if the backup was written by this version.
