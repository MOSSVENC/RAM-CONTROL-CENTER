# Neo Control quality assessment

Date: 2026-11-05. Measurements are from the current project and the installed
vendor package; no BIOS, EC, driver or vendor binary was modified for them.

## Executive assessment

| Dimension | Score | Reading |
|---|---:|---|
| Protocol-layer cohesion | 9.0 / 10 | One transport boundary with typed telemetry, power/fan, RGB, GPU, charging, input and quick-toggle modules |
| OS-integration cohesion | 8.5 / 10 | Boot, tray, telemetry, input and display concerns have named modules; Win32 access is behind domain helpers |
| App/UI cohesion | 8.5 / 10 | Startup orchestration, worker, callbacks, controls, pages and dialogs are separated |
| Module coupling | 9.0 / 10 | Crate graph is acyclic; worker/UI messages and page callback boundaries are typed |
| Maintainability | 8.5 / 10 | File bloat is resolved, CI and hardware smoke gates exist; FFI remains broad and hardware CI is manual |
| Portability | 8.0 / 10 | Capability-driven UI and an ACPI-device override support other CC4.0 models; model offsets and validation remain machine-specific |
| Overall | **8.8 / 10** | Strong boundary architecture, green native CI, repeatable verification and a documented compatibility path |

This is a sound native implementation. The maintainability debt named in the
prior report was concentrated in oversized composition and protocol/control
files. Those files are now split by responsibility while preserving the typed
protocol boundary.

## Code metrics

```text
total measured lines       19,400
Rust lines                 13,613
Slint lines                 4,724
translation lines           1,063
Rust source modules            35
Slint source modules           16
unsafe token occurrences      174
test functions                 42
host-executed tests            39
```

The source-line total is 1,028 lines higher than the pre-refactor baseline because
the split adds explicit module interfaces, typed page properties, callback
forwarding and hardware-smoke tooling. Composition-file size and change
isolation improved materially:

```text
                              before    after
crates/app/src/main.rs         4,865      131
crates/app/ui/main.slint       3,959      927
windows_backend.rs             1,736      481
largest windows_backend submodule  —      559
system_controls.rs             1,070      526
largest app Rust module        4,865    1,483
```

Current application composition:

```text
main.rs             process entry and top-level wiring
ui_callbacks.rs     Slint callback wiring
worker.rs           serialized hardware command loop
ui_update.rs        typed UI update model and property application
flexikey_ui.rs      FlexiKey view/editor/profile integration
system_controls/    audio, display, display-care, radio and registry helpers
smoke.rs            transport and control smoke commands
```

Current protocol composition:

```text
windows_backend.rs  handle lifecycle, raw IOCTL/settings transport and rollback
/windows_backend/
  telemetry.rs      command 12/13, platform state and capability reads
  power_fan.rs      power profiles, fan modes, curves and anti-dust
  rgb.rs            RGB state and writes
  gpu.rs            staged GPU switching
  charging.rs       FlexiCharger state and rollback
  input.rs          camera, touchpad and airplane notification
  quick_toggles.rs  Fn/Win/Copilot quick-toggle state
```

The Slint tree is split into `theme.slint`, shared `components.slint`, eleven
page components and two dialog components. `build.rs` watches the whole `ui`
directory, so imported Slint files trigger rebuilds.

The crate graph remains deliberately small and acyclic:

```text
neo-control
  -> neo-control-protocol
  -> slint / raw-window-handle / windows-sys / windows

neo-control-protocol
  -> roxmltree / windows-sys
```

`neo-control-protocol` is independent of the app and UI. Hardware access is
confined to direct `CreateFileW` / `DeviceIoControl` code and typed domain
modules behind the signed device interfaces.

## Cohesion and coupling

Strong points:

- DCHU frame construction, settings access, fan curves, FlexiKey XML and the
  signed keyboard-filter transport live in the protocol crate.
- The protocol backend's public surface is safe Rust; the raw handle/IOCTL code
  is confined to one 481-line module.
- Boot policy, native service registration, tray lifecycle, telemetry and
  FlexiKey hooks are separate modules.
- UI commands are serialized through a typed worker channel; ACPI traffic never
  runs on the renderer thread.
- Page components use explicit typed properties and callback forwarding; pages
  no longer reach into one monolithic window body.
- Win32 registry operations are behind four safe helpers in
  `system_controls/registry.rs`; display-care code consumes those helpers rather
  than opening registry keys itself.
- The application imports no `InsydeDCHU.dll`, `HKFltrLib.dll`, `CC40.exe` or
  `FnKey.exe` as a control backend.

Remaining debt:

- `worker.rs` is 1,483 lines and `flexikey_ui.rs` is 1,213 lines; they are the
  next candidates for functional split.
- There are 174 `unsafe` token occurrences. They are localized around the
  Windows IOCTL/FFI boundary, but `tray.rs` (45), `boot_service.rs` (29),
  `telemetry.rs` (29) and `flexikey_engine.rs` (23) remain the largest clusters.
