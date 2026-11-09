# RAM CONTROL CENTER quality assessment

Date: 2026-11-06. Measurements are from the current project and the installed
vendor package; no BIOS, EC, driver or vendor binary was modified for them.

## Executive assessment

| Dimension | Score | Reading |
|---|---:|---|
| Protocol-layer cohesion | 8.8 / 10 | One transport boundary with typed telemetry, power/fan, RGB, GPU, charging, input and quick-toggle modules |
| OS-integration cohesion | 8.6 / 10 | Boot, tray, telemetry, input and display concerns have named modules; Win32 handles and resources are wrapped behind RAII helpers and every raw FFI block has a documented safety contract |
| Functional coverage | 7.6 / 10 | The tested CC4.0 controls work on this machine, but capability-gated branches and uncommon hardware paths still need broader validation |
| Visual design | 6.5 / 10 | The rail wordmark is the only in-content product mark, hardware state has one focused location, and navigation/switch geometry is clean; repeated bordered cards still read as an engineering control panel |
| Motion and interaction | 8.0 / 10 | A shared travelling navigation capsule, opacity-only page transitions, immediate control feedback, draggable RGB channels, interpolated palettes and expressive switches are implemented; reduced-motion behavior remains absent |
| Accessibility and responsiveness | 4.5 / 10 | No complete keyboard focus system, focus rings, screen-reader semantics or compact layout policy |
| Packaging and deployment | 7.6 / 10 | The canonical path and Git-based versioning are defined, the audited binary is running, and temporary deployment artifacts are removed |
| Overall | **7.5 / 10** | Solid native backend, verified hardware controls and a responsive motion system; visual-system depth, accessibility and edge-case parity still keep it below finished-product quality |

This is a functional native control panel, not an 8.9/10 finished product. The
backend architecture is substantially stronger than the product surface. The
next quality gain must come from interaction design, layout responsiveness,
accessibility and stricter end-to-end validation rather than another backend
split.

## 2026-11-06 audit

- Host Clippy across all targets and features now passes with `-D warnings`;
  Windows-only FlexiKey mapping helpers are gated out of non-test host builds.
- All 77 Win32/NVML/Shell `unsafe` blocks carry an individual `SAFETY:` contract.
  The Windows Clippy gate fails on any undocumented block.
- Page transitions use opacity only. Content geometry remains fixed, and a
  transparent input guard covers the workspace during the swap.
- Page, option, segmented-control and press-feedback transitions were exercised
  through live frame capture, not only compile checks.
- Light/dark switching interpolates palette colours and gradients over 260 ms
  with an ease-out-cubic curve; rapid repeated switches settle on the correct
  final theme.
- The navigation rail uses one shared 320 ms selection capsule. Native Windows
  frame capture verified intermediate capsule positions and continuous rail
  visibility.
- Quick toggles update their visible state before the worker command; observed
  Fn-lock feedback began about 26 ms after the click. Brightness segmented
  controls also move immediately.
- RGB sliders update the preview directly from pointer movement. A captured
  drag produced 13 consecutive preview colours while the R value moved from 85
  through 253.
- Thermal snapshots now carry the last valid sample through light refreshes.
  All-zero or implausible DCHU responses are rejected, and invalid values render
  as `--°C`; a 15-second native Windows capture kept CPU and GPU temperatures
  populated across the full interval.
- Remaining debt: the 1,213-line FlexiKey editor still performs XML/engine work
  synchronously on the UI thread; `wire_ui` (699 lines), `apply_update` (520
  lines) and `wire_flexikey` (544 lines) need functional decomposition; no
  automated visual-regression, keyboard-focus or reduced-motion suite exists.

## Code metrics

```text
total measured lines       19,974
Rust lines                 13,875
Slint lines                 5,033
translation lines           1,066
Rust source modules            45
Slint source modules           16
unsafe token occurrences      90
documented unsafe blocks       77
test functions                 44
host-executed tests            40
```

The source-line total reflects explicit module interfaces, typed page
properties, callback forwarding and hardware-smoke tooling. Composition-file
size and change isolation improved materially:

```text
                              before    after
crates/app/src/main.rs         4,865      131
crates/app/ui/main.slint       3,959      927
windows_backend.rs             1,736      481
largest windows_backend submodule  —      559
system_controls.rs             1,070      526
worker.rs                      1,483      330
largest app Rust module        4,865    1,213
```

