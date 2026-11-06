# Verification log

Date: 2026-11-07
Target: Firebat/Clevo ZY-T6K, Windows host, signed AcpiBridge device

## Build

```text
target/x86_64-pc-windows-gnu/release/neo-control.exe
target/x86_64-pc-windows-gnu/release/neo-dchu-probe.exe
```

Last successful Release cross-build after the final audit fixes:

```text
neo-control.exe        11,046,912 bytes
sha256                 05e5eb53c6461a70a2de2702ee4124437d0a19f77c2548afe5f6255584f32b9f
neo-dchu-probe.exe     347,648 bytes
```

The main executable imports no `InsydeDCHU.dll`, `HKFltrLib.dll`, `CC40.exe` or
`FnKey.exe`. Its transport imports are limited to Windows system DLLs such as
`kernel32.dll` and `ntdll.dll`.
The original user-mode DLL role is implemented directly in
`neo-control-protocol`; no replacement DLL is loaded at runtime.

Final source gates:

```text
cargo fmt --all -- --check                         passed
cargo test --workspace                             40 passed
cargo clippy --workspace --all-targets \
  --target x86_64-pc-windows-gnu -- \
  -D warnings -D clippy::undocumented_unsafe_blocks passed
cargo build --release --target x86_64-pc-windows-gnu --workspace  passed
clean-target full verification                     passed
```

The same sequence is now the single local gate in `scripts/verify.sh` and is
also configured in `.github/workflows/ci.yml`, with a native Windows test job.

The separate hardware gate is `scripts/hardware-smoke.ps1`. It passed against
the deployed executable, including camera/touchpad/airplane restore, display
care readbacks and DCHU offsets 45-48.

## Direct transport reads

Live probe through:

```text
\\?\ACPI#CLV0001#1#{86994c74-ad43-4812-b7e7-0c420b5c5fd7}
```

Verified successfully:

- command 12 thermal buffer, including CPU/GPU RPM, duty and temperature;
- integer commands 16 and 70;
- settings page 1 offsets 1, 16, 36, 37, 38, 43 and 44;
- settings page 4 offsets 5 and 7;
- command 13 runtime fan curve;
- command 4 GPU mode read: current mode 3, supported mask `0x06`;
- full settings store read, exactly 4096 bytes.

All UI temperatures use an explicit `°C` suffix.

## Direct power and fan writes

A same-value smoke test re-applied the current values:

```text
Performance -> Performance
Automatic -> Automatic
```

Both writes persisted and read back successfully through the direct
AcpiBridge transport.

The full power transition was then re-applied:

```text
WriteAppSettings(1, 16, [2])
WriteAppSettings(1, 1,  [2])
SetDCHU_Data(121, [2, 0, 0, 25])
SetDCHU_Data(79,  [0, 0, 0, 0])
```

Readback after the transition:

```text
settings[1,1,1]  = 02
settings[1,16,1] = 02
DCHU command 79  = success
```

The boot-agent log contains `power reapplied and read back: Performance`.
The same transition now sends the second-stage boost command required for the
CPU/EC to leave its cold-boot low-power state.

## CPU telemetry

The CPU clock is sampled from Windows PDH `% Processor Performance` against the
reported nominal/base frequency rather than a hard-coded value. A live sample
during verification was:

```text
clock=4782 MHz
nominal=3601 MHz
```

The current clock therefore changes with load; the nominal value is displayed
separately.

## Boot service

The same executable is registered as the native Windows service
`NeoControlBoot`:

```text
"C:\ProgramData\NeoControl\neo-control.exe" --boot-service --wait-ms 120000
```

Verified service configuration:

- display name: `Neo Control Boot Agent`;
- account: `LocalSystem`;
- start mode: automatic;
- dependency: signed `AcpiBridge` driver;
- no window and no persistent helper process.

The installer also configures SCM failure actions for 1 s, 2 s and 5 s retries
and enables them for non-crash service failure exits. A live reinstall/query
confirmed the configured actions, the service returned to `STOPPED` with exit
code 0, and the boot log recorded another `Performance` replay:

```text
power reapplied and read back: Performance
agent complete in 6 ms (1 attempts)
```

A live Service Control Manager start completed successfully:

```text
state after run: STOPPED
win32 exit code: 0
service exit code: 0
power reapplied and read back: Performance
agent complete in 7 ms (1 attempt)
```

