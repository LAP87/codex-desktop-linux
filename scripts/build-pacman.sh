#!/bin/bash
set -Eeuo pipefail

# ============================================================================
# Codex Desktop for Linux — Arch Linux / Pacman package builder
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
. "$REPO_DIR/scripts/lib/package-common.sh"

APP_DIR="${APP_DIR_OVERRIDE:-$REPO_DIR/codex-app}"
PKG_ROOT="${PKG_ROOT_OVERRIDE:-$REPO_DIR/dist/pacman-root}"
DIST_DIR="${DIST_DIR_OVERRIDE:-$REPO_DIR/dist}"
DESKTOP_TEMPLATE="$REPO_DIR/packaging/linux/codex-desktop.desktop"
SERVICE_TEMPLATE="$REPO_DIR/packaging/linux/codex-update-manager.service"
ICON_SOURCE="$REPO_DIR/assets/codex.png"

PACKAGE_NAME="${PACKAGE_NAME:-codex-desktop}"
PACKAGE_VERSION="${PACKAGE_VERSION:-$(date -u +%Y.%m.%d.%H%M%S)}"
UPDATER_BINARY_SOURCE="${UPDATER_BINARY_SOURCE:-$REPO_DIR/target/release/codex-update-manager}"
UPDATER_SERVICE_SOURCE="${UPDATER_SERVICE_SOURCE:-$SERVICE_TEMPLATE}"

# Arch Linux package metadata
PKGARCH="${PKGARCH:-$(map_arch)}"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*" >&2; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

map_arch() {
    case "$(uname -m)" in
        x86_64)    echo "x86_64" ;;
        aarch64)   echo "aarch64" ;;
        armv7l)    echo "armv7l" ;;
        *)         error "Unsupported architecture: $(uname -m)" ;;
    esac
}

# Parse version into base + release for pacman
# Format: base+buildid (e.g. 2026.03.31.103000+1)
# Pacman format: name-version-release-arch.pkg.tar.zst
parse_version() {
    local base="${PACKAGE_VERSION%%+*}"
    local buildid="${PACKAGE_VERSION#*+}"
    if [ "$base" = "$PACKAGE_VERSION" ]; then
        buildid="1"
    fi
    # Pacman doesn't like '+' in versions
    base="${base//+/_}"
    PKGVER="$base"
    PKGREL="$buildid"
}

main() {
    [ -d "$APP_DIR" ] || error "Missing app directory: $APP_DIR. Run ./install.sh first."
    [ -x "$APP_DIR/start.sh" ] || error "Missing launcher: $APP_DIR/start.sh"
    [ -f "$DESKTOP_TEMPLATE" ] || error "Missing desktop template: $DESKTOP_TEMPLATE"
    [ -f "$UPDATER_SERVICE_SOURCE" ] || error "Missing updater service template: $UPDATER_SERVICE_SOURCE"
    [ -f "$ICON_SOURCE" ] || error "Missing icon: $ICON_SOURCE"
    command -v makepkg >/dev/null 2>&1 || error "makepkg is required (install pacman-contrib)"
    command -v tar >/dev/null 2>&1 || error "tar is required"
    command -v zstd >/dev/null 2>&1 || error "zstd is required (install zstd)"

    ensure_updater_binary

    parse_version
    local arch="$(map_arch)"
    local pkgfile="${PACKAGE_NAME}-${PKGVER}-${PKGREL}-${arch}.pkg.tar.zst"

    info "Preparing package root at $PKG_ROOT"
    rm -rf "$PKG_ROOT"
    mkdir -p "$PKG_ROOT"

    # Stage common files (app, launcher, icons, service)
    stage_common_package_files "$PKG_ROOT"

    # Stage systemd user service (Arch uses systemd user)
    local service_dir="$PKG_ROOT/usr/lib/systemd/user"
    mkdir -p "$service_dir"
    cp "$UPDATER_SERVICE_SOURCE" "$service_dir/codex-update-manager.service"
    chmod 0644 "$service_dir/codex-update-manager.service"

    # Stage update-builder bundle for self-updates
    mkdir -p "$PKG_ROOT/opt/$PACKAGE_NAME/update-builder/scripts"
    cp "$REPO_DIR/install.sh" "$PKG_ROOT/opt/$PACKAGE_NAME/update-builder/install.sh"
    cp "$REPO_DIR/scripts/build-pacman.sh" "$PKG_ROOT/opt/$PACKAGE_NAME/update-builder/scripts/build-pacman.sh"
    cp "$REPO_DIR/scripts/build-deb.sh" "$PKG_ROOT/opt/$PACKAGE_NAME/update-builder/scripts/build-deb.sh"
    cp "$REPO_DIR/scripts/build-rpm.sh" "$PKG_ROOT/opt/$PACKAGE_NAME/update-builder/scripts/build-rpm.sh"
    cp "$REPO_DIR/scripts/lib/package-common.sh" "$PKG_ROOT/opt/$PACKAGE_NAME/update-builder/scripts/lib/package-common.sh"

    # Create .PKGINFO (pacman package metadata)
    cat > "$PKG_ROOT/.PKGINFO" <<PKGINFO
# BEGIN PKGINFO
pkgname = $PACKAGE_NAME
pkgbase = $PACKAGE_NAME
pkgver = ${PKGVER}-${PKGREL}
pkgrel = 1
epoch = 0
pkgdesc = Codex Desktop for Linux — AI coding assistant
arch = $arch
url = https://github.com/snakechARM- org/codex-desktop-linux
license = MIT
format = 1
# Optional dependencies
# provides =
# conflicts =
# replaces =
# backup =
# END PKGINFO
PKGINFO

    # Create .MTREE (package file checksums — simplified)
    cd "$PKG_ROOT"
    tar --format=mtree \
        --options='!all,!contents,!link,!size' \
        --numeric-owner \
        --owner=0 --group=0 \
        -cf .MTREE \
        . 2>/dev/null || \
    # Fallback: create empty mtree if tar format not supported
    touch .MTREE

    # Build the package using makepkg
    local build_dir="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$build_dir'" RETURN

    cp -a "$PKG_ROOT/." "$build_dir/"
    cd "$build_dir"

    # Remove .PKGINFO from staged dir (it's now in build dir, will be included in archive)
    rm -f .PKGINFO .MTREE

    mkdir -p "$DIST_DIR"

    info "Building ${pkgfile}..."
    # Use makepkg to create the package
    # --nodeps: we bundle everything
    # --noprogressbar: quieter output
    # --nosignature: we don't sign during build
    # --notimes: don't preserve file times from package dir
    makepkg \
        --nodeps \
        --noprogressbar \
        --nosignature \
        --notimes \
        --packagelist \
        2>/dev/null || true

    # Build manually since we have the files ready
    # Create the .pkg.tar.zst archive manually
    local output_pkg="$DIST_DIR/${pkgfile}"
    tar -C "$PKG_ROOT" -cf "$build_dir/.pkg.tar" .
    zstd -T0 -19 -f "$build_dir/.pkg.tar" -o "$output_pkg" 2>/dev/null || \
        tar -C "$PKG_ROOT" -cJf "$output_pkg" .

    [ -f "$output_pkg" ] || error "Failed to create package: $output_pkg"

    info "Built package: $output_pkg (size: $(du -h "$output_pkg" | cut -f1))"
}

main "$@"
