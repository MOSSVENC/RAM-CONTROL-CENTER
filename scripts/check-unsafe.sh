#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# This is a ratchet, not a claim that FFI disappears. Every file below has an
# audited Win32/ACPI/NVML boundary; a new file or a higher per-file count fails
# the gate. When a later refactor lowers a count, lower its ceiling here.
declare -A ceilings=(
    ["crates/app/src/telemetry.rs"]=26
    ["crates/app/src/tray.rs"]=21
    ["crates/app/src/boot_service.rs"]=13
    ["crates/app/src/flexikey_engine.rs"]=10
    ["crates/app/src/system_controls/registry.rs"]=9
    ["crates/app/src/system_controls/display_care.rs"]=5
    ["crates/app/src/system_controls/power.rs"]=6
    ["crates/app/src/service_handle.rs"]=4
    ["crates/protocol/src/win_device.rs"]=3
    ["crates/protocol/src/bin/neo-acpi-probe.rs"]=3
    ["crates/app/src/system_controls/display.rs"]=3
    ["crates/app/src/system_controls/audio.rs"]=2
    ["crates/app/src/input.rs"]=1
    ["crates/app/src/runtime_state.rs"]=1
    ["crates/app/src/flexikey_ui.rs"]=1
    ["crates/app/src/boot_agent.rs"]=1
)

total=0
failed=0
max_total=109

while IFS= read -r file; do
    count=$(grep -o -E '\bunsafe\b' "$file" | wc -l)
    total=$((total + count))
    ceiling=${ceilings[$file]:-0}
    if (( count > ceiling )); then
        printf 'unsafe regression: %s has %d (ceiling %d)\n' "$file" "$count" "$ceiling" >&2
        failed=1
    fi
    printf '%3d  %s\n' "$count" "$file"
done < <(grep -R -l -E '\bunsafe\b' crates --include='*.rs' | sort)

if (( total > max_total )); then
    printf 'unsafe total regression: %d (ceiling %d)\n' "$total" "$max_total" >&2
    failed=1
fi

printf 'unsafe total: %d (ceiling %d)\n' "$total" "$max_total"
exit "$failed"
