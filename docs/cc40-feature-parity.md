# CC40 feature parity matrix

Target: Firebat/Clevo ZY-T6K, CC40 7.94/7.101 protocol family.

Local capability evidence is read through the signed AcpiBridge transport:

- feature page: AppSettings page 7, 256 bytes;
- WMI support words: commands 16, 122 and 96;
- keyboard/fan identity: command 13;
- UI style: AppSettings page 1 offset 45 (`1` on this machine);
- OLED panel flag: page 1 offset 49 (`0` on this machine).

## Local platform capabilities

```text
BIOS feature page version : 0x0100
UI style                  : 1
Power profiles            : Performance, Entertainment
Fan control               : RPM ten-point, Turbo fan, Noiseless fan
Fan channels              : 2 (CPU, GPU)
Preference toggles        : Fn lock, Win lock, Win/Fn swap, Control Center hotkey, Copilot
Camera toggle             : supported
Touchpad toggle           : supported
Airplane toggle           : supported
GPU switch                : dGPU and MSHybrid (mask 0x06)
NVIDIA power-off          : supported
Keyboard                  : 15-color RGB, type 6
Keyboard controls         : brightness, static/breathe/cycle, sleep timer, boot effect
Flexikey                  : supported
FlexiCharger              : supported
CPU OC                    : unsupported
Simple GPU OC             : unsupported
IQST                      : unsupported
PD power level            : unsupported
OLED care                 : unsupported on this panel
Light bar / cover logo    : unsupported
```

Unsupported feature surfaces are capability-gated and appear only when the
platform reports support.

## Protocol map

### Power profile

```text
WriteAppSettings(page 1, offset 16, mode)
WriteAppSettings(page 1, offset 1, mode)
SetDCHU_Data(121, [mode, 0, 0, 25])
SetDCHU_Data(79, [boost_mode, 0, 0, 0])
ReadAppSettings(page 1, offset 1)
ReadAppSettings(page 1, offset 16)
```

`boost_mode` is `0` for Performance, `1` for Power saving/Quiet+, and `2`
for Quiet/Entertainment, matching the FnKey transition. This second command is
what actually commits the processor boost policy during boot.

### Panel low refresh rate

```text
Support flag: ReadAppSettings(page 1, offset 27)
Current state: ReadAppSettings(page 1, offset 28)
Write state:   WriteAppSettings(page 1, offset 28, [enabled])
```

CC40 exposes this control in Power saving and Quiet+ profiles. The setting
stores the vendor mode byte; the panel may blank briefly when the active refresh
rate changes.

### Display care / OLED page

CC40's `InsydeDriver` wrapper treats the first argument as a linear byte index
and internally converts it to `page = index / 256`, `offset = index % 256`.
The recovered `45..48` values are therefore **page 0 offsets**, not pages.

```text
Taskbar transparency: page 0 offset 45
Taskbar auto-hide:    page 0 offset 46
Dark appearance:      page 0 offset 47
Screen saver mode:    page 0 offset 48

Taskbar state:
  SHAppBarMessage(ABM_GETSTATE / ABM_SETSTATE)

Taskbar transparency:
  HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize
    EnableTransparency = 0 / 1

Dark appearance:
  AppsUseLightTheme = 0 / 1
  SystemUsesLightTheme = 0 / 1

Monitor timeout:
  active power scheme
  GUID_VIDEO_SUBGROUP / GUID_VIDEO_POWERDOWN_TIMEOUT
  AC and DC seconds, converted to minutes in the UI

Screen saver:
  HKCU\Control Panel\Desktop
    ScreenSaveActive, SCRNSAVE.EXE, ScreenSaveTimeOut
  SystemParametersInfoW(SPI_SETSCREENSAVEACTIVE)
  SystemParametersInfoW(SPI_SETSCREENSAVETIMEOUT)

Modes:
  0 none, 1 3D Text, 2 Blank, 3 Bubbles, 4 Mystify, 5 Photos, 6 Ribbons
```

