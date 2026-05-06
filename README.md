# bilive

`bilive` is a local Bilibili live management service. It runs as a headless
Rust service, listens on a loopback address by default, and serves a static
browser admin UI from `web/`.

The project has three main pieces:

- A Rust CLI and service backend.
- An embedded, plain HTML/CSS/JavaScript admin UI with no frontend build step.
- Linux systemd and RPM packaging kept separate from the service implementation.

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
packaging/          # systemd and RPM service units
xtask/              # packaging helpers, including cargo build-rpm
```

## Quick Start

Run the service in the foreground from the repository root:

```bash
cargo run -p bilive -- serve --listen 127.0.0.1:22333
```

Then open:

```text
http://127.0.0.1:22333
```

By default the frontend is served from static files embedded in the binary.
During UI development, pass `--web-dir web` to serve files directly from the
working tree and refresh the browser after edits.

## CLI Usage

For day-to-day local use, the CLI can manage a background service:

```bash
cargo run -p bilive -- start --listen 127.0.0.1:22333
cargo run -p bilive -- status
cargo run -p bilive -- restart --listen 127.0.0.1:22333
cargo run -p bilive -- stop
```

`start` writes `bilive.pid` and `bilive.log` under the state directory unless
`--pid-file` or `--log-file` is provided. `serve` runs the same service in the
foreground and is the command used by the systemd unit.

The `serve` subcommand is hidden from top-level help because it is mostly an
implementation detail for foreground development, the background child process,
and service managers.

If you use non-default state paths, pass the same `--state-dir`, `--pid-file`,
or `--log-file` values to `status`, `restart`, and `stop`. If you use a
non-default listen address, pass the same `--listen` value to `status` for the
health check.

## Configuration

The application config is stored as JSON. By default, the config file lives at:

```text
~/.config/bilive/config
```

When `XDG_CONFIG_HOME` is set, the default config path is:

```text
$XDG_CONFIG_HOME/bilive/config
```

For compatibility, if the new default config path does not exist, bilive will
read the previous default `config.json` from the state directory once and write
future saves to the new config path.

Background runtime state is separate. `start` writes `bilive.pid` and
`bilive.log` under the platform state directory. On Linux this is usually:

```text
~/.local/state/bilive
```

Useful overrides:

- `--config` or `BILIVE_CONFIG`: config JSON file path.
- `--listen` or `BILIVE_LISTEN`: service bind address.
- `--web-dir` or `BILIVE_WEB_DIR`: override the embedded UI with a static UI
  directory.
- `--state-dir` or `BILIVE_STATE_DIR`: state directory for background control.
- `--pid-file` and `--log-file`: explicit background pid and log files.
- `--timeout`: seconds to wait for health checks or shutdown during background
  control operations.
- `BILIVE_FFMPEG`: `ffmpeg` executable used by the stream test endpoint.
- `RUST_LOG`: tracing filter; the default enables bilive crates and
  `tower_http` at `info` level.

Danmu desktop notifications are off by default. Enable them from the danmu
settings in the web UI, or set `danmu_notifications.enabled` in the config
file. `danmu_notifications.expire_timeout_ms` controls the requested display
duration on Linux; `0` uses the notification daemon default. On Linux, bilive
calls `notify-send`, so Wayland compositors such as Hyprland need a notification
daemon like `mako`, `dunst`, or `swaync` running in the user session. System
services usually cannot reach the desktop session; use `bilive start` from the
user session or a systemd user service for desktop notifications.

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

For RPM packaging, the repository defines a Cargo alias:

```bash
cargo build-rpm
```

## Service Install

Build the release binary:

```bash
cargo build --release -p bilive
```

Install the release binary:

```text
/usr/local/bin/bilive
```

Then enable the service:

```bash
sudo cp packaging/systemd/bilive.service /etc/systemd/system/bilive.service
sudo systemctl daemon-reload
sudo systemctl enable --now bilive.service
```

The packaged service listens on `127.0.0.1:22333`, stores state in
`/var/lib/bilive`, and sets conservative systemd sandboxing options.

## RPM Package

Install the RPM packaging helper once:

```bash
cargo install cargo-generate-rpm
```

Then build the release binary and generate the RPM from the workspace root:

```bash
cargo build-rpm
```

The RPM is written under `target/generate-rpm/`. It installs `bilive` to
`/usr/bin/bilive`, installs the systemd unit to
`/usr/lib/systemd/system/bilive.service`, and uses the embedded web UI.

Install and start it with:

```bash
sudo dnf install ./target/generate-rpm/bilive-*.rpm
sudo systemctl enable --now bilive.service
```

## Security Notes

Keep the default listener on `127.0.0.1` unless the deployment is intentionally
protected by another access-control layer. Do not log cookies, CSRF tokens,
danmu tokens, or stream keys. API responses should expose only the minimum
state the local UI needs.
