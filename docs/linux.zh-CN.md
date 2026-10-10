# Linux

Folio 使用原生 X11 和 Wayland 窗口，并通过 Unix PTY 运行终端会话。当前发布构建目标为
`x86_64-unknown-linux-gnu`。

## 当前支持范围

- 已在真实 X11 和 Wayland 会话中检查终端启动、窗口呈现和 PTY 输出。
- Linux 窗口使用 Folio 自带的标题栏和窗口按钮。拖动标题栏移动窗口，拖动边缘调整大小。
- 剪贴板读取在后台异步执行，并有数据上限。X11 读取支持本地 UNIX 显示套接字，或
  `DISPLAY` 中直接填写的 IP 地址；暂不支持远程显示主机名。剪贴板所有权和文本写入使用当前
  窗口选择的原生后端。
- Wayland 剪贴板操作需要合成器支持 `ext-data-control` 或 `wlr-data-control` 1 及以上版本。
  缺少这两种协议时，剪贴板操作会报告错误；终端仍可启动。
- 目录监视的启动和退役在后台执行。桌面打开/定位、文件选择器、通知和视频海报帧提取
  以及回收站操作也不会阻塞窗口线程。Folio 最终退出最多等待 3 秒让桌面清理结束，复用
  会话保存的时间预算。到期仍未结束时，Folio 会记录诊断信息并继续退出。清理线程会关闭回收站队列，
  不再接收新请求，并让已接受的请求排空。Folio 不会为赶上期限而取消这些请求，因此进程退出时，
  操作可能仍未完成。关闭发起操作的窗口不会取消文件系统操作。
- 网页预览需要支持加载未打包 Manifest V3（MV3）扩展的完整 Chromium。Folio 不附带
  Chromium。它先检查 `FOLIO_CHROMIUM_PATH`，再从 `PATH` 查找 `chromium`、
  `chromium-browser`、`google-chrome-stable` 或 `google-chrome`。找到同名可执行文件
  只代表候选；Folio 会先确认私有请求策略扩展已加载，再安装网页。有些 Google Chrome
  版本会禁用未打包扩展；这时启动会报告所选浏览器未加载 Folio 私有 MV3 策略扩展，并要求
  改用支持未打包 MV3 扩展的完整 Chromium。Chrome for Testing 的 Full Chrome
  154.0.8037.92 用于本地引擎验证；Folio 不会打包或安装它。如果 Chromium 不在 `PATH`，
  请将 `FOLIO_CHROMIUM_PATH` 设为其完整可执行文件路径。
- 桌面通知通过会话 D-Bus 的 `org.freedesktop.Notifications` 接口发送。通知服务支持操作时，
  点击通知会打开对应的 Folio 目标。缺少通知服务或请求失败时，Folio 异步报告错误。
- 本地视频会在 Folio 的视频窗格中播放。GStreamer（Linux 上的音视频库）负责解码文件。
  Folio 显示解码后的画面，并通过 `autoaudiosink` 输出音频。可播放的音视频格式取决于系统安装的
  GStreamer 插件。
- 视频播放栏支持播放/暂停、时间轴定位、音量、静音和四种速度：1 倍、1.25 倍、1.5 倍和 2 倍。
  按空格键播放或暂停，按 `←` 或 `→` 每次定位五秒，按 `↑` 或 `↓` 调节音量，按 `M` 切换静音。
  Folio 无法打开或解码视频时会显示错误。
- 安装 `ffprobe` 和 `ffmpeg` 后，本地视频还可以提取一张海报帧。

X11 可以提供全局指针、窗口、显示器和工作区坐标。Wayland 不提供这些全局坐标。选择全局
坐标所在的显示器或按绝对位置摆放窗口等操作，可能会报告合成器没有提供所需能力。Wayland
启动和终端使用不依赖这些操作。

| 操作 | X11 | Wayland |
| --- | --- | --- |
| 最小化 | 请求窗口管理器执行 | 请求合成器执行 |
| 从 Folio 恢复最小化窗口 | 支持 | winit 不提供此操作；通过合成器恢复 |
| 请求焦点 | 请求窗口管理器执行 | 没有激活令牌时拒绝；已获得焦点的窗口无需请求 |
| 按桌面坐标定位窗口 | 支持 | 不提供 |
| 全局快捷键和下拉终端呼出 | 支持 | 报告缺少全局快捷键或定位能力 |

