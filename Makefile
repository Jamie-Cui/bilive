# Copyright (C) 2026 Jamie Cui
# Author: Jamie Cui
# SPDX-License-Identifier: GPL-3.0-or-later

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo
INSTALL ?= install

BINARIES := bilive bilive-danmu
RELEASE_BINARIES := $(addprefix target/release/,$(BINARIES))

.PHONY: all help build install clean check test fmt

all: build

help:
	@printf '%s\n' 'Usage: make [target]'
	@printf '\n%s\n' 'Targets:'
	@printf '  %-10s %s\n' 'all' 'Build release binaries (default)'
	@printf '  %-10s %s\n' 'build' 'Build release binaries for the workspace'
	@printf '  %-10s %s\n' 'install' 'Install existing release binaries into $(DESTDIR)$(BINDIR)'
	@printf '  %-10s %s\n' 'check' 'Type-check the workspace'
	@printf '  %-10s %s\n' 'test' 'Run workspace tests'
	@printf '  %-10s %s\n' 'fmt' 'Check Rust formatting'
	@printf '  %-10s %s\n' 'clean' 'Remove Cargo build artifacts'
	@printf '  %-10s %s\n' 'help' 'Show this help message'
	@printf '\n%s\n' 'Variables:'
	@printf '  %-10s %s\n' 'PREFIX' 'Install prefix (default: /usr/local)'
	@printf '  %-10s %s\n' 'BINDIR' 'Install binary directory (default: $(PREFIX)/bin)'
	@printf '  %-10s %s\n' 'DESTDIR' 'Staging root for installation'
	@printf '  %-10s %s\n' 'CARGO' 'Cargo executable (default: cargo)'
	@printf '  %-10s %s\n' 'INSTALL' 'Install executable (default: install)'

build:
	$(CARGO) build --release --workspace

install: $(RELEASE_BINARIES)
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 $(RELEASE_BINARIES) "$(DESTDIR)$(BINDIR)/"

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
