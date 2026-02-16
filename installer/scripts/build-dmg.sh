#!/bin/bash
set -euo pipefail

# Build a macOS .dmg installer for Meepo
# Usage: ./build-dmg.sh [--skip-build] [--arch universal|arm64|x86_64]
#
# Prerequisites:
#   - Rust toolchain (cargo)
#   - macOS with hdiutil, sips, iconutil
#   - Optional: create-dmg (brew install create-dmg) for prettier DMG
#
# Output: installer/dist/Meepo-<version>-macOS.dmg

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALLER_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(dirname "$INSTALLER_DIR")"
DIST_DIR="$INSTALLER_DIR/dist"
STAGING_DIR="$INSTALLER_DIR/.staging"
ASSETS_DIR="$INSTALLER_DIR/assets"

# Parse arguments
SKIP_BUILD=false
ARCH="$(uname -m)"  # Default to current architecture

while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build) SKIP_BUILD=true; shift ;;
        --arch) ARCH="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--skip-build] [--arch universal|arm64|x86_64]"
            echo ""
            echo "Options:"
            echo "  --skip-build    Skip cargo build, use existing binary"
            echo "  --arch          Target architecture (default: current machine)"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Get version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
APP_NAME="Meepo"
DMG_NAME="${APP_NAME}-${VERSION}-macOS"
APP_BUNDLE="$STAGING_DIR/${APP_NAME}.app"

echo "╔══════════════════════════════════════════╗"
echo "║       Meepo DMG Installer Builder        ║"
echo "║       Version: $VERSION                       ║"
echo "╚══════════════════════════════════════════╝"
echo ""

# --- Step 1: Build the binary ---
if [ "$SKIP_BUILD" = false ]; then
    echo "▸ Building Meepo (release)..."

    if [ "$ARCH" = "universal" ]; then
        echo "  Building universal binary (arm64 + x86_64)..."
        cargo build --release --target aarch64-apple-darwin --manifest-path "$PROJECT_ROOT/Cargo.toml"
        cargo build --release --target x86_64-apple-darwin --manifest-path "$PROJECT_ROOT/Cargo.toml"
        BINARY_ARM="$PROJECT_ROOT/target/aarch64-apple-darwin/release/meepo"
        BINARY_X86="$PROJECT_ROOT/target/x86_64-apple-darwin/release/meepo"
    else
        cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"
    fi

    echo "  ✓ Build complete"
else
    echo "▸ Skipping build (--skip-build)"
fi

# Locate the binary
if [ "$ARCH" = "universal" ]; then
    BINARY="$STAGING_DIR/meepo-universal"
else
    BINARY="$PROJECT_ROOT/target/release/meepo"
fi

