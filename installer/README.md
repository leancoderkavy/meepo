# Meepo macOS Installer

Build a `.dmg` disk image for easy Meepo installation on macOS.

## Quick Start

```bash
# Build the DMG (builds the Rust binary + packages everything)
./installer/scripts/build-dmg.sh

# Or skip the cargo build if you already have a release binary
./installer/scripts/build-dmg.sh --skip-build
```

The DMG is output to `installer/dist/Meepo-<version>-macOS.dmg`.

## What's Inside the DMG

- **Meepo.app** — Drag to `/Applications` to install
  - First launch opens the setup wizard in Terminal
  - Subsequent launches start the daemon in the background
  - The `meepo` CLI binary is at `Meepo.app/Contents/MacOS/meepo-bin`
- **Applications symlink** — Drag-and-drop target
- **README.txt** — Quick-start instructions

## Prerequisites

### Required
- **Rust toolchain** — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **macOS 12+** with `hdiutil`, `sips`, `iconutil` (built-in)

### Optional (for better output)
- **create-dmg** — Styled DMG with background image and icon positioning
  ```bash
  brew install create-dmg
  ```
- **librsvg** — Best SVG-to-PNG conversion for the icon
  ```bash
  brew install librsvg
  ```

## Build Options

```bash
# Default: build for current architecture
./installer/scripts/build-dmg.sh

# Universal binary (arm64 + x86_64)
./installer/scripts/build-dmg.sh --arch universal

# Skip cargo build (use existing target/release/meepo)
./installer/scripts/build-dmg.sh --skip-build
```

## Directory Structure

```
installer/
├── assets/
│   ├── icon.svg              # Meepo app icon (Geomancer theme)
│   ├── dmg-background.svg    # DMG window background
│   └── Meepo.icns            # Generated macOS icon (after first build)
├── scripts/
│   ├── build-dmg.sh          # Main build script
│   └── generate-icon.sh      # SVG → .icns converter
├── dist/                     # Build output (gitignored)
│   └── Meepo-x.y.z-macOS.dmg
└── README.md                 # This file
```

## Customizing the Icon

Edit `installer/assets/icon.svg` in any SVG editor, then regenerate:

```bash
# Delete old icon and rebuild
rm -f installer/assets/Meepo.icns
./installer/scripts/generate-icon.sh
```

The icon uses the Meepo color palette:
- **Hood Blue** `#3B6B8A` — Primary accent
- **Earth Brown** `#8B6914` — Leather/wood elements
- **Warm Tan** `#C4A265` — Skin tones
- **Gold Accent** `#D4A843` — Eyes, highlights
- **Cave Dark** `#1A1410` — Background

## After Installation

Once users install Meepo.app:

1. **First launch** opens Terminal with the interactive setup wizard
2. **Add to PATH** (optional):
   ```bash
   sudo ln -sf /Applications/Meepo.app/Contents/MacOS/meepo-bin /usr/local/bin/meepo
   ```
3. **Run as background service:**
   ```bash
   meepo start
   ```

## iOS Companion App

See [docs/IOS_SETUP_GUIDE.md](../docs/IOS_SETUP_GUIDE.md) for instructions on building and connecting the iOS app.
