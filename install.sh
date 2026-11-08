#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# OneNoteOnly — Native Linux OneNote Editor & Viewer Universal Installer
# ==============================================================================

echo "🚀 Installing OneNoteOnly for Linux..."

BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

mkdir -p "${BIN_DIR}" "${APP_DIR}" "${ICON_DIR}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_RELEASE="/mnt/games/cargo_target/release"

# Determine binary location
if [ -f "${TARGET_RELEASE}/onenote-viewer" ]; then
    SRC_BIN="${TARGET_RELEASE}/onenote-viewer"
elif [ -f "${SCRIPT_DIR}/target/release/onenote-viewer" ]; then
    SRC_BIN="${SCRIPT_DIR}/target/release/onenote-viewer"
elif [ -f "${BIN_DIR}/onenote-viewer" ]; then
    SRC_BIN="${BIN_DIR}/onenote-viewer"
else
    echo "⚠️ Release binary not found. Building now..."
    cd "${SCRIPT_DIR}"
    cargo build --release -p onenote-viewer
    SRC_BIN="${SCRIPT_DIR}/target/release/onenote-viewer"
fi

# Install main binaries
echo "📦 Installing binary to ${BIN_DIR}/onenoteonly..."
cp -f "${SRC_BIN}" "${BIN_DIR}/onenoteonly"
chmod +x "${BIN_DIR}/onenoteonly"
ln -sf "${BIN_DIR}/onenoteonly" "${BIN_DIR}/onenote-viewer"

# Install export tool if available
if [ -f "${TARGET_RELEASE}/onenote-export" ]; then
    cp -f "${TARGET_RELEASE}/onenote-export" "${BIN_DIR}/onenote-export"
    chmod +x "${BIN_DIR}/onenote-export"
elif [ -f "${BIN_DIR}/onenote-export" ]; then
    echo "ℹ️  onenote-export already installed in ${BIN_DIR}"
fi

# Install app icon
echo "🎨 Installing icon to ${ICON_DIR}/onenoteonly.svg..."
if [ -f "${SCRIPT_DIR}/crates/onenote-viewer/resources/icons/scalable/apps/onenoteonly.svg" ]; then
    cp -f "${SCRIPT_DIR}/crates/onenote-viewer/resources/icons/scalable/apps/onenoteonly.svg" "${ICON_DIR}/onenoteonly.svg"
elif [ -f "/tmp/onenoteonly_logo.svg" ]; then
    cp -f "/tmp/onenoteonly_logo.svg" "${ICON_DIR}/onenoteonly.svg"
fi

# Install desktop entry
echo "🖥️  Installing desktop entry to ${APP_DIR}/OneNoteOnly.desktop..."
if [ -f "${SCRIPT_DIR}/packaging/OneNoteOnly.desktop" ]; then
    cp -f "${SCRIPT_DIR}/packaging/OneNoteOnly.desktop" "${APP_DIR}/OneNoteOnly.desktop"
fi

# Update desktop and icon databases
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${APP_DIR}" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" 2>/dev/null || true
fi

echo ""
echo "✅ OneNoteOnly installed successfully!"
echo "👉 Launch from your Application Menu or run:"
echo "   onenoteonly [file.one]"
