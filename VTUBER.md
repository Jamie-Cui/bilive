<!--
Copyright (C) 2026 Jamie Cui
Author: Jamie Cui
SPDX-License-Identifier: GPL-3.0-or-later
-->

# VTuber 部署、使用与测试指南

bilive 内置一个 VTuber 控制面，可以在网页里配置、启动、停止虚拟形象，并查看运行
状态与日志。**真正的形象渲染由外部的 EasyVtuber 完成，bilive 不内置任何推理逻辑。**

EasyVtuber 项目地址：<https://github.com/yuyuyzl/EasyVtuber>

## 职责分工

| 组件 | 负责 |
|------|------|
| **bilive** | 配置、启动/停止、状态、运行日志、OBS 接入指引（控制面） |
| **EasyVtuber** | 加载模型、面捕输入、生成画面、输出到 Spout2 / 虚拟摄像头 / 调试窗口（推理运行时） |

bilive 直接执行 `python -m src.main ...`（不经过 EasyVtuber 自带的 wxPython 启动器），
并按你在网页里填的参数拼出命令行。所以**部署的关键是先让 EasyVtuber 能独立跑起来**，
之后 bilive 只是替你拼参数、起进程、看日志。

> ⚠️ 平台说明：EasyVtuber 官方只在 **Windows 10/11** 上测试。Linux 上可以跑，但属于
> 非官方用法，需要自己替换少量 Windows 专属依赖（见下文）。本指南以 **Linux** 为主，
> 末尾给出 Windows 要点。
>
> 只想验证 bilive 这一侧（无需 GPU/模型）：直接看文末「测试与验证」。

---

## 第一步：部署 EasyVtuber

### Windows（官方支持，最省事）

上游提供整合包，解压即用，自带 Python 环境和依赖：

- 夸克网盘 / 谷歌网盘 / 磁力链接见 EasyVtuber README 的「整合包版本」。
- 解压后先双击 `01A.启动器.bat` 确认能正常出画面，再回到 bilive 配置。

从源码安装则参考其 README 的 *Installation* 一节（Anaconda + CUDA/TensorRT-RTX +
`pip install -r requirements.txt`）。

### Linux（本仓库目标平台，非官方）

推荐 **NVIDIA 显卡 + CUDA**，这是 Linux 上最可行的组合（AMD/Intel 在 Linux 上没有
DirectML，需要自行折腾 ROCm/onnxruntime，难度更高）。

1. **克隆代码（含子模块）**，`ezvtuber-rt` 是必须的子模块：

   ```bash
   git clone https://github.com/yuyuyzl/EasyVtuber.git
   cd EasyVtuber
   git submodule update --init --recursive
   ```

2. **建 Python 3.10 环境**（conda 或 venv 均可）：

   ```bash
   conda create -y -n easyvtuber python=3.10 && conda activate easyvtuber
   # 或：python3.10 -m venv .venv && source .venv/bin/activate
   ```

3. **安装依赖，并替换 Windows 专属项**。`requirements.txt` 里有两处在 Linux 不适用：

   - `onnxruntime-directml`（DirectML 仅 Windows）→ 换成 `onnxruntime-gpu`
     （NVIDIA CUDA）或退而求其次的 `onnxruntime`（CPU，很慢）。
   - `wxpython`（只给自带启动器用，bilive 不需要）→ Linux 上常常编译困难，可跳过。

   做法是先删掉这两行再安装，缺的单独补：

   ```bash
   grep -viE '^(onnxruntime-directml|wxpython)\b' requirements.txt > /tmp/req.linux.txt
   pip install -r /tmp/req.linux.txt --no-warn-script-location
   pip install onnxruntime-gpu        # NVIDIA；无 N 卡则改用 onnxruntime
   ```

   > TensorRT：上游的 TensorRT-RTX 是 Windows 包。Linux 首次跑**先不要开 TensorRT**，
   > 用 onnxruntime(CUDA) 跑通再说；要加速可后续按 NVIDIA 官方方式装 `tensorrt`。

4. **下载模型**到 `data/models/`：地址见 EasyVtuber README 的「下载模型」一节
   （Google Drive），解压后目录形如 `data/models/<模型文件>`。

5. **准备角色图**：把一张 **512×512、带透明通道的 32 位 PNG** 放到 `data/images/`，
   文件名（不含 `.png`）就是 bilive 里要填的「角色名」。

6. **先独立验证**（这一步通过，bilive 那边基本就稳了）：

   ```bash
   python -m src.main --character <你的角色名> --debug_input --output_debug
   ```

   能弹出 OpenCV 窗口、看到形象、日志无「找不到模型/缺依赖」即可。若报错，先在这里
   解决——bilive 启动失败也是同样的原因。

---

## 第二步：在 bilive 中配置

启动 bilive 并打开网页（默认 <http://127.0.0.1:22333>），进入 **VTuber** 标签页，
填写「EasyVtuber 运行参数」：