The RAM CONTROL CENTER implementation applies the Windows state first, writes the
vendor mirror byte, verifies both, and restores the previous Windows state when
either side fails. The page remains available on LCD systems because taskbar,
dark-mode, timeout and screen-saver controls are Windows features; panel-only
pixel shifting/refresh is not exposed.

### Fan policies

```text
Auto / Max / Custom / IQST:
  SetDCHU_Data(121, [mode, 0, 0, 1])
  WriteAppSettings(page 4, offset 5, mode)

Noiseless:
  SetDCHU_Data(121, [8, 0, 0, 1])
  WriteAppSettings(page 4, offset 9, enabled)
  WriteAppSettings(page 4, offset 5, 0)

Turbo fan (Performance only):
  WriteAppSettings(page 4, offset 8, enabled)
  WriteAppSettings(page 1, offset 32, 0)
  SetDCHU_Data(121, [(2 | enabled << 6), 0, 0, 25])
  WriteAppSettings(page 4, offset 5, 0)
  WriteAppSettings(page 4, offset 72, 0)
  If enabled: SetDCHU_Data(121, [0, 0, 0, 1])

Fan offset:
  SetDCHU_Data(121, [round(percent / 100 * 255), 0, 0, 14])
  WriteAppSettings(page 4, offset 7, percent)

Anti-dust:
  SetDCHU_Data(121, [1, 0, 0, 41])
  WriteAppSettings(page 4, offset 80, 1)
```

### RPM ten-point fan table

Read:

```text
SetDCHU_DataEx(4, [40], output[256])
```

Write:

```text
SetDCHU_DataEx(4, frame[256], output[256])
frame[0] = 41
frame[16..128]  = CPU module, 112 bytes
frame[128..240] = GPU module, 112 bytes
```

Each 112-byte module:

```text
0       module ID
1..7    thermal thresholds
8..13   idle/max/turbo RPM (big endian)
14..21  reserved vendor bytes
22..31  down temperatures (10)
32..41  up temperatures (10)
42..61  down RPM, u16 big endian (10)
62..81  up RPM, u16 big endian (10)
82..91  up duty (10)
92..101 down duty (10)
102..111 packed down/up timing nibbles (10)
```

Fan 3/4 use command 42 read and command 43 write.

### Preference toggles

```text
SetDCHU_Data(4, [25, fn, win, swap, hotkeys, copilot])
WriteAppSettings(page 1, offsets 36, 37, 38, 43, 44)
```

Win lock is inverted in the native representation. Camera uses page 1
offset 6 plus `SetDCHU_Data(34, [value, 0, 0, 0])`. Touchpad state is persisted
at page 1 offset 10 after the vendor Ctrl+Win+F24 chord.

### 15-color RGB keyboard type 6

```text
Brightness control:
  SetDCHU_Data(103, [raw, 0, 0, 244])
  raw = 0, 47, 95, 143, 191

Color control:
  SetDCHU_Data(103, [value, 0, 0, 240/241/242])
  value = ((blue/2) << 16) | ((red/2) << 8) | green/2
  WriteAppSettings(page 2, offsets 81..83, red/green/blue)
  WriteAppSettings(page 2, offset 32, 8)

Effect:
  SetDCHU_DataEx(4, [63, effect], output[256])
  effect 2 = breathe, 3 = cycle, 8 = static
  WriteAppSettings(page 2, offset 32, effect)

Sleep timer:
  SetDCHU_Data(121, [data & 0xff, (data >> 8) & 0xff, data >> 16, 24])
  data = (seconds << 8) | 0xff, or 0 when disabled
  WriteAppSettings(page 2, offset 36, enabled)
  WriteAppSettings(page 2, offsets 37..39, hours/minutes/seconds)

Boot override:
  SetDCHU_Data(121, [value, 0, 0, 30])
  WriteAppSettings(page 2, offset 7, value)
```

### FlexiKey profile data

Vendor profile path:

```text
C:\ComboKey\profile.xml
```

RAM CONTROL CENTER parses and writes the same XML schema:

