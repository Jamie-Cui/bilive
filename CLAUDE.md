<!--
Copyright (C) 2026 Jamie Cui
Author: Jamie Cui
SPDX-License-Identifier: GPL-3.0-or-later
-->

# CLAUDE.md

## Dev Commands

```sh
cargo fmt --all                         # format all Rust code
cargo fmt --all -- --check              # check Rust formatting
cargo check --workspace                 # type-check the workspace
cargo test --workspace                  # run Rust tests
node --check web/app.js                 # validate frontend JavaScript syntax
cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web
cargo run -p bilive -- start --listen 127.0.0.1:22333 --web-dir web
cargo run -p bilive -- status
cargo run -p bilive -- stop
cargo build --release -p bilive
cargo build-rpm                         # alias for xtask RPM packaging
```

`serve` runs the HTTP service in the foreground and is hidden from top-level
help. `start` launches the same service as a detached child process, writes a
pid file and log file under the state directory, and health-checks
`/api/health` before returning.

The frontend is served from embedded assets by default. Use `--web-dir web` for
UI development so edits to `web/index.html`, `web/app.js`, and `web/styles.css`
are visible after a browser refresh without rebuilding.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `BILIVE_LISTEN` | `127.0.0.1:22333` | Service bind address |
| `BILIVE_WEB_DIR` | embedded UI | Static frontend directory override |
| `BILIVE_CONFIG` | platform config path | JSON config file override |
| `BILIVE_STATE_DIR` | platform state path | Background pid/log directory and legacy config fallback |
| `BILIVE_FFMPEG` | `ffmpeg` | FFmpeg executable used by stream tests |
| `RUST_LOG` | `bilive=info,bilive_server=info,bilive_core=info,tower_http=info` | Tracing filter |

Default config path is `~/.config/bilive/config` on Linux-like systems, or
`$XDG_CONFIG_HOME/bilive/config` when `XDG_CONFIG_HOME` is set. If that file is
missing and no explicit `--config`/`BILIVE_CONFIG` is set, the app reads the
legacy state-dir `config.json` once and saves future changes to the new config
path.

Background control also supports `--state-dir`, `--pid-file`, `--log-file`, and
`--timeout`. Reuse the same path overrides for `status`, `restart`, and `stop`.

## Architecture

Rust 2024 edition workspace with resolver v3 and MSRV from `Cargo.toml`.

- **bilive-cli** — Clap entry point for `start`/`stop`/`status`/`restart` and hidden foreground `serve`; derives runtime pid/log paths and launches the server.
- **bilive-core** — Bilibili HTTP client, WBI/app signing, config persistence, stream credential parsing, danmu TCP protocol/client, and shared event types.
- **bilive-server** — Axum HTTP/WebSocket server; exposes API routes, renders QR SVGs, serves embedded or filesystem static UI assets, broadcasts events, stores recent danmu history, and runs stream tests.
- **web** — Vanilla HTML/CSS/JavaScript admin UI; no npm, Vite, bundler, or generated frontend artifacts.
- **xtask** — Packaging helper behind `cargo build-rpm`.

## Backend Notes

- `Event` in `crates/bilive-core/src/event.rs` serializes as snake_case tagged JSON: `connection`, `danmu_raw`, and `error`.
- Server WebSocket clients subscribe to a `tokio::sync::broadcast` channel with capacity 1024 at `/api/events`.
- `BiliClient` in `crates/bilive-core/src/bili/client.rs` talks to `api.bilibili.com`, `api.live.bilibili.com`, and `passport.bilibili.com`.
- Request signing lives in `crates/bilive-core/src/bili/sign.rs`: WBI signing for web APIs and app signing for mobile APIs.
- `ConfigStore` in `crates/bilive-core/src/config.rs` wraps `AppConfig`, persists pretty JSON, and auto-saves through `ConfigStore::update()`.
- Public config responses must go through the server-side public config shape; never expose cookies, CSRF tokens, danmu tokens, or raw stream secrets unnecessarily.

## API Surface

Main server routes include:

- Health/events/config: `/api/health`, `/api/events`, `/api/config`.
- Auth: `/api/auth/status`, `/api/auth/bootstrap`, `/api/auth/cookie`, `/api/auth/logout`, `/api/auth/qrcode/generate`, `/api/auth/qrcode/poll`.
- Live: `/api/live/room-id`, `/api/live/areas`, `/api/live/danmu-info`, `/api/live/version`, `/api/live/title`, `/api/live/area`, `/api/live/start`, `/api/live/test-stream`, `/api/live/stop`, `/api/live/comment`, `/api/live/contribution-rank`.
- Danmu: `/api/danmu/connect`, `/api/danmu/disconnect`, `/api/danmu/messages`, `/api/danmu/status`.
- Manager: room admins, silent users, room silent, blocked words, user search, and online rank workflows under `/api/manager/*`.

## Frontend

The UI has account, stream, comments, and manager tabs. Comments include danmu
connect/disconnect, recent/history loading, rank lookup, comment sending, and
optional desktop notification settings. Manager workflows cover admins, user
silence, global room silence, blocked words, and user search.

Keep `web/app.js` dependency-free and simple. Embedded assets use `include_bytes!`
from `crates/bilive-server/src/lib.rs`, so rebuild the Rust binary when shipping
changes to `web/` without `--web-dir`.

## Key Files

| File | What |
|------|------|
| `crates/bilive-cli/src/main.rs` | CLI entry point and background service control |
| `crates/bilive-core/src/bili/client.rs` | Bilibili API methods and config mutation helpers |
| `crates/bilive-core/src/bili/sign.rs` | WBI and app request signing |
| `crates/bilive-core/src/config.rs` | `AppConfig`, `ConfigStore`, default paths, cookie parsing |
| `crates/bilive-core/src/danmu/client.rs` | Danmu TCP connection and heartbeat |
| `crates/bilive-core/src/danmu/protocol.rs` | Binary danmu protocol codec |
| `crates/bilive-core/src/event.rs` | Shared event enum |
| `crates/bilive-server/src/lib.rs` | HTTP routes, WebSocket handler, static UI, notifications, stream test |
| `web/app.js` | Frontend state, API calls, rendering, event handling |
| `packaging/systemd/bilive.service` | Manual systemd unit template |
| `packaging/rpm/bilive.service` | RPM-installed systemd unit |
| `xtask/src/main.rs` | `cargo build-rpm` implementation |
