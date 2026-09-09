#!/usr/bin/env bash
# ==============================================================================
# BLACK SPARROW // OFFICIAL STANDALONE INSTALLER PIPELINE
# High-Performance Website Crawler & AI-Native Technical SEO Engine in Rust
# Repository: https://github.com/Shantodotdev/blacksparrow
# ==============================================================================

set -euo pipefail

# ------------------------------------------------------------------------------
# 1. Terminal Styling & Exact 256-Color Pink/Maroon Palette (matches terminal.rs)
# ------------------------------------------------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_PINK="\033[38;5;198m"
  C_MAROON="\033[38;5;161m"
  C_CYAN="$C_PINK"
  C_GREEN="\033[38;5;48m"
  C_YELLOW="\033[38;5;220m"
  C_RED="\033[38;5;196m"
  C_WHITE="\033[38;5;231m"
  C_DIM="\033[38;5;244m"
  C_BOLD="\033[1m"
  C_RESET="\033[0m"
else
  C_PINK=""
  C_MAROON=""
  C_CYAN=""
  C_GREEN=""
  C_YELLOW=""
  C_RED=""
  C_WHITE=""
  C_DIM=""
  C_BOLD=""
  C_RESET=""
fi

REPO="Shantodotdev/blacksparrow"
DEFAULT_TAG="v0.1.0-rc.2"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${VERSION:-$DEFAULT_TAG}"
TMP_DIR=""

cleanup() {
  if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT INT TERM

print_banner() {
  printf "\n%b%b" "$C_BOLD" "$C_PINK"
  cat << 'EOF'
  ███████╗██████╗  █████╗ ██████╗ ██████╗  ██████╗ ██╗    ██╗
  ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔══██╗██╔═══██╗██║    ██║
  ███████╗██████╔╝███████║██████╔╝██████╔╝██║   ██║██║ █╗ ██║
  ╚════██║██╔═══╝ ██╔══██║██╔══██╗██╔══██╗██║   ██║██║███╗██║
  ███████║██║     ██║  ██║██║  ██║██║  ██║╚██████╔╝╚███╔███╔╝
  ╚══════╝╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝  ╚══╝╚══╝ 
EOF
  printf "%b  %b%bB L A C K   S P A R R O W%b\n\n" "$C_RESET" "$C_BOLD" "$C_MAROON" "$C_RESET"
}

# Matches exactly `format_section_header` in src/report/terminal.rs
print_header() {
  local title="$1"
  local pad=$((${#title} + 4))
  local border=$(printf '─%.0s' $(seq 1 "$pad"))

  printf "  %b%b┌%s┐%b\n" "$C_BOLD" "$C_MAROON" "$border" "$C_RESET"
  printf "  %b%b│%b  %b%b%s%b  %b%b│%b\n" "$C_BOLD" "$C_MAROON" "$C_RESET" "$C_BOLD" "$C_WHITE" "$title" "$C_RESET" "$C_BOLD" "$C_MAROON" "$C_RESET"
  printf "  %b%b└%s┘%b\n" "$C_BOLD" "$C_MAROON" "$border" "$C_RESET"
}

print_step() {
  local symbol="$1"
  local text="$2"
  printf "  %b[%s]%b %s\n" "$C_CYAN" "$symbol" "$C_RESET" "$text"
}

print_success_step() {
  local text="$1"
  printf "  %b[✓]%b %b%s%b\n" "$C_GREEN" "$C_RESET" "$C_BOLD" "$text" "$C_RESET"
}

print_warn_step() {
  local text="$1"
  printf "  %b[!]%b %b%s%b\n" "$C_YELLOW" "$C_RESET" "$C_YELLOW" "$text" "$C_RESET"
}

print_error() {
  local msg="$1"
  printf "\n  %b[✗] ERROR:%b %b%s%b\n\n" "$C_RED" "$C_RESET" "$C_RED" "$msg" "$C_RESET" >&2
  exit 1
}

# ------------------------------------------------------------------------------
# 2. Architecture & Platform Detection
# ------------------------------------------------------------------------------
detect_target() {
  local os
  local arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
        *) print_error "Unsupported Linux architecture: $arch" ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
        x86_64) TARGET="x86_64-apple-darwin" ;;
        *) print_error "Unsupported macOS architecture: $arch" ;;
      esac
      ;;
    *)
      print_error "Unsupported operating system: $os (Use PowerShell installer for Windows)"
      ;;
  esac
}

