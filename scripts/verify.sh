#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# Prefer the rustup-managed toolchain when the login shell has /usr/bin first.
if [[ -d "$HOME/.cargo/bin" ]]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

target="${NEO_WINDOWS_TARGET:-x86_64-pc-windows-gnu}"

# rustc looks up the GNU dlltool by its target-prefixed name. Keep the Zig
# shim on PATH even when the checkout path itself contains spaces.
export PATH="$PWD/.cargo:$PATH"

echo "==> rustfmt"
cargo fmt --all -- --check

echo "==> unsafe boundary"
./scripts/check-unsafe.sh

echo "==> host tests"
cargo test --workspace

echo "==> Windows clippy"
cargo clippy --workspace --all-targets --target "$target" -- -D warnings

echo "==> Windows release build"
cargo build --release --target "$target" --workspace