if [ "$ARCH" != "universal" ] && [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    echo "Run without --skip-build or build first with: cargo build --release"
    exit 1
fi

# --- Step 2: Generate icon if needed ---
ICNS_PATH="$ASSETS_DIR/Meepo.icns"
if [ ! -f "$ICNS_PATH" ]; then
    echo "▸ Generating app icon..."
    bash "$SCRIPT_DIR/generate-icon.sh"
fi

if [ ! -f "$ICNS_PATH" ]; then
    echo "Warning: Could not generate .icns icon. DMG will use default icon."
    ICNS_PATH=""
fi

# --- Step 3: Create .app bundle ---
echo "▸ Creating ${APP_NAME}.app bundle..."
rm -rf "$STAGING_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binary (named meepo-bin to avoid case collision with Meepo launcher on HFS+/APFS)
if [ "$ARCH" = "universal" ]; then
    lipo -create "$BINARY_ARM" "$BINARY_X86" -output "$APP_BUNDLE/Contents/MacOS/meepo-bin"
else
    cp "$BINARY" "$APP_BUNDLE/Contents/MacOS/meepo-bin"
fi

# Create the launcher script that runs setup on first launch, then starts daemon
cat > "$APP_BUNDLE/Contents/MacOS/Meepo" << 'LAUNCHER'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MEEPO_BIN="$SCRIPT_DIR/meepo-bin"
MEEPO_DIR="$HOME/.meepo"

# Create PATH symlink if possible (silent, best-effort)
if [ ! -e "/usr/local/bin/meepo" ] && [ -w "/usr/local/bin" ]; then
    ln -sf "$MEEPO_BIN" "/usr/local/bin/meepo" 2>/dev/null || true
fi

# If first launch (no config), run setup in a terminal
if [ ! -f "$MEEPO_DIR/config.toml" ]; then
    osascript -e "
        tell application \"Terminal\"
            activate
            do script \"'$MEEPO_BIN' setup && echo '' && echo '✓ Meepo is ready! You can close this window.' && echo 'Run: meepo start'\"
        end tell
    "
else
    # Already configured — start the daemon
    "$MEEPO_BIN" start &

    # Show a notification
    osascript -e 'display notification "Meepo daemon started. Divided we stand." with title "Meepo" sound name "Submarine"' 2>/dev/null || true
fi
LAUNCHER
chmod +x "$APP_BUNDLE/Contents/MacOS/Meepo"
chmod +x "$APP_BUNDLE/Contents/MacOS/meepo-bin"

# Create Info.plist
cat > "$APP_BUNDLE/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>com.meepo.agent</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleExecutable</key>
    <string>Meepo</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleSignature</key>
    <string>MEPO</string>
    <key>CFBundleIconFile</key>
    <string>Meepo</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright © 2024-2026 Meepo Contributors. MIT License.</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.productivity</string>
</dict>
</plist>
EOF

# Copy icon
if [ -n "$ICNS_PATH" ] && [ -f "$ICNS_PATH" ]; then
    cp "$ICNS_PATH" "$APP_BUNDLE/Contents/Resources/Meepo.icns"
fi

# Copy default config template
cp "$PROJECT_ROOT/config/default.toml" "$APP_BUNDLE/Contents/Resources/default.toml"

# Also symlink the binary to /usr/local/bin on install
cat > "$APP_BUNDLE/Contents/Resources/postinstall.sh" << 'POSTINSTALL'
#!/bin/bash
# Optional: create symlink so 'meepo' is available in PATH
MEEPO_BIN="/Applications/Meepo.app/Contents/MacOS/meepo-bin"
LINK_PATH="/usr/local/bin/meepo"

if [ -f "$MEEPO_BIN" ]; then
    if [ -w "/usr/local/bin" ]; then
        ln -sf "$MEEPO_BIN" "$LINK_PATH" 2>/dev/null && \
            echo "✓ Linked meepo to $LINK_PATH" || true
    fi
fi
POSTINSTALL
chmod +x "$APP_BUNDLE/Contents/Resources/postinstall.sh"

# --- Step 3b: Ad-hoc code sign the app bundle ---
echo "▸ Code signing (ad-hoc)..."
codesign --force --deep -s - "$APP_BUNDLE" 2>/dev/null && \
    echo "  ✓ Ad-hoc signed (bypasses Gatekeeper 'damaged' errors)" || \
    echo "  ⚠ codesign failed (app will still work but may trigger Gatekeeper warnings)"

echo "  ✓ App bundle created"

# --- Step 4: Create DMG ---
echo "▸ Creating DMG..."
mkdir -p "$DIST_DIR"

DMG_PATH="$DIST_DIR/${DMG_NAME}.dmg"
rm -f "$DMG_PATH"

# Convert DMG background SVG to PNG
BG_SVG="$ASSETS_DIR/dmg-background.svg"
BG_PNG="$STAGING_DIR/dmg-background.png"

if [ -f "$BG_SVG" ]; then
    if command -v rsvg-convert &>/dev/null; then
        rsvg-convert -w 660 -h 400 "$BG_SVG" -o "$BG_PNG"
    elif command -v cairosvg &>/dev/null; then
        cairosvg "$BG_SVG" -o "$BG_PNG" -W 660 -H 400
    else
        # Use qlmanage as fallback
        qlmanage -t -s 660 -o "$STAGING_DIR" "$BG_SVG" 2>/dev/null
        mv "$STAGING_DIR/dmg-background.svg.png" "$BG_PNG" 2>/dev/null || true
    fi
fi

# Create the README for inside the DMG
cat > "$STAGING_DIR/README.txt" << 'README'
╔══════════════════════════════════════════════════════════╗
║                    Welcome to Meepo                      ║
║              Local AI Agent for macOS                    ║
║                 Divided We Stand                         ║
╚══════════════════════════════════════════════════════════╝

INSTALLATION
────────────
1. Drag Meepo.app to the Applications folder
2. Launch Meepo from Applications
3. On first launch, a Terminal window opens with the setup wizard
4. Follow the prompts to configure API keys and permissions

AFTER INSTALLATION
──────────────────
• The 'meepo' CLI binary is at:
  /Applications/Meepo.app/Contents/MacOS/meepo-bin

• To add it to your PATH (if not done automatically), run:
  sudo ln -sf /Applications/Meepo.app/Contents/MacOS/meepo-bin /usr/local/bin/meepo

• Common commands:
  meepo start          Start the agent daemon
  meepo stop           Stop the daemon
  meepo ask "..."      One-shot question
  meepo setup          Re-run setup wizard
  meepo doctor         Diagnose issues

GATEKEEPER NOTE
───────────────
If macOS says the app is "damaged" or from an "unidentified developer":
  Right-click Meepo.app → Open → click Open in the dialog
Or run:  xattr -cr /Applications/Meepo.app

iOS COMPANION APP
─────────────────
See the iOS Setup Guide at:
  https://github.com/leancoderkavy/meepo/blob/main/docs/IOS_SETUP_GUIDE.md

UNINSTALLING
────────────
To completely remove Meepo:
  1. rm -rf /Applications/Meepo.app
  2. rm -f /usr/local/bin/meepo
  3. rm -rf ~/.meepo                  (removes config, knowledge, logs)
  4. launchctl unload ~/Library/LaunchAgents/com.meepo.meepo.plist 2>/dev/null
     rm -f ~/Library/LaunchAgents/com.meepo.meepo.plist

REQUIREMENTS
────────────
• macOS 12.0 (Monterey) or later
• At least one LLM provider:
  - Anthropic Claude API key
  - OpenAI API key
  - Google Gemini API key
  - Ollama (free, runs locally)

MORE INFO
─────────
• Documentation: https://github.com/leancoderkavy/meepo
• Report issues: https://github.com/leancoderkavy/meepo/issues
README

# Try create-dmg first (prettier), fall back to hdiutil
if command -v create-dmg &>/dev/null; then
    echo "  Using create-dmg for styled DMG..."

    CREATE_DMG_ARGS=(
        --volname "$APP_NAME"
        --volicon "$ICNS_PATH"
        --window-pos 200 120
        --window-size 660 400
        --icon-size 80
        --icon "$APP_NAME.app" 175 190
        --app-drop-link 485 190
        --text-size 12
        --hide-extension "$APP_NAME.app"
        --add-file "README.txt" "$STAGING_DIR/README.txt" 330 340
    )

    # Add background if we have it
    if [ -f "$BG_PNG" ]; then
        CREATE_DMG_ARGS+=(--background "$BG_PNG")
    fi

    create-dmg "${CREATE_DMG_ARGS[@]}" "$DMG_PATH" "$APP_BUNDLE" || {
        echo "  create-dmg failed, falling back to hdiutil..."
        FALLBACK=true
    }
else
    FALLBACK=true
fi

if [ "${FALLBACK:-false}" = true ]; then
    echo "  Using hdiutil (install 'brew install create-dmg' for styled DMG)..."

    # Create a temporary DMG directory
    DMG_STAGING="$STAGING_DIR/dmg"
    mkdir -p "$DMG_STAGING"
    cp -R "$APP_BUNDLE" "$DMG_STAGING/"
    cp "$STAGING_DIR/README.txt" "$DMG_STAGING/"

    # Create Applications symlink
    ln -s /Applications "$DMG_STAGING/Applications"

    # Create DMG
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$DMG_STAGING" \
        -ov -format UDZO \
        "$DMG_PATH"
fi

# --- Step 5: Clean up ---
echo "▸ Cleaning up..."
rm -rf "$STAGING_DIR"

# --- Done ---
DMG_SIZE=$(du -h "$DMG_PATH" | cut -f1)
echo ""
echo "╔══════════════════════════════════════════╗"
echo "║            DMG Build Complete!            ║"
echo "╠══════════════════════════════════════════╣"
echo "║  File: ${DMG_NAME}.dmg"
echo "║  Size: ${DMG_SIZE}"
echo "║  Path: $DMG_PATH"
echo "╚══════════════════════════════════════════╝"
echo ""
echo "To test: open \"$DMG_PATH\""
