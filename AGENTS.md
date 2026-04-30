<!--
Copyright (C) 2026 Jamie Cui
Author: Jamie Cui
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace with a no-build static web UI.

- `crates/bilive-cli/`: command-line entry point, foreground `bilive serve`,
  and background `start`/`stop`/`status`/`restart` control.
- `crates/bilive-server/`: axum HTTP/WebSocket service, API routes, QR SVG rendering, and static file serving.
- `crates/bilive-core/`: Bilibili HTTP client, WBI/app signing, state storage, danmu client, and shared event types.
- `web/`: plain `index.html`, `styles.css`, and `app.js`; no npm, Vite, or frontend build step.
- `packaging/systemd/`: Linux service unit templates.
- `Cargo.toml` and `Cargo.lock`: workspace manifest and locked Rust dependencies.

Keep tests near the Rust module they cover with inline `#[cfg(test)]` modules or crate-local `tests/`.

## Build, Test, and Development Commands

- `cargo fmt --all`: format all Rust code.
- `cargo fmt --all -- --check`: verify formatting in CI or before commits.
- `cargo check --workspace`: type-check the complete workspace.
- `cargo test --workspace`: run all Rust tests.
- `node --check web/app.js`: validate frontend JavaScript syntax.
- `cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web`: run the local service and web UI.
- `cargo run -p bilive -- start --listen 127.0.0.1:22333 --web-dir web`: start the local service in the background.
- `cargo run -p bilive -- status` / `cargo run -p bilive -- stop`: inspect
  or stop the background service.
- `cargo build --release -p bilive`: build the release binary.

The frontend is served directly from `web/`; edit files and refresh the
browser. `serve` is hidden from the top-level help but is used by the
background CLI child process and the systemd unit.

## Coding Style & Naming Conventions

Use standard Rust formatting via `rustfmt`. Keep protocol and Bilibili logic in `bilive-core`, HTTP routing in `bilive-server`, and CLI parsing in `bilive-cli`.

Rust names should follow idiomatic conventions: `snake_case` for functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Keep JavaScript in `web/app.js` simple and dependency-free.

Keep public API config responses routed through the server-side public config
shape; do not return raw cookies, CSRF tokens, or danmu tokens. When changing
stream credential handling, preserve stream-key redaction in logs and error
messages.

## Testing Guidelines

Add unit tests for signing, protocol parsing, event serialization, state storage, and request handling. Use names like `decodes_brotli_danmu_packet`. Run `cargo test --workspace` before submitting.

For CLI background-service changes, cover pid/log path derivation and stale pid
behavior where practical. For stream-test changes, keep tests around URL
joining, stderr sanitization, and short failure messages.

## Commit & Pull Request Guidelines

This repository currently has no commit history, so there is no established local convention. Use concise imperative commits, optionally scoped, for example:

- `core: add danmu packet decoder tests`
- `server: expose login status endpoint`
- `web: simplify connection form`

Pull requests should include a short summary, verification commands run, and screenshots or notes for visible UI changes. Link related issues when available and call out service, storage, or systemd behavior changes explicitly.

## Security & Configuration Tips

Default services should listen on `127.0.0.1`, not `0.0.0.0`. Do not log cookies, CSRF tokens, danmu tokens, or stream keys. Keep state paths compatible with `/var/lib/bilive`, `BILIVE_STATE_DIR`, and cross-platform CLI use.

`--config`/`BILIVE_CONFIG` select the JSON config file.
`--state-dir`/`BILIVE_STATE_DIR` control background pid/log placement and the
default base directory for `config.json`. `BILIVE_FFMPEG` can point stream
testing at a non-default `ffmpeg` binary.
