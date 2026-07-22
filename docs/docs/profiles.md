---
layout: default
title: Profiles — WinDisplayManager
permalink: /docs/profiles/
---

# Display profiles

A **profile** is a saved snapshot of your monitor layout — which displays are
enabled, their resolution, position, and orientation — captured via the
Windows CCD (Connecting and Configuring Displays) API. Profiles let you flip
between layouts (e.g. "Docked", "Laptop only", "Presentation") in one click or
a single hotkey.

## Saving a profile

1. Arrange your displays the way you want them (using Windows display
   settings or your dock/monitor switch).
2. Open the **Profiles** page in WinDisplayManager and click **Save Current
   as Profile**.
3. Give it a name — this becomes the filename, so avoid characters that
   aren't valid in Windows filenames (`< > : " / \ | ? *`); the app will
   strip/reject invalid names automatically.

## Applying a profile

Click a saved profile in the **Profiles** page to apply it immediately, or
bind it to a global hotkey from the [Hotkeys settings]({{ site.baseurl }}/docs/hotkeys/)
so you can switch layouts without opening the app.

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

<footer class="site-footer">
  <p>
    <a href="{{ site.baseurl }}/">Home</a> ·
    <a href="https://github.com/cyb0rg56/WinDisplayManager">Source on GitHub</a> ·
    <a href="{{ site.baseurl }}/privacy/">Privacy Policy</a>
  </p>
</footer>