Current application composition:

```text
main.rs             process entry and top-level wiring
ui_callbacks.rs     Slint callback wiring
worker.rs           worker state machine, command dispatch and retry loop
worker/             power/fan, RGB, input, I/O and snapshot responsibilities
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
  -> windows-registry / windows-service

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
  is confined to one 484-line module.
- Boot policy, native service registration, tray lifecycle, telemetry and
  FlexiKey hooks are separate modules.
- UI commands are serialized through a typed worker channel; ACPI traffic never
  runs on the renderer thread.
- Page components use explicit typed properties and callback forwarding, with
  page-local state kept behind each component.
- Win32 registry operations are behind safe helpers in
  `system_controls/registry.rs`; display-care and tray startup consume those
  helpers.
- Microsoft `windows-registry` owns registry handles and typed values; Mullvad
  `windows-service` owns SCM/service handles, dispatcher startup, status
  reporting and failure actions. The app contains no service-manager FFI.
- Service, registry, power-scheme, device-context, PDH and NVML resources are
  released through RAII wrappers, including all early-return paths.
- The application imports no `InsydeDCHU.dll`, `HKFltrLib.dll`, `CC40.exe` or
  `FnKey.exe` as a control backend.

Remaining debt:

- `flexikey_ui.rs` is 1,213 lines; its editor/profile state is the next
  candidate for functional split. The hardware worker is now 330 lines plus
  five focused submodules.
- There are 90 `unsafe` token occurrences and 77 raw blocks. They are localized
  around Windows IOCTL/FFI, with `telemetry.rs` (18 blocks), `tray.rs` (20) and
  `flexikey_engine.rs` (8) as the largest clusters. Every block has a specific
  `SAFETY:` invariant. `scripts/check-unsafe.sh` ratchets the token count and
  Clippy rejects undocumented blocks.
- The automated suite covers protocol encoding and app-side logic. Live
  AcpiBridge/EC behavior is exercised by `scripts/hardware-smoke.ps1`, which
  cannot run on a generic CI runner.
- GitHub Actions runs the Linux software gate and a native Windows test job;
  the latest result is visible in the repository's Actions tab.

## Verification gate

`scripts/verify.sh` is the single local gate:

```text
cargo fmt --all -- --check
./scripts/check-unsafe.sh
cargo test --workspace
cargo clippy --workspace --all-targets \
  --target x86_64-pc-windows-gnu -- \
  -D warnings -D clippy::undocumented_unsafe_blocks
cargo build --release --target x86_64-pc-windows-gnu --workspace
```

The script prefers the rustup-managed toolchain when `$HOME/.cargo/bin` exists,
then uses the configured Zig cross linker. `.github/workflows/ci.yml` runs the
same script on GitHub-hosted Ubuntu with Rust and Zig 0.14.1.

Local result on 2026-11-06:

```text
rustfmt                                      passed
host tests                                   39 passed
Windows-target clippy with both hard gates   passed
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

Current RAM CONTROL CENTER:

```text
neo-control.exe          11,046,912 bytes
```

```text
Neo / CC40.exe                       36.46%  (63.54% smaller)
Neo / (CC40 + DLLs)                  33.23%  (66.77% smaller)
```

The latest background process sample was:

```text
neo-control.exe   working set 132,206,592 bytes  private 145,522,688 bytes
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
`C:\ProgramData\NeoControl\neo-control.exe`. All eleven page components were
captured and reviewed at the production 1180×820 size: overview, performance,
cooling, fan curve, battery, system, lighting, FlexiKey, input/output, display
care and settings. They rendered with live telemetry and the original theme.
The Settings page still reports GPU composition. The rail logo is the sole
in-content wordmark, and Overview carries the hardware-online indicator.

After the backend/control split, the hardware smoke suite again exercised live
telemetry, camera/touchpad/airplane restore, Windows display care and DCHU
settings offsets 45-48.

## Final assessment

Maintainability work is structurally complete for this slice. The entry point,
Slint window, protocol backend, Win32 system controls and hardware worker are
split into domain modules, the worker/UI boundary remains typed, and automated
plus hardware-backed verification are documented. Remaining work is narrower:
splitting the FlexiKey editor, reviewing the remaining FFI clusters and adding a
hardware runner separate from generic CI.
