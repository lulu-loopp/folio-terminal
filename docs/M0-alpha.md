# M0-alpha：可运行 shell 的原生窗口

日期：2026-07-16

## 结论

**go**。五轮返工全部收口，代码路径、真实 ConPTY 行为测试、全 workspace 门禁和人工验收清单全项通过。resize 四角同机同操作对照 Windows Terminal 后，BetterTerminal 的表现与 WT 同级，满足第四轮返工单的人工横杆。

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

1. 测启动速度时先执行 `cargo build -p bt-app --release --locked`，再直接运行 `target\release\bt-app.exe`；不要使用 `cargo run` 计时，因为 Cargo 的新鲜度检查发生在应用进程启动前，不属于应用启动耗时。确认出现 `BetterTerminal M0-alpha` 与 PowerShell prompt。
2. 需要可见计时时，在 PowerShell 中执行 `$env:BT_STARTUP_TRACE=1; .\target\release\bt-app.exe`；首文本 present 后标题应变为 `BetterTerminal M0-alpha — bg …ms · text …ms`。
3. 输入 `cargo --version`、回车，再输入 `dir`、回车；确认输入回显和命令输出均落在等宽网格中。
4. 输入一个拼错的 ASCII 单词，用 Backspace 修正后执行，确认退格只删除一个 ASCII cell。
5. 运行 `ping 127.0.0.1 -t`，按 Ctrl+C，确认连续输出停止并回到 prompt。
6. BetterTerminal 与 Windows Terminal 在同一台机器上分别快速拖大、慢速拖大、斜拉窗口角；确认网格跟随、PowerShell 收到新 cell 尺寸、变高不回填旧空白 history。重点观察新暴露区是否出现持续黑/白填充；BetterTerminal 应不差于 Windows Terminal，WT 若也只有轻微瞬时伪影则同级即可。
7. 用标题栏关闭窗口，在任务管理器中确认对应 PowerShell 与 headless conhost 均已退出。

## 遗留风险

- resize 四角同机对照 Windows Terminal 已人工 PASS：右下角拖拽零伪影；右上、左下、左上三角在新暴露区仍有瞬时黑块与旧帧轻微拉伸，方向与窗口原点移动一致，但与 WT 同机同操作表现同级。
- 正常单屏尺寸内的拖大不再调用 `ResizeBuffers`；若窗口跨屏或超过预分配的最大单显示器宽/高，renderer 会按 1.5 倍增长 swapchain，并退回“末刻 configure + 同步主题 present”。该越界增长点仍可能留下一个 DWM 合成周期的瞬时伪影；是否与 WT 同级必须由人工观察，不宣称物理上的零伪影。
- 为避免正常拖拽期间重建 swapchain，第四轮以最大单显示器宽/高预分配 back buffer；按 4K（3840×2160、4 bytes/pixel）计算约为 33.2 MB/buffer，FLIP 双缓冲合计约 66 MB。多显示器或高分辨率机器会增加显存占用，但不按整个虚拟桌面总宽预分配，以免横向多屏造成无界浪费。
- 本片明确不保证 IME、CJK/emoji cell 宽度、复杂文本装饰或滚动回溯 UI。
- Windows M0 ASCII-only 路径有意只加载 4 个 Consolas 文件；CJK/IME 片必须在渲染任何非 ASCII PTY 输出之前恢复字体回退，否则会静默显示豆腐块。
- M0 frozen-line quota 当前是 app 显式拥有的 100,000 行固定值；后续配置切片可公开它，但转录层已经是唯一执行配额的权威。
- **resize 品质的后续路线**：若要超越 Windows Terminal，需要 ① wgpu 上游开放 `DXGI_SCALING_NONE`（已评估 fork `wgpu-hal` 不划算）；② `SCALING_NONE` 配合缓冲区外围预填充 overdraw，拖大时直接揭开现成内容；③ 对左/上边缘窗口原点移动实施 DirectComposition 视觉偏移补偿。该工作归属 latency 打磨片，已在 PROBLEM-LIST 登记为 P2-13。

## 偏离申请

无。

## 返工记录

基线：`68440cc`。本节只覆盖 M0-alpha 返工，不进入 IME、CJK/emoji 宽度正确性或滚动回溯 UI。

