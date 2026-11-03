#!/usr/bin/env bash
set -euo pipefail

# Karkinos installer.
# Supports a user-local install (default) or a system-wide install when run as root.

# Resolve the repository root (parent of the directory containing this script).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Detect whether we are running as root to choose the install prefix.
if [ "$(id -u)" -eq 0 ]; then
    PREFIX="/usr/local"
else
    PREFIX="$HOME/.local"
fi

BIN_DIR="$PREFIX/bin"
APPS_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/256x256/apps"

echo "==> Installing Karkinos into $PREFIX"

# Create the target directories.
mkdir -p "$BIN_DIR" "$APPS_DIR" "$ICON_DIR"

# Build (when needed) and install the binary.
if [ -f "$REPO_ROOT/target/release/karkinos" ]; then
    echo "==> Using existing release binary"
else
    echo "==> Release binary not found, building with cargo"
    (cd "$REPO_ROOT" && cargo build --release)
fi

install -Dm755 "$REPO_ROOT/target/release/karkinos" "$BIN_DIR/karkinos"

# Install the desktop entry and the application icon.
install -Dm644 "$REPO_ROOT/assets/karkinos.desktop" "$APPS_DIR/karkinos.desktop"
install -Dm644 "$REPO_ROOT/assets/karkinos.png" "$ICON_DIR/karkinos.png"

# Refresh the icon and desktop caches when the tools are available.
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS_DIR" || true
fi

# Check whether $PREFIX/bin is on PATH and warn when it is not.
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo ""
        echo "==> Heads up: $BIN_DIR is not on your PATH."
        echo "    To launch Karkinos from anywhere, add this line to your shell profile:"
        echo ""
        echo "        export PATH=\"$BIN_DIR:\$PATH\""
        echo ""
        echo "    Bash:  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc"
        echo "    Zsh:   echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc"
        echo "    Then run 'source ~/.bashrc' (or ~/.zshrc) or open a new terminal."
        ;;
esac

echo ""
echo "==> Karkinos installed successfully!"
echo "    Run 'karkinos' to start the IDE."