| 字段 | 说明 |
|------|------|
| 运行目录 | EasyVtuber 根目录（其下要有 `src/main.py`），如 `/home/you/EasyVtuber` |
| Python | 该环境的解释器，**建议填绝对路径**（venv/conda 的 `python`），避免起错环境 |
| 角色名 | `data/images/` 下的 PNG 名，不含 `.png` |
| 输入 | `鼠标` / `鼠标 + 音频` / `iFacialMocap` / `OpenSeeFace` / `摄像头` / `调试输入` |
| 输入地址 | iFacialMocap、OpenSeeFace 才需要，如 `192.168.1.10:49983`、`127.0.0.1:11573` |
| 鼠标区域 | 鼠标输入的屏幕区域 `x,y,w,h`，默认 `0,0,1920,1080`，多屏/非 1080p 请改 |
| 输出 | **Linux 选「调试窗口」**（见下一步）；Spout2 仅 Windows |
| 模型 / FPS / 补帧 / 超分 / 缓存 / TensorRT / 额外参数 | 与你在 EasyVtuber 里用的取值一致 |

操作：

- **保存设置**：写入配置（持久化为 TOML）。
- **启动形象**：起 EasyVtuber 进程；状态显示「运行中 · PID …」。
- **停止形象**：结束该进程。
- **运行日志**：实时显示 EasyVtuber 的 stdout/stderr（也落盘到
  `~/.cache/bilive/vtuber.log`，每次启动清空）。**起不来先看这里。**

---

## 第三步：在 OBS 中显示

> 背景：EasyVtuber 的 `--output_spout2` 仅 Windows；`--output_virtual_cam` 固定用
> pyvirtualcam `backend='obs'`，Linux 上不可用。所以 **Linux 上开箱即用的输出是
> 「调试窗口」**，靠 OBS 采集那个窗口。

### Linux（推荐，开箱即用）

1. bilive 输出选 **调试窗口**，启动形象 → EasyVtuber 打开标题为
   `EasyVtuber Debug Frame` 的窗口。
2. OBS：来源 → 添加 → **窗口采集 (Xcomposite)** → 选 `EasyVtuber Debug Frame`。
3. 给该来源加 **色度键 / 亮度键** 抠背景，再 **裁剪/变换** 到形象区域。

### Linux 进阶：虚拟摄像头（v4l2loopback）

仅当你用的是支持 v4l2loopback 后端的 EasyVtuber（或自行打了补丁）时才有效：

```bash
sudo modprobe v4l2loopback exclusive_caps=1 card_label="EasyVtuber"
v4l2-ctl --list-devices
```

bilive 输出选 **OBS 虚拟摄像头**，再在 OBS 添加 **视频采集设备 (V4L2)** 选该设备。
若黑屏/无画面，多半是上游 `backend='obs'` 限制，回看运行日志的 pyvirtualcam 报错，
并退回「调试窗口」方案。bilive 的 **OBS 显示** 卡片会列出检测到的 V4L2 设备。

### Windows

