# Neo Control architecture

## Stack

```text
Slint UI (Rust, GPU-rendered)
        │
        │ typed commands on one worker thread
        ▼
neo-control-protocol
        │ CreateFileW + DeviceIoControl
        ▼
signed AcpiBridge device          signed HKKbdFltr device
        │                                  │
        ▼                                  ▼
     ACPI / EC                    keyboard filter / macros
```

The accepted runtime is a two-crate Rust workspace with a native Slint UI.
Hardware access is implemented through the signed `AcpiBridge` and
`HKKbdFltr` device interfaces; firmware and driver signatures remain unchanged.

## Application composition

The application entry point only performs argument dispatch, single-instance
creation, Slint window startup, tray setup and worker launch. Hardware and UI
concerns are isolated behind named modules:

```text
main.rs             process entry and top-level composition
ui_callbacks.rs     Slint callback -> typed WorkerCommand wiring
worker.rs           worker state machine, command dispatch, retry and UI handoff
worker/             power/fan, RGB, input, I/O controls and snapshot sampling
ui_update.rs        typed update snapshot -> Slint property application
flexikey_ui.rs      FlexiKey page/editor state and profile persistence
system_controls/    audio, display, display-care, radio and registry helpers
boot_service.rs     safe service lifecycle through the windows-service crate
smoke.rs            reversible transport/control smoke commands
```

The Slint tree mirrors the same boundaries:

```text
ui/theme.slint             shared palette and gradients
ui/components.slint        reusable controls and telemetry widgets
ui/pages/*.slint           one component per navigation page
ui/dialogs/*.slint         Quiet and FlexiKey editor dialogs
ui/main.slint              MainWindow composition and callback contract
```

Page components receive typed properties and forward callbacks to the window;
they do not depend on a monolithic UI file. Imported Slint files are covered by
the `build.rs` rebuild watch.

## Transport

## Deployment naming

The installed executable has one canonical name: `neo-control.exe`. Source
versions are represented by Git commits; build output is never deployed as
`-final`, `-preview` or numbered side-by-side executables. Replacing the running
app is a stop, atomic file replacement and restart operation against the single
canonical path.

Device interface:

```text
\\?\ACPI#CLV0001#1#{86994c74-ad43-4812-b7e7-0c420b5c5fd7}
```

The default path is the validated ZY-T6K `CLV0001` interface. Set
`NEO_CONTROL_ACPI_DEVICE` to test another CC4.0 model path.

The protocol crate builds the recovered DCHU request frames directly:

- `DeviceIoControl(0x322400)` for `_DSM` DCHU commands;
- `DeviceIoControl(0x32240C)` for app-settings reads and writes.

ACPI responses use the `AeoB` frame header and typed descriptors. The parser
rejects truncated headers, truncated descriptors, short integer records and
output buffers that received fewer bytes than requested.

`win_device.rs` is the common signed-device open/ioctl/close boundary used by
both AcpiBridge and HKKbdFltr. `windows_backend.rs` keeps DCHU frame assembly,
settings access and shared rollback helpers. Its domain modules contain only
the typed platform operations:

```text
windows_backend/telemetry.rs       command 12/13, platform and capabilities
windows_backend/power_fan.rs       power, fan modes, curves and anti-dust
windows_backend/rgb.rs             RGB state and writes
windows_backend/gpu.rs             staged GPU switching
windows_backend/charging.rs        FlexiCharger state and rollback
windows_backend/input.rs           camera, touchpad and airplane notification
windows_backend/quick_toggles.rs   Fn/Win/Copilot state
```

The public `Dchu` API remains inherent methods; callers do not need a second
abstraction layer or bridge.

## FlexiKey action engine

The Slint process also hosts the FlexiKey action engine:

```text
profile.xml
    │ parse / activate
    ▼
FlexiKeyStore ── editor / atomic XML save
    │
    ▼
active keyboard + mouse action table
    │
    ▼
WH_KEYBOARD_LL + WH_MOUSE_LL thread ── trigger match ── action worker
                                              │
                         SendInput / ShellExecuteW
```

The hook thread only matches, captures and queues actions. A separate worker
owns key injection, text input and process launch so the low-level hook returns
quickly. Injected events use a private `dwExtraInfo` marker to avoid recursive
macros. The action table is replaced after profile activation, binding edits
and deletions; the XML replacement is performed through `MoveFileExW` with
write-through semantics.

