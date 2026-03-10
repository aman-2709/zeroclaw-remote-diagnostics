#!/usr/bin/env bash
# Install prerequisites for ZeroClaw Remote Diagnostics.
# Usage: ./dev/setup.sh
#
# Supports: Ubuntu/Debian, Fedora/RHEL, macOS (Homebrew)
# Installs: Rust (nightly for edition 2024), just, mosquitto, pnpm, Ollama

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; }

# --- Detect OS ---
detect_os() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "macos"
    elif [[ -f /etc/os-release ]]; then
        . /etc/os-release
        case "$ID" in
            ubuntu|debian|pop|linuxmint) echo "debian" ;;
            fedora|rhel|centos|rocky|alma) echo "fedora" ;;
            arch|manjaro) echo "arch" ;;
            *) echo "unknown" ;;
        esac
    else
        echo "unknown"
    fi
}

OS=$(detect_os)
info "Detected OS: $OS"

# --- Check if command exists ---
has() { command -v "$1" &>/dev/null; }

# --- Install system packages ---
install_pkg() {
    case "$OS" in
        debian) sudo apt-get update -qq && sudo apt-get install -y -qq "$@" ;;
        fedora) sudo dnf install -y "$@" ;;
        arch)   sudo pacman -S --noconfirm "$@" ;;
        macos)
            if ! has brew; then
                error "Homebrew not found. Install it from https://brew.sh"
                exit 1
            fi
            brew install "$@"
            ;;
        *)
            error "Unsupported OS. Please install manually: $*"
            exit 1
            ;;
    esac
}

# --- 1. Rust ---
echo ""
info "=== 1/6 Rust toolchain ==="
if has rustup; then
    info "rustup already installed"
    rustup update stable
else
    info "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi
# Edition 2024 requires nightly or a recent stable (1.85+)
RUST_VERSION=$(rustc --version | grep -oP '\d+\.\d+')
info "Rust version: $(rustc --version)"
if [[ "$(echo "$RUST_VERSION >= 1.85" | bc -l 2>/dev/null || echo 0)" == "1" ]]; then
    info "Rust version supports edition 2024"
else
    warn "Edition 2024 requires Rust >= 1.85. Updating..."
    rustup update stable
fi

# --- 2. just (task runner) ---
echo ""
info "=== 2/6 just (task runner) ==="
if has just; then
    info "just already installed: $(just --version)"
else
    info "Installing just..."
    if [[ "$OS" == "macos" ]]; then
        brew install just
    else
        cargo install just
    fi
fi

# --- 3. Mosquitto (MQTT broker) ---
echo ""
info "=== 3/6 Mosquitto (MQTT broker) ==="
if has mosquitto; then
    info "mosquitto already installed"
else
    info "Installing mosquitto..."
    case "$OS" in
        debian) install_pkg mosquitto mosquitto-clients ;;
        fedora) install_pkg mosquitto ;;
        arch)   install_pkg mosquitto ;;
        macos)  install_pkg mosquitto ;;
        *)      warn "Please install mosquitto manually" ;;
    esac
fi

# --- 4. Node.js + pnpm ---
echo ""
info "=== 4/6 Node.js + pnpm ==="
if has node; then
    info "Node.js already installed: $(node --version)"
else
    warn "Node.js not found. Install via your preferred method:"
    warn "  https://nodejs.org  or  nvm (https://github.com/nvm-sh/nvm)"
fi

if has pnpm; then
    info "pnpm already installed: $(pnpm --version)"
else
    if has node; then
        info "Installing pnpm via corepack..."
        sudo corepack enable && corepack prepare pnpm@latest --activate 2>/dev/null \
            || sudo npm install -g pnpm
    else
        warn "Skipping pnpm (Node.js not found)"
    fi
fi

# --- 5. Ollama (local LLM inference) ---
echo ""
info "=== 5/6 Ollama (local LLM, optional) ==="
if has ollama; then
    info "ollama already installed: $(ollama --version 2>/dev/null || echo 'unknown version')"
else
    info "Installing ollama..."
    curl -fsSL https://ollama.com/install.sh | sh
fi

# Pull the default model used by the fleet agent
if has ollama; then
    if ollama list 2>/dev/null | grep -q "phi3:mini"; then
        info "phi3:mini model already pulled"
    else
        info "Pulling phi3:mini model (this may take a few minutes)..."
        ollama pull phi3:mini || warn "Failed to pull phi3:mini. You can pull it later with: ollama pull phi3:mini"
    fi
fi

# --- 6. Frontend dependencies ---
echo ""
info "=== 6/6 Frontend dependencies ==="
FRONTEND_DIR="$(cd "$(dirname "$0")/.." && pwd)/frontend"
if [[ -d "$FRONTEND_DIR" ]] && has pnpm; then
    info "Installing frontend dependencies..."
    (cd "$FRONTEND_DIR" && pnpm install)
else
    warn "Skipping frontend install (pnpm or frontend/ not available)"
fi

# --- Build check ---
echo ""
info "=== Verifying Rust workspace builds ==="
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
(cd "$ROOT" && cargo check --workspace) && info "Workspace builds successfully" \
    || warn "Workspace build had issues — check output above"

# --- Summary ---
echo ""
echo "=========================================="
info "Setup complete! Summary:"
echo "=========================================="
echo ""
has rustc     && echo -e "  ${GREEN}ok${NC}  Rust     $(rustc --version)" \
              || echo -e "  ${RED}--${NC}  Rust     not found"
has just      && echo -e "  ${GREEN}ok${NC}  just     $(just --version)" \
              || echo -e "  ${RED}--${NC}  just     not found"
has mosquitto && echo -e "  ${GREEN}ok${NC}  mosquitto installed" \
              || echo -e "  ${RED}--${NC}  mosquitto not found"
has node      && echo -e "  ${GREEN}ok${NC}  Node.js  $(node --version)" \
              || echo -e "  ${RED}--${NC}  Node.js  not found"
has pnpm      && echo -e "  ${GREEN}ok${NC}  pnpm     $(pnpm --version)" \
              || echo -e "  ${RED}--${NC}  pnpm     not found"
has ollama    && echo -e "  ${GREEN}ok${NC}  ollama   installed" \
              || echo -e "  ${YELLOW}--${NC}  ollama   not found (optional)"
echo ""
info "Next steps:"
echo "  1. Run tests:        cargo test --workspace"
echo "  2. Start everything: ./dev/run-local.sh"
echo "  3. Open dashboard:   cd frontend && pnpm dev -- --port 5174"
echo ""