Five SCM start-to-stop measurements completed in 60, 67, 69, 70 and 163 ms.
The first run paid the cold file-cache cost; the worker portion was 6-7 ms.
Install, reinstall and uninstall were exercised through the executable's own
service commands, and the reinstall returned the same service to the stopped
state with both exit codes at zero.

The boot path starts the signed AcpiBridge driver service on demand, retries at
10 ms, writes the full power transition and intentionally skips the expensive
thermal read before restoring the power profile. It does not write Auto fan
mode or create a Slint window.

## FlexiKey XML model

The protocol crate now has unit coverage for:

- parsing all four CC4.0 action types (`keypress`, `launch`, `text`, `disable`);
- round-tripping the vendor XML schema;
- skipping malformed bindings without losing the profile;
- converting keypress bindings to the signed keyboard-filter frame;
- changing the active profile in the model;
- adding, replacing and removing bindings;
- preserving vendor launch event IDs and text start/send keys during edits;
- accepting decimal and hexadecimal trigger codes while rejecting unsupported
  mouse buttons.

A live UI run loaded a temporary four-profile `profile.xml`, rendered the
FlexiKey page in Chinese, selected the second profile, and persisted it through
the atomic XML replacement. The resulting file contained `Active="1"` with
only the Work profile selected, the temporary `.neo.tmp` file was gone, and the
action engine logged a profile reload. The save-and-reload behavior also has an
app unit test.

The FlexiKey page and editor were rendered at the production 1180×820 size.
The editor exposes device and action chips, key capture, key code and
display-name fields, action-specific input, delete, cancel and atomic save
controls. The single-line path field stays on one line; long paths scroll
rather than wrapping into the field border.

A live editor run then exercised the real Windows hooks and profile writer:

```text
trigger capture armed
trigger captured: device=2 code=0x0040 name=Mouse Back
```

Saving that binding was rejected without touching the file because `0x40`
already belonged to another binding. The validation message named the duplicate
trigger. A second run captured the unused left button (`0x01`), saved
successfully, updated `profile.xml`, and reloaded the engine:

```text
profile loaded: 1 keyboard action(s), 3 mouse action(s)
```

The written first binding became `Device="2" KeyCode="1"` with the localized
display name `鼠标左键`. Deleting that binding then reduced the XML to three
`Set_` entries and reloaded:

```text
profile loaded: 1 keyboard action(s), 2 mouse action(s)
```

The temporary test profile was restored from its pre-test copy afterwards; the
runtime `C:\ComboKey\profile.xml` was not modified by this test.

FlexiKey profile management is implemented in the same transactional path:
new profiles are appended and selected, rename trims and validates the name,
and delete refuses to remove the final profile. The model unit tests cover all
three operations. The live UI created a second profile and persisted it as
`Active="1"`; the new profile row, name field, name-save button, two-step delete
button and reload control were rendered at the production size without
clipping.

## Panel low refresh rate

Live reads through the direct settings channel:

```text
settings[1,27,1] = 01   support
settings[1,28,1] = 01   current state
```

A same-value write to `settings[1,28,1]` completed and read back `01`. The
System page gates the control to Power saving and Quiet+ modes, matching the
vendor behavior.

## Display care

The CC4.0 `InsydeDriver.WriteAppSettings(index, length, buffer)` wrapper treats
the first argument as a linear settings index. Its `45..48` calls therefore map
to page 0 offsets, not page numbers. The direct transport was corrected to use
`page 0 / offsets 45..48` before enabling the UI write path.

The `--display-care-smoke` command returned exit code 0. Its same-value pass
covers taskbar auto-hide, taskbar transparency, dark appearance, AC/DC monitor
timeouts, screen saver, and DCHU page 0 offsets 45–48. The UI readback showed
`taskbar_hide=false`, `transparency=true`, `dark=true`, monitor timeouts
`0/0`, and screen saver `disabled / none / 0 min`.

The production UI was rendered at 1180×820 and the Display care page was
inspected in Chinese. The LCD badge, three immediate Windows toggles, AC/DC
monitor timeout steppers, screen-saver mode chips and apply controls fit without
clipping.

Live UI writes:

```text
Taskbar auto-hide: enabled  -> "显示保护设置已应用并读回。"
Taskbar auto-hide: restored -> "显示保护设置已应用并读回。"
Screen saver: same-value    -> "屏幕保护程序已应用并读回。"
```

