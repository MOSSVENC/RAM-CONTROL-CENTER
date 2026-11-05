# CC Control Center protocol map

This document records evidence recovered from the installed Control Center
applications and from live transport instrumentation.

Neo Control's implemented path is the direct `CreateFileW` / `DeviceIoControl`
branch below. The vendor UI/DLL call graph is retained as protocol provenance,
not as a runtime dependency.

## Evidence levels

- **Confirmed**: observed in a live call or directly in the vendor service code.
- **Strong**: repeated in the decompiled UI/service plus an independent reference.
- **Candidate**: decompiled call site exists, but the feature/argument semantics
  still need an isolated test.

## Transport stack

```text
Neo Control                              Vendor reference
     │                                        │
     │ CreateFileW + DeviceIoControl          ├─ InsydeDCHU.dll
     │                                        │      ├─ GetDCHU_Data_*
     │                                        │      ├─ SetDCHU_Data*
     │                                        │      └─ Read/WriteAppSettings
     └──────────────┬─────────────────────────┘
                    ▼
             signed AcpiBridge1.sys
                    │
                    ▼
                 ACPI / EC
```

### Confirmed ACPI bridge facts

- Device interface:
  `\\?\ACPI#CLV0001#1#{86994c74-ad43-4812-b7e7-0c420b5c5fd7}`
- IOCTL values observed in the live `CC40.exe` trace:
  - `0x470807` (4655111) — interface/query traffic with a 40-byte input.
  - `0x322400` (3286016) — ACPI read-method traffic; input starts with
    `93F224E4-FBDC-4BBF-ADD6-DB71BDC0AFAD` and the `_DSM` method name.
  - `0x32240C` (3286028) — app-settings read/write traffic. Reads use a
    12-byte request with `0x10000000` at offset 4 and return the full 4096-byte
    page store. Writes use `0x1008` bytes with a 16-bit encoded address and
    length.
- ACPI method GUIDs from the vendor service:
  - `ACPIBIOS_READ  = 93F224E4-FBDC-4BBF-ADD6-DB71BDC0AFAD`
  - `ACPIBIOS_WRITE = 0B6D7934-1EB5-453E-5825-D945072A45AA`
  - `ACPIBIOS_EVENT = 6076EF27-160E-4686-35E5-924A50B4DEAA`
- IOCTL values from `DCHUService` source:
  - `IOCTL_ACPIBIOS_READ_METHOD      = 3286016`
  - `IOCTL_ACPIBIOS_WAIT_EVENT       = 3286020`
  - `IOCTL_ACPIBIOS_GET_NOTIFY_VALUE = 3286024`

The live trace proves CC40 itself talks directly to the signed AcpiBridge
device for reads. There is no evidence of a user-mode encryption layer.

## Direct ACPI frame

The mounted DCHU request uses a 0x420-byte input and a 0x40C-byte response:

```text
00..10  ACPI read GUID E4 24 F2 93 DC FB BF 4B AD D6 DB 71 BD C0 AF AD
14..18  "_DSM"
18..1C  command (little endian)
1C..1E  01 04
1E..22  01 00 00 02
22..122  up to 256 payload bytes
```

Responses begin with the `AeoB` frame header. Offset 8 is the descriptor count;
each descriptor is `kind:u16, length:u16` followed by its payload. Kind 2 is a
byte buffer and kind 0 is a four-byte integer.

Settings reads use `0x32240C` with the full-page request. Settings writes use:

```text
00..04  u32 1
04..06  u16 page * 256 + offset
06..08  u16 payload length
08..    payload
```

## Vendor DCHU ABI reference

Validated vendor exports:

```c
int GetDCHU_Data_Integer(int command, int *data);
int GetDCHU_Data_Buffer(int command, byte *buffer);
int SetDCHU_Data(int command, byte *buffer, int length);
int SetDCHU_DataEx(int command, byte *buffer, int length, byte *output);
int ReadAppSettings(int page, int offset, int length, byte *buffer);
int WriteAppSettings(int page, int offset, int length, byte *buffer);
```