# ------------------------------------------------------------------------------
# 3. Main Installation Pipeline
# ------------------------------------------------------------------------------
main() {
  # Parse CLI arguments if any
  while [ $# -gt 0 ]; do
    case "$1" in
      --system)
        INSTALL_DIR="/usr/local/bin"
        shift
        ;;
      --dir)
        INSTALL_DIR="$2"
        shift 2
        ;;
      --version)
        VERSION="$2"
        shift 2
        ;;
      *)
        shift
        ;;
    esac
  done

  detect_target

  print_banner
  print_header "BLACK SPARROW // NATIVE INSTALLER PIPELINE"

  printf "  %b├─%b Platform       : %b%s (%s)%b\n" "$C_CYAN" "$C_RESET" "$C_BOLD" "$(uname -s)" "$(uname -m)" "$C_RESET"
  printf "  %b├─%b Target Triple  : %b%s%b\n" "$C_CYAN" "$C_RESET" "$C_DIM" "$TARGET" "$C_RESET"
  printf "  %b├─%b Target Version : %b%s%b\n" "$C_CYAN" "$C_RESET" "$C_GREEN" "$VERSION" "$C_RESET"
  printf "  %b└─%b Destination    : %b%s/blacksparrow%b\n\n" "$C_CYAN" "$C_RESET" "$C_CYAN" "$INSTALL_DIR" "$C_RESET"

  # Dependencies check
  if ! command -v curl >/dev/null 2>&1; then
    print_error "'curl' is required but not installed."
  fi
  if ! command -v tar >/dev/null 2>&1; then
    print_error "'tar' is required but not installed."
  fi

  local tarball="blacksparrow-${TARGET}.tar.xz"
  local url="https://github.com/${REPO}/releases/download/${VERSION}/${tarball}"
  local checksum_url="${url}.sha256"

  TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'blacksparrow')"

  print_header "DOWNLOAD & ARTIFACT EXTRACTION"
  print_step "•" "Connecting to GitHub Releases..."

  # Download tarball (attempt blacksparrow, fallback to seo-lens for legacy tags)
  if ! curl -fsSL "$url" -o "${TMP_DIR}/${tarball}" 2>/dev/null; then
    local legacy_tarball="seo-lens-${TARGET}.tar.xz"
    local legacy_url="https://github.com/${REPO}/releases/download/${VERSION}/${legacy_tarball}"
    if curl -fsSL "$legacy_url" -o "${TMP_DIR}/${tarball}"; then
      checksum_url="${legacy_url}.sha256"
    else
      print_error "Failed to download $url or $legacy_url. Check release tag or network."
    fi
  fi
  print_success_step "Downloaded release payload"

  # Verify SHA-256 if available
  if curl -fsSL "$checksum_url" -o "${TMP_DIR}/${tarball}.sha256" 2>/dev/null; then
    local expected_hash
    expected_hash="$(awk '{print $1}' "${TMP_DIR}/${tarball}.sha256")"
    local actual_hash=""

    if command -v sha256sum >/dev/null 2>&1; then
      actual_hash="$(sha256sum "${TMP_DIR}/${tarball}" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
      actual_hash="$(shasum -a 256 "${TMP_DIR}/${tarball}" | awk '{print $1}')"
    fi

    if [ -n "$actual_hash" ]; then
      if [ "$expected_hash" = "$actual_hash" ]; then
        print_success_step "Cryptographic SHA-256 hash verified"
      else
        print_error "Checksum verification failed! Expected: $expected_hash, Got: $actual_hash"
      fi
    fi
  fi

  print_step "•" "Extracting executable payload..."
  tar -xf "${TMP_DIR}/${tarball}" -C "$TMP_DIR"

  local bin_source="${TMP_DIR}/blacksparrow"
  if [ ! -f "$bin_source" ]; then
    bin_source="$(find "$TMP_DIR" -type f \( -name blacksparrow -o -name seolens \) | head -n 1)"
  fi

  if [ -z "$bin_source" ] || [ ! -f "$bin_source" ]; then
    print_error "Could not find 'blacksparrow' executable in unpacked archive."
  fi

  # Require sudo if writing to system directory without permissions
  local use_sudo=""
  if [ ! -w "$INSTALL_DIR" ] && [ "$INSTALL_DIR" = "/usr/local/bin" ] && [ "$(id -u)" -ne 0 ]; then
    use_sudo="sudo"
  fi

  # Create install directory (e.g. ~/.local/bin)
  if [ -n "$use_sudo" ]; then
    $use_sudo mkdir -p "$INSTALL_DIR"
  else
    mkdir -p "$INSTALL_DIR"
  fi

  # Install primary binary: blacksparrow
  local dest_bin="${INSTALL_DIR}/blacksparrow"
  if [ -n "$use_sudo" ]; then
    $use_sudo cp -f "$bin_source" "$dest_bin"
    $use_sudo chmod 755 "$dest_bin"
  else
    cp -f "$bin_source" "$dest_bin"
    chmod 755 "$dest_bin"
  fi

  print_success_step "Installed primary binary: ${dest_bin}"

  # Create terminal alias symlink: sparrow -> blacksparrow (Option 1)
  local dest_alias="${INSTALL_DIR}/sparrow"
  if [ -n "$use_sudo" ]; then
    $use_sudo ln -sf "$dest_bin" "$dest_alias"
  else
    ln -sf "$dest_bin" "$dest_alias"
  fi
  print_success_step "Created terminal alias: ${dest_alias} -> ${dest_bin}"

  # Verification test
  local version_output
  if version_output="$("$dest_bin" --version 2>&1)"; then
    print_success_step "Binary self-test passed: ${version_output}"
  else
    print_error "Failed to execute installed binary ${dest_bin}."
  fi

  # ----------------------------------------------------------------------------
  # 4. PATH Verification & Shell Profile Auto-Configuration
  # ----------------------------------------------------------------------------
  printf "\n"
  print_header "SHELL ENVIRONMENT & PATH CHECK"

  local in_path=0
  local dir_expanded
  dir_expanded="$(eval echo "$INSTALL_DIR")"
  IFS=:
  for p in $PATH; do
    if [ "$p" = "$dir_expanded" ] || [ "$p" = "$INSTALL_DIR" ]; then
      in_path=1
      break
    fi
  done
  unset IFS

  if [ "$in_path" -eq 1 ]; then
    print_success_step "'${INSTALL_DIR}' is already in your active PATH."
    printf "\n"
    print_header "INSTALLATION COMPLETE // READY TO RUN"
    printf "  %bRun Black Sparrow from anywhere:%b\n\n" "$C_GREEN$C_BOLD" "$C_RESET"
    printf "    %bblacksparrow --help%b  %b(or: sparrow --help)%b\n" "$C_CYAN$C_BOLD" "$C_RESET" "$C_DIM" "$C_RESET"
    printf "    %bsparrow inspect https://example.com%b\n" "$C_CYAN" "$C_RESET"
    printf "    %bblacksparrow audit https://example.com --format html%b\n" "$C_CYAN" "$C_RESET"
    printf "    %bblacksparrow mcp%b\n\n" "$C_CYAN" "$C_RESET"
  else
    print_warn_step "'${INSTALL_DIR}' is not yet in your current \$PATH."

    # Detect user shell config file
    local rc_file=""
    local user_shell
    user_shell="$(basename "${SHELL:-/bin/bash}")"

    case "$user_shell" in
      zsh)
        rc_file="$HOME/.zshrc"
        ;;
      bash)
        if [ -f "$HOME/.bashrc" ]; then
          rc_file="$HOME/.bashrc"
        elif [ -f "$HOME/.bash_profile" ]; then
          rc_file="$HOME/.bash_profile"
        else
          rc_file="$HOME/.bashrc"
        fi
        ;;
      fish)
        rc_file="$HOME/.config/fish/config.fish"
        ;;
      *)
        if [ -f "$HOME/.profile" ]; then
          rc_file="$HOME/.profile"
        fi
        ;;
    esac

    local export_line="export PATH=\"${INSTALL_DIR}:\$PATH\""
    if [ "$user_shell" = "fish" ]; then
      export_line="fish_add_path ${INSTALL_DIR}"
    fi

    local added=0
    if [ -n "$rc_file" ]; then
      if [ ! -f "$rc_file" ] || ! grep -qF "$INSTALL_DIR" "$rc_file"; then
        printf "\n# Added by Black Sparrow installer\n%s\n" "$export_line" >> "$rc_file"
        print_success_step "Automatically configured ${rc_file}"
        added=1
      else
        print_step "•" "${rc_file} already contains ${INSTALL_DIR}"
      fi
    fi

    printf "\n"
    print_header "ACTION REQUIRED // ACTIVATE YOUR SHELL"
    printf "  %bTo run 'blacksparrow' (or 'sparrow') in your current terminal session, run:%b\n\n" "$C_BOLD" "$C_RESET"
    if [ "$added" -eq 1 ] && [ -n "$rc_file" ]; then
      printf "    %bsource %s%b\n\n" "$C_CYAN$C_BOLD" "$rc_file" "$C_RESET"
    else
      printf "    %b%s%b\n\n" "$C_CYAN$C_BOLD" "$export_line" "$C_RESET"
    fi
    printf "  %bOr execute directly:%b\n\n" "$C_DIM" "$C_RESET"
    printf "    %b%s/seolens --version%b\n\n" "$C_DIM" "$INSTALL_DIR" "$C_RESET"
  fi
}

main "$@"