窗口管理器和合成器决定是否接受请求。

## 运行环境

请在 X11 或 Wayland 图形会话中运行 Folio。Wayland 需要有效的 `WAYLAND_DISPLAY` 和
`XDG_RUNTIME_DIR`；X11 需要有效的 `DISPLAY`。Folio 使用 wgpu 的 Vulkan 和 OpenGL ES 后端，
因此系统需要可用的 Vulkan loader 和驱动，或 EGL/OpenGL ES 实现。二进制针对 GNU/Linux
动态链接。

原生键盘栈会加载 `libxkbcommon.so.0`；X11 还会使用 `libxkbcommon-x11.so.0`。Wayland
后端在运行时动态加载 `libwayland-client.so.0`。Vulkan 后端会加载 `libvulkan.so.1`；OpenGL ES
后端需要 `libEGL.so.1` 和系统 GLES 驱动。不同发行版的软件包名称会不同；系统必须提供当前显示
后端和图形驱动所需的库。构建和安装脚本不会打包桌面服务或 GPU 驱动。

下列命令行辅助程序只供对应功能使用：

| 辅助程序 | 功能 |
| --- | --- |
| `/usr/bin/env` | 桌面入口安全地传入已安装程序路径 |
| `gio` | 打开桌面文件并将文件移入回收站 |
| `xdg-open` | `gio` 不存在时，用作打开路径的后备程序 |
| `gdbus` | 请求支持 FileManager1 的文件管理器定位路径；请求不可用时，Folio 可以打开父目录 |
| `zenity` 或 `kdialog` | 文件和文件夹选择器；Folio 按此顺序尝试 |
| `ffprobe` 和 `ffmpeg` | 提取一张本地视频海报帧 |

缺少某个辅助程序只会影响对应功能，操作会报告错误；它们不是启动终端的必要条件。

### 视频播放所需的运行库

开始播放时，Folio 会加载以下 GStreamer 1.x 库：
`libgstreamer-1.0.so.0`、`libgstapp-1.0.so.0`、`libgobject-2.0.so.0` 和
`libglib-2.0.so.0`。系统还需要提供 `playbin`、`appsink` 和 `autoaudiosink` 插件，
以及支持该视频格式的 GStreamer 插件。不同发行版的软件包名称会不同。构建 Folio 不需要
GStreamer 开发头文件。缺少运行库或插件时，视频无法播放，但终端仍可启动。

## 下载 Linux 工作流构建

Linux 工作流会把 `.tar.gz` 作为 GitHub Actions workflow artifact 保存，不会把它发布为 GitHub
Release 资源。归档会保留可执行权限，并包含暂存的构建文件、安装和卸载脚本、这两份 Linux 指南、
`BUILD-INFO.txt` 和 `SHA256SUMS`。

从对应的 workflow run 下载 artifact，先校验归档，再解压并校验内容：

```sh
gh run download <run-id> --name folio-<version>-linux-x86_64
sha256sum -c folio-<version>-linux-x86_64.tar.gz.sha256
tar -xzf folio-<version>-linux-x86_64.tar.gz
cd folio-<version>-linux-x86_64
sha256sum -c SHA256SUMS
./scripts/release/install-linux.sh --from "$PWD"
```

卸载时，在解压后的目录运行 `./scripts/release/uninstall-linux.sh`。每个发布压缩包中的
`BUILD-INFO.txt` 会记录 runner 镜像、构建主机的 glibc 版本，以及该二进制需要的最高 GLIBC
符号版本。Ubuntu 构建产物需要 GLIBC 2.39；本地 Fedora 构建需要 GLIBC 2.43。这些版本仅适用于
对应构建产物；Folio 不声明适用于所有 Linux 发行版的统一最低版本。

## 系统外观

主题设为“系统”时，Folio 会从 XDG Settings 门户读取桌面的
`org.freedesktop.appearance/color-scheme` 值。窗口打开期间，Folio 会跟随该值的变更通知。
门户、键或值不可用时，Folio 使用现有的深色后备主题。Folio 不会根据桌面名称或 GTK 设置推测主题。

## 构建

构建脚本从 `rust-toolchain.toml` 读取固定的编译器版本，使用 Linux 主机工具链，覆盖仅供
Windows 使用的静态 CRT 设置，并从 `Cargo.lock` 锁定依赖构建：

