# Handoff:公式渲染(M1.9 线)— 下个 session 从这里接手

_最后更新:2026-07-28,HEAD `a85ed32`_

## 当前状态:**公式战线全线收官,用户真机总验通过(2026-07-28「这次确实没看出什么问题」)**

2026-07-24→28 大扫除全纪录见 memory `m1-9j-queue.md`(30+ 提交、5 次独立审因、2 次 Codex 方案
复核,收官笔 `a85ed32` = "输出时滚动"族终局:quantum 批量证明+快照不推翻已证重锚+桥前缀退役)。

**剩余挂账(用户判定为小问题,下次可接)**:
1. ~~**zoom 时稍卡**~~ → 确诊完成 `23da689`(bt-zoom-perf harness,release 分段计时):字体重测
   0.6-0.9ms、stale 光栅 GPU 采样零逐帧重上传(首帧 cache fill <1ms)全洗清;真大头=off-thread
   Typst 重排窗口 79-124ms/次(不阻塞事件线程,决定 stale 窗口长度)+ 同步 canonical resize/reflow
   1-11ms。无「大头+低风险」可修项,产品路径未动(候选「空 parser tail 免第二克隆」微基准赢、
   端到端 A/B 无稳定收益,已回退)。**后续二选**:①真机窗口 BT_PERF_TRACE 追 headless 里两次
   ~0.5s queue.submit 尖峰(driver/backlog 信号,置信度中);②跨 DPI raster cache/prewarm
   (架构项,消 Typst 窗口)。
2. ~~**zoom 完成后闪一下**~~ → 已修 `398725b`:根因=`finish_resize_if_quiescent` 静止边清
   off-band 队列,把「静止」当成「fresh 光栅完成」——zoom 的干净重印晚于静止判定到达时,唯一
   精确源码见证被丢,重印落地即露源。修=队列释放保留 stale DPI 过渡记录(与其它 drain 点的
   stale-pending 语义对齐,确定事件退场);宽度型 resize 保持原释放。钉死测试修前红;24 录制
   双模逐字节 no-op(zoom 不进 PTY 字节,回放测不到,属预期)。a85ed32 的 2 个转换换边新红
   查实=**同 DPI resize 收尾换边**(scroll-strand 延迟 frame2510:宽块 `A=pmatrix` 回源、
   窄块同帧渲染,invalidations=0,离 resize 极远),另单处理(下条)。
2b. **同 DPI resize 收尾换边**(scroll-strand 延迟红 + resize-endflash 同步红)——占有权
   从宽块切到窄块的转换换边,渲染集合不变;待续修或给忠实行为几何论证。盲目延长同 DPI 保持
   会在普通 reflow 录制里制造新换边(Codex 实测过),需精细修。
3. perf-check ISOLATION_GAP=1(既有)、bridge 判定唯一化(投影层猜 vs visible_frame 真判的
   架构耦合,终局项)、①认证检查点播种(性能加固)、bt-render 选区 band 既有红。
4. Codex CLI 上游忠实行为不修:吃 `\\`/`\[`/`\,`/`=`、reflow 自毁转录、代码块拷贝显源。

以下为历史记录(2026-07-23 收线时点,后续演进见 memory 与 git log):

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
  `scripts/dev/trace_blocks.py <frames.txt>` 直接解析每帧、追踪每个块的 RENDERED↔SOURCE。
  **判据 = `RENDERED -> SOURCE` flips 数(0 = 不闪)**。
- **oracle 延迟完成模式(`BT_PROBE_MATH_LATENCY_US=<微秒>`)**:模拟 off-thread bt-math。设了这个
  环境变量,oracle 就不再在 feed 里同步 render+apply,而是 taken 时打 `now+latency` 时间戳、到
  期才 render+apply,并在 chunk 之间按最早内部截止时间(math 到期 / resize 静止 / live 稳定)驱动
  tick(镜像 winit `WaitUntil`),让"渲染中被重印打断"“落地后立刻被重印"的真机时序在回放里成形。
  **不设=历史同步行为(回归跑法逐字节不变)**。典型:`BT_PROBE_MATH_LATENCY_US=50000`(50ms)。
  注意:同步模式塌缩异步间隙 → 对**单行块的原子重印闪**假绿(块在同 feed 立刻重渲);延迟模式才暴露。
  (故意不驱动 synchronized-update 超时:录制里 `2026h`/`2026l` 成对、feed 内 in-band 收口,驱动超时会
  抢跑把还差一 chunk 的更新提交、打乱回放。)
