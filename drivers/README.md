# Local driver payload

OEM driver binaries are intentionally not committed to this repository.

The deployed application looks for this layout next to `neo-control.exe`:

```text
drivers/
  acpibridge/
    AcpiBridge.inf
    AcpiBridge.sys
    acpibridge.cat
  hkkbdfltr/
    HKKbdFltr.inf
    HKKbdFltr.sys
    hkkbdfltr.cat
    HKFltrInstaller.exe
```

`acpibridge` is required. `hkkbdfltr` is retained as an optional payload for
future hardware-filter work; the current FlexiKey implementation uses a
user-mode hook and does not load `HKKbdFltr`.

The application never installs anything at startup. The Overview driver card
runs detection, and installation is only started by the explicit button, which
launches the same executable with `--install-drivers` through UAC.
