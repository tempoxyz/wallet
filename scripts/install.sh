#!/usr/bin/env bash
set -euo pipefail

# Tempo CLI installer
#
# Usage:
#   curl -fsSL https://cli.tempo.xyz/install.sh | bash
#   ./install              Build from source, install extensions from remote manifests
#   ./install --uninstall  Remove all installed binaries
#
# To add a new extension, add an entry to EXTENSIONS below.

BIN_DIR="${HOME}/.local/bin"
R2_BASE_URL="https://cli.tempo.xyz"

# Detect if running from a local checkout (./install) vs piped (curl | bash)
SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

TMP_DIR=""
cleanup() { [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]] && rm -rf "${TMP_DIR}"; }
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Extensions
# ---------------------------------------------------------------------------
# Each entry: "name|binary_name"

EXTENSIONS=(
    "wallet|tempo-wallet"
    "mpp|tempo-mpp"
)

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
fail()  { echo -e "  ${RED}✗${RESET} $*"; exit 1; }

# ---------------------------------------------------------------------------
# Agent skill directories (for reporting)
# ---------------------------------------------------------------------------

# Keep in sync with cli/src/installer.rs AGENT_SKILL_DIRS.
AGENT_SKILL_DIRS=(
    "${HOME}/.agents/skills"
    "${HOME}/.claude/skills"
    "${HOME}/.config/agents/skills"
    "${HOME}/.cursor/skills"
    "${HOME}/.copilot/skills"
    "${HOME}/.codex/skills"
    "${HOME}/.gemini/skills"
    "${HOME}/.config/opencode/skills"
    "${HOME}/.config/goose/skills"
    "${HOME}/.windsurf/skills"
    "${HOME}/.codeium/windsurf/skills"
    "${HOME}/.continue/skills"
    "${HOME}/.roo/skills"
    "${HOME}/.kiro/skills"
    "${HOME}/.augment/skills"
    "${HOME}/.trae/skills"
)

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

detect_platform() {
    local platform
    platform="$(uname -s | tr '[:upper:]' '[:lower:]')"
    case "${platform}" in
        linux*)   PLATFORM="linux" ;;
        darwin*)  PLATFORM="darwin" ;;
        *)        fail "Unsupported platform '${platform}'" ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    case "${arch}" in
        x86_64|amd64)   ARCH="amd64" ;;
        aarch64|arm64)  ARCH="arm64" ;;
        *)              fail "Unsupported architecture '${arch}'" ;;
    esac
}

# ---------------------------------------------------------------------------
# Banner
# ---------------------------------------------------------------------------

banner() {
    echo ""
    echo -e "${BOLD}   __"
    echo -e "  / /____  ____ ___  ____  ____"
    echo -e " / __/ _ \\/ __ \`__ \\/ __ \\/ __ \\"
    echo -e "/ /_/  __/ / / / / / /_/ / /_/ /"
    echo -e "\\__/\\___/_/ /_/ /_/ .___/\\____/"
    echo -e "                 /_/${RESET}"
    echo ""
}

# ---------------------------------------------------------------------------
# Extension helpers
# ---------------------------------------------------------------------------

parse_extension() {
    local entry="$1"
    EXT_NAME="${entry%%|*}"
    EXT_BINARY="${entry#*|}"
}

install_extension_remote() {
    local name="$1" binary="$2"
    "${BIN_DIR}/tempo" add "${name}" > /dev/null
    ok "Installed ${binary} to ${BIN_DIR}"
}

report_installed_skills() {
    local name="$1"
    local skill_dir="tempo-${name}"
    local count=0
    for dir in "${AGENT_SKILL_DIRS[@]}"; do
        if [[ -f "${dir}/${skill_dir}/SKILL.md" ]]; then
            count=$((count + 1))
        fi
    done
    if [[ $count -gt 0 ]]; then
        ok "Installed tempo-${name} skill to ${count} agent(s)"
    fi
}

# ---------------------------------------------------------------------------
# Install tempo CLI
# ---------------------------------------------------------------------------

install_tempo_remote() {
    command -v curl >/dev/null 2>&1 || fail "curl is required but not installed"
    detect_platform
    detect_arch

    local binary_name="tempo-${PLATFORM}-${ARCH}"
    local download_url="${R2_BASE_URL}/tempo/${binary_name}"

    TMP_DIR=$(mktemp -d)
    chmod 700 "${TMP_DIR}"
    local tmp_file="${TMP_DIR}/tempo"

    info "Downloading from ${download_url}"
    curl -fsSL "${download_url}" -o "${tmp_file}" || fail "Download failed"
    chmod 755 "${tmp_file}"

    if ! "${tmp_file}" --version >/dev/null 2>&1; then
        fail "Binary failed sanity check (--version)"
    fi

    mkdir -p "${BIN_DIR}"
    mv "${tmp_file}" "${BIN_DIR}/tempo"
    ok "Installed tempo to ${BIN_DIR}"
}

