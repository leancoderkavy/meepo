#!/usr/bin/env bash
# Install git hooks by symlinking from scripts/ into .git/hooks/
# Usage: ./scripts/install-hooks.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HOOKS_DIR="$REPO_ROOT/.git/hooks"

if [ ! -d "$HOOKS_DIR" ]; then
    echo "Error: .git/hooks directory not found. Are you in a git repo?"
    exit 1
fi

HOOKS=(pre-commit pre-push)
INSTALLED=0

for hook in "${HOOKS[@]}"; do
    SRC="$SCRIPT_DIR/$hook"
    DST="$HOOKS_DIR/$hook"

    if [ ! -f "$SRC" ]; then
        echo "Warning: $SRC not found, skipping"
        continue
    fi

    # Remove existing hook (or symlink)
    if [ -e "$DST" ] || [ -L "$DST" ]; then
        rm "$DST"
    fi

    ln -s "$SRC" "$DST"
    chmod +x "$SRC"
    echo "Installed $hook -> $DST"
    INSTALLED=$((INSTALLED + 1))
done

echo ""
echo "Done. $INSTALLED hook(s) installed."
echo "To uninstall: rm .git/hooks/pre-commit .git/hooks/pre-push"
