# Handoff:公式渲染(M1.9 线)— 下个 session 从这里接手

_最后更新:2026-07-22,HEAD `951c3fc`_

## 当前状态

**公式渲染核心问题 + occlusion 边界都已解决并提交,用户真机确认**。剩一个几何/滚动问题(M1.9v)。

已提交链(main 分支):
- `bac8d5a` M1.9k — 检测器重写(多行 `$$`/`\[` 渲染,长会话不熄火,两条红线)
- `20846b1` M1.9m — 呈现模型(块高=净高+对称 padding,竖直居中,底锚)
- `efd2587` M1.9o — 消灭滚/点/双击闪回(矮块;DEC 2026 重绘识别 + oracle 工具)
- `b9faf31` M1.9t — **多行大块跨 CC 内部窗格滚动保持渲染**(identity/placement 分层
  + 事务级分段映射 + occlusion)+ 数学环境 `\\` 还原(带开关)+ 顶部出屏保持
- `951c3fc` M1.9u — **occluded 尾行不再露源码(①)+ Jump 芯片不再烤进渲染(②)**。
  projection 产出 `occluded_visible_rows`(occluded 且仍是本块 proven source 精确前缀的
  终端行)→ viewport 沿 band 清行路径清掉;chrome 不匹配 proven source 绝不清。检测加
  默认开关 `reject_claude_code_jump_chip_overlay` 只拒确切芯片签名。真红门
  `scripts/dev/check-occlusion-leak.py`:cc-topbot 30→0。oracle 加 `BT_PROBE_VERBOSE`
  行级 dump + `BT_PROBE_GEOMETRY` placement dump。

**工作树干净,dist = `951c3fc`(用户已真机确认 ①②)。**

## 下次第一件事:M1.9v — 高多行公式顶裁 = 滚动溢出量不足(与大屏滚不到顶同源)

用户 2026-07-22 实拍(旧 image236 顶裁 + 大屏滚不到顶):**多行高公式(aligned/`(a+b)²`
三行组)顶部被窗格裁**,主屏+备用屏都有;且**大屏向上滚到某处就卡住、上不去**(小屏能滚到顶)。

**统一真根因(几何实测,非猜——`BT_PROBE_GEOMETRY` dump cc-topbot 得):**
- alt 是 **expand-only**(`bt-viewport/lib.rs:1666` 注释明说):高公式 band 比可见网格高,
  **额外像素溢出到窗格上边之外**,本就要**向上滚**才看得全。这是设计,不是渲染 bug。
- 实测 aligned 块(9 行 band、artifact 6 行高):居中 `content_off=+31744`,但静止(bottom)时
  `top_sub=-47104` → artifact 顶 = -47104+31744 = **-15360(窗格顶之上 0.83 行)→ 被窗格顶裁**。
  **块完整可见(top_sub≥0)时顶不裁**——所以顶裁只发生在"没滚够、块顶露在窗格外"时。
- **和大屏滚不到顶是同一个 bug**:允许滚进 live 溢出区的行数 `last_live_overflow_rows`
  (`lib.rs:956`,= ceil(live_extra_height/cell))+ `scroll_by_rows`(`lib.rs:782`)在大屏/公式多时
  **不够**,滚不到能把多行块顶部完整拉进视野的位置 → 块顶永久卡在窗格顶被裁。用户"公式多导致"准。
- **未做实的细节**:大屏 vs 小屏为何差异(alt vs primary 路径?`live_rows` 变大后 overflow/max
  计算偏差?)。**下次先用 `BT_PROBE_GEOMETRY` + 真机复现把大小屏差异挖实,再改滚动溢出量。**

**❌ 别再走的死路**:M1.9u 初版把顶裁当 bt-math 光栅 alpha 贴边、加 1px 透明 guard —— **错根因,已撤**
(顶裁是好几像素的窗格裁切,不是贴边)。

**根因已由真机 `cc-large.vt`(约 212×48,gitignored)字节级坐实(2026-07-22)**——用户假设正确:
① CC **确实滚到它自己的顶**(录制里 banner「Claude Code」在文件 0.949、prompt 在 0.991 被 CC 重绘);
② 我们 expand-only **向上**溢出的撑高累积把 banner 顶出窗格上边;③ 可上滚量 `last_live_overflow_rows`
(`lib.rs:956`)实测只 5、顶出约 11 → 够不回;④ **最顶 aligned 块 `content_off` 变负**
(`band=0..=7 top_sub=-84309 content_off=-17579 clip_h<art_h`)→ artifact 戳出自己 band 顶被裁。

**⚠️ M1.9v Codex 首试(2026-07-22)被后台超时杀、且方向不足,已撤**:它加了
`alternate_live_math_vertical_geometry` 辅助,但 before/after 探针 **live_allow 只 5→6、min_artifact_top
仍 -83456(还是裁的)**——证明**光加可上滚量不够**。关键:高块 band 就在 row 0,`content_off` 负让
artifact 戳出 band 顶,滚窗口也没用(它 band 之上没内容可露)。**真修得让「可达最顶」态 content_off ≥ 0
(顶对齐不戳出)+ 可上滚量覆盖全部向上撑高**,两者都要。任务书
`docs/prompts/codex-M1-9v-scroll-overflow-reveal.md` 已写;诊断工具
(`BT_PROBE_SCROLLTOP`/`BT_PROBE_GEOMETRY`/`debug_scroll_extent`)在工作树未提交。
**下次可先提交诊断工具再重派 Codex(给更长 runway)或自己改。**

