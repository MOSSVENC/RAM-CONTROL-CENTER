# User-mode binary replacement

Neo Control replaces the vendor user-mode control path while keeping the
signed hardware boundary intact. The replacement is statically linked into one
deployment binary, so there is no separate codec DLL to load, initialize,
discover or version during startup.

## Mapping

| Vendor component | Replacement | State |
|---|---|---|
| `CC40.exe` | Slint UI in `neo-control.exe` | Implemented |
| `InsydeDCHU.dll` | `neo-control-protocol` direct AcpiBridge transport | All five exports have a direct equivalent |
| `HKFltrLib.dll` | `neo-control-protocol` direct `HKKbdFltr.sys` transport | Filter control, clearing, USB enable, macro add and transactional macro-set replacement are implemented |
| vendor boot-policy delivery | `NeoControlBoot` service mode plus interactive `neo-control.exe` | Power replay starts before the user shell; the vendor hotkey application remains independently installed |

The signed `AcpiBridge.sys` and `HKKbdFltr.sys` drivers are not replaced,
patched, re-signed or wrapped by a custom kernel component.

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
Neo Control feature set:

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
discards the actual remove result. Neo Control replaces the complete set with a
validated clear-and-reapply transaction instead.

## Boot service

`neo-control.exe --boot-service` is a one-shot Windows service entry point.
The installed service is named `NeoControlBoot`, starts automatically as
`LocalSystem`, and depends on the signed `AcpiBridge` driver. It runs the same
headless worker as `--boot-agent`, applies the persisted power transition,
verifies the readback and exits without creating a window.
