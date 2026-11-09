# User-mode binary replacement

RAM CONTROL CENTER replaces the vendor user-mode control path while keeping the
signed hardware boundary intact. The replacement is statically linked into one
deployment binary, so there is no separate codec DLL to load, initialize,
discover or version during startup.

## Mapping

| Vendor component | Replacement | State |
|---|---|---|
| `CC40.exe` | Slint UI in `neo-control.exe` | Implemented |
| `InsydeDCHU.dll` | `neo-control-protocol` direct AcpiBridge transport | All five exports have a direct equivalent |
| `HKFltrLib.dll` | `neo-control-protocol` direct `HKKbdFltr.sys` transport | Protocol is implemented; the current FlexiKey engine uses a user-mode hook and does not require this driver |
| vendor boot-policy delivery | `NeoControlBoot` service mode plus interactive `neo-control.exe` | Power replay starts before the user shell; the vendor hotkey application remains independently installed |

The signed `AcpiBridge.sys` and `HKKbdFltr.sys` drivers are not replaced,
patched, re-signed or wrapped by a custom kernel component. `AcpiBridge.sys` is
the required runtime boundary; `HKKbdFltr.sys` is optional for the current
FlexiKey implementation.

The exact firmware/driver/user-service dependency boundary and the five local vendor packages are recorded in [vendor-dependency-matrix.md](vendor-dependency-matrix.md). In particular, `AcpiBridge.sys` is the required ACPI/EC boundary; `InsydeDCHU.dll`, `DCHUService.exe`, `CC40.exe` and `FnKey.exe` are not runtime dependencies of the Rust implementation.

## InsydeDCHU.dll exports

```text
GetDCHU_Data_Buffer   -> Dchu buffer command through DeviceIoControl(0x322400)
GetDCHU_Data_Integer  -> Dchu integer command through DeviceIoControl(0x322400)
ReadAppSettings       -> settings read through DeviceIoControl(0x32240C)
SetDCHU_Data          -> DCHU method frame through DeviceIoControl(0x322400)
WriteAppSettings      -> settings write through DeviceIoControl(0x32240C)
```

The Rust transport owns one device handle, constructs the recovered request
frames, validates the `AeoB` descriptor response and exposes typed platform
operations. The deployed executable import table contains neither
`InsydeDCHU.dll` nor `HKFltrLib.dll`.

## HKFltrLib.dll exports

The direct keyboard-filter transport covers the control messages used by the
RAM CONTROL CENTER feature set:

```text
0xBA400  filter enable/disable
0xBA404  add filter macro
0xBA408  remove filter macro
0xBA40C  clear filter macros
0xBA41C  USB filter device enable/disable
```

Open, filter enable/disable, clear and USB enable are live-verified. Macro add
uses the vendor 16-byte key-collection format. Selective `0xBA408` removal is
not used because the vendor wrapper clears the complete macro set first and
discards the actual remove result. RAM CONTROL CENTER replaces the complete set with a
validated clear-and-reapply transaction instead.

## Boot service

`neo-control.exe --boot-service` is a one-shot Windows service entry point.
The installed service is named `NeoControlBoot`, starts automatically as
`LocalSystem`, and depends on the signed `AcpiBridge` driver. It runs the same
headless worker as `--boot-agent`, applies the persisted power transition,
verifies the readback and exits without creating a window.

## Driver-only installation

The signed OEM driver is a deployment dependency, not a component linked into
`neo-control.exe`. The application looks for a local payload beside the
executable:

```text
drivers/acpibridge/
  AcpiBridge.inf
  AcpiBridge.sys
  acpibridge.cat
```

The Overview page detects whether the AcpiBridge device is actually reachable.
When it is missing and the payload is present, the explicit install button
launches the same executable with `--install-drivers` through UAC. The helper
uses Windows `pnputil` to install only the signed AcpiBridge package, then
waits for the device to open. It does not install CC40, FnKey, DCHUService,
InsydeDCHU, HKFltrLib, or any vendor service.

`HKKbdFltr` is displayed as an optional component. The current FlexiKey engine
uses a user-mode low-level hook, so the default installer does not install that
filter driver.

Driver binaries are ignored by Git. See
[`drivers/README.md`](../drivers/README.md) and
[`docs/driver-installation.md`](driver-installation.md).