```text
Profile_N
  Name, IsSelected, CurrentDevice, IsKbEnable, IsMouseEnable, Icon
  Set_N
    Device, KeyCode, UiName, KeyDisplay, Action, IsKeypressDelay, Tip, CheckBoxName
    keypress: Index_N KeyCode/KeysGroup/KeyMakeBreak/KeyDelay/KeyDisplay
    launch:   Index_0 AppPath/AppEventId
    text:     StartKey*, Text, SendKey*
    disable:  no child elements
```

The Rust model round-trips keypress, launch, text and disable actions without
dropping their vendor fields. Selecting a profile in the UI activates it with
an atomic `profile.xml` replacement and reloads the user-mode action table.
The editor can add, replace and remove bindings. It captures a keyboard key or
one of the five standard mouse buttons, preserves vendor-only fields on edits,
and writes through `MoveFileExW` with replace/write-through flags. When the
vendor XML is absent, a blank selected profile is exposed in memory and the
file is created on the first binding save.

Profiles can be created, renamed, selected and deleted from the same page.
Deletion is two-step and is rejected when only one profile remains. Every
profile change updates the active engine table and atomically rewrites the
vendor XML.

Hotkey actions accept readable space-separated chords such as `Ctrl+Shift+S`
or `Win+R Ctrl+C`. Modifier make/break ordering is generated automatically.

### FlexiKey action engine

The action engine installs `WH_KEYBOARD_LL` and `WH_MOUSE_LL` hooks in the
application process and maps keyboard scan codes to the CC4.0 key-code
encoding:

```text
ordinary key         -> scan code
extended key         -> 0xE000 | scan code
Pause special case   -> 57629
```

Supported actions:

- keypress: ordered make/break scan-code events through `SendInput`;
- launch: `ShellExecuteW`;
- text: optional start keys, Unicode text, then optional send keys;
- disable: swallow the trigger key.

Injected events created by RAM CONTROL CENTER carry a private `dwExtraInfo` marker and
are ignored by the hook to prevent macro loops. The interactive process is
single-instance, remains resident in the tray after its window closes, and uses
the current-user Run key to start in the background at logon. Mouse bindings
use the vendor button values:

```text
left 1, right 4, middle 0x10, back 0x40, forward 0x100
```

Trigger capture uses a one-shot hook state: the next key or mouse button is
swallowed through its release, then returned to the editor without dispatching
the binding.

## Verified live

- Direct command 12 telemetry and command 13 fan identity.
- Direct page 7 support decode.
- Full two-stage power transition with readback of page 1/1 and page 1/16.
- Direct RGB keyboard state read.
- Same-value RGB brightness, sleep timer, and boot-effect writes.
- Same-value ten-point RPM fan table write with full readback.
- Camera and touchpad state reads.
- Native boot-service start with exit code 0 and no UI process.
- Low-refresh-rate support and current state both read as `1`; a same-value
  `page 1 / offset 28` write was persisted and read back successfully.
- Display-care same-value smoke passed for taskbar auto-hide, transparency,
  dark appearance, monitor timeouts, screen saver, and page 0 offsets 45–48.
- Live taskbar auto-hide was enabled and restored through the UI with
  `Display protection applied and read back` after both changes. Screen-saver
  same-value apply also passed with a readback success message.
- A live F24 FlexiKey launch action triggered a temporary command file through
  the low-level keyboard hook and logged `launch action dispatched`.
- A live XBUTTON1 (mouse back) launch binding produced
  `mouse xbutton event: message=0x020b data=0x00010000`, dispatched the launch
  action, and created its marker file. XBUTTON1 release was swallowed by the
  binding hook as expected.
- A focused WinForms text target received `Hello Neo 世界b`: the text action
  emitted the Unicode string, the keypress action emitted scan-code `B`, and a
  keyboard `A` disable binding was swallowed without appearing in the target.

## Remaining implementation

- Battery charge notification is capability-gated and unavailable on this
  machine.
- Panel-specific OLED pixel shifting/refresh remains hidden because this panel
  reports LCD.
- PD charging remains hidden because the platform reports no supported path.
- CPU/GPU overclocking remains locked because this machine reports no supported
  OC path.
