#!/usr/bin/env bash
set -euo pipefail

# presto installer script

PRESTO_BANNER='
$$$$$$$\  $$$$$$$\  $$$$$$$$\  $$$$$$\ $$$$$$$$\  $$$$$$\
$$  __$$\ $$  __$$\ $$  _____|$$  __$$\\__$$  __|$$  __$$\
$$ |  $$ |$$ |  $$ |$$ |      $$ /  \__|  $$ |   $$ /  $$ |
$$$$$$$  |$$$$$$$  |$$$$$\    \$$$$$$\    $$ |   $$ |  $$ |
$$  ____/ $$  __$$< $$  __|    \____$$\   $$ |   $$ |  $$ |
$$ |      $$ |  $$ |$$ |      $$\   $$ |  $$ |   $$ |  $$ |
$$ |      $$ |  $$ |$$$$$$$$\ \$$$$$$  |  $$ |    $$$$$$  |
\__|      \__|  \__|\________| \______/   \__|    \______/
'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="tempoxyz/presto"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="presto"
R2_BASE_URL="https://presto-binaries.tempo.xyz"

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

install_presto() {
    local binary_name="presto-${PLATFORM}-${ARCH}"
    local download_url="${R2_BASE_URL}/${binary_name}"
    
    # Create secure temp directory
    TMP_DIR=$(mktemp -d)
    chmod 700 "${TMP_DIR}"
    
    local tmp_file="${TMP_DIR}/${BINARY_NAME}"

    echo ""
    echo "Downloading presto..."
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
    if command -v presto >/dev/null 2>&1; then
        echo "presto is installed and available in PATH"
        echo ""
        presto --version
    else
        echo "presto was installed but is not in PATH"
        echo "Make sure ${INSTALL_DIR} is in your PATH"
    fi
}

install_ai_skill() {
    local skill_dir="${HOME}/.claude/skills/presto"
    local skill_file="${skill_dir}/SKILL.md"
    local local_skill="${SCRIPT_DIR}/.ai/skills/presto/SKILL.md"

    mkdir -p "${skill_dir}" 2>/dev/null || return 0

    if [[ -f "${local_skill}" ]]; then
        # Use local copy when available (--local install or running from repo)
        cp "${local_skill}" "${skill_file}"
        echo "AI skill installed to: ${skill_file}"
    else
        # Download from GitHub
        local skill_url="https://raw.githubusercontent.com/${REPO}/main/.ai/skills/presto/SKILL.md"
        if curl -fsSL "${skill_url}" -o "${skill_file}" 2>/dev/null; then
            echo "AI skill installed to: ${skill_file}"
        fi
    fi
}

remove_file() {
    local path="$1"
    local label="$2"
    if [[ ! -f "${path}" && ! -d "${path}" ]]; then
        echo "  ${label}: not found (already removed)"
        return 0
    fi
    if rm -rf "${path}" 2>/dev/null; then
        echo "  ${label}: ${path}"
    elif sudo rm -rf "${path}"; then
        echo "  ${label}: ${path}"
    else
        echo "  ${label}: FAILED to remove ${path}"
    fi
}

uninstall_presto() {
    echo "Uninstalling presto..."

    # Remove binary
    remove_file "${INSTALL_DIR}/${BINARY_NAME}" "Binary"

    # Remove config + data directory
    # macOS: ~/Library/Application Support/presto (config and data share the same dir)
    # Linux: ~/.config/presto (config), ~/.local/share/presto (data)
    if [[ "$(uname -s)" == "Darwin" ]]; then
        remove_file "${HOME}/Library/Application Support/presto" "Data"
    else
        remove_file "${XDG_CONFIG_HOME:-${HOME}/.config}/presto" "Config"
        remove_file "${XDG_DATA_HOME:-${HOME}/.local/share}/presto" "Data"
    fi

    # Remove AI skill
    remove_file "${HOME}/.claude/skills/presto" "AI skill"

    echo ""
    echo "presto has been uninstalled."
}

install_local() {
    echo ""
    echo "Building presto from source..."

    if ! command -v cargo >/dev/null 2>&1; then
        echo "Error: cargo is required for --local install. Install Rust: https://rustup.rs/"
        exit 1
    fi

    cargo build --release --manifest-path="${SCRIPT_DIR}/Cargo.toml"

    local built_binary="${SCRIPT_DIR}/target/release/${BINARY_NAME}"
    if [[ ! -f "${built_binary}" ]]; then
        echo "Error: Build succeeded but binary not found at ${built_binary}"
        exit 1
    fi

    echo "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."

    if cp "${built_binary}" "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null; then
        echo "Installation successful!"
    elif sudo cp "${built_binary}" "${INSTALL_DIR}/${BINARY_NAME}"; then
        echo "Installation successful!"
    else
        echo "Error: Failed to install to ${INSTALL_DIR}"
        echo "Try running with sudo or install manually"
        exit 1
    fi
}

main() {
    if [[ "${1:-}" == "--uninstall" ]]; then
        uninstall_presto
        exit 0
    fi

    if [[ "${1:-}" == "--reinstall" ]]; then
        uninstall_presto
        echo ""
        install_local
        verify_installation
        install_ai_skill
        echo ""
        echo "Reinstall complete!"
        exit 0
    fi

    echo "$PRESTO_BANNER"
    echo ""

    if [[ "${1:-}" == "--local" ]]; then
        install_local
    else
        check_dependencies
        detect_platform
        detect_arch
        install_presto
    fi

    verify_installation
    install_ai_skill

    echo ""
    echo "Installation complete!"
    echo ""
    echo "Get started:"
    echo "  presto login         # Connect your Tempo wallet"
    echo "  presto --help        # Show all options"
    echo ""
    echo "Documentation:"
    echo "  https://github.com/${REPO}"
    echo ""
}

main "$@"
