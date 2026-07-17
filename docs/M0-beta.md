# M0-beta：中文 IME 输入

日期：2026-07-16

## 真人验收（二轮返工前）

- 微软拼音组字期间 Backspace / Esc 不穿透：**PASS**。
- 微软拼音候选框会随组字连续移动，但锚点远离 terminal 光标；记为“连续跟随会移动，位置错误”，进入本轮 must-fix 2。
- 失焦行为按“`Ime::Commit` 来什么收什么”裁决分开记录：微软拼音失焦时未选组字被输入法丢弃，应用未收到幽灵 commit；微信输入法失焦时发出半串 commit，该文本原样进入 shell。两种行为均符合裁决，不合并成同一种输入法行为。
- 搜狗输入法与英文回归项：待本轮三项修复完成后重测。

本轮真人实机另发现三项必须返工：高缩放屏启动字体过小、候选框锚点远离光标、CJK 相对 ASCII 字号偏小且基线偏高。修复与复测结论记录在文末“二轮返工记录”。

## 结论与证据边界

代码与自动化门已完成；三家输入法的候选框连续跟随、跨 DPI 相对位置和真人打字仍是人工验收门，不能由单元测试冒充。实现遵循 `docs/spikes/04-ime-cjk-manual-results.md` 的实测裁决，没有修改 grapheme、ambiguous width 或 emoji/ZWJ 策略。

失焦或切换输入法时，IMM32 可能提交半成品。本片的明确产品决定是：**照单全收**。winit 交付的每一个 `Ime::Commit` 都按 UTF-8 原样写入 PTY，不在应用层猜测、取消或去重；这与 Windows Terminal 的可观察行为一致。

## 改动与自动化证据

| 改动 | 行为 | 自动化证据 |
|---|---|---|
| logical Process 过滤 | `logical_key == Named(Process)` 在任何 modifier 下均不产生 PTY 字节；实现不读取 `physical_key` | `keyboard_mapping_is_ascii_only_and_preserves_terminal_controls` |
| IME commit | commit 的 UTF-8 字节原样进入唯一 PTY writer；非 ASCII 不再经过普通键盘映射 | `ime_commit_is_the_exact_utf8_pty_payload`；`real_powershell_input_reaches_a_viewport_owned_frame` 通过同一 commit 字节函数写真实 ConPTY |
| alacritty 宽格消费 | 保留 `WIDE_CHAR` lead 与 spacer，不在 renderer 重算 committed cell width | `committed_utf8_projects_as_alacritty_wide_lead_and_spacer_cells` |
| 预编辑覆盖层 | preedit 在 terminal cursor 处临时合成、逐 cell 下划线、只消费 collapsed caret 起点；不修改 Term/原始 frame | `preedit_is_transient_underlined_grid_content_with_a_collapsed_caret` |
| 宽字符渲染 | 行缓冲为 wide lead 预留两格，宽字形另行锚定到 lead cell 的 2-cell slot，后续 ASCII 不受 fallback advance 影响 | `mixed_cjk_ascii_wide_slots_use_exact_terminal_cell_origins`（严格 `assert_eq`，无容差） |
| 宽格光标 | 光标位于 lead 或 spacer 时均从 lead 覆盖两格 | `cursor_on_either_half_of_a_wide_cell_covers_both_cells` |
| CJK fallback | 只加载 Consolas + Microsoft YaHei UI + DengXian + SimSun 固定文件白名单；不扫描 Fonts 目录 | `bounded_cjk_fallback_shapes_chinese_without_notdef` 断言“你好世界”四个 glyph 均非 `.notdef` |
| 候选区节流 | 相同位置不重复发；变化位置最多 60 Hz（16 ms）；末次坐标由 event-loop deadline 保证 flush | `ime_cursor_area_throttle_coalesces_to_sixty_hz_and_flushes_the_last_area` |
| 微软拼音 system caret | bt-platform 在活动 keyboard layout 的 primary language 为 Chinese 时创建/移动 1×1 Win32 caret；其他语言销毁，不影响微信/搜狗 | `chinese_system_caret_gate_uses_primary_language_bits`；候选连续跟随本身留人工 |

`set_ime_cursor_area` 的 16 ms 阈值依据：spike 04 在高频坐标更新时观察到应用自身重绘掉帧，而候选框仍流畅。16 ms 对齐 60 Hz 一帧，限制重复工作，同时 pending deadline 保证停止移动后的最终位置不会永久丢失。