Both DCHU mirror writes and Windows readbacks succeeded. The taskbar was left
in its original visible state.

## FlexiKey action engine

The app-level action engine is covered by unit tests for CC4.0 scan-code
mapping and profile persistence. A live test enabled the opt-in engine log,
loaded an active profile with an F24 launch binding, injected F24 into the
desktop, and verified:

```text
keyboard hook active
profile loaded: 2 keyboard action(s), 2 mouse action(s)
launch action dispatched: C:\Windows\Temp\neo-flexikey-action.cmd
marker file created: true
```

The XML replacement path is atomic through a `.neo.tmp` file and
`MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. The saved active profile was reloaded
successfully in the live UI test and unit test.
Keypress, text, launch and disable share the same hook table. The hook lives in
the resident tray process, so closing the main window does not stop profile
dispatch.

Mouse dispatch was verified separately with XBUTTON1 (`Mouse Back`, vendor key
code `0x40`):

```text
mouse xbutton event: message=0x020b data=0x00010000
launch action dispatched: C:\ProgramData\NeoControl\mouse-marker.cmd
mouse xbutton event: message=0x020c data=0x00010000
marker file created: true
```

The down edge dispatched the launch action; the up edge was consumed by the
binding hook and did not produce a second action.

### Focused-window integration

A temporary WinForms text box was given focus, with IME composition disabled
so the ASCII result was deterministic. The active profile mapped:

```text
mouse back    -> text "Hello Neo 世界"
mouse forward -> keypress B
keyboard A    -> disable/swallow
```

The harness injected XBUTTON1, XBUTTON2 and A in sequence. The target text box
contained the exact UTF-8 text:

```text
Hello Neo 世界b
```

The engine log recorded both `text action dispatched` and
`keypress action dispatched`; the A trigger logged `trigger disabled` and was
not inserted into the target. With an active IME, scan-code keypresses behave
like the physical key and may enter composition, while text actions use
Unicode and bypass the IME.

The keypress sender now resolves the scan code to a virtual key with
`MapVirtualKeyW(MAPVK_VSC_TO_VK_EX)` and supplies both values to `SendInput`,
falling back to scancode-only injection when Windows has no VK mapping. This
keeps ordinary text fields and app shortcuts working while preserving the
vendor scan-code action model.

## Tray residency and logon startup

Verified through real Windows window handles:

```text
background: process alive, main window at (-30000, -30000), visible false
tray window: present
second launch: new process exits, existing window restored at (362, 110)
tray exit: process exits and tray window is removed
```

The embedded ICO was accepted by `CreateIconFromResourceEx`; the engine log
reported `tray icon: embedded logo` and `tray icon installed`. The current-user
Run entry was installed, removed, and reinstalled successfully:

```text
"C:\ProgramData\NeoControl\neo-control.exe" --background
```

No scheduled task or helper executable was added for tray residency.

## Anti-dust schedule backend

CC40's schedule layout was recovered from the installed `FAN.cs` and added to
the protocol crate:

```text
packed = day + (resume << 7) + (hour << 8)
SetDCHU_Data(121, [packed & 0xff, (packed >> 8) & 0xff, packed >> 16, 40])
WriteAppSettings(4, 81, [1, day | (resume << 7), hour, 0, 0])
ReadAppSettings(4, 81, 3)
```

The read/encode path has unit coverage and the Cooling page exposes a run-now
button plus weekday/hour/resume scheduling. A live same-device cycle restored
the previous schedule after a reversible disable test:

```text
before        = enabled, day 0, hour 0
disabled      = disabled, day 127, hour 0
after restore = enabled, day 0, hour 0
```

The readback and rollback path is now verified against the live device.

## Keyboard filter transport

The signed `HKKbdFltr.sys` interface is reached directly without
`HKFltrLib.dll`:

```text
\\.\{2dc8164f-3a23-4d71-9fa6-91f9ead88613}
```

Verified live:

- `CreateFileW` on the filter interface succeeds;
- `DeviceIoControl(0xBA400, [1])` enables the filter;
- `DeviceIoControl(0xBA400, [0])` disables the filter;
- `DeviceIoControl(0xBA40C)` clears the filter macro set;
- `DeviceIoControl(0xBA41C, [1])` enables the USB filter device;
- filter enable was restored after the test.

The 16-byte collection layout was cross-checked against the vendor collection
builder:

```text
key code:     mode=0, down_delay, up_delay, key, kind=0
macro down:   mode=0, delay, 0, key, kind=1
macro up:     mode=0, 0, delay, key, kind=2
macro frame:  u32 trigger_size, u32 action_size, trigger records, action records
```

The driver macro add/remove path is intentionally not used by the UI. The
resident user-mode action engine covers keypress, launch, text and disable
bindings, while the signed filter device is used for filter enable/disable,
USB filtering and verified clearing. A direct `0xBA404` smoke write returned
success, but the matching `0xBA408` removal returned Windows error `1168`
(`ERROR_NOT_FOUND`); the transactional complete-set path remains the safe
driver-level route if it is needed for compatibility.

## Renderer

- Slint notifier: `GraphicsAPI::NativeOpenGL`.
- GPU 3D engine activity was observed while the window is visible.
- UI animation and hardware polling are separated by a worker channel.
- Navigation updates the rail immediately. One shared capsule travels vertically
  over 320 ms, and the page content dims to 74% over 90 ms, swaps at a fixed
  position, then returns to full opacity over 170 ms.
- Page content uses opacity only during a switch. It does not translate, scale,
  or place a sheet over the workspace. A transparent input guard covers the
  content viewport while the swap is active.
- Native captures showed stable content geometry through the swap. The maximum
  blue-dominant pixel fraction in the content area was 0.093, confined to normal
  accent surfaces rather than a full-area overlay.
- Segmented controls share a single moving highlight pill rather than changing
  each button independently; supported groups use a 240 ms `ease-out-back`
  translation. Pointer presses also render a radial highlight like KernelSU's
  InteractiveHighlight.
- Brightness and effect choices update the requested state before the worker
  command is queued. The selected pill therefore moves immediately, and the
  page does not wait for a hardware response.
- RGB uses three draggable channel tracks. The preview swatch binds directly to
  the requested channel values with no intermediate release event.
- Light/dark switching interpolates the complete palette and gradient set over
  260 ms with an ease-out-cubic curve. Rapid repeated toggles settle on the
  correct final theme and segmented-control selection.
- Option controls share the same motion system: 200–220 ms surface/radius
  morphs, 240 ms `ease-out-back` selection indicators, 240 ms toggle-thumb
  travel, and 260 ms pressed-scale release feedback.
- Debug snapshots at the production 1180×820 size were used to verify the
  overview, performance, cooling, battery, settings, FlexiKey and display-care
  layouts after the empty status-container fix. Windows frame tracing remains
  the final native-runtime confirmation and is kept separate from the visual
  claim.
- `scripts/check-unsafe.sh` fails if a new source file gains an `unsafe` token
  or any allowlisted FFI file exceeds its current ceiling. The current total is
  83 tokens across 70 raw blocks. All 70 blocks have individual `SAFETY:`
  comments, and Windows Clippy fails on any undocumented block.

## 2026-11-06 interaction audit

The final interaction audit was performed against the native Windows process.
The Release PE imports `opengl32.dll` and system DLLs only, with no
`InsydeDCHU.dll`, `HKFltrLib.dll`, `CC40.exe` or `FnKey.exe` dependency.

- Native Windows screen captures verified the fixed top header, the single
  hardware-online pill in Overview, and the absence of global status banners.
- Fn lock changed its visible switch state approximately 26 ms after the click;
  the page stayed in place and no status banner appeared.
- The Lighting brightness capsule moved through intermediate positions and
  settled on the chosen option; the first visible motion appeared about 20 ms
  after the click.
- Dragging the R slider from 85 to 253 produced 13 sampled preview colours from
  `(87, 80, 200)` through `(170, 80, 200)` to `(253, 80, 200)`, confirming the
  preview follows pointer movement continuously.
- The Settings `Start with Windows` row was captured on Windows; the switch track
  and label both measured `center_y = 526.5 px`, a 0 px vertical offset.
- Idle frame diagnostics reported zero rendered frames per two-second interval;
  animation intervals produced continuous frame counts. Rendering remains GPU
  backed rather than a CPU software-rendering fallback.

## Thermal stability audit

The worker stores the latest valid thermal sample in `WorkerRuntime`. Full DCHU
readbacks refresh that cache; the intervening one-second snapshots reuse it
instead of sending an empty thermal update to the UI. Samples with no plausible
CPU or GPU temperature are ignored, and the cache is cleared only after three
consecutive invalid or failed full reads.

Temperature formatting maps values outside `1..=125 °C` to `--°C`, so an
all-zero EC response cannot render as `0°C`.

Native Windows screenshots taken at 0, 6 and 15 seconds showed:

```text
CPU   80°C → 92°C → 89°C
GPU   59°C → 59°C → 60°C
```

The temperature area remained populated for the full capture window. The action
log recorded only valid cached samples, with no ignored or failed thermal reads
in that interval.

The Windows deployment was relaunched with the audited binary and captured from
its real Win32 window. Temporary scripts, BMP/PNG frame series, and logs were
removed after verification; `C:\ProgramData\NeoControl` contains only the
canonical executable and boot metadata.

Thermal command 12 still takes roughly 330 ms at the firmware level. It runs on
the hardware worker and does not block the UI thread.

The worker uses a 1 s light system sample and a 5 s full thermal/extended read.
Completed control commands publish their status through a light snapshot first;
the next full read reconciles persisted and hardware values.


## Maintainability refactor runtime check

The refactored Release executable was deployed to
`C:\ProgramData\NeoControl\neo-control.exe` and exercised on the Windows host:

```text
visible window handle    median 63.1 ms   range 47.2–92.3 ms
background handle        median 62.9 ms   range 62.1–64.0 ms
hooks + tray + worker    median 466.9 ms  range 447.9–496.7 ms
```

All eleven extracted pages were captured at the production size: Overview,
Performance, Cooling, Fan curve, Battery, System, Lighting, FlexiKey,
Input & I/O, Display care and Settings. They retained live telemetry, Chinese
labels, the rail wordmark, GPU composition and the accepted motion system. No
captured page was blank or clipped.


## Backend/control module split

The protocol transport and app-side Win32 controls were split without changing
their public APIs:

```text
protocol/src/windows_backend.rs               484 lines
protocol/src/windows_backend/telemetry.rs     109 lines
protocol/src/windows_backend/power_fan.rs     559 lines
protocol/src/windows_backend/rgb.rs           233 lines
protocol/src/windows_backend/gpu.rs            89 lines
protocol/src/windows_backend/charging.rs      148 lines
protocol/src/windows_backend/input.rs          70 lines
protocol/src/windows_backend/quick_toggles.rs  95 lines

