# bilive

`bilive` is a local Bilibili live management service. It runs as a headless
Rust service, listens on a loopback address by default, and serves a static
browser admin UI from `web/`.

The project has three main pieces:

- A Rust CLI and service backend.
- A plain HTML/CSS/JavaScript admin UI with no frontend build step.
- Linux systemd packaging kept separate from the service implementation.

## Features

- Cookie login and Bilibili app QR-code login.
- Login bootstrap for user profile, room id, live areas, and danmu token.
- Live title and area updates.
- Start and stop live streams, capture stream credentials, and optionally test
  a stream credential with `ffmpeg`.
- Danmu connect/disconnect, WebSocket event streaming, and comment sending.
- Room admin, user silent, global silent, blocked word, user search, and online
  rank management APIs.
- Static UI tabs for account, stream, danmu, and manager workflows.

## Layout

```text
crates/
  bilive-cli/       # bilive start/stop/status/restart and foreground serve
  bilive-core/      # Bilibili API client, signing, state, danmu, events
  bilive-server/    # axum HTTP/WebSocket routes and static file serving
web/                # No-build static admin UI
packaging/systemd/  # Linux service unit template
```

## Quick Start

Run the service in the foreground from the repository root:

```bash
cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web
```

Then open:

```text
http://127.0.0.1:22333
```

The frontend is served directly from `web/`. Edit `web/index.html`,
`web/styles.css`, or `web/app.js` and refresh the browser.

## CLI Usage

For day-to-day local use, the CLI can manage a background service:

```bash
cargo run -p bilive -- start --listen 127.0.0.1:22333 --web-dir web
cargo run -p bilive -- status
cargo run -p bilive -- restart --listen 127.0.0.1:22333 --web-dir web
cargo run -p bilive -- stop
```

`start` writes `bilive.pid` and `bilive.log` under the state directory unless
`--pid-file` or `--log-file` is provided. `serve` runs the same service in the
foreground and is the command used by the systemd unit.

If you use non-default state paths, pass the same `--state-dir`, `--pid-file`,
or `--log-file` values to `status`, `restart`, and `stop`. If you use a
non-default listen address, pass the same `--listen` value to `status` for the
health check.

## Configuration

Runtime state is stored as JSON. By default, `config.json`, `bilive.pid`, and
`bilive.log` live under the platform state directory. On Linux this is usually:

```text
~/.local/state/bilive
```

Useful overrides:

- `--config` or `BILIVE_CONFIG`: config JSON file path.
- `--listen` or `BILIVE_LISTEN`: service bind address.
- `--web-dir` or `BILIVE_WEB_DIR`: static UI directory.
- `--state-dir` or `BILIVE_STATE_DIR`: state directory for background control,
  and the default base directory for `config.json`.
- `BILIVE_FFMPEG`: `ffmpeg` executable used by the stream test endpoint.

## Development

Useful checks:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
node --check web/app.js
```

The workspace uses Rust 2024 and the Rust version declared in `Cargo.toml`.
The frontend has no npm, Vite, or bundler dependency.

## Service Install

Build the release binary:

```bash
cargo build --release -p bilive
```

Install files with paths matching `packaging/systemd/bilive.service`, for
example:

```text
/usr/local/bin/bilive
/opt/bilive/web
```

Then enable the service:

```bash
sudo cp packaging/systemd/bilive.service /etc/systemd/system/bilive.service
sudo systemctl daemon-reload
sudo systemctl enable --now bilive.service
```

The packaged service listens on `127.0.0.1:22333`, stores state in
`/var/lib/bilive`, and sets conservative systemd sandboxing options.

## Security Notes

Keep the default listener on `127.0.0.1` unless the deployment is intentionally
protected by another access-control layer. Do not log cookies, CSRF tokens,
danmu tokens, or stream keys. API responses should expose only the minimum
state the local UI needs.