预编辑 target clause 未实现：三家真人日志的 cursor range 全程 `a == b`，M0-beta 只画整串下划线和 collapsed caret。搜狗不上报非空 preedit 时覆盖层自然为空，只处理最终 commit，没有 workaround。

## 字体回退启动打点

同机、同 release 二进制启动两轮；命令均为先 `cargo build -p bt-app --release --locked`，再以 `BT_STARTUP_TRACE=1` 直接运行 `target\release\bt-app.exe`。样本量仅 2，GPU adapter/device 抖动明显，因此单列字体阶段和用户可见里程碑，不把几毫秒噪声包装成精确基准。

| 阶段 | M0-alpha 4-face（2 轮均值） | M0-beta 固定 CJK 链（2 轮均值） | 差值 |
|---|---:|---:|---:|
| font system | 0.5 ms | 2.0 ms | +1.5 ms |
| font metrics | 1.0 ms | 2.0 ms | +1.0 ms |
| renderer total | 558.0 ms | 565.5 ms | +7.5 ms |
| runtime ready | 619.5 ms | 626.5 ms | +7.0 ms |
| background visible | 681.5 ms | 670.0 ms | -11.5 ms |
| first text present | 1099.5 ms | 1075.0 ms | -24.5 ms |

结论：固定 CJK 链只给字体阶段增加约 1–2 ms；首次背景与首次文本没有回退。仍保留 M0-alpha 的关键收益：没有全系统字体扫描。

原始 M0-alpha 两轮：`fonts=1/0, metrics=1/1, renderer=557/559, runtime=613/626, background=651/712, first-text=1071/1128 ms`。M0-beta 两轮：`fonts=2/2, metrics=2/2, renderer=579/552, runtime=638/615, background=691/649, first-text=1103/1047 ms`。

## 怎么手动验

1. 执行 `cargo build -p bt-app --release --locked`，再直接运行 `target\release\bt-app.exe`。确认窗口标题为 `BetterTerminal M0-beta` 且 PowerShell prompt 可输入。
2. 微软拼音：逐字输入 `echo 你好世界`。组字串应在 terminal cursor 处带下划线；连续输入、左右移动预编辑 caret 时，候选框应连续跟随；选字回车后 shell 回显为中文且四个汉字各占两格。组字中按 Backspace 只改拼音，Esc 只取消预编辑，prompt 中已有字符不得被真退格删除。
3. 微信输入法：重复第 2 项。候选框和内嵌预编辑都应跟随；Backspace/Esc 不穿透。
4. 搜狗拼音：重复输入与提交。**看不到应用内 preedit 是预期行为**；候选窗可由搜狗自绘，但最终 commit 必须完整进入 shell，不能丢字、重复或乱码。不为“没有内嵌串”判失败。
5. 三家各自输入 `ls 文档/` 与 `echo A你B好C`，观察中英混排：汉字占两格，后续 ASCII 回到准确 cell，光标不漂移、无豆腐块。
6. 在微软拼音/微信组字期间连续 Backspace，再 Esc；随后切回英文输入 `cargo --version`、Backspace 修正拼写、Ctrl+C 中断长命令。英文路径应与 M0-alpha 一致。
7. 组字一半时 Alt+Tab 失焦或切换输入法。若 IMM32 当场发出半串 commit，该文本应原样出现在 shell；这是本片接受的“幽灵 commit”裁决，不应崩溃或吞字。
8. 将窗口在不同 DPI 显示器间移动，三家分别重做候选框检查。候选框相对 terminal cursor 的位置应稳定；微软拼音重点看连续跟随，微信/搜狗不得因 system caret 补丁退化。
9. 关闭窗口，确认 PowerShell/conhost 被回收。

人工记录需逐家写明：IME 名称/版本、preedit 是否出现、Backspace/Esc、一次移动跟随、连续跟随、跨 DPI、最终 commit、混排/豆腐块、英文回归。当前代码执行中没有采集新的三家真人事件序列，因此没有声称 winit 序列与 spike 04 “再次一致”；若人工验收看到差异，必须逐事件记录，不硬凑。

## 二轮返工记录（2026-07-17，已完成人工复审）

本轮只处理真人验收提出的三项 must-fix，没有扩展 M0-beta 范围：

