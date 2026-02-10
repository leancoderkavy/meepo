#!/bin/bash
set -euo pipefail

# Meepo Installer
# Usage: curl -sSL https://raw.githubusercontent.com/kavymi/meepo/main/install.sh | bash

REPO="kavymi/meepo"
INSTALL_DIR="${MEEPO_INSTALL_DIR:-$HOME/.local/bin}"

# ── Detect platform ──────────────────────────────────────────────

detect_platform() {
    local os arch

    case "$(uname -s)" in
        Darwin) os="darwin" ;;
        Linux)  os="linux" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *)
            echo "Error: Unsupported OS: $(uname -s)"
            echo "Meepo supports macOS and Windows."
            exit 1
            ;;
    esac

    case "$(uname -m)" in
        arm64|aarch64) arch="arm64" ;;
        x86_64|amd64)  arch="x64" ;;
        *)
            echo "Error: Unsupported architecture: $(uname -m)"
            exit 1
            ;;
    esac

    if [ "$os" = "linux" ]; then
        echo ""
        echo "Note: Meepo on Linux has limited functionality."
        echo "Email, calendar, and UI automation tools require macOS or Windows."
        echo ""
    fi

    echo "meepo-${os}-${arch}"
}

# ── Find latest version ──────────────────────────────────────────

get_latest_version() {
    local version
    version=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' \
        | head -1 \
        | sed 's/.*"tag_name": *"//;s/".*//')

    if [ -z "$version" ]; then
        echo "Error: Could not determine latest version."
        echo "Check https://github.com/${REPO}/releases"
        exit 1
    fi
    echo "$version"
}

# ── Main ─────────────────────────────────────────────────────────

main() {
    echo ""
    echo "  Meepo Installer"
    echo "  ────────────────"
    echo ""

    local platform version url archive

    platform=$(detect_platform)
    echo "  Platform: ${platform}"

    version=$(get_latest_version)
    echo "  Version:  ${version}"

    if [[ "$platform" == *"windows"* ]]; then
        archive="${platform}.zip"
    else
        archive="${platform}.tar.gz"
    fi

    url="https://github.com/${REPO}/releases/download/${version}/${archive}"
    echo "  URL:      ${url}"
    echo ""

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    # Download and extract
    echo "  Downloading..."
    local tmpdir
    tmpdir=$(mktemp -d)
    trap "rm -rf $tmpdir" EXIT

    if ! curl -fsSL "$url" -o "$tmpdir/$archive"; then
        echo ""
        echo "  Error: Failed to download from $url"
        echo "  Check your internet connection and try again."
        echo "  Releases: https://github.com/${REPO}/releases"
        exit 1
    fi

    echo "  Extracting..."
    if [[ "$archive" == *.tar.gz ]]; then
        tar xzf "$tmpdir/$archive" -C "$tmpdir"
        mv "$tmpdir/meepo" "$INSTALL_DIR/meepo"
        chmod +x "$INSTALL_DIR/meepo"
    else
        unzip -q "$tmpdir/$archive" -d "$tmpdir"
        mv "$tmpdir/meepo.exe" "$INSTALL_DIR/meepo.exe"
    fi

    echo "  Installed to: $INSTALL_DIR/meepo"

    # Check PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
        echo ""
        echo "  $INSTALL_DIR is not in your PATH."
        echo ""
        local shell_rc=""
        case "${SHELL:-}" in
            */zsh)  shell_rc="$HOME/.zshrc" ;;
            */bash) shell_rc="$HOME/.bashrc" ;;
        esac
        if [ -n "$shell_rc" ]; then
            echo "  Add it now? This appends to $shell_rc"
            printf "  [Y/n] "
            if read -r yn </dev/tty 2>/dev/null; then : ; else yn="Y"; fi
            if [ "${yn:-Y}" != "n" ] && [ "${yn:-Y}" != "N" ]; then
                echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$shell_rc"
                export PATH="$INSTALL_DIR:$PATH"
                echo "  Added to $shell_rc"
            fi
        else
            echo "  Add this to your shell profile:"
            echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
        fi
    fi

    echo ""
    echo "  Meepo ${version} installed!"
    echo ""

    # Run setup (skip if non-interactive)
    if [ -t 0 ] 2>/dev/null || { echo -n '' > /dev/tty; } 2>/dev/null; then
        printf "  Run interactive setup now? [Y/n] "
        if read -r yn </dev/tty 2>/dev/null; then : ; else yn="n"; fi
        if [ "${yn:-Y}" != "n" ] && [ "${yn:-Y}" != "N" ]; then
            echo ""
            "$INSTALL_DIR/meepo" setup
        else
            echo ""
            echo "  Next steps:"
            echo "    meepo setup          # interactive setup wizard"
            echo "    meepo start          # start the daemon"
            echo ""
        fi
    else
        echo "  Next steps:"
        echo "    meepo setup          # interactive setup wizard"
        echo "    meepo start          # start the daemon"
        echo ""
    fi
}

main
