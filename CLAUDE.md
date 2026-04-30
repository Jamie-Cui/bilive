<!--
Copyright (C) 2026 Jamie Cui
Author: Jamie Cui
SPDX-License-Identifier: GPL-3.0-or-later
-->

# CLAUDE.md

## Dev Commands

```sh
cargo build                # build all crates
cargo run -- serve         # run server (default: 127.0.0.1:22333)
cargo check                # type-check without building
cargo clippy               # lint
cargo test                 # run tests (minimal coverage currently)
```

The binary is `bilive` with a `serve` subcommand. CLI args map to env vars below.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `BILIVE_LISTEN` | `127.0.0.1:22333` | Server listen address |
| `BILIVE_WEB_DIR` | (embedded UI) | Static frontend directory override |
| `BILIVE_CONFIG` | (none) | Config file path override |
| `BILIVE_STATE_DIR` | platform default | State directory override |
| `BILIVE_FFMPEG` | `ffmpeg` | FFmpeg binary path |
| `RUST_LOG` | `bilive=info,...` | Logging filter |

State directory defaults: macOS `~/Library/Application Support/bilive`, Linux `$XDG_STATE_HOME/bilive`.

## Architecture

Rust 2024 edition workspace (resolver v3, MSRV 1.90) with three crates:

- **bilive-cli** — Binary entry point. Clap arg parsing, logging init, launches server.
- **bilive-core** — Bilibili API client (`BiliClient`), danmu protocol, config persistence, event types. No server dependencies.
- **bilive-server** — Axum HTTP/WS server. Serves static web UI, proxies API calls to `BiliClient`, broadcasts events over WebSocket.

### Event System

`Event` enum in `bilive-core/src/event.rs`: `Connection(status)`, `DanmuRaw { payload }`, `Error { message }`. Distributed via `tokio::sync::broadcast` (capacity 1024). Server subscribes each WS client to the broadcast channel.

### API Client

`BiliClient` in `bilive-core/src/bili/client.rs` (~1000 lines). Hits three base URLs:
- `api.bilibili.com` — general API
- `api.live.bilibili.com` — live streaming API
- `passport.bilibili.com` — auth (QR login)

Request signing: WBI signing (img_key/sub_key + MD5) for web APIs, app signing (APP_KEY/APP_SEC) for mobile APIs. Both in `bili/sign.rs`.

### Config & State

`ConfigStore` wraps `AppConfig` with JSON file persistence. Stores cookies, user info, room settings, stream credentials. Auto-saves on mutation via `ConfigStore::update()`.

## Frontend

Vanilla JS — no build step, no framework. Just `web/index.html`, `web/app.js`, `web/styles.css`.

Tabs: Account (login), Stream (go live, credentials), Comments (danmu), Manager (admins, silence, blocked words). Connects to server via WebSocket for real-time events.

## Key Files

| File | What |
|------|------|
| `crates/bilive-core/src/bili/client.rs` | All Bilibili API methods |
| `crates/bilive-core/src/bili/sign.rs` | WBI + app request signing |
| `crates/bilive-core/src/config.rs` | AppConfig, ConfigStore |
| `crates/bilive-core/src/danmu/client.rs` | Danmu TCP connection + heartbeat |
| `crates/bilive-core/src/danmu/protocol.rs` | Binary danmu protocol codec |
| `crates/bilive-core/src/event.rs` | Event enum definition |
| `crates/bilive-server/src/lib.rs` | HTTP routes + WS handler |
| `crates/bilive-cli/src/main.rs` | CLI entry point |
| `web/app.js` | Frontend logic |