1. **DPI / 启动字号**：窗口创建时只读取一次 `window.scale_factor()` 并交给 `CellMetrics`；renderer 初始化后硬校验 metrics 与该报告值相等。隐藏窗口显示前再次对账，所有 `Resized` 也主动按窗口当前报告值修正 metrics，不再只依赖 `ScaleFactorChanged` 的事件时序。`startup_metrics_must_match_the_window_reported_scale_factor` 覆盖启动断言；`metrics_and_ime_client_rect_apply_the_reported_dpi_scale` 覆盖 100%、125%、150%、200% 的字号、行高和客户区 IME 矩形。
2. **候选框锚点**：renderer、winit `PhysicalPosition` 与 Win32 caret 统一使用客户区原点的物理像素，不做屏幕原点平移。winit 的 `CFS_EXCLUDE` 之外，中文布局同时发送 `CFS_CANDIDATEPOS`（TSF/CUAS 路径）并维护 1×1 system caret（旧路径）；16 ms throttle 继续合并更新且 deadline flush 的是最新完整矩形。`ime_cursor_area_throttle_coalesces_to_sixty_hz_and_flushes_the_last_area` 与上述多 DPI 矩形测试覆盖 CPU 可判定部分。
3. **CJK 字号 / 基线**：宽字仍由独立 buffer 画在原 lead cell 的严格两格 slot；buffer 的 `monospace_width` 现在以两格 em 为目标做 fallback 字号补偿，并用 Consolas 行基线减去 CJK buffer 行基线得到每个宽字的 top offset。`cjk_size_compensation_targets_the_two_cell_em`、`baseline_offset_aligns_an_independent_fallback_buffer`、`cjk_wide_buffer_matches_two_cells_and_the_ascii_baseline` 覆盖补偿和基线计算；`mixed_cjk_ascii_wide_slots_use_exact_terminal_cell_origins`、`cursor_on_either_half_of_a_wide_cell_covers_both_cells` 继续零容差通过。

自动化门禁：`cargo test --workspace --locked` PASS；`cargo clippy --workspace --all-targets --locked -- -D warnings` PASS；全部一方 crate 的 `cargo fmt ... -- --check` PASS（vendor 保持上游格式，和 CI 约定一致）。

二轮人工复审结论：CJK/ASCII 对齐 **PASS**；验收者观察到英文相比汉字略小，这是 CJK 字形撑满两格 em 的视觉效果，登记为 P3 调优项，本轮不改。启动字号 **FAIL**（高缩放屏整窗字体仍极小）；候选框位置 **FAIL**（候选框落在窗口中部偏右，没有位于组字处下方）。此前已记录的微软拼音 Backspace/Esc PASS 与两家失焦行为裁决不因该轮结果改写。

## 三轮返工记录（2026-07-17，待真人复审）

本轮只修复二轮暴露且已由 winit/Win32 行为证实的两条错误链路，不调整已经 PASS 的 CJK/ASCII 对齐：

1. **候选框所有权与 system caret**：删除 `bt-platform` 对同一 HIMC 发送的 `ImmSetCandidateWindow(CFS_CANDIDATEPOS)`。winit 0.30.13 继续作为候选/组字窗口表单的唯一写入者，保留其 `CFS_EXCLUDE` 与 `CFS_POINT` 行为，应用不再在后调用中覆盖它。微软拼音兼容桥只维护不可见的 1×1 Win32 system caret；`SetCaretPos` 使用 renderer 产出的组字 cell 左上角，坐标系是客户区物理像素，不做屏幕原点平移。
2. **Win32 权威 DPI 对账**：`bt-platform` 增加 `GetDpiForWindow` 桥接，应用统一以 `GetDpiForWindow(hwnd) / 96.0` 作为 metrics scale 的权威值，不再用 winit 缓存值决定字号。窗口创建时即用该值初始化 renderer；`ShowWindow` 后、首个可见 frame present 后，以及每次 `Resized` / `ScaleFactorChanged` 都重新查询。发现 metrics 不一致时立即重度量字体、按当前客户区像素重算并 resize terminal/ConPTY 网格、更新 layout key，再 republish 完整帧。
3. **永久诊断**：上述 create、show、first-present、resized、scale-factor-changed 对账点无条件向 stderr 输出单行 `BT_DPI`，字段为 `stage`、`winit_scale`、`win32_dpi`、`authoritative_scale`、`metrics_scale` 和 Win32 窗口矩形 `rect=left,top,right,bottom`。这不依赖 `BT_STARTUP_TRACE`。设置 `BT_STARTUP_TRACE=1` 时，标题栏还会持续显示当前权威 scale（例如 `BetterTerminal M0-beta · 1.5x`；首文本后保留 bg/text 时间并追加 scale）。

