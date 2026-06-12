# Copyright (C) 2026 Jamie Cui
# Author: Jamie Cui
# SPDX-License-Identifier: GPL-3.0-or-later

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPSDIR ?= $(DATADIR)/applications
CARGO ?= cargo
INSTALL ?= install

BINARIES := bilive bilive-danmu
RELEASE_BINARIES := $(addprefix target/release/,$(BINARIES))

DESKTOP_TEMPLATE := packaging/desktop/bilive.desktop.in
DESKTOP_NAME := bilive.desktop

.PHONY: all help build install uninstall clean check test fmt

all: build

help:
	@printf '%s\n' 'Usage: make [target]'
	@printf '\n%s\n' 'Targets:'
	@printf '  %-10s %s\n' 'all' 'Build release binaries (default)'
	@printf '  %-10s %s\n' 'build' 'Build release binaries for the workspace'
	@printf '  %-10s %s\n' 'install' 'Install release binaries and bilive.desktop under $(DESTDIR)$(PREFIX)'
	@printf '  %-10s %s\n' 'uninstall' 'Remove installed binaries and bilive.desktop'
	@printf '  %-10s %s\n' 'check' 'Type-check the workspace'
	@printf '  %-10s %s\n' 'test' 'Run workspace tests'
	@printf '  %-10s %s\n' 'fmt' 'Check Rust formatting'
	@printf '  %-10s %s\n' 'clean' 'Remove Cargo build artifacts'
	@printf '  %-10s %s\n' 'help' 'Show this help message'
	@printf '\n%s\n' 'Variables:'
	@printf '  %-10s %s\n' 'PREFIX' 'Install prefix (default: /usr/local)'
	@printf '  %-10s %s\n' 'BINDIR' 'Install binary directory (default: $(PREFIX)/bin)'
	@printf '  %-10s %s\n' 'DATADIR' 'Install data directory (default: $(PREFIX)/share)'
	@printf '  %-10s %s\n' 'APPSDIR' 'Desktop entry directory (default: $(DATADIR)/applications)'
	@printf '  %-10s %s\n' 'DESTDIR' 'Staging root for installation'
	@printf '  %-10s %s\n' 'CARGO' 'Cargo executable (default: cargo)'
	@printf '  %-10s %s\n' 'INSTALL' 'Install executable (default: install)'

build:
	$(CARGO) build --release --workspace

install: $(RELEASE_BINARIES)
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 $(RELEASE_BINARIES) "$(DESTDIR)$(BINDIR)/"
	$(INSTALL) -d "$(DESTDIR)$(APPSDIR)"
	sed 's|@BINDIR@|$(BINDIR)|g' "$(DESKTOP_TEMPLATE)" > "$(DESTDIR)$(APPSDIR)/$(DESKTOP_NAME)"
	chmod 0644 "$(DESTDIR)$(APPSDIR)/$(DESKTOP_NAME)"

uninstall:
	for bin in $(BINARIES); do rm -f "$(DESTDIR)$(BINDIR)/$$bin"; done
	rm -f "$(DESTDIR)$(APPSDIR)/$(DESKTOP_NAME)"

$(RELEASE_BINARIES):
	@printf 'error: missing %s\n' '$@' >&2
	@printf 'run `make build` before `make install`\n' >&2
	@exit 1

check:
	$(CARGO) check --workspace

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all -- --check

clean:
	$(CARGO) clean
