#!/usr/bin/env bash
# Anvil installer. Installs the `smith` CLI from this workspace.
#
# Usage (from the workspace root):
#   ./scripts/install.sh
#
# Or one-liner-style (after the workspace is on crates.io / GitHub):
#   curl -sSL https://anvil-rs.dev/install.sh | sh

set -euo pipefail

# Locate workspace root: the directory holding this script's parent.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ ! -f "$WORKSPACE_ROOT/crates/smith/Cargo.toml" ]; then
    echo "error: could not find Anvil workspace at $WORKSPACE_ROOT" >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    # Try the standard rustup install location.
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        export PATH="$HOME/.cargo/bin:$PATH"
    elif [ -d "$HOME/.rustup/toolchains" ]; then
        TOOLCHAIN=$(ls "$HOME/.rustup/toolchains" | head -1)
        if [ -n "$TOOLCHAIN" ] && [ -x "$HOME/.rustup/toolchains/$TOOLCHAIN/bin/cargo" ]; then
            export PATH="$HOME/.rustup/toolchains/$TOOLCHAIN/bin:$PATH"
        fi
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust first: https://rustup.rs" >&2
    exit 1
fi

echo "installing smith from $WORKSPACE_ROOT..."
cargo install --path "$WORKSPACE_ROOT/crates/smith" --force --quiet

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$CARGO_BIN"; then
    echo
    echo "  ⚠  $CARGO_BIN is not in your PATH."
    echo "     Add this to your shell profile:"
    echo "       export PATH=\"$CARGO_BIN:\$PATH\""
    echo
fi

echo
echo "  ✓ smith installed"
echo
echo "  try it:"
echo "    smith new my-app"
echo "    cd my-app && smith serve"
echo