### 第二轮人工验收结果（第三轮返工前）

- 空格输入：**PASS**。
- 宽行对格与光标贴字：**PASS**；在两种窗口尺寸/缩放下附图确认，上一轮漂移修复有效。
- Ctrl+C 中断：**PASS**。
- 标题栏关闭后的进程回收：**PASS**；对应 PowerShell 正确退出。

### must-fix

1. **空格与 Named 键审计**：`keyboard_bytes` 新增 `NamedKey::Space -> b' '`，且 Ctrl+Space 不误发。审计 winit 的 `NamedKey` 后，本片会产生文本/终端控制字节的命名键为 Space、Enter、Backspace、Tab、Escape；其余字符键由 `Key::Character` 承担。`keyboard_mapping_is_ascii_only_and_preserves_terminal_controls` 现在显式覆盖空格、Ctrl+C、Ctrl+非 C、非 ASCII。
2. **启动关键路径**：保留 `BT_STARTUP_TRACE=1` 的分段打点。Windows M0 ASCII 路径只加载 Consolas regular/bold/italic/bold-italic 四个文件，不再让 `FontSystem::new()` 扫描全部系统字体；窗口在首个 swapchain 主题 clear 前保持隐藏，clear 成功（或 hidden surface 返回 Occluded 后在显示时同步重试）才显示；首文本 present 前每 10 ms 主动 drain PTY，首文本后恢复事件驱动 `Wait`，消除首个 wake 的盲区。6 轮 release 启动打点手工实测如下（同机、同目录、逐轮启动真实 PowerShell）：
   - 返工前：字体扫描均值 54.3 ms，字体度量 37.3 ms，renderer 788.8 ms，runtime ready 894.0 ms；只有 5/6 轮在 6 秒内出现首文本，成功轮均值 1415.8 ms。
   - 返工后：字体加载均值 0.0 ms，字体度量 1.0 ms，renderer 554.5 ms，runtime ready 612.7 ms；主题背景可见均值 681.7 ms，6/6 轮首文本均成功，范围 1046–1163 ms、均值 1088.7 ms。
   - 本机成功轮没有复现审核机的“超过 3 秒”，但返工前确实复现了 1/6 首文本超过 6 秒仍未出现；返工后连续样本全部低于 1.2 秒。窗口不再先暴露系统默认 client brush：返工前窗口创建均值 72.3 ms 后会在 renderer ready 前露底约 822 ms，返工后首次显示即为主题 clear。
3. **resize 跟手与露底（第二轮实现）**：`WindowEvent::Resized` 在 Windows 模态 move/size 回调内完成 surface configure、ConPTY/Term resize、frame publish 和同步 present，不再只排队 `request_redraw`。通过 `BT_RESIZE` 打点手工测量（不是自动化回归测试）：先向真实 PowerShell 写入 300 行满宽 ASCII，再按 50 ms 间隔请求 30 次尺寸变化；`68440cc` 在 1.45 秒连续序列内产生 0 个 resize-triggered present，返工版完成全部 30 次（另含 2 个初始化 resize），event→present 均值 1.694 ms、P50 1.607 ms、P95 2.551 ms、最大 2.954 ms。`surface_clear_is_exactly_the_default_cell_background` 钉死 clear 与 default-bg cell 为同一 RGB 数值。第三轮实测仍发现 configure 后的短暂黑块与 DXGI stretch 抖动，根因和后续修复见本文“第三轮返工记录”，不追溯美化第二轮记录。

### should-fix

4. **surface Lost**：Lost 与 Outdated 统一执行 `surface.configure` 并返回 `PresentOutcome::Reconfigure`，frame 被重新排队，不再冒泡到 `event_loop.exit()`；`lost_surface_reconfigures_instead_of_becoming_fatal` 覆盖恢复策略，Validation 仍保持致命以免掩盖 API 误用。
5. **glyph prepare**：glyphon 在返回 `AtlasFull` 前已经按 2 倍增长 atlas；设备上限仍不足时，renderer 记录一次诊断、trim atlas、本帧只绘制主题 clear/背景矩形并在后续帧重试，不再退出 app。`atlas_exhaustion_degrades_the_frame_instead_of_exiting` 覆盖非致命策略。
6. **整形复用**：删除“每个非空 cell 一个临时 Buffer”的路径，改为每个可见行一个持久 cosmic-text `Buffer`；相邻同样式 cell 合并为 run，空列保持占位，只在该行 cell 快照变化时重建/整形，DPI 变化才整体失效。80×24 满屏的首次整形单位从最多 1920 个 cell Buffer 降为 24 行，完全相同的下一帧为 0 行，单 cell 变化为 1 行。`text_cache_reshapes_rows_instead_of_cells_and_reuses_unchanged_rows` 和 `row_runs_preserve_blank_columns_and_style_boundaries` 覆盖复用、空列和样式边界。

