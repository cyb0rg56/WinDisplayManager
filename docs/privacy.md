---
layout: default
title: Privacy Policy — WinDisplayManager
permalink: /privacy/
---

{% include nav.html %}

# Privacy Policy

_Last updated: 2026-07-22_

## The short version

**WinDisplayManager collects nothing and sends nothing over the network.**
There is no telemetry, no analytics, no crash reporting, and no network code
in the application at all — this has been verified by reviewing the source
and its dependencies.

## What the app does locally

Everything WinDisplayManager reads or writes stays on your own machine:

- **Hotkey configuration**: Your global hotkey bindings and settings are
  saved as JSON at `%APPDATA%\windisplaymanager\config.json`. See the
  [Hotkeys guide]({{ site.baseurl }}/docs/hotkeys/).
- **Display profiles**: Saved monitor layouts are stored as JSON files
  under `%APPDATA%\MonitorSwitcher\Profiles\`. See the
  [Profiles guide]({{ site.baseurl }}/docs/profiles/).
- **Monitor communication (DDC/CI)**: Brightness, contrast, input, and power
  mode changes are sent directly from your PC to your monitor over the
  physical display cable (DDC/CI). This is local hardware I/O, not a network
  protocol and none of it is transmitted anywhere else.

None of the above data ever leaves your device, the app has no accounts,
sign-in, or cloud sync of any kind.

## Third-party services

WinDisplayManager does not embed any third-party SDKs, analytics services, or
advertising in the application itself.

## This website

This site is a static page hosted on GitHub Pages. WinDisplayManager (the
project) does not add its own analytics or tracking scripts to this site.
GitHub's own hosting infrastructure may collect standard web server logs as
described in [GitHub's Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) —
that collection is GitHub's, not ours.

## Changes to this policy

If this policy ever needs to change (for example, if analytics were ever
added to this website), this page will be updated and the "last updated"
date above will reflect that change.

## Questions

Open an issue on the [GitHub repository](https://github.com/cyb0rg56/WinDisplayManager)
if you have any questions about this policy.

{% include footer.html %}
