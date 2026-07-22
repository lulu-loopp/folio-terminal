# Handoff:公式渲染(M1.9 线)— 下个 session 从这里接手

_最后更新:2026-07-22,HEAD `b9faf31`_

## 当前状态

**公式渲染的核心问题都已解决并提交**。Claude Code 在 BetterTerminal 里输出 LaTeX,
渲染 + 几何 + 跨滚动/点击/双击/重绘保持,都工作了。

已提交链(main 分支):
- `bac8d5a` M1.9k — 检测器重写(多行 `$$`/`\[` 渲染,长会话不熄火,两条红线)
- `20846b1` M1.9m — 呈现模型(块高=净高+对称 padding,竖直居中,底锚)
- `efd2587` M1.9o — 消灭滚/点/双击闪回(矮块;DEC 2026 重绘识别 + oracle 工具)
- `b9faf31` M1.9t — **多行大块跨 CC 内部窗格滚动保持渲染**(identity/placement 分层
  + 事务级分段映射 + occlusion)+ 数学环境 `\\` 还原(带开关)+ 顶部出屏保持

**工作树干净,dist = `b9faf31`。**

## 下次第一件事:M1.9u — 三个剩余小问题

都是"部分可见/遮挡/几何"细节,比大块 flash 小一个量级。用户 2026-07-22 实拍
(image 233/234/235/236)。

### ① + ② occlusion 边界打磨(同源,image233 + image234/235)
- **image233**:块**渲染一半 + 源码尾巴一半**并存 —— 被 CC Jump 芯片/内部窗格底遮住的
  尾行,该干净隐藏(occluded),却露出了源码。
- **image234/235**:Jump 芯片文字**混进公式渲染**(积分里出现 `∈Jumptobottom(ctrl+End)↓`)
  —— CC 的 "Jump to bottom" overlay 文字写进了 grid 那行,被当成公式内容渲染。
- **根因方向**:M1.9t 已做 occlusion,但边界处理不干净:被 chrome/Jump 芯片覆盖的行
  应完全 occluded(不渲染那段、也不把芯片文字当公式内容),现在还漏。
- **修**:凡被应用 chrome / Jump 芯片覆盖的行 → occluded,可见渲染部分与被遮部分的
  交界要干净(不露源码尾、不吸芯片文字)。这是 M1.9t occlusion 的延续打磨。

### ③ 顶部裁切(image236,独立几何,主屏+备用屏都有)
- **现象**:渲染公式**顶部被裁**(如 `∇·E = ρ/ε₀` 的 ρ 上缘被切)。
- **根因方向**:band 顶部 clip 偏差,**主屏也有** → 是几何/clip 层的问题,不是 alt 特有,
  和 occlusion 无关。独立查 bt-viewport 的顶部 clip 几何。

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
  → `Out-File -Encoding utf8`(PowerShell `>` 会写 UTF-16 让 grep 失效)→ `trace_blocks.py`。
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
