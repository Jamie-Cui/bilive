<!--
Copyright (C) 2026 Jamie Cui
Author: Jamie Cui
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace with an embedded, no-build static web UI.

- `crates/bilive-cli/`: command-line entry point, foreground `bilive serve`,
  and background `start`/`stop`/`status`/`restart` control.
- `crates/bilive-server/`: axum HTTP/WebSocket service, API routes, QR SVG
  rendering, embedded/static file serving, danmu history, notifications, and
  stream testing.
- `crates/bilive-danmu/`: deprecated compatibility terminal UI; prefer the web
  UI comments tab for danmu viewing, history, connection control, and sending.
- `crates/bilive-core/`: Bilibili HTTP client, WBI/app signing, config storage,
  stream credential parsing, danmu client/protocol, and shared event types.
- `web/`: plain `index.html`, `styles.css`, `app.js`, and `favicon.svg`; no
  npm, Vite, bundler, or generated frontend artifacts.
- `packaging/systemd/`: Linux service unit template.
- `Cargo.toml` and `Cargo.lock`: workspace manifest and locked Rust dependencies.

Keep tests near the Rust module they cover with inline `#[cfg(test)]` modules
or crate-local `tests/`.

## Build, Test, and Development Commands

- `cargo fmt --all`: format all Rust code.
- `cargo fmt --all -- --check`: verify formatting in CI or before commits.
- `cargo check --workspace`: type-check the complete workspace.
- `cargo test --workspace`: run all Rust tests.
- `node --check web/app.js`: validate frontend JavaScript syntax.
- `cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web`: run
  the local service and serve UI files from the working tree.
- `cargo run -p bilive -- start --listen 127.0.0.1:22333 --web-dir web`: start
  the local service in the background.
- `cargo run -p bilive-danmu -- --url http://127.0.0.1:22333`: deprecated
  compatibility terminal viewer; prefer the web UI comments tab.
- `cargo run -p bilive -- status` / `cargo run -p bilive -- stop`: inspect or
  stop the background service.
- `cargo build --release -p bilive`: build the release binary with embedded UI
  assets.
The frontend is embedded into the Rust binary by default. During UI work, pass
`--web-dir web`, edit files directly, and refresh the browser. Rebuild the Rust
binary before relying on embedded UI assets. `serve` is hidden from top-level
help but is used by foreground development, the background CLI child process,
and systemd units.

## Coding Style & Naming Conventions

Use standard Rust formatting via `rustfmt`. Keep protocol and Bilibili logic in
`bilive-core`, HTTP routing in `bilive-server`, and CLI parsing/background
control in `bilive-cli`.

Rust names should follow idiomatic conventions: `snake_case` for
functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for
constants. Keep JavaScript in `web/app.js` simple and dependency-free.

Keep public API config responses routed through the server-side public config
shape; do not return raw cookies, CSRF tokens, danmu tokens, or raw stream keys.
When changing stream credential handling, preserve stream-key redaction in logs,
`ffmpeg` stderr sanitization, and error messages.

## Testing Guidelines

Add unit tests for signing, protocol parsing, event serialization, state/config
storage, route helpers, and request handling. Use descriptive names like
`decodes_brotli_danmu_packet`. Run `cargo test --workspace` before submitting.

For CLI background-service changes, cover pid/log path derivation, web-dir
resolution, and stale pid behavior where practical. For stream-test changes,
keep tests around URL joining, stderr sanitization, and short failure messages.
For frontend-only changes, run `node --check web/app.js`.

## Commit & Pull Request Guidelines

This repository currently has no commit history, so there is no established
local convention. Use concise imperative commits, optionally scoped, for example:

- `core: add danmu packet decoder tests`
- `server: expose login status endpoint`
- `web: simplify connection form`

Pull requests should include a short summary, verification commands run, and
screenshots or notes for visible UI changes. Link related issues when available
and call out service, storage/config, packaging, or systemd behavior changes
explicitly.

## Security & Configuration Tips

Default services should listen on `127.0.0.1`, not `0.0.0.0`. Do not log
cookies, CSRF tokens, danmu tokens, or stream keys. Keep state paths compatible
with `/var/lib/bilive`, `BILIVE_STATE_DIR`, and cross-platform CLI use.

`--config`/`BILIVE_CONFIG` select the JSON config file. The default config path
is under the platform config directory, such as `~/.config/bilive/config` on
Linux-like systems; the old state-dir `config.json` is only a compatibility
fallback when no explicit config path is set.

`--state-dir`/`BILIVE_STATE_DIR` control background pid/log placement and the
legacy config fallback path. `--pid-file`, `--log-file`, and `--timeout` tune
background control behavior. `BILIVE_FFMPEG` can point stream testing at a
non-default `ffmpeg` binary.

Danmu desktop notifications are disabled by default and controlled by
`danmu_notifications` in config or the web UI. Linux notifications use
`notify-send`; prefer running `bilive start` from the user session, or use a
systemd user service, when desktop notifications must reach the compositor.

VTuber control (`/api/vtuber/*`) manages an external EasyVtuber process only.
The child's stdout/stderr are captured to `<cache_dir>/vtuber.log` (truncated
per run, exposed via `/api/vtuber/logs`); never send them to `/dev/null`.
Output-mode handling is platform-aware: reject `spout2` off Windows, and treat
the `debug` OpenCV window (captured into OBS via Window Capture) as the working
Linux path. When touching the EasyVtuber command builder, keep the generated
flags aligned with upstream `src/args.py` and cover them with `vtuber_command`
unit tests.