### 记录项与门禁

- Ctrl+非 C 字母继续按 M0 scope 静默吞掉，代码已注明这是有意行为。
- `terminal_color` 已注明 Named 16..=28 来自 `bt-transcript` 的稳定编码。
- PTY reader 的 16 KiB wake 合并本片只记录、未改动；它不是 must/should 项，后续应与事件背压一起设计，避免首输出通知竞态。
- clear 和 rect 仍未做完整 sRGB 管理；代码已注明当前统一采用 channel/255，并用行为测试保证 default-bg 数值一致。
- `cargo test --workspace --locked`：通过，0 failed / 0 ignored。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo fmt -p bt-app -p bt-render -p bt-pty -- --check`：通过。

偏离申请：无。PTY wake 合并属于“记录即可，顺手可修”，本片选择记录而不引入额外事件协议。

## 第三轮返工记录

### 第三轮人工验收结果（第四轮返工前）

- 光标贴字：**PASS**；用户实测并附图确认。
- resize 过程文字不再抖动：**PASS**。
- 标题计时功能：**PASS**；实测显示 `bg 631ms · text 5365ms`。其中 text 耗时的大头已查明来自用户 PowerShell profile 的 conda init，不是本应用问题。
- resize 新暴露区：**FAIL**；拖大窗口时仍持续出现肉眼明显的黑色和白色填充，不是单帧一闪。第四轮只处理此项。

### must-fix 1：高 DPI 长行光标错位

- **根因**：alacritty `grid().cursor.point` 是正确的下一待写 cell，viewport 也原样转发；DECSC/恢复路径同样正确。真正的漂移来自上一轮整行 cosmic-text buffer：`set_monospace_width` 会规范 fallback 字体尺寸，但不会把主字体每个 glyph 的自然 advance 量化到 `ceil` 后的终端 cell 宽。1.5× 等缩放下单格约少推进 0.58 px，长 prompt 累积后文字比绝对网格光标落后约一个 cell。
- **修复**：`CellMetrics` 同时保留主字体自然 advance 和整数 cell 宽，为每个 style run 注入两者差值的 tracking；文字 `TextArea` 与背景/光标矩形统一经过 `cell_bounds_px`，不再各自重复坐标公式。
- **证据**：`byte_driven_prompt_cursor_is_the_cell_after_typed_text_and_ignores_prediction` 从 VT 字节驱动普通 `carg` 和“保存光标→绘制 `--version` 预测→恢复光标”两条路径，分别断言 frame 光标等于输入末列+1且预测不参与光标列；`mixed_prompt_glyphs_and_cursor_share_the_same_cell_axis` 在 1.0×/1.25×/1.5×/1.75×/2.0× 下逐 glyph 断言 `x == column * cell_width`，该测试在修复前会于 1.5× column 1 得到 13.416666 px 而非 14 px。

### must-fix 2：resize 黑块与文字抖动

- **黑块根因**：DX12 `ResizeBuffers` 丢弃旧 swapchain buffers；上一轮在 `WindowEvent::Resized` 一进来就 `surface.configure`，随后才做 ConPTY/Term resize、projection 和文字准备，因此 DWM 有机会合成未初始化的默认黑 buffer。
- **文字抖动根因**：wgpu DX12 HWND swapchain 固定使用 `DXGI_SCALING_STRETCH`。新尺寸 frame 未 present 前，DWM 会短暂拉伸旧帧；这不是 padding 重算或基线漂移。`cell_bounds_px` 的纯像素原点只依赖固定 padding、row/column 和 cell metrics，`top_left_cell_origins_do_not_depend_on_surface_width` 钉死左上锚定。
- **修复**：renderer 的 resize 先只记录目标尺寸，保留旧 back buffers 完成 frame/资源准备，直到 acquire 前最后一刻才 configure；app 在 grid reflow 前先把缓存的上一张完整 frame 按新 surface 同步 present，令新暴露区由同一个主题 clear 填充、已有文字仍按原像素原点绘制。像素变化尚未跨 cell 边界时，这张稳定帧就是最终帧，避免同一 resize 事件双 present；跨 cell 后再同步 present 新网格。
- **平台边界**：wgpu 的安全公共 API 不暴露 HWND swapchain 的 scaling/background-color，也不暴露窗口类背景刷。上述改动把 `ResizeBuffers` 到主题 present 的空档压到 acquire/submit 边界，并避免在该空档内做 PTY、projection 或 shaping；但“DWM 绝不在两次调用之间合成任何一帧”无法由应用层自动测试证明。若审核机仍能捕捉到单 compositor frame，后续需要受审计的 Win32/wgpu-hal 平台桥接，而不能把现象写成已完全消失。本轮仍以第 6 步肉眼持续拖拽为最终裁决。

### should-fix 3：启动耗时可视化

- `BT_STARTUP_TRACE=1` 时继续输出原分段诊断，并记录首次主题背景与首次文本 present 的进程内 elapsed；首文本出现后窗口标题更新为 `BetterTerminal M0-alpha — bg Nms · text Nms`。未设置环境变量时标题不变。
- `startup_trace_title_is_human_readable_without_console_output` 钉死标题格式；人工验收已改为直接运行 `target\release\bt-app.exe`，明确排除 `cargo run` 的进程前新鲜度检查。
- 当前 release 产物直接启动实测一次：`background_visible=676ms`、`first_text_present=1100ms`；两段均来自应用进程内时钟，不包含 Cargo 前置检查。

### 第三轮门禁

- `cargo test --workspace --locked`：通过，0 failed / 0 ignored；包含 vendor 回归与真实 ConPTY 测试。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo fmt -p bt-app -p bt-render -p bt-term -- --check`：通过；未用 workspace-wide rustfmt 重写 vendor/upstream 风格。
- `cargo build -p bt-app --release --locked`：通过。

