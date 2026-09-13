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

## Applying a profile

Click **Apply** next to a saved profile to apply it immediately. To bind a
profile to a global hotkey, click **Set Hotkey** next to it — this adds a
matching *Apply Profile* hotkey on the [Hotkeys page]({{ site.baseurl }}/docs/hotkeys/)
and jumps there so you can record a key combination — letting you switch
layouts without opening the app.

## Where profiles are stored

Each profile is saved as its own JSON file at:

```
%APPDATA%\MonitorSwitcher\Profiles\<name>.json
```

(This location matches the original *MonitorSwitcher* tool's layout, so
existing profiles from that tool can be reused.) Each file contains the
profile name, a creation timestamp, and the captured CCD display
configuration. As with hotkeys, you don't need to edit these by hand, but
they're plain JSON if you want to inspect, back up, or share them.

{% include footer.html %}