## 关键工具 / 方法(今天血泪换来的,务必沿用)

- **派 Codex 必带 `-BypassSandbox`**(`ask_codex.ps1` 已加此开关)。否则 Codex 的 Windows
  沙箱每跑 cargo 就 `CreateProcessAsUserW 1312` 失败被杀 —— 今天为此栽了 4 次才发现。
- **Codex 常被后台超时杀**(架构改 + 跑门禁耗时长),但**核心代码通常已写完**;惯例:
  我接手验证(cargo check 编译 → 逐源轨迹 → 门禁),别因为"被杀"就当失败。
- **交互态问题必须用真实录制字节验证**:`BT_PTY_DUMP=<path>` 启动 bt-app 录 CC 原始 VT,
  产出 `.vt` + `.chunks`;合成/静态截图会给**假 green**(今天栽过 M1.9j/n/o 初版)。
- **不信 oracle 的 EXIT 码**(诊断豁免会假绿,今天 Jump-chip 豁免坑过)。用逐源状态轨迹:
  `scratchpad/trace_blocks.py <frames.txt>` 直接解析每帧、追踪每个块的 RENDERED↔SOURCE。
  **判据 = `RENDERED -> SOURCE` flips 数(0 = 不闪)**。
- **连续返工打偏 ≥3 轮 → 切独立审因**(read-only,不许修、只找因、可翻架构,无假设)。
  今天大块 flash 我带错假设栽 3 轮,靠这个纪律翻案成功(见下方审因报告)。

## 红线(不可破)

- **M1.9p**:回看态**首次检测**多行对称 `$$…$$` = NO-GO(第0行前缀不可信,屏内两个 `$$`
  与"屏外残留闭符+散文+下一开符"不可区分 → 绝不为渲染猜前缀,否则把散文排成公式)。
  见 `docs/reviews/M1.9p-scrollback-symmetric-ambiguity-nogo.md`。
- **M1.9k**:错配散文红线、CJK 散文守卫、九类误报守卫、CommonMark 代码上下文。
- **保持命中须源码精确相等**(防显示错误公式)。
- **不 reconcile 重装**(M1.9n 栽过:重算 band → 真机损坏、静态门禁全绿)。能保留就保留、
  能平移就平移。

## 验证素材位置

- `.tmp-repaint-capture/cc-topbot.vt`(+`.chunks`)— 真实 CC,含多行大块 aligned/pmatrix
  (gitignored,本地存在;若丢需用户重录)。**M1.9u 的 occlusion 打磨可用它验**
  (occluded 边界是否干净)。
- `.tmp-repaint-capture/cc-scrollout.vt` — 矮块(回归基线,R→S 应恒 0)。
- oracle 跑法:`BT_PROBE_INPUT=<vt> BT_PROBE_CHUNKS=<chunks> BT_PROBE_COLUMNS=106
  BT_PROBE_ROWS=33 cargo run --locked --offline -p bt-term --bin bt-repaint-oracle`
  → `Out-File -Encoding utf8`(PowerShell `>` 会写 UTF-16 让 grep 失效)。
- **oracle 诊断开关**:`BT_PROBE_VERBOSE=1` → stderr dump 每个 Mixed 帧的 block band 行范围
  + 可见 grid(喂 `scripts/dev/check-occlusion-leak.py` 判 ① 尾行泄漏);`BT_PROBE_GEOMETRY=1`
  → 每个 Rendered 块的 `top_sub/content_off/clip_h/art_h/pad`(M1.9v 顶裁/滚动几何用这个)。
- 门禁:全 workspace `--locked --offline`、clippy `-D warnings`、fmt。注意 bt-pty 的
  `real_conpty_child_receives_color_environment…` 在宿主 ConPTY 不稳时 **flaky**,与公式无关。

## 完整背景

- 大块 flash 根因(架构级):`docs/reviews/M1.9-large-block-flash-independent-audit.md`
  (CC 内部滚动窗格边界 + 6 条架构方向 —— M1.9t 就是照它做的)。
- 公式管线总审:`docs/reviews/M1.9-formula-pipeline-audit.md`。
- 逐轮流水与决策:memory `m1-9j-queue.md`。

## 已知非我方问题(别当 bug 修)

- 复杂多行公式若在 CC 里挤成一行 = **CC markdown 把 `\\` 吃成 `\`**(dump 里双反斜杠 0 次
  为证)。M1.9t 已在数学环境内还原(开关 `restore_stripped_environment_newlines` 默认 on);
  **CC 修好后应置 false**。`\[`/`\\` 被吃同源。
- Codex CLI 是**主屏 TUI**(非 alt),用 `\x1b[3J` 清 scrollback = "输出被吃掉"是它主动清、
  按 xterm 标准执行,非我方 bug。Codex 的公式支持是**单独战线**(未做)。