第三轮偏离申请：无。resize 的 DWM compositor 边界按上述平台限制如实保留人工裁决，不把未可证的“零黑帧”硬写成 PASS。

## 第四轮返工记录

本轮只处理第三轮人工验收唯一 FAIL：拖大窗口时新暴露区域持续出现肉眼明显的黑色和白色填充。

### 白色：winit 窗口类背景刷

- 本地锁定的 winit 0.30.13 注册 `WNDCLASSEXW` 时明确令 `hbrBackground = 0`；第四轮从 raw Win32 window handle 取得 HWND，创建与 `default-bg` 完全相同的 `[9, 11, 14]` 实心刷，并通过 `SetClassLongPtrW(GCLP_HBRBACKGROUND)` 安装到 winit 窗口类。系统在模态 resize 中先于 swapchain present 擦除新客户区时，不再回退为白色。
- 选择类背景刷而不是 subclass/接管 `WM_ERASEBKGND`：M0 是单窗口、固定主题，winit 在进程内复用同一个窗口类；一把主题刷覆盖所有同类窗口正是期望语义。桥接使用 `OnceLock` 保证只创建/安装一次，成功后令刷子随 winit 类存活到进程结束，避免重复替换后误删仍被类引用的 GDI 对象。subclass 会额外引入窗口过程链、线程归属和销毁时恢复原过程的竞态，本片没有收益。
- 所有 Win32/wgpu-hal `unsafe` 都隔离在新增的 `bt-platform` 内部 crate；`bt-app`、`bt-render` 等现有一方 crate 继续继承 workspace 的 `unsafe_code = "deny"`。

### 黑色：DXGI 背景色、scaling 评估与替代

