#!/bin/bash
# Deploy pftui binary: build release, atomic install, restart services.
#
# Eliminates "text file busy" errors by using atomic rename instead of
# overwriting the running binary in-place.
#
# Usage:
#   scripts/deploy.sh              # build + deploy + restart
#   scripts/deploy.sh --skip-build # deploy existing binary (skip cargo build)
#   scripts/deploy.sh --dry-run    # show what would happen without doing it

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BINARY="$REPO_DIR/target/release/pftui"
INSTALL_DIR="/root/.cargo/bin"
INSTALL_PATH="$INSTALL_DIR/pftui"
SERVICES=(pftui-daemon pftui-mobile)

SKIP_BUILD=false
DRY_RUN=false

for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=true ;;
        --dry-run)    DRY_RUN=true ;;
        -h|--help)
            echo "Usage: scripts/deploy.sh [--skip-build] [--dry-run]"
            echo ""
            echo "Build the release binary and deploy it atomically."
            echo ""
            echo "Options:"
            echo "  --skip-build  Skip cargo build (use existing target/release/pftui)"
            echo "  --dry-run     Show what would happen without doing it"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

log() { echo "==> $*"; }
run() {
    if $DRY_RUN; then
        echo "  [dry-run] $*"
    else
        "$@"
    fi
}

# Step 1: Build
if ! $SKIP_BUILD; then
    log "Building release binary..."
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env" 2>/dev/null || true
    if $DRY_RUN; then
        echo "  [dry-run] cargo build --release"
    else
        (cd "$REPO_DIR" && cargo build --release)
    fi
fi

# Verify binary exists
if ! $DRY_RUN && [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY" >&2
    echo "Run without --skip-build or build manually first." >&2
    exit 1
fi

# Step 2: Atomic install
#
# The "text file busy" error occurs when cp tries to write into a file
# that a running process has open. The fix: copy to a temp file in the
# same directory, then mv (atomic rename). The old inode stays valid for
# running processes; new executions use the new inode.
log "Installing binary atomically..."
TMPBIN="$INSTALL_DIR/.pftui.new.$$"
run cp "$BINARY" "$TMPBIN"
run chmod 755 "$TMPBIN"
run mv -f "$TMPBIN" "$INSTALL_PATH"

# Step 3: Restart services
log "Restarting services..."
for svc in "${SERVICES[@]}"; do
    if systemctl is-enabled "$svc" &>/dev/null; then
        run sudo systemctl restart "$svc"
    else
        echo "  (skipping $svc — not enabled)"
    fi
done

# Step 4: Verify
if ! $DRY_RUN; then
    sleep 3
    log "Service status:"
    for svc in "${SERVICES[@]}"; do
        if systemctl is-enabled "$svc" &>/dev/null; then
            status=$(systemctl is-active "$svc" 2>/dev/null || true)
            if [ "$status" = "active" ]; then
                echo "  ✓ $svc — active"
            else
                echo "  ✗ $svc — $status" >&2
                systemctl status "$svc" --no-pager | head -8 >&2
            fi
        fi
    done

    # Show version
    if command -v pftui &>/dev/null; then
        version=$(pftui --version 2>/dev/null || echo "unknown")
        log "Deployed: $version"
    fi
fi

log "Done."