- **trace_blocks 多行暴露**已补:oracle 现打 `source_plane=...`(整屏逐行文本),trace_blocks 用它把
  多行块 body 被拆进多行 source_rows 的回源也计成 R→S(旧版只认整串/单 `$$` → 多行块假绿)。
  **已知盲点**:alt 借行滚动时,分隔符会瞬帧进 source_rows(env 名匹配误判 1 flip);此处 **crate 内
  `FormulaFlashOracle`(逐 occurrence + occlusion)权威、判 0**,cc-topbot/cc-scrollout 的那 1 是工具误报,
  与产品码无关(修前修后逐字节相同)。占用重的 alt 场景以 crate oracle `flash=` 为准。
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
- `a66eb84` **回看位移跨 transcript 重写保持**:锚死于清史(Codex reflow)时存
  `displaced_review_rows`,重印填回历史后逐帧重新锚定复位;任何主动滚动接管清除。真 cls 场景
  用户一碰滚轮即回旧行为。回归+变异红。~~已知观感:恢复过程可见"跳一下再回来"~~ → 已修
  `33fb866`(见下)。
- ~~**primary resize 公式闪回源码一下(alt 不闪)**~~ → 已修 `002acc7`(见下):根因=`resize_at`
  对 primary 走 `invalidate_all_live_decorations()`(全失效),alt 走 snapshot+stale-artifact
  保持(M1.9)。

### Resize 残留 → 三项全落地(2026-07-24,opus 子 agent 三连)

1. `0848375` **滞留源码块修复 = primary 跨界 bridge**:劈在 frozen/live 边界的块
   (opener+body 已冻、closer 在 live)现在作为**单一 MathBlockPlacement 从 frozen history
   行升入 live band** 渲染。检测层:`live_occurrence_segments` 把 frozen-tail 片段映射为
   `MathSourceLine::Transcript(id)`(真实已证文本,不违 M1.9p),锚定在 live 部分、frozen 段
   作前缀;守卫:≥1 live 行、frozen 行须为连续前导段、staging 空、live 部分起于 grid row 0、
   前缀恰为连续 history tail 且无行已是渲染态 history artifact,不满足回源码(绝不错渲)。
   codex-issues.vt 终态劈裂 `\sum` 块渲染、`i\hbar...\hat{H}\Psi` 旧挂账 live 失败块也渲染,
   codex-formula ever-rendered 3→4,四录制 R→S flips 全 0。钉死测试:detect 两条 + viewport
   `boundary_split_block_renders_as_one_bridge_across_frozen_and_live`。
2. `002acc7` **primary resize 闪回修复 = off-band 保持推广双屏**:alt 的 off-band 队列
   (`offscreen_decorations`,精确 `original_source` 相等才重锚)推广到 primary,仅
   `resize_epoch.is_active()` 期间启用;`invalidate_all_live_decorations` 在窗口内改为
   drain 进队列而非清除(守卫在函数内部,所有清除点全覆盖);`mark_pty_resize_requested_at`
   末尾二次 retain+restore(vendor reconcile 在 resize_at 后 bump grid generation,不补
   会差一代被 viewport 丢弃——第一版就栽在这);`finish_resize_if_quiescent` 清队列。
   重排损毁的 opener(`# $$` 等)精确相等不中 → 正当回源重检测,非闪。resize-repro.vt 三个
   快速拖拽帧公式保持渲染(此前整屏回源);144s+ 的 Source 帧核实为 resize 前(129s)即已
   暴露的流内重印闪,非 resize 引起(见下方挂账)。钉死测试
   `primary_resize_preserves_live_formula_as_stale_instead_of_flashing_to_source`(替换了
   断言旧"resize 即全失效"行为的过时 M1.9e 测试)。
