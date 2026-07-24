# Handoff:公式渲染(M1.9 线)— 下个 session 从这里接手

_最后更新:2026-07-23,HEAD `241e74b`_

## 当前状态:**M1.9 公式线全部收线,用户真机确认「CC 没什么问题了」**

已提交链(main 分支):
- `bac8d5a` M1.9k — 检测器重写(多行 `$$`/`\[` 渲染,长会话不熄火,两条红线)
- `20846b1` M1.9m — 呈现模型(块高=净高+对称 padding,竖直居中,底锚)
- `efd2587` M1.9o — 消灭滚/点/双击闪回(矮块;DEC 2026 重绘识别 + oracle 工具)
- `b9faf31` M1.9t — 多行大块跨 CC 内部窗格滚动保持渲染(identity/placement 分层
  + 事务级分段映射 + occlusion)+ 数学环境 `\\` 还原(带开关)
- `951c3fc` M1.9u — occluded 尾行不露源码 + Jump 芯片不烤进渲染
  (`reject_claude_code_jump_chip_overlay` 开关;红门 `scripts/dev/check-occlusion-leak.py` 30→0)
- `e66fe88` — oracle 诊断:`BT_PROBE_VERBOSE`(行级 dump)/`BT_PROBE_GEOMETRY`(placement)/
  `BT_PROBE_SCROLLTOP`(逐行上滚探针)/`debug_scroll_extent`
- `65f0fa4` — **bt-math 页面 margin:display 墨迹溢出不再被裁**(真根因:Typst `margin: 0pt`
  把溢出排版盒的墨迹光栅时切掉,Maxwell aligned 首行实测被切 42px——这就是"顶裁"的**纹理级**
  根因,两屏共享。display 加 1em 垂直 margin+crop 裁回;**inline 保持 0 margin 逐位不变**,
  它的 baseline 计量只在 0 margin 下成立(canvas 与 page 坐标系不同源的既有 quirk),有钉死测试)
- `241e74b` M1.9v — **溢出可达 + occlusion 逐 cell 清 + 粘滞本地回看**:
  ① clipped-top 撑高折进首可见 band 行(进 `live_row_prefix`/滚动 extent,底锚像素不变),
  content_off 不再扣 hidden-height 变负 → 可达最顶时 artifact 顶 ≥0;
  ② 占用行清除改**逐 cell 判定**(cell 内容==proven source 同列 cell 才清)→ 芯片前后
  泄漏源码都清、芯片文字+高亮样式保留(修灰条+芯片后尾巴 `}{\partial t}`);
  ③ **粘滞本地回看**(bt-app):视口位移进本地溢出层时普通滚轮双向归本地、滑回底自动恢复
  转发 CC;ctrl+End 两层同归底;指示条「N rows above · Shift+wheel」教入口。
  Shift+wheel 仍是**进入**本地层的唯一方式(CC 到顶无信号,自动接管=时序猜测,违质量红线)。

**工作树只剩 handoff 未提交,dist = `241e74b`,用户真机确认全部 OK。**

## M1.9 线剩余小尾巴(都不紧急,下次可选)

- **M1.9r 补行中 `\ ` 还原**:单行 pmatrix(`$$A=\begin{pmatrix} a & b \ c & d \end{pmatrix}$$`)
  渲染成一行(image232)。行中还原比行尾微妙(防误伤 `\frac` 等命令与 `\ ` 控制空格)。
- **M1.9q 底部出屏方向**(closer 滚出屏底的保持,image228 一类)——M1.9t 的分段映射可能已盖住
  大部分,需真机确认还有没有残留场景。
- **inline 渲染休眠 quirk**(本次发现):inline 的 baseline 计量在 canvas/page 坐标系不同源下
  只是凑巧成立、`y` 的降部在 0 margin 下被裁——当前 inline 在 CC 里保持源码显示,不可见,
  不紧急;若未来开 inline 渲染须先修 baseline 计量。
- **Codex CLI 公式战线**(主屏 TUI 路径,战略上"两条路径都通=覆盖绝大多数 CLI",未开工)。

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

## Codex CLI 战线(2026-07-23/24 开工,四提交落地)

- `55f5c41` **锚顶滚动区捕获**:Codex(ratatui inline viewport)靠 top=1 的 DECSTBM 区域滚动提交定稿行,
  xterm/alacritty 语义该进 scrollback(vendored grid `region.start==0` 就推史,grid/mod.rs:308);我们
  scope 分类曾要求区域底=屏底 → 全部 Ignore → **吞输出+滚不动**。已对齐 grid 事实(过 row 0 即
  FullScreen scope)。变异红验证。**重复是 Codex 自己的**(最终 reflow 后原始字节里回复印了两遍,
  MCP 警告插入触发,不修)。
