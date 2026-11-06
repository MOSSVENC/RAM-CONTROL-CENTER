# Driver detection and installation

Neo Control treats the signed OEM driver as a separate deployment dependency.
It never installs a driver at startup and never embeds a `.sys` file inside the
GUI executable.

## Required driver

The current application needs only:

```text
AcpiBridge.sys
AcpiBridge.inf
acpibridge.cat
```

These files expose the `ACPI\CLV0001` device interface used by the Rust
protocol layer. `HKKbdFltr` is optional: the current FlexiKey implementation
uses a user-mode low-level keyboard hook and does not open the hardware filter
device.

## Local payload layout

The deployed app looks beside `neo-control.exe` first, then in
`%ProgramData%\NeoControl\drivers`:

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

The repository intentionally ignores all files below `drivers/` except its
README. The local deployment may contain the OEM files; the public Git tree
does not.

## Detection

The Overview driver card reports three independent states:

- `AcpiBridge` device opens successfully: core driver ready.
- The service or device is present but cannot be opened: installed but not
  responding.
- Neither the device nor the service is present: missing.

The card also checks whether the three required payload files exist. A missing
payload disables the install button and displays the expected directory.

No driver is launched or installed merely by opening the application.

## One-click installation

The install button starts the same `neo-control.exe` with:

```text
--install-drivers
```

The helper is started through `ShellExecuteExW` with the `runas` verb. It runs
on a worker thread after COM initialization, then:

1. exits successfully if `AcpiBridge` already opens;
2. refuses to run without administrator rights;
3. verifies the local payload exists;
4. invokes `%SystemRoot%\System32\pnputil.exe /add-driver <AcpiBridge.inf> /install`;
5. attempts to start the `AcpiBridge` service;
6. waits for the device interface to open;
7. returns a specific exit code for the UI.

The helper does not install `DCHUService`, `CCDUService`, `FnKey`, `CC40`,
`InsydeDCHU.dll`, `HKFltrLib.dll`, or any vendor UI/service.

The elevated process writes its result and `pnputil` output to:

```text
%ProgramData%\NeoControl\driver-install.log
```

The exact exit codes are:

```text
0  driver is ready
2  local driver payload is missing
3  helper was not elevated
4  pnputil command failed
5  package was staged but the device did not become ready
```

## Validation

The Windows Release build was checked with:

```text
neo-control.exe --driver-smoke
```

Current host result:

```text
acpi_ready=true
acpi_installed=true
keyboard_filter_ready=true
keyboard_filter_installed=true
acpi_payload_ready=true
keyboard_payload_ready=true
payload_path=C:\ProgramData\NeoControl\drivers
```

The idempotent helper path was also checked without changing the system:

```text
neo-control.exe --install-drivers
exit=0
installer: start
installer: AcpiBridge already responding; nothing to install
```

A clean-machine install with the AcpiBridge device actually absent remains the
next destructive-boundary validation. It must be performed on a disposable
machine or with a documented driver rollback snapshot.
