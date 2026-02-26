#!/usr/bin/env bash
set -euo pipefail

# presto installer script

SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
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

# ---------------------------------------------------------------------------
# Colors / formatting
# ---------------------------------------------------------------------------

BOLD="\033[1m"
DIM="\033[2m"
GREEN="\033[32m"
RED="\033[31m"
RESET="\033[0m"

# Disable colors if not a terminal
if [[ ! -t 1 ]]; then
    BOLD="" DIM="" GREEN="" RED="" RESET=""
fi

info()  { echo -e "  ${DIM}›${RESET} $*"; }
ok()    { echo -e "  ${GREEN}✓${RESET} $*"; }
fail()  { echo -e "  ${RED}✗${RESET} $*"; }

# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------

check_dependencies() {
    if ! command -v curl >/dev/null 2>&1; then
        fail "curl is required but not installed"
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
            fail "Unsupported platform '${platform}'"
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
            fail "Unsupported architecture '${arch}'"
            exit 1
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

install_presto() {
    local binary_name="presto-${PLATFORM}-${ARCH}"
    local download_url="${R2_BASE_URL}/${binary_name}"

    # Create secure temp directory
    TMP_DIR=$(mktemp -d)
    chmod 700 "${TMP_DIR}"

    local tmp_file="${TMP_DIR}/${BINARY_NAME}"

    info "Downloading from ${download_url}"

    if ! curl -fsSL "${download_url}" -o "${tmp_file}"; then
        fail "Download failed"
        exit 1
    fi

    chmod 755 "${tmp_file}"

    # Verify the binary is actually executable
    if ! file "${tmp_file}" | grep -q "executable"; then
        fail "Downloaded file is not a valid executable"
        exit 1
    fi

    # Quick sanity check
    if ! "${tmp_file}" --version >/dev/null 2>&1; then
        fail "Binary failed sanity check (--version)"
        exit 1
    fi

    if mv "${tmp_file}" "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null; then
        ok "Installed to ${INSTALL_DIR}/${BINARY_NAME}"
    elif sudo mv "${tmp_file}" "${INSTALL_DIR}/${BINARY_NAME}"; then
        ok "Installed to ${INSTALL_DIR}/${BINARY_NAME}"
    else
        fail "Failed to install to ${INSTALL_DIR}"
        echo "  Try running with sudo or install manually"
        exit 1
    fi
}

verify_installation() {
    if command -v presto >/dev/null 2>&1; then
        ok "$(presto --version)"
    else
        echo ""
        echo -e "  ${DIM}Note: ${INSTALL_DIR} is not in your PATH${RESET}"
    fi
}

install_ai_skill() {
    local skill_variant="${1:-local}"
    local skill_dir="${HOME}/.claude/skills/presto"
    local skill_file="${skill_dir}/SKILL.md"
    local local_skill="${SCRIPT_DIR}/.agents/skills/presto-${skill_variant}/SKILL.md"

    mkdir -p "${skill_dir}" 2>/dev/null || return 0

    if [[ -n "${SCRIPT_DIR}" && -f "${local_skill}" ]]; then
        cp "${local_skill}" "${skill_file}"
    else
        local skill_url="${R2_BASE_URL}/SKILL-${skill_variant}.md"
        curl -fsSL "${skill_url}" -o "${skill_file}" 2>/dev/null || return 0
    fi
}

# ---------------------------------------------------------------------------
# Uninstall / helpers
# ---------------------------------------------------------------------------

remove_file() {
    local path="$1"
    local label="$2"
    if [[ ! -f "${path}" && ! -d "${path}" ]]; then
        return 0
    fi
    if rm -rf "${path}" 2>/dev/null || sudo rm -rf "${path}"; then
        ok "Removed ${label}"
    else
        fail "Failed to remove ${label}: ${path}"
    fi
}

uninstall_presto() {
    echo -e "\n${BOLD}Uninstalling presto${RESET}\n"

    remove_file "${INSTALL_DIR}/${BINARY_NAME}" "binary"

    if [[ "$(uname -s)" == "Darwin" ]]; then
        remove_file "${HOME}/Library/Application Support/presto" "data"
    else
        remove_file "${XDG_CONFIG_HOME:-${HOME}/.config}/presto" "config"
        remove_file "${XDG_DATA_HOME:-${HOME}/.local/share}/presto" "data"
    fi

    remove_file "${HOME}/.claude/skills/presto" "AI skill"
    remove_file "${HOME}/.claude/skills/presto-local" "AI skill (local)"
    remove_file "${HOME}/.claude/skills/presto-passkey" "AI skill (passkey)"

    echo ""
    ok "Done"
}

install_local() {
    if ! command -v cargo >/dev/null 2>&1; then
        fail "cargo is required for --local install"
        echo "  Install Rust: https://rustup.rs/"
        exit 1
    fi

    info "Building from source..."
    cargo build --release --manifest-path="${SCRIPT_DIR}/Cargo.toml"

    local built_binary="${SCRIPT_DIR}/target/release/${BINARY_NAME}"
    if [[ ! -f "${built_binary}" ]]; then
        fail "Build succeeded but binary not found at ${built_binary}"
        exit 1
    fi

    if cp "${built_binary}" "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null; then
        ok "Installed to ${INSTALL_DIR}/${BINARY_NAME}"
    elif sudo cp "${built_binary}" "${INSTALL_DIR}/${BINARY_NAME}"; then
        ok "Installed to ${INSTALL_DIR}/${BINARY_NAME}"
    else
        fail "Failed to install to ${INSTALL_DIR}"
        echo "  Try running with sudo or install manually"
        exit 1
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

banner() {
    echo ""
    echo -e "${BOLD}                          __"
    echo -e "    ____  ________  _____/ /_____"
    echo -e "   / __ \\\\/ ___/ _ \\\\/ ___/ __/ __ \\\\"
    echo -e "  / /_/ / /  /  __(__  ) /_/ /_/ /"
    echo -e " / .___/_/   \\\\___/____/\\\\__/\\\\____/"
    echo -e "/_/${RESET}   ${DIM}HTTP client with built-in payments${RESET}"
    echo ""
}

main() {
    local wallet_type="passkey"
    local mode=""
    local args=()

    for arg in "$@"; do
        case "${arg}" in
            --wallet=*)   wallet_type="${arg#--wallet=}" ;;
            --passkey)    wallet_type="passkey" ;;
            --uninstall)  mode="uninstall" ;;
            --reinstall)  mode="reinstall" ;;
            --local)      mode="local" ;;
            *)            args+=("${arg}") ;;
        esac
    done

    if [[ "${wallet_type}" != "local" && "${wallet_type}" != "passkey" ]]; then
        fail "Unknown wallet type '${wallet_type}'. Use 'local' or 'passkey'."
        exit 1
    fi

    if [[ "${mode}" == "uninstall" ]]; then
        uninstall_presto
        exit 0
    fi

    if [[ "${mode}" == "reinstall" ]]; then
        banner
        remove_file "${INSTALL_DIR}/${BINARY_NAME}" "binary"
        install_local
        verify_installation
        install_ai_skill "${wallet_type}"
        echo ""
        ok "Done"
        exit 0
    fi

    banner

    if [[ "${mode}" == "local" ]]; then
        install_local
    else
        check_dependencies
        detect_platform
        detect_arch
        install_presto
    fi

    verify_installation
    install_ai_skill "${wallet_type}"

    echo ""
    echo -e "  ${BOLD}Get started:${RESET}"
    if [[ "${wallet_type}" == "local" ]]; then
        echo -e "    ${DIM}\$${RESET} presto wallet create"
    else
        echo -e "    ${DIM}\$${RESET} presto login"
    fi
    echo -e "    ${DIM}\$${RESET} presto --help"
    echo ""
}

main "$@"