3. `33fb866` **回看恢复跳动修复 = 状态驱动 frame hold**:`review_hold = primary ∧
   resize epoch active ∧ displaced_review_rows.is_some()`(viewport 算,bt-app
   `publish_frame_inner` 读到 hold 且有 last_presented_frame 就跳过 publish → 旧帧驻屏)。
   进入/退出全由确定状态驱动:重锚复位、用户滚动/输入接管、resize 静止三者任一释放,无定时器。
   **真 cls 天然区分**:用户 cls 不开 resize 事务 → hold 不启用,行为同 a66eb84。时序恰合:
   重印在 vendor resize 事务内 staging、`finish_resize_if_quiescent` 收割处正是 bt-app
   republish 处 → hold 跨 清史→重印→收割 全程,释放帧即复位帧,底部帧从未呈现。六条钉死测试
   (viewport 状态机 2 + bt-term 真字节全程 3 + bt-app 呈现 1)。

### 当前挂账(都不紧急)

- **流内重印闪(非 resize)** — **两阶段落地(`1f963c9` 原子类 + `d7adce8` 渐进类)**:
  - `1f963c9` 原子重印:`primary_repaint_in_progress`(`contains_clear_home_snapshot_boundary`
    边界,2026 跨 commit)→ `invalidate_live_row` drain off-band + 精确源码重锚。不收编
    002acc7/59b393e(resize reflow 未必同 feed 带重印边界,互补并存)。
  - `d7adce8` 渐进多行重印:**primary 隔离版 suppress+remap**——`primary_repaint_snapshot`
    在重印边界拍快照,风暴期间 `observe_live_damage` 对已装饰行**suppress 不失效**(旧栅压着
    Codex 改写中的行继续渲染),`finish_primary_repaint` 用 `segmented_row_mapping` **+ 强制
    identity 映射**(primary 重印只改几行,未动的记录必须能原样映射——没有 identity 就把没动的
    记录也丢了,这是前两版试错的关键教训)经 `project_live_record_uniquely` 重投影;未解记录落
    off-band 精确重锚或正当放归重检测。**不碰 alt 函数**,alt 两份回放同步+延迟双模逐字节等同。
    曾实测"resize 期间 stand-down 重印窗口"更差(3→5),故与 resize 保持并存不互斥。
  - **实测(延迟 50ms,基线→现在)**:resize-repro 28→3、resize-endflash 7→4、codex-issues
    3→2、codex-formula 7→4;渲染集不缩(resize-repro 终态 rendered 18→28)。
  - **审因修正**:多行块的闪同步模式也在(旧 trace 多行盲才假绿);单行原子重印闪才是纯异步间隙。
  - **残余(新的下一步)= 边界滚动族,非重印族**:剩余 flips 全是 `invalidations=0` 的回源——块
    整体在网格里,但滚动使顶行(`$$`/`\begin`)越过 live row 0 进 history,`project_live_record`
    (仅 grid 行)返 None、精确匹配对不连续全源失败 → 丢投影。这是 `0848375` bridge 的地界:
    **正解 = 重印窗口与 bridge/occlusion 整合,顶部裁进 history 的记录用 `clipped_top` 重投影
    而非丢弃**(session↔viewport 双侧改,值得独立一单)。基线里这族也闪(被重印闪淹没),非回归。
- ~~**zoom 后部分公式回源码 + 旧栅残片**~~ → 已修 `6b906db`,且**翻了挂账假设**:不是检测竞态,
  是呈现层——`sync_live_math_artifacts` 只收 `render_scale_milli==1000`,zoom 把 live 光栅降为
  按 DPI 比例缩放的 stale 后被拒 → 整个异步重排版窗口回源;band 塌回一行盖不住旧像素 = 残片。
  修=接受缩放刻度 stale(band 按缩放高保留、整栅原子呈现);无需新触发,stale-pending 记录态
  已覆盖窗口、退场全确定事件。两条 zoom 钉死测试按 bt-app 真实调用序列双向驱动,修前红。
  非 DPI 录制构造性无影响(native 刻度是 no-op,回放逐字节同)。
- ~~**zoom 后位置跳变/跳底**~~ → 已修 `ed40450`,又翻挂账假设:滚动锚本来就对(review_hold 已覆盖
  zoom 链,zoom 经 apply_zoom→resize_at 开同一 epoch),真根因=**投影行高在构造时钉死永不更新**,
  zoom 后公式带按旧行距定位、文本按新行距画。修=`sync_projection_state` 先推会话行高进投影,
  变更即全量重投影。resize 不跳是因为不改 DPI。三条钉死测试;回放构造性 no-op 逐字节同。