app/src/system_controls/mod.rs                 17 lines
app/src/system_controls/audio.rs              111 lines
app/src/system_controls/display.rs             96 lines
app/src/system_controls/power.rs              126 lines
app/src/system_controls/display_care.rs       465 lines
app/src/system_controls/radio.rs               85 lines
app/src/system_controls/registry.rs           108 lines
```

`windows_backend.rs` now contains handle lifecycle, raw IOCTL/settings access
and shared rollback helpers. The domain modules expose the same inherent
`Dchu` methods, so callers did not gain a new layer. Registry access is behind
safe helpers used by display-care and tray-startup code. Service, registry,
power-scheme, device-context, PDH and NVML resources now use RAII wrappers.

## Driver health and driver-only installation

### Required dependency audit

A fresh code-path audit found that the application opens `AcpiBridge` for the
DCHU transport and does not call `KeyboardFilter::open` from the FlexiKey
engine. The current FlexiKey implementation uses `WH_KEYBOARD_LL` and
`WH_MOUSE_LL`; `HKKbdFltr` remains a protocol-level optional path, not a current
runtime requirement.

### Live detection

Run on the Windows host:

```text
neo-control.exe --driver-smoke
acpi_ready=true
acpi_installed=true
keyboard_filter_ready=true
keyboard_filter_installed=true
acpi_payload_ready=true
keyboard_payload_ready=true
payload_path=C:\ProgramData\NeoControl\drivers
```

The driver payload exists only in the local ProgramData deployment. It is
ignored by Git.

### Idempotent installer helper

Run on the Windows host with the driver already available:

```text
neo-control.exe --install-drivers
exit=0
installer: start
installer: AcpiBridge already responding; nothing to install
```

This verified the helper's no-op path and log output. The UAC/`pnputil` path
still needs a clean-machine test with the required device absent.

### Overview UI

The driver-health card is rendered on the Overview page. It shows the required
`AcpiBridge` state, the optional `HKKbdFltr` state, payload presence, and an
explicit install button when the required driver is unavailable. Driver files
are loaded from the executable-adjacent `drivers/` directory or the
ProgramData fallback.
