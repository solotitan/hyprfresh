#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="hyprfresh"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/hypr"
SYSTEMD_DIR="${HOME}/.config/systemd/user"

echo "Building ${BINARY_NAME}..."
cargo build --release

echo ""
echo "Installing binary to ${INSTALL_DIR}/"
mkdir -p "${INSTALL_DIR}"
cp "target/release/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "Installing default config to ${CONFIG_DIR}/"
mkdir -p "${CONFIG_DIR}"
if [ ! -f "${CONFIG_DIR}/${BINARY_NAME}.toml" ]; then
    cp "config/${BINARY_NAME}.toml" "${CONFIG_DIR}/${BINARY_NAME}.toml"
    echo "  Created ${CONFIG_DIR}/${BINARY_NAME}.toml"
else
    echo "  Config already exists, skipping (see config/${BINARY_NAME}.toml for reference)"
fi

echo "Installing systemd user service to ${SYSTEMD_DIR}/"
mkdir -p "${SYSTEMD_DIR}"
cp "systemd/${BINARY_NAME}.service" "${SYSTEMD_DIR}/${BINARY_NAME}.service"

echo ""
echo "Enabling systemd user service..."
systemctl --user daemon-reload
systemctl --user enable --now "${BINARY_NAME}.service"

echo ""
echo "Installation complete!"
echo "  Service: systemctl --user status ${BINARY_NAME}"
echo "  Config:  ${CONFIG_DIR}/${BINARY_NAME}.toml"
