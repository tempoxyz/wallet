#!/usr/bin/env bash
set -euo pipefail

#  tempo-walletinstaller script
#
# Usage:
#   curl -fsSL https://presto-binaries.tempo.xyz/install.sh | bash
#
# Options:
#   --wallet=local    Install with local wallet mode (default: passkey)
#   --from-source     Build and install from source (requires cargo)
#   --uninstall       Remove  tempo-walletbinary, config, data, and AI skills
#
# Environment:
#   PRESTO_WALLET_TYPE=local   Same as --wallet=local

SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
INSTALL_DIR="${HOME}/.local/bin"
LEGACY_INSTALL_DIR="/usr/local/bin"
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

if [[ ! -t 1 ]]; then
    BOLD="" DIM="" GREEN="" RED="" RESET=""
fi

info()  { echo -e "  ${DIM}›${RESET} $*"; }
ok()    { echo -e "  ${GREEN}✓${RESET} $*"; }
fail()  { echo -e "  ${RED}✗${RESET} $*"; }

# ---------------------------------------------------------------------------
# Shared agent directory list
# ---------------------------------------------------------------------------

# Format: "parent_dir|skills_dir|agent_name"
# Based on https://github.com/vercel-labs/skills
AGENT_DIRS=(
    "${HOME}/.agents|${HOME}/.agents/skills|universal"
    "${HOME}/.claude|${HOME}/.claude/skills|Claude Code"
    "${HOME}/.config/agents|${HOME}/.config/agents/skills|Amp"
    "${HOME}/.cursor|${HOME}/.cursor/skills|Cursor"
    "${HOME}/.copilot|${HOME}/.copilot/skills|GitHub Copilot"
    "${HOME}/.codex|${HOME}/.codex/skills|Codex"
    "${HOME}/.gemini|${HOME}/.gemini/skills|Gemini CLI"
    "${HOME}/.config/opencode|${HOME}/.config/opencode/skills|OpenCode"
    "${HOME}/.config/goose|${HOME}/.config/goose/skills|Goose"
    "${HOME}/.windsurf|${HOME}/.windsurf/skills|Windsurf"
    "${HOME}/.codeium/windsurf|${HOME}/.codeium/windsurf/skills|Windsurf"
    "${HOME}/.continue|${HOME}/.continue/skills|Continue"
    "${HOME}/.roo|${HOME}/.roo/skills|Roo"
    "${HOME}/.kiro|${HOME}/.kiro/skills|Kiro"
    "${HOME}/.augment|${HOME}/.augment/skills|Augment"
    "${HOME}/.trae|${HOME}/.trae/skills|Trae"
)

# ---------------------------------------------------------------------------
# Platform detection
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
# Install helpers
# ---------------------------------------------------------------------------

# Move or copy a binary to INSTALL_DIR, creating it if needed.
install_binary() {
    local src="$1"
    local cmd="$2"  # "mv" or "cp"

    mkdir -p "${INSTALL_DIR}"
    if "${cmd}" "${src}" "${INSTALL_DIR}/${BINARY_NAME}"; then
        ok "Installed to ${INSTALL_DIR}/${BINARY_NAME}"
    else
        fail "Failed to install to ${INSTALL_DIR}"
        exit 1
    fi
}

ensure_tmp_dir() {
    if [[ -z "${TMP_DIR}" ]]; then
        TMP_DIR=$(mktemp -d)
        chmod 700 "${TMP_DIR}"
    fi
}

remove_file() {
    local path="$1"
    local label="$2"
    if [[ ! -f "${path}" && ! -d "${path}" ]]; then
        return 0
    fi
    if rm -rf "${path}"; then
        ok "Removed ${label}"
    else
        fail "Failed to remove ${label}: ${path}"
    fi
}

# ---------------------------------------------------------------------------
# Install modes
# ---------------------------------------------------------------------------

install_remote() {
    check_dependencies
    detect_platform
    detect_arch

    local binary_name="presto-${PLATFORM}-${ARCH}"
    local download_url="${R2_BASE_URL}/${binary_name}"

    ensure_tmp_dir
    local tmp_file="${TMP_DIR}/${BINARY_NAME}"

    info "Downloading from ${download_url}"

    if ! curl -fsSL "${download_url}" -o "${tmp_file}"; then
        fail "Download failed"
        exit 1
    fi

    chmod 755 "${tmp_file}"

    if ! file "${tmp_file}" | grep -q "executable"; then
        fail "Downloaded file is not a valid executable"
        exit 1
    fi

    if ! "${tmp_file}" --version >/dev/null 2>&1; then
        fail "Binary failed sanity check (--version)"
        exit 1
    fi

    install_binary "${tmp_file}" "mv"
}

install_from_source() {
    if [[ -z "${SCRIPT_DIR}" ]]; then
        fail "--from-source requires running the script from a local checkout"
        echo "  Usage: ./install.sh --from-source"
        exit 1
    fi

    if ! command -v cargo >/dev/null 2>&1; then
        fail "cargo is required for --from-source install"
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

    install_binary "${built_binary}" "cp"
}

clean_legacy_install() {
    local legacy="${LEGACY_INSTALL_DIR}/${BINARY_NAME}"
    if [[ -f "${legacy}" ]]; then
        if rm -f "${legacy}" 2>/dev/null; then
            ok "Removed old binary at ${legacy}"
        else
            echo ""
            echo -e "  ${RED}⚠${RESET}  Old binary exists at ${legacy}"
            echo -e "  ${DIM}Remove it manually: sudo rm ${legacy}${RESET}"
        fi
    fi
}

