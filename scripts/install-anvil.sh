#!/usr/bin/env bash
# Anvilforge CLI installer — downloads a pre-built `anvil` binary from the
# latest GitHub release, no Rust toolchain required.
#
# Usage (one-liner):
#   curl -sSf https://anvilforge.dev/install.sh | sh
#
# Or pin a specific version:
#   ANVIL_VERSION=0.3.2 curl -sSf https://anvilforge.dev/install.sh | sh
#
# This is the recommended path for end users — `cargo install anvilforge-cli`
# also works but takes 5–15 minutes on a cold Rust toolchain because it
# compiles every dependency. With pre-built binaries the install is seconds.

set -euo pipefail

REPO="anvilforge/anvilforge"
INSTALL_DIR="${ANVIL_INSTALL_DIR:-${HOME}/.local/bin}"

# ─── Detect platform ──────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64|amd64)   TARGET=x86_64-unknown-linux-musl ;;
            aarch64|arm64)  TARGET=aarch64-unknown-linux-musl ;;
            *)              echo "unsupported Linux arch: $ARCH" >&2; exit 1 ;;
        esac
        ;;
    Darwin)
        case "$ARCH" in
            x86_64)         TARGET=x86_64-apple-darwin ;;
            arm64|aarch64)  TARGET=aarch64-apple-darwin ;;
            *)              echo "unsupported macOS arch: $ARCH" >&2; exit 1 ;;
        esac
        ;;
    *)
        echo "unsupported OS: $OS (on Windows, download from $REPO/releases manually or use scoop/choco)" >&2
        exit 1
        ;;
esac

# ─── Resolve version ──────────────────────────────────────────────────────
if [ -z "${ANVIL_VERSION:-}" ]; then
    echo "→ resolving latest release..."
    if command -v curl >/dev/null 2>&1; then
        ANVIL_VERSION="$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" | sed -n 's/.*"tag_name": "v\(.*\)".*/\1/p' | head -n 1)"
    else
        echo "curl is required" >&2; exit 1
    fi
    if [ -z "${ANVIL_VERSION:-}" ]; then
        echo "could not resolve latest release; set ANVIL_VERSION manually" >&2
        exit 1
    fi
fi

# ─── Download + extract ───────────────────────────────────────────────────
ASSET="anvilforge-cli-v${ANVIL_VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/v${ANVIL_VERSION}/${ASSET}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "→ downloading ${URL}"
curl -fSL "$URL" -o "$TMP/$ASSET"

echo "→ extracting"
tar -xzf "$TMP/$ASSET" -C "$TMP"

# ─── Install ──────────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
SRC="$TMP/anvilforge-cli-v${ANVIL_VERSION}-${TARGET}/anvil"
DEST="$INSTALL_DIR/anvil"

if [ ! -x "$SRC" ]; then
    echo "downloaded archive doesn't contain the anvil binary at $SRC" >&2
    exit 1
fi

install -m 0755 "$SRC" "$DEST"

echo
echo "  ✓ installed anvil v${ANVIL_VERSION} → $DEST"

# ─── PATH check ───────────────────────────────────────────────────────────
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo
        echo "  ⚠  $INSTALL_DIR is not in your PATH."
        echo "     Add this to your shell profile:"
        echo "       export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac

echo
echo "  try it:"
echo "    anvil new my-app && cd my-app && anvil serve"
echo