- ~~**历史奇偶毒块=整屏不渲染**~~ → 已修 `3da6d64`(审因 `docs/reviews/live-norender-audit.md`):
  Codex 重排丢开符 → 冻结历史结构性 `$$` 奇数 → live 扫描(平推 1024 行历史)跨边界时已在"块内"
  → 网格全部块配对错位 0 渲染,zoom 全量重印才救活(用户"间歇不渲染/像吃输出",字节证无内容丢失)。
  修=frozen→live 边界重同步:frozen 前缀的悬空 Dollars 开符,拼合 body 过 `valid_display_body`
  才算真桥,否则废弃、网格重新配对。**oracle 新红门 `isolation_gap`**(孤立可证但缺席检测,
  `ISOLATION_GAP final/max`)。live-norender 0/5→5/5。
  **架构重构三批(2026-07-25,审因×3+Codex 复核定稿后实施)**:
  - `85776a7` 定稿文档:第三轮审因(全局 toggle 对无界单符损伤结构性不稳)+ Codex 复核
    (认证检查点+归属账本、红门拆分、顺序⑥→①④②→③)。注意 codex-review 文件是 wrapper
    合流输出(含 shell 转写),评审正文在文件后段。
  - `54c8d55` **批⑥ 归属账本+红门拆分**:每个结构定界符在扫描器真实判定点记归属
    (Owned/合法拒绝枚举/Orphan),recorder 以 Option 穿线、产品调用 no-op(账本不可能偏离
    检测,有钉死)。oracle 红门拆两层:`BT_PROBE_ANNOTATIONS` 精确标注上游字节损伤(报告不红)
    + `BT_PROBE_OWNERSHIP` 终态未标注 orphan 硬红。18 回放逐字节零行为变更。
  - `8e62e4c` **批④② 剪切证据收容**:缝口相位可证 closed+网格首 `$$` 前为合法数学体+其间无
    开符 → 该 `$$` 按 above-window closer 收容(0848375 桥的镜像,不合成开符不强制相位),
    下游干净重配对。compress-rewrite 红门转绿(detected 7→10,滞留 `\zeta` 渲染)。
    ①认证检查点播种按证据划阶挂账(语料上收容已全绿,①余值=窗口性能+更远毒源加固)。
  - `e50a455` **批③ HeldUnbacked 诚实**:resident 带栅记录若当前扫描不再 own 其源码 →
    上报 `held_unbacked`(帧字段+终态摘要+明细),显示行为逐字节不变;`BT_PROBE_HELD_UNBACKED`
    终态未标注硬红,瞬态只作信号。compress-rewrite 终态 1 例=合法长存(Fourier 开符出窗,
    M1.9p 正当不可重导出,pre-clip hold 撑显示),标注即绿。off-band 队列有意不计(从不上屏)。
  **`3875209` 收敛性修正(3da6d64 首版回归,审修 stream-mispair.vt)**:废弃判定曾不收敛——
  `=ad-bc` 被旧散文启发按 `-` 切成两"词"→跨界真块 body 误判散文→真开符被废→下方整屏 `$$`
  错一位卡死(环境抢跑渲染+裸 `$$`+半行残片三形态同根)。修:①废弃仅当"网格 `$$` 当新开符
  能向前配出合法显示块"(奇偶幻影的真签名);body 仅仅不合法的跨界块只丢自己、保下方奇偶。
  ②散文判定改按空白分词,`=ad-bc`/`mc^2`/`x_i` 不再算词,真散文仍拦。③isolation_gap 对
  bridge 后缀匹配不再误报。stream-mispair 三形态全消;codex-formula 5→4 flips、+2 渲染。
- **共享边**(记录在案,非 zoom 特有):若重排后精确源码锚不上,off-band 记录在 resize 静止时被清
  → 回源。与 resize 路径同边(Codex 重印同文,实际都能锚上),既有行为未恶化。
- 若 resize epoch 在重印到达前先静止(Codex 实测不会,重印即时且撑住 epoch),hold 会释放到
  空底——不劣于 a66eb84 且仍确定性;记录在案。
