# bilive

`bilive` is a local Bilibili live management service. It runs as a regular
headless service and exposes a browser-based admin UI on a loopback address.

The project is replacing the original Tauri desktop shape with:

- Rust service backend
- Static web admin frontend with plain HTML/CSS/JavaScript
- WebSocket event stream for live danmu events
- Linux systemd packaging, with the core binary kept portable

## Current Status

This repository currently contains a functional local service:

- `GET /api/health`
- `GET /api/events` WebSocket event stream
- Cookie and QR-code login
- Login bootstrap for user info, room id, area list, and danmu token
- Live title/area update, start/stop, stream credential capture
- Danmu connect/disconnect, danmu event stream, comment sending
- Room admins, silent users, global silent mode, blocked words, and online rank APIs
- Static no-build web UI served from `web`

The service stores local state in JSON. By default it uses a platform state
directory; override with `--config` or `BILIVE_STATE_DIR`.

## Layout

```text
crates/
  bilive-core/      # Bilibili API client, signing, state, protocols, events
  bilive-server/    # axum HTTP/WebSocket server
  bilive-cli/       # bilive command-line entry point
web/                # No-build static admin UI
packaging/systemd/  # Linux service unit template
```

## Development

Run the service from the repository root:

```bash
cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web
```

Then open:

```text
http://127.0.0.1:22333
```

The frontend has no npm dependency or build step. Edit files under `web/` and
refresh the browser.

Useful checks:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
node --check web/app.js
```

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

The default service listens on `127.0.0.1:22333`.