The editor arms a one-shot capture state in the same hook. The next keyboard
key or standard mouse button is swallowed, held until its release, and returned
to the UI. Hotkey actions are parsed into make/break scan-code sequences with
modifiers released in reverse order.

## Startup path

The same binary exposes a native Windows service mode:

```text
"C:\ProgramData\NeoControl\neo-control.exe" --boot-service --wait-ms 120000
```

The installed `NeoControlBoot` service is automatic, runs as `LocalSystem`,
and declares a dependency on the signed `AcpiBridge` driver. The Service
Control Manager starts it before the user shell. It creates no UI, opens
AcpiBridge, retries at 10 ms, applies the complete vendor power transition
(`page 1/16`, `page 1/1`, DCHU `121/25`, DCHU `79`), verifies the persisted
profile and exits. Automatic fan mode is already the firmware fallback and is
not rewritten on this path. The agent intentionally does not wait for the
~330 ms thermal command during boot. Failure actions restart a non-zero exit
after 1, 2 and 5 seconds and are enabled for non-crash service failures.

The service command is installed and removed by the same executable:

```text
neo-control.exe --install-boot-service
neo-control.exe --uninstall-boot-service
```

`--boot-agent` remains available as a direct diagnostic invocation of the
headless worker.

## Resident UI and FlexiKey

The interactive process has three Windows responsibilities:

- a hidden tray window with the embedded Neo Control icon;
- `WH_KEYBOARD_LL` and `WH_MOUSE_LL` FlexiKey hooks plus an action worker;
- a single-instance mutex that restores the existing window and prevents a
  second hook from starting.

The main window is marked `WS_EX_TOOLWINDOW`, so it does not create a taskbar
button. Closing the window moves it off-screen and keeps the Slint event loop
alive; the tray restores and centers the same HWND. A current-user Run entry
starts the process with `--background` at logon. No helper executable or
scheduled task is used.

## Threading

The UI thread only mutates Slint properties. One hardware worker owns the
AcpiBridge handle and serializes reads and writes. ACPI calls never run on the
renderer thread.

```text
Slint event loop
   │ commands
   ▼
hardware worker ── AcpiBridge IOCTL ── readback
   │ snapshots
   ▼
slint::invoke_from_event_loop
```

Thermal data is polled every two seconds; extended reads run every other cycle.
The UI animation is independent of hardware sampling.

## Write policy

- Validate all user input before constructing a frame.
- Persist the setting when the vendor protocol requires it.
- Poll for the expected value rather than treating a successful IOCTL as proof.
- Restore the previous state when readback differs and restoration is safe.
- Keep Automatic fan mode as the recovery fallback.

## Keyboard filter transport

The signed `HKKbdFltr.sys` control device is opened directly:

```text
\\.\{2dc8164f-3a23-4d71-9fa6-91f9ead88613}
```

Recovered control messages:

```text
0xBA400  enable/disable filter (one-byte input)
0xBA404  add filter macro (two 16-byte key collections)
0xBA408  remove filter macro (trigger collection)
0xBA40C  clear all filter macros
0xBA41C  enable/disable USB filter device
```

The protocol crate has typed 16-byte records for trigger keys and macro
down/up actions. Direct open, enable, disable and clear were verified on the
host. Macro editing is implemented as a transactional clear-and-reapply of the
complete set because the vendor remove path clears all macros first and
discards the driver's selective-delete result.

## Display care

The recovered CC4.0 `OLEDCare` path combines Windows state with four persisted
AppSettings bytes. The vendor wrapper treats its first argument as a linear
index, so the documented `45..48` values are page 0 offsets, not pages 45–48.

- taskbar auto-hide uses `SHAppBarMessage` and offset 46;
- taskbar transparency uses the Windows Personalize registry value and offset
  45;
- dark appearance writes both app and system light-theme values and offset 47;
- screen saver type uses the Windows screen-saver file list and offset 48;
- monitor timeouts use the active power scheme's video-powerdown setting;
- screen-saver timeout uses the Windows desktop registry and
  `SystemParametersInfoW`.

Every operation reads the previous state, applies the change, verifies the
readback and restores the previous Windows state when verification or the DCHU
mirror write fails. The UI exposes the recovered controls on the Display care
page even when the panel reports LCD; OLED-only pixel shifting remains hidden.

## Remaining CC4.0 work

- Panel-specific OLED pixel shifting and periodic refresh on OLED hardware.
- PD charging.
- CPU/GPU overclocking and voltage controls.