# Append a PATH export line to a shell rc file if not already present.
add_to_shell_rc() {
    local rc_file="$1"
    local line='export PATH="$HOME/.local/bin:$PATH"'

    if [[ -f "${rc_file}" ]] && grep -qF '.local/bin' "${rc_file}" 2>/dev/null; then
        return 0
    fi

    [[ -f "${rc_file}" ]] || return 0

    echo "" >> "${rc_file}"
    echo "# Added by  tempo-walletinstaller" >> "${rc_file}"
    echo "${line}" >> "${rc_file}"
    ok "Added ${INSTALL_DIR} to PATH in ${rc_file/#${HOME}/~}"
}

ensure_in_path() {
    ok "$("${INSTALL_DIR}/${BINARY_NAME}" --version)"

    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) return 0 ;;
    esac

    # INSTALL_DIR is not in PATH — try to add it to shell rc files
    local added=false
    local shell_name
    shell_name="$(basename "${SHELL:-}")"

    case "${shell_name}" in
        zsh)
            add_to_shell_rc "${HOME}/.zshrc" && added=true
            ;;
        bash)
            if [[ -f "${HOME}/.bash_profile" ]]; then
                add_to_shell_rc "${HOME}/.bash_profile" && added=true
            elif [[ -f "${HOME}/.bashrc" ]]; then
                add_to_shell_rc "${HOME}/.bashrc" && added=true
            fi
            ;;
        fish)
            # fish uses a different syntax; just print the hint
            ;;
    esac

    echo ""
    if [[ "${added}" == true ]]; then
        echo -e "  ${DIM}Restart your shell or run:${RESET}"
    else
        echo -e "  ${DIM}Add ${INSTALL_DIR} to your PATH:${RESET}"
    fi
    echo -e "    ${DIM}export PATH=\"${INSTALL_DIR}:\$PATH\"${RESET}"
}

# ---------------------------------------------------------------------------
# AI skill management
# ---------------------------------------------------------------------------

install_ai_skill() {
    local skill_variant="${1:-passkey}"
    local skill_content=""

    # Resolve skill content: prefer local file, fall back to R2 download
    local local_skill="${SCRIPT_DIR}/.agents/skills/presto-${skill_variant}/SKILL.md"
    if [[ -n "${SCRIPT_DIR}" && -f "${local_skill}" ]]; then
        skill_content="${local_skill}"
    else
        ensure_tmp_dir
        local tmp_skill="${TMP_DIR}/SKILL.md"
        local skill_url="${R2_BASE_URL}/SKILL-${skill_variant}.md"
        if curl -fsSL "${skill_url}" -o "${tmp_skill}" 2>/dev/null; then
            skill_content="${tmp_skill}"
        else
            return 0
        fi
    fi

    # Only install if the agent's parent config dir already exists
    local installed_names=()
    for entry in "${AGENT_DIRS[@]}"; do
        IFS='|' read -r parent skill_base agent_name <<< "${entry}"
        if [[ -d "${parent}" ]]; then
            local skill_dir="${skill_base}/presto"
            mkdir -p "${skill_dir}" 2>/dev/null || continue
            cp "${skill_content}" "${skill_dir}/SKILL.md" 2>/dev/null || continue
            installed_names+=("${agent_name}")
        fi
    done

    if [[ ${#installed_names[@]} -gt 0 ]]; then
        local IFS=', '
        ok "Installed AI skill to ${#installed_names[@]} agent(s): ${installed_names[*]}"
    fi
}

uninstall_ai_skills() {
    for entry in "${AGENT_DIRS[@]}"; do
        IFS='|' read -r _ skill_base _ <<< "${entry}"
        for name in  tempo-walletpresto-local presto-passkey; do
            remove_file "${skill_base}/${name}" "AI skill (${skill_base}/${name})"
        done
    done
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------

uninstall_presto() {
    echo -e "\n${BOLD}Uninstalling presto${RESET}\n"

    remove_file "${INSTALL_DIR}/${BINARY_NAME}" "binary"
    remove_file "${LEGACY_INSTALL_DIR}/${BINARY_NAME}" "legacy binary"

    if [[ "$(uname -s)" == "Darwin" ]]; then
        remove_file "${HOME}/Library/Application Support/presto" "data"
    else
        remove_file "${XDG_CONFIG_HOME:-${HOME}/.config}/presto" "config"
        remove_file "${XDG_DATA_HOME:-${HOME}/.local/share}/presto" "data"
    fi

    uninstall_ai_skills

    echo ""
    ok "Done"
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
    local wallet_type="${PRESTO_WALLET_TYPE:-passkey}"
    local mode=""

    for arg in "$@"; do
        case "${arg}" in
            --wallet=*)    wallet_type="${arg#--wallet=}" ;;
            --uninstall)   mode="uninstall" ;;
            --from-source) mode="from-source" ;;
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

    banner

    if [[ "${mode}" == "from-source" ]]; then
        install_from_source
    else
        install_remote
    fi

    clean_legacy_install
    ensure_in_path
    install_ai_skill "${wallet_type}"

    echo ""
    echo -e "  ${BOLD}Get started:${RESET}"
    if [[ "${wallet_type}" == "local" ]]; then
        echo -e "    ${DIM}\$${RESET}  tempo-walletwallet create"
    else
        echo -e "    ${DIM}\$${RESET}  tempo-walletlogin"
    fi
    echo -e "    ${DIM}\$${RESET}  tempo-wallet--help"
    echo ""
}

main "$@"
