SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c

DEFAULT_DMG := $(CURDIR)/Codex.dmg
APP_DIR := $(CURDIR)/codex-app
PACKAGE_NAME := codex-desktop
DEB_GLOB := $(CURDIR)/dist/$(PACKAGE_NAME)_*.deb
RPM_GLOB := $(CURDIR)/dist/$(PACKAGE_NAME)-*.rpm
PACMAN_GLOB := $(CURDIR)/dist/$(PACKAGE_NAME)-[0-9]*.pkg.tar.*

.DEFAULT_GOAL := help

.PHONY: help check test build-updater build-app deb rpm pacman package install clean-dist clean-state

help:
	@printf '\nCodex Desktop Linux Make Targets\n\n'
	@printf '  %-18s %s\n' "make check" "Run cargo check for codex-update-manager"
	@printf '  %-18s %s\n' "make test" "Run updater test suite"
	@printf '  %-18s %s\n' "make build-updater" "Build codex-update-manager in release mode"
	@printf '  %-18s %s\n' "make build-app" "Run install.sh and regenerate codex-app/"
	@printf '  %-18s %s\n' "make deb" "Build the Debian package into dist/"
	@printf '  %-18s %s\n' "make rpm" "Build the RPM package into dist/ (Fedora)"
	@printf '  %-18s %s\n' "make pacman" "Build the pacman package into dist/ (Arch)"
	@printf '  %-18s %s\n' "make package" "Build native package (auto-detects deb, rpm, or pacman)"
	@printf '  %-18s %s\n' "make install" "Install the latest generated native package"
	@printf '  %-18s %s\n' "make clean-dist" "Remove generated dist/ artifacts"
	@printf '  %-18s %s\n' "make clean-state" "Remove updater runtime state from XDG directories"
	@printf '\nVariables:\n\n'
	@printf '  %-18s %s\n' "DMG=/path/file.dmg" "Override the DMG passed to install.sh (default: $(DEFAULT_DMG))"
	@printf '  %-18s %s\n' "PACKAGE_VERSION=..." "Override the package version for make deb / make rpm / make pacman"
	@printf '  %-18s %s\n' "DEB=/path/file.deb" "Override the .deb used by make install"
	@printf '  %-18s %s\n' "RPM=/path/file.rpm" "Override the .rpm used by make install"
	@printf '  %-18s %s\n' "PKG=/path/file.pkg.tar.zst" "Override the pacman package used by make install"
	@printf '\nExamples:\n\n'
	@printf '  %s\n' "make build-app DMG=/tmp/Codex.dmg"
	@printf '  %s\n' "make deb PACKAGE_VERSION=2026.03.24.220723+88f07cd3"
	@printf '  %s\n' "make rpm PACKAGE_VERSION=2026.03.24.220723+88f07cd3"
	@printf '  %s\n' "make pacman PACKAGE_VERSION=2026.03.24.220723+88f07cd3"
	@printf '  %s\n\n' "make install"

check:
	@echo "[make] Running cargo check"
	cargo check -p codex-update-manager

test:
	@echo "[make] Running cargo test"
	cargo test -p codex-update-manager

build-updater:
	@echo "[make] Building codex-update-manager (release)"
	cargo build --release -p codex-update-manager

build-app:
	@echo "[make] Regenerating codex-app from DMG"
	./install.sh "$(or $(DMG),$(DEFAULT_DMG))"

deb: build-updater
	@echo "[make] Building Debian package"
	PACKAGE_VERSION="$(or $(PACKAGE_VERSION),)" ./scripts/build-deb.sh

rpm: build-updater
	@echo "[make] Building RPM package"
	PACKAGE_VERSION="$(or $(PACKAGE_VERSION),)" ./scripts/build-rpm.sh

pacman: build-updater
	@echo "[make] Building pacman package"
	PACKAGE_VERSION="$(or $(PACKAGE_VERSION),)" ./scripts/build-pacman.sh

package: build-updater
	@echo "[make] Building native package (auto-detecting distro)"
	@if command -v makepkg >/dev/null 2>&1 && ! command -v dpkg-deb >/dev/null 2>&1; then \
		PACKAGE_VERSION="$(or $(PACKAGE_VERSION),)" ./scripts/build-pacman.sh; \
	elif command -v dpkg-deb >/dev/null 2>&1; then \
		PACKAGE_VERSION="$(or $(PACKAGE_VERSION),)" ./scripts/build-deb.sh; \
	elif command -v rpmbuild >/dev/null 2>&1; then \
		PACKAGE_VERSION="$(or $(PACKAGE_VERSION),)" ./scripts/build-rpm.sh; \
	else \
		echo "[make] No supported packaging tool found. Install dpkg-dev (Debian), rpm-build (Fedora), or pacman (Arch)." >&2; \
		exit 1; \
	fi

install:
	@echo "[make] Installing latest native package"
	@if command -v pacman >/dev/null 2>&1 && ! command -v dpkg >/dev/null 2>&1; then \
		pkg="$${PKG:-$$(ls -1 $(PACMAN_GLOB) 2>/dev/null | sort -V | tail -n 1)}"; \
		if [ -z "$$pkg" ]; then \
			echo "[make] No pacman package found. Run 'make pacman' first." >&2; exit 1; \
		fi; \
		echo "[make] Installing $$pkg"; \
		sudo pacman -U --noconfirm "$$pkg"; \
	elif command -v dpkg >/dev/null 2>&1; then \
		deb="$${DEB:-$$(ls -1 $(DEB_GLOB) 2>/dev/null | sort -V | tail -n 1)}"; \
		if [ -z "$$deb" ]; then \
			echo "[make] No Debian package found. Run 'make deb' first." >&2; exit 1; \
		fi; \
		echo "[make] Installing $$deb"; \
		sudo dpkg -i "$$deb"; \
	elif command -v rpm >/dev/null 2>&1; then \
		rpm="$${RPM:-$$(ls -1 $(RPM_GLOB) 2>/dev/null | sort -V | tail -n 1)}"; \
		if [ -z "$$rpm" ]; then \
			echo "[make] No RPM package found. Run 'make rpm' first." >&2; exit 1; \
		fi; \
		echo "[make] Installing $$rpm"; \
		sudo rpm -Uvh "$$rpm"; \
	else \
		echo "[make] No supported package manager found (dpkg, rpm, or pacman)." >&2; exit 1; \
	fi

clean-dist:
	@echo "[make] Removing dist/"
	rm -rf "$(CURDIR)/dist"

clean-state:
	@echo "[make] Removing updater runtime state"
	rm -rf \
		"$$HOME/.config/codex-update-manager" \
		"$$HOME/.local/state/codex-update-manager" \
		"$$HOME/.cache/codex-update-manager"