install_tempo_from_source() {
    command -v cargo >/dev/null 2>&1 || fail "cargo is required (https://rustup.rs/)"
    cd "${SCRIPT_DIR}"
    cargo build --release --bin tempo 2>&1 | grep -v "Finished\|Compiling\|Downloading\|Downloaded" || true
    mkdir -p "${BIN_DIR}"
    cp "${SCRIPT_DIR}/target/release/tempo" "${BIN_DIR}/tempo"
    chmod +x "${BIN_DIR}/tempo"
    ok "Installed tempo to ${BIN_DIR}"
}

# ---------------------------------------------------------------------------
# PATH helpers
# ---------------------------------------------------------------------------

add_to_shell_rc() {
    local rc_file="$1"
    local line='export PATH="$HOME/.local/bin:$PATH"'

    if [[ -f "${rc_file}" ]] && grep -qF '.local/bin' "${rc_file}" 2>/dev/null; then
        return 0
    fi
    [[ -f "${rc_file}" ]] || return 0

    echo "" >> "${rc_file}"
    echo "# Added by Tempo installer" >> "${rc_file}"
    echo "${line}" >> "${rc_file}"
    ok "Added ${BIN_DIR} to PATH in ${rc_file/#${HOME}/~}"
}

ensure_in_path() {
    case ":${PATH}:" in
        *":${BIN_DIR}:"*) return 0 ;;
    esac

    local shell_name
    shell_name="$(basename "${SHELL:-}")"
    case "${shell_name}" in
        zsh)  add_to_shell_rc "${HOME}/.zshrc" ;;
        bash)
            if [[ -f "${HOME}/.bash_profile" ]]; then
                add_to_shell_rc "${HOME}/.bash_profile"
            elif [[ -f "${HOME}/.bashrc" ]]; then
                add_to_shell_rc "${HOME}/.bashrc"
            fi
            ;;
    esac

    echo ""
    echo -e "  ${DIM}Restart your shell or run:${RESET}"
    echo -e "    ${DIM}export PATH=\"${BIN_DIR}:\$PATH\"${RESET}"
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------

do_uninstall() {
    echo -e "\n${BOLD}Uninstalling${RESET}\n"

    local bins=("tempo" "tempo-core")
    for entry in "${EXTENSIONS[@]}"; do
        parse_extension "${entry}"
        bins+=("${EXT_BINARY}")
    done

    for bin in "${bins[@]}"; do
        if [[ -f "${BIN_DIR}/${bin}" ]]; then
            rm "${BIN_DIR}/${bin}"
            ok "Removed ${bin}"
        fi
    done

    echo ""
    ok "Done"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    local mode=""

    for arg in "$@"; do
        case "${arg}" in
            --uninstall)    mode="uninstall" ;;
            --from-source)  mode="from-source" ;;
            --help|-h)
                echo "Usage: ./install [--from-source] [--uninstall]"
                echo ""
                echo "  (default)       Download pre-built binaries"
                echo "  --from-source   Build tempo from source (requires cargo)"
                echo "  --uninstall     Remove all installed binaries"
                exit 0
                ;;
            *)
                fail "Unknown flag: ${arg} (see --help)"
                ;;
        esac
    done

    banner

    if [[ "${mode}" == "uninstall" ]]; then
        do_uninstall
        exit 0
    fi

    # Build from source if running from checkout, otherwise download
    if [[ "${mode}" == "from-source" || (-n "${SCRIPT_DIR}" && -f "${SCRIPT_DIR}/Cargo.toml") ]]; then
        install_tempo_from_source
    else
        install_tempo_remote
    fi
    ok "$("${BIN_DIR}/tempo" --version 2>&1)"

    for entry in "${EXTENSIONS[@]}"; do
        parse_extension "${entry}"
        echo ""
        install_extension_remote "${EXT_NAME}" "${EXT_BINARY}"
        report_installed_skills "${EXT_NAME}"
        ok "$("${BIN_DIR}/${EXT_BINARY}" --version 2>&1)"
    done

    ensure_in_path

    echo ""
    echo -e "  ${BOLD}Get started:${RESET}"
    echo -e "    ${DIM}\$${RESET} tempo wallet login"
    echo -e "    ${DIM}\$${RESET} tempo wallet --help"
    echo ""
}

main "$@"
