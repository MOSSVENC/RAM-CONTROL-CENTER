# RAM CONTROL CENTER

<img src="assets/ram_control_center_logo.png" width="72" alt="RAM CONTROL CENTER pixel logo">

RAM CONTROL CENTER is a native Windows control center for compatible CC4.0/DCHU
platforms. It is built with Rust and Slint and communicates with the platform
through the signed AcpiBridge device interface.

The app provides a Chinese-first desktop interface for platform power, cooling,
lighting, battery, display, input and FlexiKey controls. The checked-in protocol
and UI are model-capability driven; writable controls are enabled only after
their platform support and readback behavior have been validated. Firebat/Clevo
ZY-T6K is the current hardware validation reference. Other CC4.0 models can use
the same project after the compatibility checks below.

```bash
git clone https://github.com/MOSSVENC/RAM-CONTROL-CENTER.git
cd RAM-CONTROL-CENTER
```

## Highlights

- Native Rust + Slint UI with GPU compositing and a compact tray-resident shell
- Power profiles, fan policies, custom curves and a ten-point RPM editor
- Thermal, clock, memory, disk, battery and optional NVIDIA telemetry
- Keyboard RGB with live draggable channel sliders, charging policy, GPU mode and panel refresh controls
- Immediate local feedback for navigation, quick toggles and segmented controls
- FlexiKey profiles, binding editor and resident keyboard/mouse action engine
- Windows display care with readback and rollback
- Headless boot service that restores the saved power policy before login
- Chinese and English interface switching

## Architecture

```text
neo-control
  -> neo-control-protocol
  -> Slint / Win32

neo-control-protocol
  -> signed AcpiBridge device interface (required)
  -> signed HKKbdFltr device interface (optional protocol path)
```

The workspace is split into a typed protocol layer, a serialized hardware
worker and an independently editable Slint UI tree. See
[`docs/architecture.md`](docs/architecture.md) for details.

## Compatibility

The protocol and UI layers are capability-driven and designed for CC4.0/DCHU
platforms. Compatible machines can reuse the same architecture, with these
model-level checks before enabling write controls:

- verify the `AcpiBridge` ACPI device path; the default is `CLV0001` and can be
  overridden with `NEO_CONTROL_ACPI_DEVICE` for another model;
- verify the capability page, fan count and fan-channel mapping;
- compare model-specific AppSettings offsets and power/fan transitions with the
  vendor Control Center;
- start with read-only telemetry, then exercise the reversible hardware smoke
  suite on that exact machine.

`Firebat/Clevo ZY-T6K` is the validated reference machine. Support for another
CC4.0 model requires confirming its device path, capability response, fan
mapping and settings behavior before enabling writes.

## Build and verify

Development and cross-compilation run in WSL/Arch with the configured Zig
linker:

```bash
./scripts/verify.sh
```

The gate runs formatting, host tests, Windows-target Clippy with warnings
denied, and the Release build. On a Windows host with the signed drivers
installed, run the reversible hardware smoke suite:

```powershell
pwsh -File scripts/hardware-smoke.ps1
```

Driver payloads are intentionally not committed. On a deployed machine, place
the signed OEM files under `drivers/acpibridge/` beside the executable. The
Overview page detects the live device and exposes an explicit install button;
no driver is installed at startup.

The Release executable is written to:

```text
target/x86_64-pc-windows-gnu/release/neo-control.exe
```

## Documentation

- [Architecture](docs/architecture.md)
- [Feature parity](docs/cc40-feature-parity.md)
- [Protocol map](docs/protocol-map.md)
- [Verification log](docs/verification.md)
- [Quality assessment](docs/quality-assessment.md)
- [Binary replacement notes](docs/binary-replacement.md)
- [Vendor driver and package dependency matrix](docs/vendor-dependency-matrix.md)
- [Driver detection and installation](docs/driver-installation.md)

## License

MIT. See [LICENSE](LICENSE).
