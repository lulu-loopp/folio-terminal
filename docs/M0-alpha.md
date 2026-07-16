# M0-alpha：可运行 shell 的原生窗口

日期：2026-07-16

## 结论

**go-with-caveat**。代码路径、真实 ConPTY 行为测试、全 workspace 门禁和原生窗口启动/显示均通过；仍需人工按本文末尾清单从真实 winit 键盘事件执行一次输入、拖拽 resize 和标题栏关闭验收。该人工项不阻塞代码审阅，但在完成前不把“人已实际跑过全部交互”写成 PASS。

本次只完成 M0 第一片：单窗口、单 session、单 pane、默认 PowerShell、ASCII 输入和纯文本 GPU 绘制。没有进入 IME、CJK 宽度裁决、滚动回溯 UI、富内容、标签、分屏或 M1。

## crate 交付与行为证据

### `bt-pty`

- 用 `portable-pty` 创建真实 ConPTY，拥有 shell、master、writer 和 reader thread 的完整生命周期。
- 每 session 的 PTY→Term 缓冲按字节精确限制为 1 MiB；写满后 reader 阻塞，不丢字节。
- Term 每次最多取 256 KiB，保持 DESIGN.md §1.3 actor quantum。
- resize 同时携带 cell 和像素尺寸；shutdown 先回收子进程，再关闭 master 并 join reader，避免僵尸和读线程死锁。

行为测试：

- `full_ring_blocks_writer_until_term_drains_bytes`：容量为 4 的反向探针确实阻塞 producer，消费后才恢复；最大占用等于容量。
- `real_conpty_delivers_command_output_without_exceeding_ring`：真实 `cmd.exe` 经 ConPTY 输出并完成 DSR 握手，且水位不超过 1 MiB。
- `resize_reaches_the_real_conpty_and_shutdown_reaps_child`：真实 PowerShell 从 40×8 改为 96×31，backend 回读一致，shutdown 后 child id 消失。

### `bt-render`

- wgpu DX12 surface + cosmic-text shaping + glyphon atlas；按 viewport frame 绘制等宽 cell、基本 SGR 前景/背景和光标。
- 字体/DPI 实测得到 cell metrics，并把 cell height 以定点 subpixel 注入 `DualPlaneSession`。
- 公共 API 不依赖 winit；窗口只通过 wgpu surface target 进入构造阶段。
- `FrameTrigger` / `PresentReceipt` 保留事件→submit→present-call 的可替换时序边界；`LatestFrameSlot` 合并未呈现快照而不排无界队列。

行为测试：

- `grid_dimensions_are_nonzero_and_derived_from_metrics`：像素尺寸按实际 cell metrics 导出网格。
- `latest_frame_slot_overwrites_instead_of_queueing`：待呈现帧只保留最新版本。
- `timing_gate_rejects_inverted_boundaries`：时序边界可验证且错误时间戳会失败。
- `sgr_palette_and_inverse_are_resolved_before_rendering`：基础颜色与 inverse 映射可观察。

### `bt-app`

- winit `ApplicationHandler` 串起 Keyboard→PTY、PTY→`DualPlaneSession`→`ViewportProjection`→renderer，以及 window resize→ConPTY→Term→projection。
- ASCII、Enter、Backspace、Tab、Escape、Ctrl+C 有明确字节映射；非 ASCII 和本阶段未实现的输入路径不发送。
- alacritty 生成的 DSR 等 `PtyWrite` 回复回到唯一 PTY writer；没有字节嗅探器。
- 关闭窗口、shell 自行退出和错误退出都进入同一 shutdown 路径。
- 产品构造显式注入字体高度、staging quota 和 M0 frozen-line quota，不让 app 隐式继承 spike 高度/配额。

行为测试：

- `keyboard_mapping_is_ascii_only_and_preserves_terminal_controls`：验证 ASCII、Enter、Backspace、Ctrl+C，并证明中文不会误入本阶段路径。
- `pty_pixel_size_is_clamped_to_backend_width`：系统边界的像素尺寸不会溢出 ConPTY 的 u16 字段。
- `real_powershell_input_reaches_a_viewport_owned_frame`：真实交互式 PowerShell 完成协议握手，输入命令，输出标记依次经过 ConPTY、Term 和 viewport-owned frame；测试不手工构造中间网格。

### 既有 crate 的接线

- `bt-term` 捕获稳定光标状态与 terminal-generated PTY replies，并提供 `viewport_frame`；renderer 不读取 alacritty grid。
- `bt-viewport` 新增稳定 `ViewportFrame`，严格检查行列数后才展平 `CapturedCell`；畸形投影返回错误。
- `DualPlaneSession::with_quotas_and_cell_height` 是 M0 产品构造入口；原 spike 构造器仅保留给既有逻辑测试。

对应测试：`byte_driven_terminal_state_projects_through_the_viewport_frame_boundary` 从 SGR 字节验证到稳定 frame 的文本、颜色和光标；`live_frame_flattens_only_well_formed_viewport_rows` 验证 projection 边界及失败路径。

## 携带项

- `g1_resize_shrink_discards_blank_rows_below_the_cursor` 从 VT 字节建立 8×4 网格，缩到 8×2，断言 cursor 下方空白不进入 history 且可见内容正确。
- `docs/spikes/00-baseline.md` 已增加指向 `00b-baseline-rework.md` 的 supersession 头。

## 实测与门禁

- `cargo test --workspace --locked`：通过；259 tests（第一方 79，vendor 180），0 failed / 0 ignored。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 第一方 crates 的 `cargo fmt --check`：通过。
- `cargo metadata --no-deps --locked --format-version 1`：10 个 workspace members；`alacritty_terminal 0.26.0` 的 manifest 位于根 workspace 的 `vendor/alacritty_terminal`。
- 原生窗口实测：`cargo run -p bt-app --locked` 成功创建 `BetterTerminal M0-alpha`，真实 PowerShell prompt、等宽文本、深色背景与光标均可见。启动过程中发现并修复了 DSR 未回写会卡住 shell，以及关闭 master 时序不当会使 reader join 死锁两项真实集成问题。
- `glyphon 0.12` / `cosmic-text 0.19` 的最低 Rust 版本为 1.89，因此 workspace 声明的 `rust-version` 从已失真的 1.85 修正为 1.89；实际工具链仍由 `rust-toolchain.toml` 固定为 1.94.1。

## 人工验收步骤

1. 在仓库根目录运行 `cargo run -p bt-app --locked`，确认出现 `BetterTerminal M0-alpha` 与 PowerShell prompt。
2. 输入 `cargo --version`、回车，再输入 `dir`、回车；确认输入回显和命令输出均落在等宽网格中。
3. 输入一个拼错的 ASCII 单词，用 Backspace 修正后执行，确认退格只删除一个 ASCII cell。
4. 运行 `ping 127.0.0.1 -t`，按 Ctrl+C，确认连续输出停止并回到 prompt。
5. 分别横向、纵向和拖动窗口角改变尺寸；确认网格跟随、PowerShell 收到新 cell 尺寸、变高不回填旧空白 history。
6. 用标题栏关闭窗口，在任务管理器中确认对应 PowerShell 与 headless conhost 均已退出。

## 遗留风险

- 尚未完成人工 winit 键盘/拖拽/标题栏关闭清单，因此结论保留 caveat。
- 本片明确不保证 IME、CJK/emoji cell 宽度、复杂文本装饰或滚动回溯 UI。
- M0 frozen-line quota 当前是 app 显式拥有的 100,000 行固定值；后续配置切片可公开它，但转录层已经是唯一执行配额的权威。

## 偏离申请

无。
