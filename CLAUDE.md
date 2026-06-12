<!--
Copyright (C) 2026 Jamie Cui
Author: Jamie Cui
SPDX-License-Identifier: GPL-3.0-or-later
-->

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Dev Commands

```sh
cargo fmt --all                         # format all Rust code
cargo fmt --all -- --check              # check Rust formatting
cargo check --workspace                 # type-check the workspace
cargo test --workspace                  # run Rust tests
cargo test -p bilive-core config::      # run a single module's tests (filter by path)
node --check web/app.js                 # validate frontend JavaScript syntax
cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web
cargo run -p bilive -- start --listen 127.0.0.1:22333 --web-dir web
cargo run -p bilive-danmu -- --url http://127.0.0.1:22333 --overlay
cargo run -p bilive -- status
cargo run -p bilive -- stop
cargo build --release -p bilive
```

`cargo test` accepts a substring filter (`cargo test -p <crate> <filter>`); tests
live in inline `#[cfg(test)]` modules next to the code they cover.

`serve` runs the HTTP service in the foreground and is hidden from top-level
help; it is used by foreground development, the background `start` child
process, and the systemd unit. `start` launches the same service as a detached
(`setsid`) child process, writes a pid file and log file under the state
directory, and health-checks `/api/health` before returning.

The frontend is served from assets embedded into the binary by default. Use
`--web-dir web` for UI development so edits to `web/index.html`, `web/app.js`,
and `web/styles.css` are visible after a browser refresh without rebuilding.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `BILIVE_LISTEN` | `127.0.0.1:22333` | Service bind address |
| `BILIVE_WEB_DIR` | embedded UI | Static frontend directory override |
| `BILIVE_CONFIG` | platform config path | TOML user config file override |
| `BILIVE_CACHE_DIR` | platform cache path | Directory for the private `state.json` runtime cache |
| `BILIVE_STATE_DIR` | platform state path | Background pid/log directory and legacy config fallback |
| `BILIVE_FFMPEG` | `ffmpeg` | FFmpeg executable used by stream tests |
| `RUST_LOG` | `bilive=info,bilive_server=info,bilive_core=info,tower_http=info` | Tracing filter |

Default config path is `~/.config/bilive/config` (TOML, despite no extension),
honoring `XDG_CONFIG_HOME`. Runtime cache state defaults to
`~/.cache/bilive/state.json`, honoring `XDG_CACHE_HOME`. Background pid/log live
under the state dir (`~/.local/state/bilive`).

Background control also supports `--state-dir`, `--pid-file`, `--log-file`, and
`--timeout`. Reuse the same path/listen overrides for `status`, `restart`, and
`stop`. `bilive-danmu` takes its own flags (`--url`, `--overlay`, `--backend`,
geometry/font options); see `crates/bilive-danmu/src/cli.rs`.

## Architecture

Rust 2024 edition workspace, resolver v3, MSRV `1.90` (see `Cargo.toml`).

- **bilive-cli** (`bilive` binary) — Clap entry point for
  `start`/`stop`/`status`/`restart` and the hidden foreground `serve`. Derives
  runtime pid/log paths, resolves `--web-dir`, and launches `bilive-server::run`.
- **bilive-core** — Bilibili HTTP client, WBI/app request signing, config +
  cache persistence, stream credential parsing, danmu TCP protocol/client, and
  shared event types. Has no HTTP server or CLI dependencies.
- **bilive-server** — Axum HTTP/WebSocket server. Exposes API routes, renders QR
  SVGs, serves embedded or filesystem UI assets, broadcasts events, stores
  recent danmu history, runs `ffmpeg` stream tests, and controls the external
  VTuber process.
- **bilive-danmu** — Standalone **desktop danmu overlay** that connects to a
  running `bilive` service over HTTP/WebSocket and renders chat as a
  click-through, always-on-top window. Native backends: X11 (Linux, via the
  `x11` crate + `pkg-config`) and macOS. Not a deprecated terminal viewer.
- **web** — Vanilla HTML/CSS/JavaScript admin UI; no npm, Vite, bundler, or
  generated frontend artifacts.

## Config & State Split (important)

`crates/bilive-core/src/config.rs` deliberately splits persistence into two files:

- **User config (TOML)** — `UserConfig`: `theme`, `room_title`, `category_id`,
  `area_id`, `danmu_notifications`, `vtuber`. Human-editable preferences.
