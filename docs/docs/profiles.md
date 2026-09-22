---
layout: default
title: Profiles — WinDisplayManager
permalink: /docs/profiles/
---

{% include nav.html %}

# Display profiles

A **profile** is a saved snapshot of your monitor layout — which displays are
enabled, their resolution, position, and orientation — captured via the
Windows CCD (Connecting and Configuring Displays) API. Profiles let you flip
between layouts (e.g. "Docked", "Laptop only", "Presentation") in one click or
a single hotkey.

## Saving a profile

1. Arrange your displays the way you want them (using Windows display
   settings or your dock/monitor switch).
2. Open the **Profiles** page in WinDisplayManager, enter a name in the
   **New profile name** box, and click **Save Current Layout**.
3. The name becomes the filename, so avoid characters that aren't valid in
   Windows filenames (`< > : " / \ | ? *`); the app will strip/reject invalid
   names automatically.

If that profile already exists, the app asks you to confirm **Replace** before
capturing and overwriting it.

## Applying a profile

Click **Apply** next to a saved profile to apply it immediately. To bind a
profile to a global hotkey, click **Hotkey** next to it — this adds a
matching *Apply Profile* hotkey on the [Hotkeys page]({{ site.baseurl }}/docs/hotkeys/)
and jumps there so you can record a key combination — letting you switch
layouts without opening the app.

## Safe matching and limitations

Profiles identify each monitor by its Windows **device path**, compared without
ASCII case differences. Friendly names and model/EDID numbers are not unique and
are never used as a fallback. Two identical models work when their device paths
are distinct. Adapter LUIDs (both halves), source/target IDs and mode associations
are rebuilt for each monitor, not rewritten for every monitor on a GPU.

Connected but disabled monitors can be restored even when Windows reports no
live mode for them. Windows may report several possible sources for one target:
the app preserves a saved source endpoint when it still exists, otherwise uses
an unambiguous active route or uniquely determined remaining route. Cloned
monitors must share one source; separate desktops must not be merged. If those
constraints do not determine a route, applying is refused rather than choosing
the first path Windows reports.

Before any Windows validation/apply call, the app rejects missing or ambiguous
identities, inconsistent clone groups, and malformed mode data. It then asks
Windows to **validate** the remapped layout before applying it, without permitting
Windows to substitute different modes. Saved resolution, position, rotation,
scaling and timing data are retained. Unsupported resolutions or routing can
therefore fail instead of silently producing a different layout. Hardware may
still change between validation and apply; an apply error is reported without a
name-based retry.

If applying fails:

- Reconnect all monitors required by the profile and retry.
- If routing is ambiguous, enable/arrange the intended displays in Windows first.
- If a port, dock, driver, or GPU change changed a monitor's device path, arrange
  the layout again and **recapture** the profile. Profiles are not portable to
  other PCs merely because their monitors have the same model name.
- Virtual-mode/desktop-image layouts and profiles missing a source mode cannot
  currently be restored. Configure a conventional layout in Windows and
  recapture it. Missing target timing modes are allowed; Windows must validate
  the resulting layout before apply.

## Older profiles and migration

Existing JSON profiles remain readable; no schema-version bump is required.
New captures add optional `config.path_monitors` metadata, parallel to the path
array, so each target retains its device identity even without a target mode.
The original mode-indexed `config.monitors` metadata is still written.

An older profile can still be applied when its referenced target-mode metadata
contains a valid, unambiguous `device_path`. Profiles containing only friendly
names, or lacking an identity for a path without a target mode, cannot safely be
migrated automatically. The error asks you to arrange the layout in Windows and
recapture it using **Save Current Layout** (confirm **Replace**). Loading a
profile does not rewrite it, and simply re-saving its old JSON cannot invent
missing identities.

## Where profiles are stored

Each profile is saved as its own JSON file at:

```
%APPDATA%\MonitorSwitcher\Profiles\<name>.json
```

This location matches the original *MonitorSwitcher* directory convention;
that alone does not guarantee file-format or identity compatibility. Each file
contains the profile name, a creation timestamp, and the captured CCD display
configuration. As with hotkeys, you don't need to edit these by hand, but
they're plain JSON if you want to inspect, back up, or share them.

{% include footer.html %}
