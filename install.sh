#!/usr/bin/env bash
set -euo pipefail

# tempoctl installer script

TEMPOCTL_BANNER='
$$$$$$$$\ $$$$$$$$\ $$\      $$\ $$$$$$$\   $$$$$$\   $$$$$$\ $$$$$$$$\ $$\
\__$$  __|$$  _____|$$$\    $$$ |$$  __$$\ $$  __$$\ $$  __$$\\__$$  __|$$ |
   $$ |   $$ |      $$$$\  $$$$ |$$ |  $$ |$$ /  $$ |$$ /  \__|  $$ |   $$ |
   $$ |   $$$$$\    $$\$$\$$ $$ |$$$$$$$  |$$ |  $$ |$$ |        $$ |   $$ |
   $$ |   $$  __|   $$ \$$$  $$ |$$  ____/ $$ |  $$ |$$ |        $$ |   $$ |
   $$ |   $$ |      $$ |\$  /$$ |$$ |      $$ |  $$ |$$ |  $$\   $$ |   $$ |
   $$ |   $$$$$$$$\ $$ | \_/ $$ |$$ |       $$$$$$  |\$$$$$$  |  $$ |   $$$$$$$$\
   \__|   \________|\__|     \__|\__|       \______/  \______/   \__|   \________|
'

echo "$TEMPOCTL_BANNER"
echo ""

REPO="tempoxyz/pget"
BINARY_NAME="tempoctl"
INSTALL_DIR="/usr/local/bin"
R2_BASE_URL="https://tempoctl-binaries.tempo.xyz"

TMP_DIR=""

cleanup() {
    if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
        rm -rf "${TMP_DIR}"
    fi
}
trap cleanup EXIT

check_dependencies() {
    if ! command -v curl >/dev/null 2>&1; then
        echo "error: curl is required but not installed"
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
            echo "error: unsupported platform '${platform}'"
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
            echo "error: unsupported architecture '${arch}'"
            exit 1
            ;;
    esac
}

install_tempoctl() {
    local binary_name="tempoctl-${PLATFORM}-${ARCH}"
    local download_url="${R2_BASE_URL}/${binary_name}"

    TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t tempoctl)"
    chmod 700 "${TMP_DIR}"

    local tmp_file="${TMP_DIR}/${BINARY_NAME}"

    echo "info: downloading tempoctl for ${PLATFORM}-${ARCH}..."

    if ! curl -fsSL "${download_url}" -o "${tmp_file}"; then
        echo "error: download failed"
        exit 1
    fi

    chmod 755 "${tmp_file}"

    if command -v file >/dev/null 2>&1; then
        if ! file "${tmp_file}" | grep -q "executable"; then
            echo "error: downloaded file is not a valid executable"
            exit 1
        fi
    fi

    if ! "${tmp_file}" --version >/dev/null 2>&1; then
        echo "error: binary failed to execute (--version check failed)"
        exit 1
    fi

    echo "info: installing to ${INSTALL_DIR}/..."

    if mv "${tmp_file}" "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null; then
        true
    elif sudo mv "${tmp_file}" "${INSTALL_DIR}/${BINARY_NAME}"; then
        true
    else
        echo "error: failed to install to ${INSTALL_DIR}"
        echo "       try running with sudo"
        exit 1
    fi

    if ln -sf "${INSTALL_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/tempo" 2>/dev/null; then
        true
    elif sudo ln -sf "${INSTALL_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/tempo"; then
        true
    fi
}

verify_installation() {
    echo ""
    if command -v tempoctl >/dev/null 2>&1; then
        echo "info: $(tempoctl --version)"
    else
        echo "info: installed — run with: ${INSTALL_DIR}/tempoctl --version"
    fi
}

main() {
    check_dependencies
    detect_platform
    detect_arch
    install_tempoctl
    verify_installation

    echo ""
    echo "Get started:"
    echo "  tempoctl login         # Connect your Tempo wallet"
    echo "  tempoctl --help        # Show all options"
    echo ""
    echo "Documentation:"
    echo "  https://github.com/${REPO}"
    echo ""
}

main
