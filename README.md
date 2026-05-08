# bilive

`bilive` is a local Bilibili live management service. It runs as a headless
Rust service, listens on a loopback address by default, and serves a static
browser admin UI from `web/`.

The project has three main pieces:

- A Rust CLI and service backend.
- A terminal danmu viewer that connects to the local service.
- An embedded, plain HTML/CSS/JavaScript admin UI with no frontend build step.
- A Linux systemd service template kept separate from the service
  implementation.

## Features

- Cookie login and Bilibili app QR-code login.
- Login bootstrap for user profile, room id, live areas, and danmu token.
- Live title and area updates.
- Start and stop live streams, capture stream credentials, and optionally test
  a stream credential with `ffmpeg`.
- Danmu connect/disconnect, WebSocket event streaming, and comment sending.
- Room admin, user silent, global silent, blocked word, user search, and online
  rank management APIs.
- Optional VTuber control tab that saves EasyVtuber launch settings and
  starts/stops the external runtime from the web UI.
- Static UI tabs for account, stream, danmu, VTuber, and manager workflows.

## Layout

```text
crates/
  bilive-cli/       # bilive start/stop/status/restart and foreground serve
  bilive-core/      # Bilibili API client, signing, state, danmu, events
  bilive-server/    # axum HTTP/WebSocket routes and static file serving
  bilive-danmu/     # terminal danmu viewer for a running bilive service
web/                # No-build static admin UI
packaging/          # systemd service unit template
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

## TUI Usage

Run the service first, then open the terminal danmu viewer:

```bash
cargo run -p bilive-danmu -- --url http://127.0.0.1:22333
```

The TUI subscribes to `/api/events`, refreshes recent danmu through
`/api/danmu/messages`, and requests `/api/danmu/connect` on startup unless
`--no-connect` is passed. Use `r` to refresh history, `k`/`j` or arrow keys to
scroll, `u`/`d` to page, `g`/`G` for top/bottom, and `q` to quit.

## VTuber Setup

VTuber support is optional and disabled by default. If you never open the
VTuber tab or start the runtime, bilive runs normally without Python,
EasyVtuber, GPU drivers, model files, Spout2, or OBS virtual camera support.

Prepare an EasyVtuber runtime first. It must be runnable without the
EasyVtuber wxPython launcher, because bilive starts the core process directly:

```bash
python -m src.main
```

In the web UI, open the `VTuber` tab and configure:

- `运行目录`: the EasyVtuber project or unpacked runtime directory containing
  `src/main.py`, for example `/home/jamie/proj/EasyVtuber`.
- `Python`: the interpreter for that environment, for example `python`,
  `python.exe`, or a full conda/env path.
- `角色名`: a PNG name under EasyVtuber `data/images/`, without `.png`.
- `输入`: `鼠标/音频`, `iFacialMocap`, `OpenSeeFace`, `摄像头`, or `调试输入`.
- `输入地址`: required for iFacialMocap or OpenSeeFace, such as
  `192.168.1.10:49983` or `127.0.0.1:11573`.
- `输出`: choose `Spout2`, `OBS 虚拟摄像头`, or `调试窗口`.
- Model, FPS, cache, interpolation, super-resolution, TensorRT, and extra args
  should match the same values you would pass to EasyVtuber.

Click `保存设置` to persist the TOML config. Click `启动形象` only when the
EasyVtuber environment is ready. Click `停止形象` to terminate the external
process started by bilive.

The backend intentionally does not rewrite EasyVtuber in Rust. bilive owns the
control plane: config, status, start, and stop. The Python/GPU inference
runtime remains external because it depends on PyTorch, ONNX Runtime, DirectML,
TensorRT, OpenCV, Mediapipe, Spout/virtual camera output, and model artifacts.

## Configuration

The user-editable application config is stored as TOML. By default, the config
file lives at:

```text
~/.config/bilive/config
```

When `XDG_CONFIG_HOME` is set, the default config path is:

```text
$XDG_CONFIG_HOME/bilive/config
```

Cookies, CSRF tokens, WBI keys, room tokens, area metadata, and stream
credentials are runtime state, not user config. They are stored separately under
the platform cache directory. On Linux this is usually:

```text
~/.cache/bilive/state.json
```

When `XDG_CACHE_HOME` is set, the default cache state path is:

```text
$XDG_CACHE_HOME/bilive/state.json
```

For compatibility, bilive still reads the previous JSON config shape from the
selected config path, or the old default `config.json` from the state directory
when no explicit config path is provided, then rewrites future saves as TOML
config plus cache state.

Background runtime state is separate. `start` writes `bilive.pid` and
`bilive.log` under the platform state directory. On Linux this is usually:

```text
~/.local/state/bilive
```

Useful overrides:

- `--config` or `BILIVE_CONFIG`: config TOML file path.
- `--cache-dir` or `BILIVE_CACHE_DIR`: cache directory for login and live
  runtime state.
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

## Security Notes

Keep the default listener on `127.0.0.1` unless the deployment is intentionally
protected by another access-control layer. Do not log cookies, CSRF tokens,
danmu tokens, or stream keys. API responses should expose only the minimum
state the local UI needs.
