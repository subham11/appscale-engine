#!/bin/sh
# AppScale CLI installer
# Usage: curl -fsSL https://raw.githubusercontent.com/subham11/appscale-engine/main/install.sh | sh

set -e

REPO="subham11/appscale-engine"
BINARY_NAME="appscale"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
detect_platform() {
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  ARCH=$(uname -m)

  case "$OS" in
    darwin) OS="apple-darwin" ;;
    linux)  OS="unknown-linux-gnu" ;;
    *)
      echo "Error: Unsupported OS: $OS"
      echo "Use 'cargo install appscale-cli' instead."
      exit 1
      ;;
  esac

  case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *)
      echo "Error: Unsupported architecture: $ARCH"
      echo "Use 'cargo install appscale-cli' instead."
      exit 1
      ;;
  esac

  TARGET="${ARCH}-${OS}"
}

# Get latest release version from GitHub
get_latest_version() {
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | head -1 \
    | sed 's/.*"tag_name": *"//;s/".*//')

  if [ -z "$VERSION" ]; then
    echo "Error: Could not determine latest version."
    exit 1
  fi
}

main() {
  detect_platform
  get_latest_version

  ARCHIVE_NAME="${BINARY_NAME}-${VERSION}-${TARGET}.tar.gz"
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE_NAME}"

  echo "Installing ${BINARY_NAME} ${VERSION} (${TARGET})..."
  echo "  from: ${DOWNLOAD_URL}"
  echo "  to:   ${INSTALL_DIR}/${BINARY_NAME}"
  echo ""

  TMPDIR=$(mktemp -d)
  trap 'rm -rf "$TMPDIR"' EXIT

  curl -fsSL "$DOWNLOAD_URL" -o "${TMPDIR}/${ARCHIVE_NAME}"

  tar xzf "${TMPDIR}/${ARCHIVE_NAME}" -C "$TMPDIR"

  # Find the binary inside the extracted directory
  EXTRACTED_DIR=$(find "$TMPDIR" -type d -name "${BINARY_NAME}-*" | head -1)
  if [ -z "$EXTRACTED_DIR" ]; then
    BINARY_PATH="${TMPDIR}/${BINARY_NAME}"
  else
    BINARY_PATH="${EXTRACTED_DIR}/${BINARY_NAME}"
  fi

  if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: Binary not found in archive."
    exit 1
  fi

  chmod +x "$BINARY_PATH"

  if [ -w "$INSTALL_DIR" ]; then
    mv "$BINARY_PATH" "${INSTALL_DIR}/${BINARY_NAME}"
  else
    echo "Need sudo to install to ${INSTALL_DIR}"
    sudo mv "$BINARY_PATH" "${INSTALL_DIR}/${BINARY_NAME}"
  fi

  echo ""
  echo "Successfully installed ${BINARY_NAME} ${VERSION}"
  echo "Run 'appscale --help' to get started."
}

main
