#!/usr/bin/env bash
set -euo pipefail

# purl installer script

PURL_BANNER='
 /$$$$$$$  /$$   /$$ /$$$$$$$  /$$
| $$__  $$| $$  | $$| $$__  $$| $$
| $$  \ $$| $$  | $$| $$  \ $$| $$
| $$$$$$$/| $$  | $$| $$$$$$$/| $$
| $$____/ | $$  | $$| $$__  $$| $$
| $$      | $$  | $$| $$  \ $$| $$
| $$      |  $$$$$$/| $$  | $$| $$$$$$$$
|__/       \______/ |__/  |__/|________/
'

echo "$PURL_BANNER"
echo ""

REPO="tempoxyz/purl"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="purl"
R2_BASE_URL="https://purl-binaries.tempo.xyz"

# Temp directory for downloads (cleaned up on exit)
TMP_DIR=""

cleanup() {
    if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
        rm -rf "${TMP_DIR}"
    fi
}
trap cleanup EXIT

check_dependencies() {
    if ! command -v curl >/dev/null 2>&1; then
        echo "Error: curl is required but not installed"
        exit 1
    fi
}

detect_platform() {
    local platform
    platform="$(uname -s | tr '[:upper:]' '[:lower:]')"

    case "${platform}" in
        linux*)     PLATFORM="linux" ;;
        darwin*)    PLATFORM="darwin" ;;
        *)
            echo "Error: Unsupported platform '${platform}'"
            exit 1
            ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"

    case "${arch}" in
        x86_64|amd64)   ARCH="amd64" ;;
        aarch64|arm64)  ARCH="arm64" ;;
        *)
            echo "Error: Unsupported architecture '${arch}'"
            exit 1
            ;;
    esac
}

install_purl() {
    local binary_name="purl-${PLATFORM}-${ARCH}"
    local download_url="${R2_BASE_URL}/${binary_name}"
    
    # Create secure temp directory
    TMP_DIR=$(mktemp -d)
    chmod 700 "${TMP_DIR}"
    
    local tmp_file="${TMP_DIR}/${BINARY_NAME}"

    echo ""
    echo "Downloading purl..."
    echo "URL: ${download_url}"

    if ! curl -fsSL "${download_url}" -o "${tmp_file}"; then
        echo "Error: Download failed"
        exit 1
    fi

    echo ""
    echo "Making binary executable..."
    chmod 755 "${tmp_file}"
    
    # Verify the binary is actually executable
    if ! file "${tmp_file}" | grep -q "executable"; then
        echo "Error: Downloaded file is not a valid executable"
        exit 1
    fi
    
    # Quick sanity check - try to run --version
    if ! "${tmp_file}" --version >/dev/null 2>&1; then
        echo "Error: Binary failed to execute (--version check failed)"
        exit 1
    fi

    echo "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."

    if mv "${tmp_file}" "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null; then
        echo "Installation successful!"
    elif sudo mv "${tmp_file}" "${INSTALL_DIR}/${BINARY_NAME}"; then
        echo "Installation successful!"
    else
        echo "Error: Failed to install to ${INSTALL_DIR}"
        echo "Try running with sudo or install manually"
        exit 1
    fi
}

verify_installation() {
    echo ""
    if command -v purl >/dev/null 2>&1; then
        echo "purl is installed and available in PATH"
        echo ""
        purl --version
    else
        echo "purl was installed but is not in PATH"
        echo "Make sure ${INSTALL_DIR} is in your PATH"
    fi
}

main() {
    check_dependencies
    detect_platform
    detect_arch
    install_purl
    verify_installation

    echo ""
    echo "Installation complete!"
    echo ""
    echo "Get started:"
    echo "  purl init          # Configure your wallets"
    echo "  purl --help        # Show all options"
    echo ""
    echo "Documentation:"
    echo "  https://github.com/${REPO}"
    echo ""
}

main