人工复审请用新 release 构建启动一次，并原样回传从 `stage=create` 到 `stage=first-present` 的 `BT_DPI` 行；随后在混合缩放显示器间移窗，再回传对应的 resized/scale-factor-changed 行。判据是 `authoritative_scale == win32_dpi / 96`、修正后的 `metrics_scale` 在下一打点与它一致，同时肉眼字号和候选框位置正确。数据不符时按打点继续定位，不再试探性改坐标或 scale。

三轮自动化门禁：`cargo test --workspace --locked` PASS；`cargo clippy --workspace --all-targets --locked -- -D warnings` PASS；全部一方 crate 的 `cargo fmt ... -- --check` PASS（vendor 保持上游格式）。这些门禁只证明代码与 CPU 可判定不变量，候选框和混合 DPI 仍以本轮人工复审为准。

若打点表明 **GetDpiForWindow 本身也返回了与验收者所看显示器不符的 DPI**，下一步不把它当成 DPI API 读错，也不硬编码倍率；应判定 HWND 在启动竞态中关联/落到了错误显示器。预案是用日志中的窗口矩形对照系统显示器边界，再增加 `MonitorFromWindow`、monitor rect 与 per-monitor DPI 的关联诊断，确认实际 monitor handle；随后在显示前把初始位置明确落到目标显示器，待 HWND 的 monitor 关联稳定后再查询 DPI、建 renderer 和显示窗口。本三轮只记录该预案，不实现显示器选择或窗口重定位。

## 四轮返工记录（2026-07-17，方案已在五轮实测证伪）

用户回传的三轮 `BT_DPI` 在 150% 与 200% 屏均证明 winit、`GetDpiForWindow` 和 renderer metrics 正确收敛，但截图中的整张 terminal 内容仍被缩小，因此本轮只审计并修复 alpha 第四轮引入的预分配 swapchain / `SetSourceSize` 呈现路径；IME 代码没有改动。

1. **呈现尺寸只有一套物理像素约定**：`source_size` 始终直接取 winit `inner_size()` / `Resized` 携带的客户区物理像素，不乘 DPI，也不取预分配容量。create、show、first-present、resized、scale-factor-changed 的共同 reconciliation 现在会先同步 presentation source，再允许该阶段发布新帧；不会再因 metrics 已经正确而提前返回、跳过 source 对账。每次 `SetSourceSize` 后立即调用 `GetSourceSize` 读回，实参或驱动状态只要不等于客户区物理尺寸就停止并报错。
2. **容量不再泄漏到可见 viewport**：预分配 back buffer 仍可大于窗口，但它只表示附件容量。glyphon `Resolution`、render-pass viewport/scissor、矩形 NDC 分母和 DXGI source 全部使用同一个客户区物理尺寸；文字、矩形与 IME 光标坐标继续共享客户区物理像素轴。容量增长仍按原 1.5 倍策略，只在窗口超过当前容量时重建 swapchain。
3. **永久诊断闭环**：每条 `BT_DPI` 现在追加 `source_size=宽x高`、`swapchain_capacity=宽x高`、`inner_size=宽x高`。本机 200% 运行态样本为 `source_size=1920x1200 swapchain_capacity=3840x2160 inner_size=1920x1200`；create、show、first-present 与后续 resized 全部满足 `source_size == inner_size`。`swapchain_capacity` 可以更大，这是预分配，不得再作为可见 source 或 viewport。
4. **自动化证据**：`presentation_source_is_always_the_physical_client_not_dpi_or_capacity` 覆盖 100%/150%/200% 等物理客户区与容量增长，断言每次 resize 后 source 原样等于客户区且不超过容量；`recorded_source_size_matches_physical_inner_after_every_reconcile_size` 覆盖 app reconciliation 的记录判据。平台桥还在真实 DXGI 调用后做 `GetSourceSize` 强校验，CPU 单测不能冒充该运行态读回。

