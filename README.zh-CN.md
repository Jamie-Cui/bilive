# bilive

[English](README.md) | **简体中文**

`bilive` 是一个本地化的 Bilibili 直播管理服务。它以无界面（headless）的 Rust
服务方式运行，默认监听回环地址，并从 `web/` 目录提供一个静态的浏览器管理界面。

项目主要由以下几部分组成：

- 一个 Rust CLI 与服务端后端。
- 一个桌面弹幕悬浮窗（`bilive-danmu`），将聊天渲染为可穿透点击、始终置顶的窗口，
  原生支持 X11（Linux）与 macOS 后端。
- 一个内嵌的、纯 HTML/CSS/JavaScript 管理界面，无需任何前端构建步骤。
- 一个与服务实现相互独立的 Linux systemd 服务模板。

## 功能特性

- Cookie 登录与 Bilibili App 扫码登录。
- 登录初始化：拉取用户资料、房间号、直播分区与弹幕令牌。
- 直播标题与分区更新。
- 开启/关闭直播、获取推流凭据，并可选地用 `ffmpeg` 测试推流凭据。
- 弹幕连接/断开、WebSocket 事件流推送，以及发送评论。
- 房管、用户禁言、全局禁言、屏蔽词、用户搜索与在线榜管理相关 API。
- 可选的 VTuber 控制标签页：保存 EasyVtuber 启动设置，并从 Web UI 启动/停止外部运行时。
- 面向账号、推流、弹幕、VTuber 与房管工作流的静态 UI 标签页。
- 一个独立的桌面弹幕悬浮窗（`bilive-danmu`），渲染为可穿透点击、始终置顶的窗口。

## 目录结构

```text
crates/
  bilive-cli/       # bilive start/stop/status/restart 及前台 serve
  bilive-core/      # Bilibili API 客户端、签名、状态、弹幕、事件
  bilive-server/    # axum HTTP/WebSocket 路由与静态文件服务
  bilive-danmu/     # 桌面弹幕悬浮窗（原生 X11 / macOS 后端）
web/                # 免构建的静态管理界面
packaging/          # systemd 服务单元模板
```

## 快速开始

在仓库根目录以前台方式运行服务：

```bash
cargo run -p bilive -- serve --listen 127.0.0.1:22333
```

然后打开：

```text
http://127.0.0.1:22333
```

默认情况下，前端由内嵌在二进制中的静态文件提供。在进行 UI 开发时，传入
`--web-dir web` 即可直接从工作目录提供文件，编辑后刷新浏览器即可看到效果。

## CLI 用法

对于日常的本地使用，CLI 可以管理一个后台服务：

```bash
cargo run -p bilive -- start --listen 127.0.0.1:22333
cargo run -p bilive -- status
cargo run -p bilive -- restart --listen 127.0.0.1:22333
cargo run -p bilive -- stop
```

若要在浏览器中打开 Web 仪表盘（如果后台服务尚未运行则先启动它）：

```bash
cargo run -p bilive -- dashboard
```

