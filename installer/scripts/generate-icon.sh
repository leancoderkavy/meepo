#!/bin/bash
set -euo pipefail

# Generate .icns icon from SVG using macOS built-in tools
# Requires: macOS with sips and iconutil

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALLER_DIR="$(dirname "$SCRIPT_DIR")"
SVG_PATH="$INSTALLER_DIR/assets/icon.svg"
OUTPUT_DIR="$INSTALLER_DIR/assets"
ICONSET_DIR="$OUTPUT_DIR/Meepo.iconset"

echo "Generating Meepo.icns from SVG..."

# Create iconset directory
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

# Convert SVG to a high-res PNG first using qlmanage (built into macOS)
TEMP_PNG="$OUTPUT_DIR/icon_master.png"

# Try rsvg-convert first (best quality), fall back to qlmanage
if command -v rsvg-convert &>/dev/null; then
    echo "  Using rsvg-convert..."
    rsvg-convert -w 1024 -h 1024 "$SVG_PATH" -o "$TEMP_PNG"
elif command -v cairosvg &>/dev/null; then
    echo "  Using cairosvg..."
    cairosvg "$SVG_PATH" -o "$TEMP_PNG" -W 1024 -H 1024
else
    echo "  Using qlmanage (macOS built-in)..."
    qlmanage -t -s 1024 -o "$OUTPUT_DIR" "$SVG_PATH" 2>/dev/null
    # qlmanage outputs as icon.svg.png
    mv "$OUTPUT_DIR/icon.svg.png" "$TEMP_PNG" 2>/dev/null || true
fi

if [ ! -f "$TEMP_PNG" ]; then
    echo "Error: Could not convert SVG to PNG."
    echo "Install one of: brew install librsvg  OR  pip install cairosvg"
    exit 1
fi

# Generate all required icon sizes
SIZES=(16 32 64 128 256 512 1024)
for size in "${SIZES[@]}"; do
    sips -z "$size" "$size" "$TEMP_PNG" --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null 2>&1
done

# Create @2x variants (Retina)
sips -z 32 32 "$TEMP_PNG" --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null 2>&1
sips -z 64 64 "$TEMP_PNG" --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null 2>&1
sips -z 256 256 "$TEMP_PNG" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null 2>&1
sips -z 512 512 "$TEMP_PNG" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null 2>&1
sips -z 1024 1024 "$TEMP_PNG" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null 2>&1

# Remove non-standard sizes from iconset (keep only what iconutil expects)
rm -f "$ICONSET_DIR/icon_64x64.png"
rm -f "$ICONSET_DIR/icon_1024x1024.png"

# Convert iconset to icns
iconutil -c icns "$ICONSET_DIR" -o "$OUTPUT_DIR/Meepo.icns"

# Clean up
rm -rf "$ICONSET_DIR"
rm -f "$TEMP_PNG"

echo "Created: $OUTPUT_DIR/Meepo.icns"