The two-argument setter wrapper in CC40 builds a four-byte payload:

```text
[data & 0xff, (data >> 8) & 0xff, (data >> 16) & 0xff, subcommand]
```

and calls:

```c
SetDCHU_Data(command, payload, 4)
```

This is why calls such as `Set(121, 25, value)` appear in the decompiled code.
The CC40 app-settings helper encodes `page * 256 + offset`; for example, the
decompiled value `261` means native `ReadAppSettings(page=1, offset=5)`.

## Runtime process routing

The visible CC40 window is not the process that performs the control writes.
Live Frida tracing showed the actual DCHU writes executing inside `FnKey.exe`.
CC40 selects a mode in its UI, and the signed FnKey stack applies the command.

This matters for protocol research: hooking only `CC40.exe` shows UI state and
read-back calls, but misses most hardware writes.

## Confirmed high-level mappings

### Power mode

CC40 and the signed FnKey path use the following full transition:

```text
WriteAppSettings(page=1, offset=16, length=1, mode)
WriteAppSettings(page=1, offset=1, length=1, mode)
SetDCHU_Data(121, [mode, 0, 0, 25], 4)
SetDCHU_Data(79, [boost_mode, 0, 0, 0], 4)
ReadAppSettings(page=1, offset=1, length=1)
ReadAppSettings(page=1, offset=16, length=1)
```

The second command is required to commit the processor boost policy. Without
it the persisted UI byte can say Performance while the EC/CPU stays in its
cold-boot low-power state. FnKey's mapping is:

| UI profile | DCHU command 79 value |
|---|---:|
| Performance | 0 |
| Power saving / Quiet+ | 1 |
| Quiet / Entertainment | 2 |

Confirmed mode values on this machine's visible CC40 UI:

| Value | UI mode |
|---:|---|
| 2 | Performance |
| 3 | Entertainment |
| 4 | Quiet / Quiet Plus |

Values `0` and `1` remain valid enum members in CC40 (`quiet`, `pwrsaving`),
but the visible three-mode selector uses `2`, `3`, and `4` on this model.

The page `1/16` value is written before the UI profile byte in the FnKey
transition. On this machine, reapplying `[2]` followed by command `79 = 0`
returns both page `1/16` and page `1/1` to `2` with a successful status.

### Fan mode

Confirmed from both CC40 call sites and a live FnKey transition:

```text
ReadAppSettings(page=4, offset=5, length=1)   // current fan mode
SetDCHU_Data(121, [mode, 0, 0, 1], 4)
WriteAppSettings(4, 5, 1, mode)
```

A Performance-mode transition from FnKey also normalized the fan state with:

```text
WriteAppSettings(4, 7, 1, [0])
SetDCHU_Data(121, [0, 0, 0, 14], 4)
WriteAppSettings(4, 7, 1, [0])
```

Fan-mode values observed in CC40 UI handlers:

| Value | UI mode |
|---:|---|
| 0 | Auto |
| 1 | Max |
| 6 | Custom |
| 9 | Turbo |

`MaxQ` and anti-dust are model-dependent and must be tested separately.

### Fan offset

```text
ReadAppSettings(page=4, offset=7, length=1)
SetDCHU_Data(121, [offset, 0, 0, 14], 4)
WriteAppSettings(4, 7, 1, offset)
```

### Fan table

```text
SetDCHU_Data(14, table, 256)
WriteAppSettings(page=4, offset=0, length=256, table)
ReadAppSettings(page=4, offset=0, length=256)
```

CC40's custom curve table is 256 bytes. CPU and GPU curve bytes are packed in
the same buffer; the exact node layout is documented by the reference and still
needs a model-specific read-back comparison.

## Other CC40 command candidates

These calls were found in the decompiled CC40 and need isolated verification:

