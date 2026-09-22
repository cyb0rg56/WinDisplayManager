---
layout: default
title: Testing — WinDisplayManager
permalink: /docs/testing/
---

{% include nav.html %}

# Testing

Contributor runbook for automated gates and manual hardware checks. Automated
tests do not call the real display-configuration apply API and do not prove
that every monitor, dock, GPU, or layout works.

## Confirmed on 2026-09-22

The implemented change set was validated on the maintainer's hardware and
works. That includes the scalar input-source decoding fix previously checked
on an **AW2725QF** and a **DELL U2723QE**. Individual on-screen readings and
raw replies from that session were not recorded here. This confirmation covers
that setup, not every untested dock, GPU, or layout.

## Automated gates

From the repository root, on Windows, with a stable Rust toolchain that
includes `rustfmt` and `clippy`:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
pwsh .\packaging\msix\build-msix.ps1
```

`cargo clippy --all-targets` type-checks the same targets as `cargo check`, so
a separate check step is not required. The packaging script builds an
**unsigned** MSIX and takes its four-part version from `Cargo.toml` (currently
appended with `.0`). Do not pass `-Sign`. Signing and GitHub Release
publication stay on tag workflows; pull requests must not receive release
secrets.

`makeappx.exe` comes from the Windows 10/11 SDK. If it is missing locally, the
unsigned package step is still required in CI.

## Tagging a release

Publication happens only when an annotated tag is pushed. The tag must be
`v` plus the three-part `version` already committed in `Cargo.toml` on that
same commit. For the current package version that tag is `v0.1.4`. A tag such
as `v0.1.4.0` does not match and the release job fails.

1. On `main`, set `version` in `Cargo.toml` to `x.y.z`. Run a Cargo command
   such as `cargo check` so `Cargo.lock` records the same version, then commit
   both files and push `main`.
2. Run the automated gates above, or wait until the push to `main` is green.
3. Tag that commit and push only that tag:

```powershell
git checkout main
git pull
git tag -a v0.1.4 -m "WinDisplayManager 0.1.4"
git push origin v0.1.4
```

Replace `0.1.4` with the version you committed. The tag has to point at the
commit that contains that `Cargo.toml` version.

Pushing the tag runs the same validation as a pull request, then the release
job checks the tag against `Cargo.toml` and publishes the already built
artifacts. It does not rebuild or sign them. The GitHub Release receives:

- `windisplaymanager_rs-v0.1.4.exe`
- unsigned `windisplaymanager_rs-0.1.4.0.msix` (the fourth part is always `.0`)
- generated release notes

The executable and MSIX are unsigned, so SmartScreen can warn on the
executable. Do not pass `-Sign` for this workflow. Creating a release in the
GitHub UI without pushing a `vX.Y.Z` tag does not build these files. If a
published tag is wrong, ship a new patch version instead of moving or deleting
the tag.

## Manual regression checklist

Exit any running tray instance before starting a rebuilt executable. Compare
readback with the monitor's own on-screen indication. If a reading disagrees
with the panel, record the model, connection, operation, and raw reply.
Unsupported or ambiguous configurations should fail with an explicit error
rather than guess a target.

- With the AW2725QF explicitly on DisplayPort 1, compare **Retry reads** and **Refresh** with its on-screen input indication.
- With the DELL U2723QE explicitly on HDMI 1, verify **Retry reads** and **Refresh** show **HDMI 1**.
- Adjust brightness and contrast rapidly and switch between monitor pages. The final requested values must reach the intended displays.
- Refresh while changes are queued. Cancellation must be safe and the UI must stay responsive.
- Test a disconnected or DDC-unresponsive display. Errors must be visible, working features must stay independent, and retry must be able to recover.
- Rebind legacy numeric hotkey targets, save, restart, and verify the assignments persist.
- Disconnect, reconnect, or reorder monitors. Commands must not target a different physical display.
- Save and restore real layouts, including rotation, position, and any available multi-GPU or disabled-display cases.
- Apply a profile, then change brightness or input, including **All monitors**.
- Switch input on monitor A, then run an operation on monitor B.
- Exercise configuration recovery on a backed-up or disposable configuration. Do not damage the only copy of real settings.
- Smoke-test tray reopening, hotkeys, and application exit after queue activity.

## Known limitations

- Windows device paths identify device instances. They are not immutable physical identities across every port, dock, or driver change. A path change requires recapture or rebind.
- Ambiguous one-to-many physical associations are unsupported. Discovery fails for the whole snapshot instead of guessing from the model name.
- Scalar MCCS input-source readback uses the low byte only. A decoded reply is the monitor's reported setting; it does not prove which physical source is displayed or that a switch finished. This is not an MCCS 3.0 table-form input control.
- CCD remapping rejects missing identities, ambiguous routing, virtual or desktop-image layouts, and missing source modes. Applying a profile was confirmed on the maintainer's layouts, not on every virtual or multi-GPU arrangement.
- Hardware actions are not transactional. A later failure does not roll back a change that already completed. Manual **Refresh** cancels waiting work; an already-running hardware call finishes before discovery begins.

{% include footer.html %}