- **Spout2（推荐，带透明通道）**：装
  [obs-spout2-plugin](https://github.com/Off-World-Live/obs-spout2-plugin/releases)，
  OBS 添加 **Spout2 Capture**，合成模式设为「预乘 Alpha」。
- **OBS 虚拟摄像头（无透明通道）**：OBS 添加 **视频采集设备**，再用色值/色度键抠掉
  纯色背景。

---

## 输入方式速查

| 模式 | 说明 | 需要 |
|------|------|------|
| 鼠标 | 无摄像头，眼睛跟随鼠标 | 「鼠标区域」 |
| 鼠标 + 音频 | 鼠标 + 用麦克风音量驱动口型、定时眨眼/呼吸 | 麦克风 |
| iFacialMocap | iPhone 结构光面捕，效果最好（App 需购买） | iPhone、同一局域网、`ip:49983` |
| OpenSeeFace | 普通摄像头高精度面捕 | [OpenSeeFace](https://github.com/emilianavt/OpenSeeFace)，`127.0.0.1:11573` |
| 摄像头 | OpenCV 默认摄像头，精度一般 | USB/笔记本摄像头 |
| 调试输入 | 不接面捕，用于联调 | 无 |

## 性能 / 画质调参（概念）

EasyVtuber 用三类模型叠加：**THA**（基础生图，v3/v4/v4_student）、**RIFE 补帧**
（提升帧数、降占用）、**超分**（Anime4K / waifu2x / realesrgan，提升清晰度、增占用）。
配合 **缓存**（RAM/VRAM，命中即跳过 GPU 运算）和 **输入简化/量化**（让相近姿态命中同一
缓存）来平衡画质与占用。bilive 里这些字段与 EasyVtuber 启动器里的取值一一对应，详细
取舍参考其 README 的「性能配置」一节。

---

## 测试与验证

### A. 控制面测试（用桩程序，无需真实 EasyVtuber / GPU）

日常开发最快的验证方式：用一个“桩程序”冒充 EasyVtuber，bilive 完全按真实流程拼命令、
捕获输出、跟踪进程，从而验证命令拼装、日志捕获、状态/退出码、平台校验等逻辑。

1. 创建桩程序（bilive 要求运行目录下有 `src/main.py`）：

   ```bash
   WORK=$(mktemp -d /tmp/easyvtuber-stub-XXXX)
   mkdir -p "$WORK/ok/src"
   cat > "$WORK/ok/src/main.py" <<'PY'
   import sys, time
   print("ARGV:", " ".join(sys.argv), flush=True)
   print("Using OpenCV windows for output display.", flush=True)
   time.sleep(3600)
   PY
   mkdir -p "$WORK/fail/src"
   cat > "$WORK/fail/src/main.py" <<'PY'
   import sys
   print("boom: simulated launch failure", file=sys.stderr, flush=True)
   sys.exit(1)
   PY
   echo "桩目录：$WORK"
   ```

2. 启动 bilive（建议用独立配置/缓存目录，避免污染日常配置）：

   ```bash
   export BILIVE_CONFIG=/tmp/bilive-test/config
   export BILIVE_CACHE_DIR=/tmp/bilive-test/cache   # 日志在 .../cache/vtuber.log
   cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web
   ```

3. 在网页 VTuber 标签页：运行目录填 `$WORK/ok`、Python 填 `python3`、角色名
   `lambda_00`、输入 `鼠标`、鼠标区域 `0,0,1280,720`、输出 `调试窗口`。
   - 点 **启动形象** → 状态变「运行中 · PID …」。
   - **运行日志** 出现 `ARGV: …/ok/src/main.py --character lambda_00
     --mouse_input 0,0,1280,720 --output_debug …` 与 `Using OpenCV windows…`。
   - 点 **停止形象** → 回到「已配置」。
   - 把运行目录改成 `$WORK/fail` 再启动 → 状态显示「已退出 (code 1)」，日志里能看到
     `boom: simulated launch failure`（这就是真实环境缺依赖/模型时你会看到的诊断）。
   - 把输出改成 `Spout2`（Linux/macOS 上标注「仅 Windows」）→ 点启动应被拒绝。

   也可用 curl 脚本化（把 `$WORK` 换成真实路径）：

   ```bash
   B=http://127.0.0.1:22333; PY=$(command -v python3)
   curl -s -X POST $B/api/vtuber/config -H 'content-type: application/json' \
     -d "{\"config\":{\"enabled\":true,\"runtime_dir\":\"$WORK/ok\",\"python\":\"$PY\",\"character\":\"lambda_00\",\"input_mode\":\"mouse\",\"mouse_region\":\"0,0,1280,720\",\"output_mode\":\"debug\"}}" >/dev/null
   curl -s -X POST $B/api/vtuber/start  | python3 -m json.tool   # running / pid
   curl -s        $B/api/vtuber/logs    | python3 -c "import sys,json;print(json.load(sys.stdin)['log'])"
   curl -s        $B/api/vtuber/status  | python3 -m json.tool   # last_exit / diagnostics
   curl -s -X POST $B/api/vtuber/stop   >/dev/null
   ```

4. 清理：`rm -rf "$WORK" /tmp/bilive-test`。

### B. 自动化测试

```bash
cargo test --workspace          # 含 vtuber_command 拼装、mouse_region、spout2 校验、
                                # 日志 tail、patch_config 回写等单元测试
cargo test -p bilive-server vtuber
node --check web/app.js         # 前端语法检查
```

这些测试不需要真实 EasyVtuber，CI 即可运行。

---

## 常见问题

- **点启动没反应 / 状态「已退出」**：先看 **运行日志**（`~/.cache/bilive/vtuber.log`）。
  Python 路径错、缺依赖、模型缺失、面捕地址不通都会在这里报出来。
- **提示找不到 `src/main.py`**：运行目录要指向 EasyVtuber 根目录本身。
- **找不到角色 / 图像报错**：图必须是 512×512、带 Alpha 的 32 位 PNG，放在 `data/images/`。
- **Linux 上 `pip install` 失败**：多半是 `onnxruntime-directml` 或 `wxpython`，按上文
  替换/跳过。
- **Spout2 启动报错**：Linux/macOS 不支持，改用调试窗口或虚拟摄像头。
- **A 卡/I 卡画面扭曲或很慢**：DirectML 仅 Windows；Linux 上建议 NVIDIA + CUDA。
- **改了 `web/` 没生效**：用 `--web-dir web` 跑服务；不带它时前端编译进二进制，需要重新
  `cargo build`。

## 相关链接

- EasyVtuber：<https://github.com/yuyuyzl/EasyVtuber>
- OBS Spout2 插件（Windows）：<https://github.com/Off-World-Live/obs-spout2-plugin/releases>
- OpenSeeFace：<https://github.com/emilianavt/OpenSeeFace>