| Command / subcommand | Evidence | Feature candidate |
|---|---|---|
| `121, 24` | Multiple LED/charging UI classes | Keyboard backlight brightness/sleep timer |
| `121, 25` | Single power-mode feature path | Power mode / platform mode |
| `121, 26` | Keyboard/RGB initialization path | RGB keyboard enable/disable |
| `121, 30` | Repeated LED UI classes | Keyboard LED effect/mode |
| `121, 34` | Fan feature path | Fan auto/control toggle |
| `121, 40` | Fan curve UI method | Fan curve parameter |
| `121, 41` | Fan curve UI method | Fan curve parameter |
| `118, 1` | Power/time UI method | Platform time/clock-related feature |
| `14, 256-byte table` | Fan custom table | Fan curve table |
| `1029`, `1031`, `1032`, `1096`, `1104`, `1105` | App-settings pages | Stored UI/fan state, not raw EC writes |

## Settings pages observed

| Page / offset | Candidate meaning |
|---|---|
| 1 / 1 | Power mode |
| 1 / 16 | Power profile policy; `2` observed after Performance |
| 4 / 0, length 256 | Fan curve table |
| 4 / 5 | Fan mode |
| 4 / 7 | Fan offset |
| 4 / 9 | Secondary fan index/value |
| 6 / 33 | PL1 value |
| 6 / 37 | PL2 value |
| 6 / 41 | PL time |
| 6 / 49 | CPU VR current |
| 6 / 61 | CPU voltage offset |
| 6 / 69 | GPU voltage |
| 6 / 73 | CPU voltage mode |
| 6 / 74 | CPU VR auto flag |

The CC40 CPU OC page also reads per-core ratio fields from page 6. These are
**candidate mappings** and must not be written until tested one at a time.

## Transport security

The interface uses proprietary binary framing and undocumented ACPI/EC command
semantics. Treat protocol captures and device handles as privileged operational
data.

## Safety and next tests

1. Read each candidate state with `ReadAppSettings` and compare with the UI.
2. Test only one setter at a time, with a read-back after the write.
3. Keep fan auto as the fallback; never leave manual fan control unwatched.
4. Do not touch CPU voltage, Curve Optimizer, P0State VID, or EC firmware.
5. Verify every command on this exact Firebat ZY-T6K / CV3xxVNx-F model.

## Safety finding: initialize the DCHU client before keyboard reads

A diagnostic probe called:

```text
ReadAppSettings(page=519, offset=1, length=1)
```

before the Control Center initialization sequence. `InsydeDCHU.dll` raised an
access violation at `InsydeDCHU.dll+0x190d1f` and terminated the probe process.

Neo Control uses the recovered page 2 keyboard state layout and the initialized
`SetDCHU_DataEx(4, [63], ...)` / `SetDCHU_Data(103, ...)` paths.
- RGB brightness, color, effects, sleep timer and boot effect have been
  exercised through the direct transport.

The diagnostic crash was confined to a user-mode process; firmware remained
unchanged.

## Verified `SetDCHU_DataEx` and app-settings encoding

The verified IL evidence is summarized by the corrections below.

- `SetDCHU_DataEx` requires a full 256-byte output landing buffer.
- App-settings keys encode `page * 256 + offset`; `302` resolves to
  `WriteAppSettings(1, 46, 1, ...)`.
- FlexiCharger writes are `SetDCHU_DataEx(4,[31,enabled,start,stop],4,out256)`
  followed by `WriteAppSettings(1,46,1,[mode])`.



## Anti-dust transport

```text
Run now:
  SetDCHU_Data(121, [1, 0, 0, 41])
  WriteAppSettings(4, 80, [1])

Schedule:
  packed = day + (resume << 7) + (hour << 8)
  SetDCHU_Data(121, [packed, 0, 0, 40])
  WriteAppSettings(4, 81, [1, day | (resume << 7), hour, 0, 0])
```

Day `127` is the no-scheduled-day sentinel. A reversible disable and restore
cycle was verified on the live device.

## Keyboard filter transport

The signed keyboard filter is a separate device:

```text
\\.\{2dc8164f-3a23-4d71-9fa6-91f9ead88613}
```

The implemented IOCTLs and 16-byte collection frames are documented in
[`docs/verification.md`](verification.md).