- The automated suite covers protocol encoding and app-side logic. Live
  AcpiBridge/EC behavior is exercised by `scripts/hardware-smoke.ps1`, which
  cannot run on a generic CI runner.
- GitHub Actions runs the Linux software gate and a native Windows test job;
  the latest result is visible in the repository's Actions tab.

## Verification gate

`scripts/verify.sh` is the single local gate:

```text
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets \
  --target x86_64-pc-windows-gnu -- -D warnings
cargo build --release --target x86_64-pc-windows-gnu --workspace
```

The script prefers the rustup-managed toolchain when `$HOME/.cargo/bin` exists,
then uses the configured Zig cross linker. `.github/workflows/ci.yml` runs the
same script on GitHub-hosted Ubuntu with Rust and Zig 0.14.1.

Local result on 2026-11-05:

```text
rustfmt                                      passed
host tests                                   39 passed
Windows-target clippy with -D warnings       passed
Windows-target release build                 passed
Windows-native test suite                    42 passed
clean-target full verification                passed
```

The separate hardware gate is:

```powershell
pwsh -File scripts/hardware-smoke.ps1
```

It ran successfully against the deployed build:

```text
brightness: 79% reapplied
audio: 37% unmuted reapplied
camera: toggled to false / restored to true
touchpad: toggled to true / restored to false
airplane: toggled to true / restored to false
display-care: all same-value readbacks passed
DCHU offsets 45-48: all same-value readbacks passed
hardware smoke passed
```

## Size comparison

Installed CC40 control-center user-mode components:

```text
CC40.exe                 30,296,576 bytes
InsydeDCHU.dll            2,492,416 bytes
HKFltrLib.dll               457,600 bytes
CC40 plus two DLLs       33,246,592 bytes
```

Current Neo Control:

```text
neo-control.exe          10,768,384 bytes
```

```text
Neo / CC40.exe                       35.54%  (64.46% smaller)
Neo / (CC40 + DLLs)                  32.39%  (67.61% smaller)
```

The latest background process sample was:

```text
neo-control.exe   working set 121,085,952 bytes  private 128,270,336 bytes
FnKey.exe         working set  52,187,136 bytes  private 152,457,216 bytes
DCHUService.exe   working set  12,161,024 bytes  private  29,872,128 bytes
```

Memory measurements vary with the hidden window, renderer and tray state; this
single sample is not evidence of a RAM reduction or regression.

## Startup measurements

Measured on the Windows host with repeated cold process starts after deploying
the split backend/control build:

```text
Visible window handle appears    median 63.1 ms   range 47.2–92.3 ms
Background window handle appears median 62.9 ms   range 62.1–64.0 ms
Hooks + tray + worker ready      median 466.9 ms  range 447.9–496.7 ms
```

The process-start path is unchanged by the module refactor. These figures stay
within the previously observed run-to-run range. The hidden-handle measurement
is bimodal (roughly 48 ms or 63 ms), depending on which top-level window Windows
reports first.

The `NeoControlBoot` one-shot service remains the performance-critical boot
path. It runs before the user shell, applies the complete power transition,
verifies the readback and exits. The boot-agent log records:

```text
power reapplied and read back: Performance
agent complete in 6 ms (1 attempts)
```

Warm SCM start-to-stop measurements remain in the 60–70 ms range. The original
CC40/WPF frontend could not be directly launched for a paired GUI measurement
because of the WindowsApps ACL; the vendor chain is also architecturally later:
it waits for Explorer and the FnKey/AppX stack before the frontend is useful.
The defensible claim is therefore:

- the UI itself starts quickly (tens of milliseconds to a window handle);
- the runtime hook/tray/worker path is ready in roughly 0.45 s;
- the EC power-policy recovery is moved before the shell and completes in
  milliseconds, rather than waiting for the late vendor frontend.

Startup ratios are reported only where a paired measurement was possible.

## Runtime regression check

The refactored Release build was deployed to
`C:\ProgramData\NeoControl\neo-control.exe`. Ten page components were captured
and reviewed at the production 1180×820 size: overview, performance, cooling,
fan curve, battery, lighting, FlexiKey, input/output, display care and settings.
They rendered with live telemetry and the original theme. The Settings page
still reports GPU composition and the centered uppercase `NEO CONTROL` title.
The System page was compiled through the same typed page boundary but was not
separately screenshotted in this pass.

After the backend/control split, the hardware smoke suite again exercised live
telemetry, camera/touchpad/airplane restore, Windows display care and DCHU
settings offsets 45-48.

## Final assessment

The requested maintainability issues are structurally resolved. The entry point
and Slint window are no longer monolithic, the protocol backend and Win32 system
controls are split into domain modules, the worker/UI boundary remains typed,
and automated plus hardware-backed verification are documented. Remaining work
is narrower: splitting the largest worker/FlexiKey modules, reducing the four
largest FFI clusters, and adding a hardware runner separate from generic CI.