- `8aae44a` **列表符定界符**:Codex 把回复放列表项里,`• $$` 开头 → 配对整体错位全不渲染。
  `delimiter_start` 跳过一个渲染态列表符(• ◦ ▪ ●),守卫全保持。
- `f407a24` **frozen 宽字符 spacer 样式**:CJK 背景条冻结后碎成条纹(spacer 丢背景),已修+回归。
- oracle 新探针:`BT_PROBE_DOCDUMP`(重建完整 scrollback+模拟 app 调度报每页渲染/失败)、
  `BT_PROBE_FROZEN`/`BT_PROBE_STYLES`(canonical frozen 行+样式)。录制:`.tmp-repaint-capture/
  codex-formula.vt`(135×40)、`codex-issues.vt`(121×32)。

### Codex 战线未闭环(下次接着)

1. **某些块不渲染(用户 image18 cases 块)**:检测器(探针验证)与 frozen 管线(模拟调度回放,每页
   2-3 块渲染 0 失败)都健康。codex-issues.vt 里两个不渲染块:一个=录制结束时 closer 未定稿(正确);
   一个=**`$$ i\hbar\frac{\partial}{\partial t}\Psi=\hat{H}\Psi $$` live 渲染失败**(MathFailurePlacement,
   Live anchor rows23-25)——**真线索:bt-math 对该输入 render error,查 `\hbar`/MiTeX**。cases 块本身
   未再复现;若再遇,用 BT_PROBE_FROZEN 拿 canonical 行判逻辑行形态。
2. **缩放(zoom)后部分公式回源码**:layout(font_rev/dpi)变更 → live 记录 layout 不匹配被丢弃重检测的
   竞态,部分块撞 M1.9p 首检回源码。headless 可复现(改 LayoutKey 重投影)。
3. **缩放后跳到底**:zoom 路径未保持滚动锚。与 2 同区(bt-app zoom 处理+projection 锚)。
4. Codex 输出裸 LaTeX(无定界符)和 `[ ... ]`(被吃反斜杠的 `\[`)不渲染=忠实行为,非 bug。

### Resize 战线(2026-07-24 续,c2ee5ab 收口)

- `eec71a6` **resize 录制/回放基建**:`BT_PTY_DUMP` 的 chunks 清单记 `# RESIZE cols rows elapsed_us`
  标记(bt-pty resize 路径写入,`#` 前缀向后兼容),oracle 在标记点 `resize_at` +
  `mark_pty_resize_requested_at`,并每步驱动 `finish_resize_if_quiescent`(**不驱动 epoch 会让回放中
  resize 后重检测永久停摆、掩盖真相**——这个坑修在 c2ee5ab 里)。
- `c2ee5ab` **`# $$` 重排配对**:Codex resize 重排把 `$$` opener 打成 ATX 标题(`# $$`/`• # $$`,
  12 个全在 RESIZE 标记后 0.1-0.2s)。拒绝畸形 opener → 整条消息配对错位 → 闭符跨 `#` 行错配**把
  `#` 排成公式**。`delimiter_start` 跳标题符(可叠列表符),守卫全保持;resize-repro.vt 回放终态全
  Rendered、错配块消失。
- **用户确认**:拖拽后短暂过渡能回来;CC(alt)resize 公式保持 ✓。

### Resize 残留(下次专项)

1. **随机个别块滞留源码**(image24,`$$\sum...$$` 块,周围块都渲染):**回放收敛全渲染复现不了**
   ——是交互态竞态。嫌疑:①resize 风暴后 staging 缝隙行无公式管线(staged 行不走 math 装饰,
   若 codex 之后无输出关闭 candidate 则永滞);②app timer 与稳定时钟的漏行。**下次**:让用户带
   `BT_RESIZE_TRACE`+dump 录到滞留现场,回放时打印 staging_len/staged 行判 ①;或 oracle 加
   wheel 交互回放。注意 codex-issues.vt 里滞留的也是 `\sum` 块(当时判"录制尾未定稿"——两次都是
   它,可能非巧合)。
2. **resize 后跳底(用户澄清:主体是 Codex/primary,CC 无此困扰)**:机制=Codex resize 用
   `2J+3J` 清史+全量重印,我们在清史瞬间把 scroll offset clamp 到 0 → 跳底;但重印马上把等价
   内容填回历史——内容并没消失。**可修方向(非启发式)**:回看态(is_scrolled)下 ED3 不重置滚动
   偏移,offset 数值保持、历史重建后视口自然落回约原位置(不猜时序,只是不抛弃用户滚动意图)。
   注意空历史期间 offset>max 的显示语义(暂显空白/live)与真 clear(用户主动 cls)场景的行为核对。
