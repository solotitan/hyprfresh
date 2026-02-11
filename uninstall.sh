#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="hyprfresh"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/hypr"
SYSTEMD_DIR="${HOME}/.config/systemd/user"

echo "Stopping ${BINARY_NAME} service..."
systemctl --user stop "${BINARY_NAME}.service" 2>/dev/null || true
systemctl --user disable "${BINARY_NAME}.service" 2>/dev/null || true

echo "Removing binary..."
rm -f "${INSTALL_DIR}/${BINARY_NAME}"

echo "Removing systemd service..."
rm -f "${SYSTEMD_DIR}/${BINARY_NAME}.service"
systemctl --user daemon-reload 2>/dev/null || true

echo ""
echo "Uninstall complete."
echo ""
echo "Config file preserved at: ${CONFIG_DIR}/${BINARY_NAME}.toml"
echo "Remove it manually if you want a clean uninstall:"
echo "  rm ${CONFIG_DIR}/${BINARY_NAME}.toml"