- `bt-platform` 通过 wgpu 30 的 `Surface::as_hal::<Dx12>()` 和 hal `Surface::swap_chain()` 安全边界取得克隆的 `IDXGISwapChain3` 引用，实际调用 `SetBackgroundColor`，再用 `GetBackgroundColor` 读回主题 RGB；它不销毁、不 resize wgpu 所有的资源，且所有调用都串行发生在 renderer/event-loop 线程。
- [Microsoft 的 `SetBackgroundColor` 文档](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgiswapchain1-setbackgroundcolor)明确背景色只影响以 `DXGI_SCALING_NONE` 创建的窗口化 swapchain；本地 wgpu-hal 30.0.0 则在 `CreateSwapChainForHwnd` 描述符中固定使用 `DXGI_SCALING_STRETCH + FLIP_DISCARD`。因此该调用在当前模式下可设置/读回，但不能单独改变屏幕上的新增区域。
- [Microsoft 的 `DXGI_SCALING` 文档](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ne-dxgi1_2-dxgi_scaling)说明 `NONE` 会保持旧 back buffer 左上静置，并用 background color 填满目标外区，视觉语义符合本卡点；但 scaling 是 swapchain 创建时属性，wgpu/wgpu-hal 没有公开配置项，现有 COM 接口也不能事后切换。为改一行私有描述符而 vendor 整个约 2.2 MB wgpu-hal fork，会把上游安全/驱动修复维护责任带入 M0，故本轮不选。
- 替代实现保留上游 wgpu：初始 swapchain 按可用显示器中的最大宽/高预分配，每帧仍把整张 back buffer clear 为主题色；客户区 resize 只用 `IDXGISwapChain2::SetSourceSize` 将左上可见源区改为实际客户区。正常单屏拖大不再触发 `ResizeBuffers`，也就没有新建默认黑 buffer 的窗口期；source 与目标同尺寸时旧文本不被 stretch。只有窗口超过预分配容量时才按 1.5 倍增长，并沿用第三轮已实现的“CPU shaping 完成后末刻 configure、随即同步主题 present”兜底。

### 自动证据与人工横杆

- `surface_capacity_covers_every_single_monitor_without_spanning_the_desktop`：预分配覆盖任一单显示器的最大宽/高，无显示器信息时回退到当前客户区，不按多屏总宽分配。
- `surface_capacity_grows_geometrically_only_after_source_exceeds_it`：容量内 resize 保持 swapchain 不变；越界时按 1.5 倍或请求值中的较大者增长，避免越界后的逐像素 `ResizeBuffers`。
- `surface_clear_is_exactly_the_default_cell_background` 继续保证 GPU clear 与 Win32/DXGI 共用的公开 `DEFAULT_BACKGROUND_RGB` 数值一致。
- 自动化工具的安全规则禁止操控 BetterTerminal 和 Windows Terminal 这类终端窗口，因此本轮没有伪造同机肉眼结论。最终横杆仍是第 6 步的快速拖大、慢速拖大、斜拉角落三项；未完成前状态为 **待人工复验**，不是 PASS。

### 第四轮门禁

- `cargo test --workspace --locked`：通过，0 failed / 0 ignored；包含 vendor 回归、真实 ConPTY 测试与新增 resize 容量测试。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 全部一方 workspace crates 的 `cargo fmt ... -- --check`：通过；`cargo fmt --all -- --check` 仍只报告 vendored `alacritty_terminal` 的上游格式差异，本轮按既有边界未改写 vendor。
- `cargo build -p bt-app --release --locked`：通过。

第四轮偏离申请：无。

## 第五轮人工验收结论

- resize 四角与 Windows Terminal 在同机、同操作下完成最终对照：
  - 右下角拖拽：零伪影。
  - 右上角拖拽：新暴露区下方仍有瞬时黑块与旧帧轻微拉伸。
  - 左下角拖拽：新暴露区右侧仍有瞬时黑块与旧帧轻微拉伸。
  - 左上角拖拽：新暴露区右侧与下方仍有瞬时黑块与旧帧轻微拉伸。
- 三个涉及窗口原点移动的方向，其伪影方位均与原点移动一致，且 **与 Windows Terminal 同机同操作表现同级**；按第四轮返工单的人工横杆判定 **PASS**。
- 至此五轮返工全部收口，人工验收清单全项通过，M0-alpha 最终结论由 **go-with-caveat** 收敛为 **go**。