- **Cache state (private JSON, `0600`)** — `CachedState`: `cookies`, `csrf`,
  `uid`, `username`, `avatar`, `room_id`, `room_token`, `area_list`, `streams`,
  `is_open_live`. Login/runtime secrets and derived state.

`AppConfig` is the in-memory union of both. `ConfigStore::save` writes the TOML
config and the private cache atomically; `ConfigStore::update()` mutates and
auto-saves. On load, a legacy full-JSON `AppConfig` config file (or the old
state-dir `config.json`) is detected and migrated into the new TOML + cache
split. When adding a persisted field, decide which half it belongs to and wire
it through `UserConfig`/`CachedState` `From`/`apply_to` impls accordingly.

## Backend Notes

- `Event` in `crates/bilive-core/src/event.rs` serializes as snake_case tagged
  JSON (`{ "type": ..., "payload": ... }`): `connection`, `danmu_raw`, `error`.
- WebSocket clients at `/api/events` subscribe to a `tokio::sync::broadcast`
  channel (capacity 1024).
- `BiliClient` (`crates/bilive-core/src/bili/client.rs`) talks to
  `api.bilibili.com`, `api.live.bilibili.com`, and `passport.bilibili.com`.
- Request signing (`crates/bilive-core/src/bili/sign.rs`): WBI signing for web
  APIs, app signing for mobile APIs.
- `/api/config` and other responses go through `public_config()` in
  `crates/bilive-server/src/lib.rs`, which omits cookies, CSRF/danmu/room
  tokens, and raw stream secrets (exposing only availability flags). Never
  bypass it. Preserve stream-key redaction in logs and `sanitize_ffmpeg_error`.
- VTuber control (`/api/vtuber/*`) starts/stops an **external** EasyVtuber
  process via `tokio::process`; bilive owns only the control plane (config,
  status, start, stop) and never reimplements the Python/GPU runtime.

## API Surface

- Health/events/config: `/api/health`, `/api/events`, `/api/config`.
- Auth: `/api/auth/status`, `/api/auth/bootstrap`, `/api/auth/cookie`,
  `/api/auth/logout`, `/api/auth/qrcode/generate`, `/api/auth/qrcode/poll`,
  `/api/user/nav`.
- Live: `/api/live/room-id`, `/api/live/areas`, `/api/live/danmu-info`,
  `/api/live/version`, `/api/live/title`, `/api/live/area`, `/api/live/start`,
  `/api/live/test-stream`, `/api/live/stop`, `/api/live/comment`,
  `/api/live/contribution-rank`.
- Danmu: `/api/danmu/connect`, `/api/danmu/disconnect`, `/api/danmu/messages`,
  `/api/danmu/status`.
- VTuber: `/api/vtuber/status`, `/api/vtuber/config`, `/api/vtuber/start`,
  `/api/vtuber/stop`, `/api/vtuber/recommendation`.
- Manager (`/api/manager/*`): room admins, silent users, room silent, blocked
  words, user search, and online rank workflows.

## Frontend

UI tabs: account, stream, comments, VTuber, and manager. Comments cover danmu
connect/disconnect, recent/history loading, rank lookup, comment sending, and
optional desktop notification settings. Keep `web/app.js` dependency-free and
simple. Embedded assets use `include_bytes!` from
`crates/bilive-server/src/lib.rs`, so rebuild the Rust binary when shipping
`web/` changes without `--web-dir`.

## Key Files

| File | What |
|------|------|
| `crates/bilive-cli/src/main.rs` | CLI entry point and background service control |
| `crates/bilive-core/src/bili/client.rs` | Bilibili API methods and config mutation helpers |
| `crates/bilive-core/src/bili/sign.rs` | WBI and app request signing |
| `crates/bilive-core/src/config.rs` | `AppConfig` + TOML/cache split, default paths, cookie parsing |
| `crates/bilive-core/src/danmu/client.rs` | Danmu TCP connection and heartbeat |
| `crates/bilive-core/src/danmu/protocol.rs` | Binary danmu protocol codec |
| `crates/bilive-core/src/event.rs` | Shared event enum |
| `crates/bilive-server/src/lib.rs` | HTTP routes, WebSocket handler, static UI, notifications, stream test, VTuber control |
| `crates/bilive-danmu/src/overlay/` | X11 and macOS overlay backends |
| `crates/bilive-danmu/src/service.rs` | Overlay's HTTP/WebSocket client to the bilive service |
| `web/app.js` | Frontend state, API calls, rendering, event handling |
| `packaging/systemd/bilive.service` | Manual systemd unit template |