`dashboard` 是幂等的：当已有服务在运行时直接复用它，否则启动一个并等待健康检查
通过，然后用 `xdg-open`（Linux）或 `open`（macOS）打开 `http://<listen>`。它正是
安装后的 `bilive.desktop` 启动器所调用的命令（见[安装](#安装)）。

`start` 会在状态目录下写入 `bilive.pid` 与 `bilive.log`，除非提供了
`--pid-file` 或 `--log-file`。`serve` 以前台方式运行相同的服务，也是 systemd
单元所使用的命令。

`serve` 子命令在顶层帮助中被隐藏，因为它主要是前台开发、后台子进程以及服务管理器的实现细节。

如果你使用非默认的状态路径，请在 `status`、`restart` 和 `stop` 中传入相同的
`--state-dir`、`--pid-file` 或 `--log-file` 值。如果你使用非默认的监听地址，请在
`status` 中传入相同的 `--listen` 值以便进行健康检查。

## 桌面弹幕悬浮窗

`bilive-danmu` 是一个独立的桌面悬浮窗，它通过 HTTP/WebSocket 连接到正在运行的
`bilive` 服务，并将聊天渲染为可穿透点击、始终置顶的窗口。它原生支持 X11（Linux）
与 macOS 后端。

请先运行服务，然后启动悬浮窗：

```bash
cargo run -p bilive-danmu -- --url http://127.0.0.1:22333 --overlay
```

它订阅 `/api/events`，通过 `/api/danmu/messages` 刷新最近弹幕，并在启动时请求
`/api/danmu/connect`（除非传入 `--no-connect`）。

常用参数：

- `--overlay`：渲染为可穿透点击、始终置顶的悬浮窗，而非普通窗口。
- `--backend auto|x11|macos`：选择窗口后端（默认 `auto`）。
- `--room-id <id>`：覆盖连接弹幕时使用的房间号。
- `--x`、`--y`、`--width`、`--height`、`--height-ratio`：悬浮窗的像素位置与大小
  （当 `--height` 为 `0` 时，高度回退为屏幕的 `--height-ratio` 比例）。
- `--font-family`、`--font-size`、`--max-lines`、`--opacity`：悬浮窗外观。
- `--show-system`：在悬浮窗中包含服务/系统消息。
- `--test-overlay`：不连接服务，显示合成的测试消息。
- `--no-click-through`：保持接收鼠标输入以便调试（仅在 `--overlay` 时生效）。

## VTuber 配置

VTuber 支持是可选的，且默认关闭。如果你从不打开 VTuber 标签页或启动运行时，
bilive 将正常运行，无需 Python、EasyVtuber、GPU 驱动、模型文件、Spout2 或 OBS
虚拟摄像头支持。

请先准备好 EasyVtuber 运行时。它必须能够在不使用 EasyVtuber 的 wxPython
启动器的情况下运行，因为 bilive 会直接启动核心进程：

```bash
python -m src.main
```

在 Web UI 中打开 `VTuber` 标签页并进行配置：

- `运行目录`：包含 `src/main.py` 的 EasyVtuber 项目或解压后的运行时目录，例如
  `/home/jamie/proj/EasyVtuber`。
- `Python`：该环境的解释器，例如 `python`、`python.exe`，或完整的 conda/env 路径。
- `角色名`：EasyVtuber `data/images/` 下的某个 PNG 文件名，不带 `.png`。
- `输入`：`鼠标/音频`、`iFacialMocap`、`OpenSeeFace`、`摄像头` 或 `调试输入`。
- `输入地址`：使用 iFacialMocap 或 OpenSeeFace 时必填，例如
  `192.168.1.10:49983` 或 `127.0.0.1:11573`。
- `输出`：选择 `Spout2`、`OBS 虚拟摄像头` 或 `调试窗口`。
- 模型、FPS、缓存、插值、超分辨率、TensorRT 以及额外参数，都应与你直接传给
  EasyVtuber 时使用的值保持一致。

点击 `保存设置` 持久化 TOML 配置。仅在 EasyVtuber 环境就绪后再点击 `启动形象`。
点击 `停止形象` 终止由 bilive 启动的外部进程。

后端有意不在 Rust 中重写 EasyVtuber。bilive 只负责控制面：配置、状态、启动与停止。
Python/GPU 推理运行时保持外部独立，因为它依赖 PyTorch、ONNX Runtime、DirectML、
TensorRT、OpenCV、Mediapipe、Spout/虚拟摄像头输出以及模型文件。

## 配置

用户可编辑的应用配置以 TOML 格式存储。默认情况下，配置文件位于：

```text
~/.config/bilive/config
```

当设置了 `XDG_CONFIG_HOME` 时，默认配置路径为：

```text
$XDG_CONFIG_HOME/bilive/config
```

Cookie、CSRF 令牌、WBI 密钥、房间令牌、分区元数据与推流凭据属于运行时状态，
而非用户配置。它们被单独存储在平台缓存目录下。在 Linux 上通常为：

```text
~/.cache/bilive/state.json
```

当设置了 `XDG_CACHE_HOME` 时，默认缓存状态路径为：

```text
$XDG_CACHE_HOME/bilive/state.json
```

为兼容性，bilive 仍会从所选配置路径读取此前的 JSON 配置结构，或在未显式提供配置
路径时从状态目录读取旧的默认 `config.json`，随后在未来的保存中改写为 TOML 配置加缓存状态。

后台运行时状态是独立的。`start` 会在平台状态目录下写入 `bilive.pid` 与
`bilive.log`。在 Linux 上通常为：

```text
~/.local/state/bilive
```

常用的覆盖项：

- `--config` 或 `BILIVE_CONFIG`：配置 TOML 文件路径。
- `--cache-dir` 或 `BILIVE_CACHE_DIR`：用于登录与直播运行时状态的缓存目录。
- `--listen` 或 `BILIVE_LISTEN`：服务绑定地址。
- `--web-dir` 或 `BILIVE_WEB_DIR`：用静态 UI 目录覆盖内嵌 UI。
- `--state-dir` 或 `BILIVE_STATE_DIR`：后台控制使用的状态目录。
- `--pid-file` 与 `--log-file`：显式指定后台 pid 与日志文件。
- `--timeout`：后台控制操作中等待健康检查或关闭的秒数。
- `BILIVE_FFMPEG`：推流测试端点使用的 `ffmpeg` 可执行文件。
- `RUST_LOG`：tracing 过滤器；默认在 `info` 级别启用 bilive 各 crate 与
  `tower_http`。

弹幕桌面通知默认关闭。可在 Web UI 的弹幕设置中启用，或在配置文件中设置
`danmu_notifications.enabled`。`danmu_notifications.expire_timeout_ms` 控制
Linux 上请求的显示时长；`0` 表示使用通知守护进程的默认值。在 Linux 上，bilive
会调用 `notify-send`，因此像 Hyprland 这样的 Wayland 合成器需要在用户会话中运行
`mako`、`dunst` 或 `swaync` 等通知守护进程。系统服务通常无法访问桌面会话；如需
桌面通知，请在用户会话中使用 `bilive start`，或使用 systemd 用户服务。

## 开发

常用检查：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
node --check web/app.js
```

该工作区使用 Rust 2024 以及 `Cargo.toml` 中声明的 Rust 版本。前端没有 npm、Vite
或打包器依赖。

## 安装

先构建 release 二进制，然后将它们连同 `bilive.desktop` 启动器一起安装：

```bash
make build
sudo make install
```

这会把 `bilive` 与 `bilive-danmu` 安装到 `$(PREFIX)/bin`（默认 `/usr/local/bin`），
并把一个 `bilive.desktop` 桌面项安装到 `$(PREFIX)/share/applications`。该桌面项
运行 `bilive dashboard`：如果后台服务尚未运行就先启动它，然后在浏览器中打开 Web
仪表盘，因此你可以从 rofi 之类的应用启动器中启动 bilive。

可用 `PREFIX`、`BINDIR` 或 `APPSDIR` 覆盖安装位置，并用 `DESTDIR` 暂存到打包根
目录。使用 `sudo make uninstall` 可再次移除全部内容。

## 服务安装

构建 release 二进制：

```bash
cargo build --release -p bilive
```

安装 release 二进制：

```text
/usr/local/bin/bilive
```

然后启用服务：

```bash
sudo cp packaging/systemd/bilive.service /etc/systemd/system/bilive.service
sudo systemctl daemon-reload
sudo systemctl enable --now bilive.service
```

打包的服务监听 `127.0.0.1:22333`，在 `/var/lib/bilive` 中存储状态，并设置了保守的
systemd 沙箱选项。

## 安全说明

除非部署环境已被另一层访问控制有意保护，否则请将默认监听器保持在 `127.0.0.1`。
不要记录 Cookie、CSRF 令牌、弹幕令牌或推流密钥。API 响应只应暴露本地 UI 所需的最小状态。
