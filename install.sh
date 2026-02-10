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
R2_BASE_URL="https://tempoctl-binaries.tempo.xyz"

TEMPO_DIR="${TEMPO_DIR:-"$HOME/.tempo"}"
TEMPO_BIN_DIR="$TEMPO_DIR/bin"

MARKER_BEGIN="# >>> tempoctl installer >>>"
MARKER_END="# <<< tempoctl installer <<<"

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

detect_shell() {
    case $SHELL in
    */zsh)
        if [[ "$PLATFORM" == "darwin" ]]; then
            PROFILE="${ZDOTDIR-"$HOME"}/.zprofile"
        else
            PROFILE="${ZDOTDIR-"$HOME"}/.zshrc"
        fi
        PREF_SHELL=zsh
        ;;
    */bash)
        if [[ "$PLATFORM" == "darwin" ]]; then
            PROFILE="$HOME/.bash_profile"
        else
            PROFILE="$HOME/.bashrc"
        fi
        PREF_SHELL=bash
        ;;
    */fish)
        PROFILE="$HOME/.config/fish/config.fish"
        PREF_SHELL=fish
        ;;
    */ash)
        PROFILE="$HOME/.profile"
        PREF_SHELL=ash
        ;;
    *)
        echo "warn: could not detect shell, manually add ${TEMPO_BIN_DIR} to your PATH"
        PROFILE=""
        PREF_SHELL=""
        ;;
    esac
}

ensure_path() {
    if [[ -z "$PROFILE" ]]; then
        return
    fi

    if grep -qsF "$MARKER_BEGIN" "$PROFILE" 2>/dev/null; then
        return
    fi

    if [[ "$PREF_SHELL" == "fish" ]]; then
        mkdir -p "$(dirname "$PROFILE")"
        {
            echo ""
            echo "$MARKER_BEGIN"
            echo "fish_add_path -gp \"$TEMPO_BIN_DIR\""
            echo "$MARKER_END"
        } >> "$PROFILE"
    else
        {
            echo ""
            echo "$MARKER_BEGIN"
            echo "export PATH=\"$TEMPO_BIN_DIR:\$PATH\""
            echo "$MARKER_END"
        } >> "$PROFILE"
    fi
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

    mkdir -p "${TEMPO_BIN_DIR}"
    mv "${tmp_file}" "${TEMPO_BIN_DIR}/${BINARY_NAME}"

    ln -sf "${TEMPO_BIN_DIR}/${BINARY_NAME}" "${TEMPO_BIN_DIR}/tempo"

    echo "info: installed tempoctl to ${TEMPO_BIN_DIR}/"
}

verify_installation() {
    export PATH="${TEMPO_BIN_DIR}:$PATH"

    if command -v tempoctl >/dev/null 2>&1; then
        echo "info: $(tempoctl --version)"
    fi
    if command -v tempo >/dev/null 2>&1; then
        echo "info: 'tempo' alias available"
    fi
}

main() {
    check_dependencies
    detect_platform
    detect_arch
    detect_shell
    install_tempoctl
    ensure_path
    verify_installation

    echo ""
    if [[ -n "$PROFILE" ]]; then
        echo "info: detected ${PREF_SHELL} — added ${TEMPO_BIN_DIR} to PATH in ${PROFILE}"
    fi

    echo ""
    echo "To start using tempoctl now, run:"
    echo ""
    echo "  export PATH=\"${TEMPO_BIN_DIR}:\$PATH\""
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