候选框判断保持单因果：此前 IME 坐标是在未压缩的客户区物理像素轴上产生，视觉内容却经过错误的呈现约定；本轮不再叠加 IME 坐标补丁。请先由真人复测微软拼音、微信与搜狗候选框相对组字光标的位置。只有在呈现缩放确认恢复后仍有偏移，才重新进入 IME 专项。

四轮自动化门禁：`cargo test --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、全部一方 crate 的 `cargo fmt ... -- --check` 与 `cargo build -p bt-app --release --locked` 均需 PASS；候选框位置与混合 DPI 视觉结果仍以真人复审为准。

## 五轮返工记录（2026-07-17，待真人复审）

独立审计与像素级实测共同确认四轮方向不成立：wgpu-hal DX12 硬编码 `DXGI_SCALING_STRETCH`，`SetSourceSize` 虽然写入成功且可读回，HWND 合成仍把整张 capacity buffer 拉伸进客户区。1946px 宽窗口中第一行 50 字符只占 440px（8.8px/字符），以及 150% 屏的 5.22px/字符，均精确命中 `inner/capacity` 拉伸模型，而不是 DPI 漏乘。IME 矩形不经过这层 swapchain 拉伸，因此候选框偏移是同一呈现错误的连带结果；本轮不修改 IME 代码。

1. **回退预分配与裁剪**：`bt-app` 不再计算最大显示器 surface capacity；`Renderer::new` 只按当前 `inner_size` 配置 surface。删除容量增长、`SetSourceSize`、`GetSourceSize` 回读、DXGI `SetBackgroundColor` 诊断和对应平台桥。
2. **恢复 ResizeBuffers**：每次有效 `Resized` 都把 `config.width/height` 严格设为客户区物理像素；下一次同步 present 在 CPU shaping 完成后末刻调用 `surface.configure`。α 第三轮的模态循环内同步 present、上一完整帧回填和主题色窗口类背景刷全部保留。
3. **单一呈现约定不变**：Metrics 继续乘 DPI，glyphon `TextArea.scale=1`，`Resolution`、viewport/scissor 和矩形 NDC 分母都直接使用当前物理 `inner_size`。没有新增 DPI 倍率或 IME 坐标补丁。
4. **诊断与自动化不变量**：`BT_DPI` 只追加 `swapchain_size=宽x高` 与 `inner_size=宽x高`；创建和每次 resize 后必须精确相等。`swapchain_size_is_always_exactly_the_physical_client_size` 与 `recorded_swapchain_size_matches_physical_inner_after_every_reconcile_size` 明确拒绝旧版 `capacity > inner` 的合法化漏洞。

五轮自动化门禁：`cargo test --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、全部一方 crate 的 `cargo fmt ... -- --check` 与 `cargo build -p bt-app --release --locked` 均已 PASS；vendor 保持上游格式。呈现比例和候选框位置由真人复测，本轮完成后停下等审。

## 五轮后最终验收（2026-07-17,关片）

- **呈现比例**：像素级实测(DPI-aware 全物理捕获)——修复前 8.8px/字符,修复后 **17.6px/字符**(200% 屏应有值),`swapchain_size == inner_size == 1920×1200`;两块屏(200%/150%)字号均正常。**PASS**
- **候选框位置**:微软拼音组字,候选框紧贴组字位置正下方并跟随(真人实测+截图)。**PASS**——确认为四轮呈现压缩的连带症状,IME 代码零改动即恢复。
- **中英混排**:同大小、同基线(三轮修复在压缩消除后依然成立)。**PASS**
- 此前各轮已 PASS 且未回退:空格/Named 键、组字期 Backspace/Esc 不穿透、UTF-8 commit、失焦幽灵 commit 照单全收、搜狗最终上屏、宽字符双格、启动收益、英文回归。

**结论:go。** 九轮返工(β 一至五轮 + 期间 xhigh 独立审计一轮)收口;经验教训已在四/五轮记录中如实保留(元数据回读≠合成生效、不变量测试不得把病态认证为合法)。

## 明确未做

- grapheme 聚类、ambiguous width 裁决、emoji/ZWJ 正确性；
- 搜狗空 preedit workaround；
- target clause 分段高亮；
- 富内容、数学、分屏、标签、多会话或滚动回溯 UI。

## 偏离申请

无。