```sh
scripts/release/build-linux.sh --out target/linux-release
```

脚本当前仅支持 x86_64 Linux。它会把可执行文件、桌面入口、图标、MIT 和 Apache 许可证、第三方
声明及商标说明放入 `--out` 指定的目录。脚本不会安装系统软件包，也不会写入用户的 XDG 目录。

## 为当前用户安装

构建完成后安装已暂存的文件：

```sh
scripts/release/install-linux.sh
```

默认位置如下：

| 文件 | 位置 |
| --- | --- |
| 可执行文件 | `~/.local/bin/folio` |
| 桌面入口 | `$XDG_DATA_HOME/applications/io.github.lulu-loopp.folio.desktop` |
| 图标 | `$XDG_DATA_HOME/icons/hicolor/512x512@2/apps/io.github.lulu-loopp.folio.png` |
| 声明文件 | `$XDG_DATA_HOME/doc/folio/` |

安装时，未设置、为空或为相对路径的 `XDG_DATA_HOME` 均使用 `~/.local/share`。桌面入口只启动 Folio，不传入文件或
URL 参数。命令行最多接收一个路径参数（文件或文件夹）：

```text
folio [--cwd <folder>] [--profile <id>] [--new-window | --tab] [--] [<path>]
folio --help | --version
```

桌面入口通过 `env` 传入已安装程序的路径；该路径不能包含 `=` 或控制字符。安装脚本会在写入
文件前检查这一点。空格、`$`、引号、反引号、反斜杠和 `%` 均可使用。

测试临时本地前缀时，安装和卸载使用相同的 `--prefix`。此参数不会重定向应用的设置或会话数据。

```sh
scripts/release/install-linux.sh \
    --from target/linux-release \
    --prefix /tmp/folio-test-prefix
scripts/release/uninstall-linux.sh --prefix /tmp/folio-test-prefix
```

安装和卸载脚本都不使用 `sudo`。卸载只移除 Folio 的可执行文件、桌面入口、图标和四个随包声明，
会保留 `$XDG_DATA_HOME/Folio`、shell 文件及其他用户数据。

## 设置和数据目录

| 内容 | 位置 |
| --- | --- |
| 设置和快捷键 | `$XDG_CONFIG_HOME/Folio/<data-tag>/` |
| 会话、配置档案和其他持久数据 | `$XDG_DATA_HOME/Folio/` |
| 网页配置数据 | `$XDG_DATA_HOME/Folio/Chromium/` |
| 网页缓存 | `$XDG_CACHE_HOME/Folio/<data-tag>/Chromium/` |
| 网页临时文件 | 私有的 `$XDG_RUNTIME_DIR/Folio/`，或 Folio 的用户临时目录 |

未设置 config、data 和 cache 变量时，分别使用 `~/.config`、`~/.local/share` 和 `~/.cache`。
`<data-tag>` 复用数据目录的实例标识，不同数据目录各自保存设置。实例锁和本地 IPC 继续使用
原有的私有用户临时目录。

应用保留原有的数据目录规则：`XDG_DATA_HOME` 为空或为相对路径时，数据位于启动目录下。
更改此规则前，需要[保证旧数据可读的迁移方案](plans/design/linux-xdg-directories.md)。
配置、缓存和网页临时目录会忽略相对路径的 XDG 覆盖值。

新设置或快捷键文件不存在时，Folio 会读取数据目录中的旧文件。后台线程复制旧文件，不覆盖
已有的新文件，也不删除原文件。更新试运行会等到允许写入后才迁移。普通卸载保留这两处文件；
明确执行应用数据清除时，只清除所选数据目录对应的命名空间。

## 显示 smoke 检查

smoke 脚本使用真实的 X11 或 Wayland 会话，在 PTY 中启动 shell，并检查终端尺寸非零且文本已经呈现。
输出文件写入 `target/linux-smoke`；它不检查键盘输入、IME 或桌面集成。

```sh
python3 scripts/ci/linux-smoke.py wayland
python3 scripts/ci/linux-smoke.py x11
```

Wayland 运行需要当前会话的 `WAYLAND_DISPLAY` 和 `XDG_RUNTIME_DIR`；X11 运行需要 `DISPLAY`。
