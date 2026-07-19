# BetterTerminal 技术方案 v3.7（M1.7 resize 单一所有权修订）

> M1.8-r4（2026-07-18）活动逻辑行修订：harvest 批尾若仍以 `WRAPLINE` 因果连续到
> live 第 0 行，则它不是可定稿历史，而是唯一的可变 staging candidate。下一次 resize 事务
> 开始时，该 candidate 原样交还 vendor native history，由 vendor 将整条活动行与 live 共同
> reflow。这个有界 reverse-harvest 不触碰任何 Frozen/History 行，因而不构成 thaw。失焦窗口
> 保留空心 cursor，聚焦恢复实心 cursor；DEC `SHOW_CURSOR` 可见性与窗口焦点呈现相互独立。
>
> M1.8（2026-07-18）补充视觉稳定性边界：viewport 滚动态只有 `Bottom` 与语义
> `Anchored(logical source, in-source offset)`；resize 只重投影、不创建或改写用户滚动锚。
> `Resized` 回调只允许“新 swapchain 几何 + 新投影 frame”一次呈现，回调前由 DWM 保持上一张
> 完整 back buffer。一次 vendor harvest 内的 WRAPLINE 是同一网格的因果事实，允许重接；已闭合
> 批尾仍强制 `wrap-split`，活动批尾按上面的 M1.8-r4 规则保留。详见
> `docs/M1.8-resize-visual-stability.md`。
>
> v3.7（M1.7 六轮、用户裁决）：撤销 v3.5/v3.6 的「resize 历史严格净零」产品承诺。
> ConPTY 的公开 resize 面没有 repaint begin/end、request ID 或输出批次身份；逐 cell 完全相同的
> 真输出与 repaint 在 VT 事实层不可区分，因此「repaint 全去除 + 真输出零丢失 + 真重复保留」
> 信息论上不能同时保证。新硬边界是：**不崩、不焊接、不静默丢真实输出**。接受 WT 级结果：
> 一次快速拖拽结束后可残留 0～几行由 vendor 原生 reflow 排整齐的干净内容副本。
>
> v3.7 采用单一所有权：resize 事务期间 vendor 原生 scrollback 独占完整可变尾部，事务结束
> 一次性收割进转录；删除 v3.5/v3.6 的 reversible resize staging、settlement 与内容去重账本。
> 本地 surface/live grid 逐个窗口事件跟随；winit 0.30.13 没有 enter/exit-size-move 事件，故以
> 200ms `Resized` 静默提交唯一最终 ConPTY 尺寸，再等 PTY 输出静默 200ms 后收割。

> v3.6（M1.7 四轮返工，**已被 v3.7 推翻，保留演进史**）：本地 PTY resize 打开静默事务；最后一次 resize 后 1200ms
> 且最后一批输出后 200ms 才结算。事务内 full-screen scroll 不定稿；结算按逻辑行将
> 待定稿尾部与 live 顶部去重，真输出保序入历史。补齐 clear 行位置过滤与 reflow 可证上界。

> v3.5（M1.7 用户实测返工，**已被 v3.7 推翻，保留演进史**）：resize 驱逐改为可逆 staging；grow 从 staging 拉回
> live，稳态 shrink-grow 不再冻结历史。resize staging 与 live 由 vendor 的原生 grid
> reflow 联合重排，WRAPLINE 边界空格保留；空白几何行丢弃；resize 行数变化不参与
> viewport 的“新内容到达”offset 补偿。冻结历史仍严格单向，不发生 History→Live thaw。

> v3.3（回应第五轮）：消除三处规格矛盾——① 删除"冻结历史占据新增视觉空间"的残留表述
> （变高=live 屏自身变大，可见历史量不变）；② 变矮的淘汰顺序以 alacritty 实际报告为准并给出
> oracle；③ 锚点迁移统一为两步（捕获时 Live→Staging，定稿时 Staging→History）。

> 前置：`docs/PROBLEM-LIST.md`、`docs/RESEARCH-SUMMARY.md`、`docs/research/`。
> 审核记录：`docs/reviews/codex-review-design-v1.md` ~ `-v4.md`。
> v3.1 要点：scrollback 所有权落定（alacritty=0，转录层唯一）、删除 thaw、ContentAnchor
> 全序/affinity/降级、DetectionRevision 独立、背压容量契约、BlockSource 快照策略。
> v3.2 修订要点（回应第四轮；其中 resize-grow 决策已由 v3.5 用户实测推翻）：① resize-grow 当时落定为不回填历史，live 屏底部加
> 空行（alacritty 无历史语义，**不是 tmux 语义**，v3.1 表述有误）；回填/thaw 作为 post-M0
> 可选增强单独立项；② staging 补 StagingId/跨界 resize 强制拆分/超限强制定稿；③ ContentAnchor
> 增 Staging 变体、按 Screen 分命名空间的全序；④ worker 溢出/最低份额/按数量限流；⑤ G1/G3 补 oracle。

## 1. 总体架构

### 1.1 核心设计不变量

1. **Canonical Transcript 是真相（对已冻结内容）**：冻结时构建规范转录（逻辑行文本 + soft-wrap 层次 + 样式/超链接 span + generation）。富内容是转录区间上的可撤销**装饰**。活动网格内容的真相是 grid 本身——两者由统一 `ContentAnchor` 桥接（§3.2）。
2. **双平面 + 单向冻结 + 可变尾部单一所有者**：稳态 alacritty scrollback=**0**，正常上滚由转录 staging/冻结历史拥有；resize 事务开始时，普通 normal staging 以 `wrap-split` 冻结，但上次 harvest 留下的唯一活动候选先交还 primary native history；此后直到事务结束，resize 驱逐与期间真实输出都只由 vendor scrollback 持有，转录中没有并行 snapshot/settlement 镜像。结束时 vendor history 按当前宽度一次性移交转录并立即恢复 scrollback=0；若批尾仍连续到 live 第 0 行，它保持 staging 而不定稿。Frozen 仍严格 Live→Frozen、无 thaw，且始终是已冻结历史的唯一所有者/配额权威。**不变量拆分(用户裁决 2026-07-19,经独立设计咨询)**:原「活动平面固定行高、只允许等高装饰(§4.3)」把**两条不同的不变量焊在了一起**,现正式拆开——**① 终端坐标不变量(不可动摇)**:ConPTY/vendor 永远持有固定 N×M 网格,应用按行列绝对重绘,富内容**绝不触发 PTY resize**、绝不改变逻辑行列语义;**② 投影布局不变量(可放宽)**:视口如何把逻辑行映射到像素是**我们的**事,因此 live 行**可以**像转录行一样拥有自由像素高度。放宽 ② 的护栏:公式撑高使总高超出窗口时**底部保底可见**(光标/输入/状态栏永不被顶出),溢出发生在**顶部**由滚动承接 + `N rows above` 指示;初期设「整屏额外高度 ≤2 行」实验护栏。收益:live 与冻结**同一套 display 排版、同一张栅格、1000‰ 不缩放**,滚出定稿复用同一 artifact 而非重排——「落地即渲染」与「前后视觉一致」同时成立。**排版模式由定界符决定,绝不由生命周期决定**(`$$` 两侧皆 display;将来的 `$` 两侧皆 inline;math mode 进 render/cache key)。被否方案备查:向 ConPTY 少报行数(动态振荡)、冻结侧改适配(废掉转录自由高度这一最大结构优势且仍不满足直接渲染)、live 单行改 inline(非 LaTeX 语义,仅可作短期止痛)。
3. **文档与投影分离**：`HistoryDocument`（转录+语义块+装饰意图，无布局）；每视口一份 `ViewportProjection`（布局、每视口高度树、滚动锚、选区、折叠）。
4. **核心逻辑与 GUI 解耦**：纯 crate + 语料回放；核心只持 `ArtifactKey` 与测量值。
5. **Damage 驱动调度，完整帧合成**：同 v3（present mode 探测、全事件源触发、`desired_maximum_frame_latency`）。

### 1.2 Workspace 划分

同 v3（bt-pty / bt-term / bt-transcript / bt-doc / bt-viewport / bt-detect / bt-richrender / bt-render / bt-session / bt-shellint / bt-config / bt-app）。bt-term 职责更新：**捕获上滚移出行**（alacritty scrollback=0 下的适配层钩子，必要时 vendor 补丁——R1 的具体化）+ 冻结管线 + 生命周期事件表。

### 1.3 线程与资源模型（v3.1 补契约）

- **会话串行保证**：每个会话绑定唯一的 per-session actor（Term 状态只被该 actor 触碰），多会话在线程池上调度但**同一会话的 VT 处理严格串行**。
- **容量契约（默认值，M-1 实测调优）**：PTY→Term 每会话 1 MiB 环形缓冲，满则阻塞 read（背压传导子进程）；Term→UI 每会话单槽 latest-state 快照（新覆盖旧）。**Worker 队列策略（v3.2 明确）**：每会话 64 任务；同一 span 的新任务**替换**队列中被取代的旧任务（派生工作幂等）；替换后仍满 → 拒绝入队并给块打 `retry-on-idle` 标记（装饰保持 Plain，空闲时重试），**永不阻塞 Term actor**。调度 = 会话轮转 × 可见性加权，不可见会话保底 **1/8 份额**（防饿死）。解析 quantum = 每轮每会话 256 KiB；**渲染类任务按数量限流**（每会话并发 ≤2、全局 ≤8），不适用字节 quantum。
- shutdown/cancel 走独立控制通道；解析线程数 `clamp(物理核-2, 1, N)`。
- **超时即抢占**：数学/SVG 渲染在可终止隔离单元（首选 worker 进程 + Job Object 限时限内存，超时 kill）。
- 预算硬上限与不可信输入防护同 v3。

## 2. 渲染管线

同 v3（cell 宽度权威在 bt-term；延迟指标事件→present 提交，60/120/144Hz 分测，洪水注入法；M-1 做一次光子侧基线校准）。

### 2.1 Unicode cell 宽度（M1 裁决）

- **单一 oracle**：字符、extended grapheme cluster 与 IME preedit 的 cell 宽度全部由 `bt-unicode` 判定；renderer 对 committed 内容只消费 grid 的 `WIDE_CHAR` lead + spacer，不重算宽度。East Asian Ambiguous 默认窄；P2-7 若公开配置，只能切换同一 oracle 的 policy，禁止复制表或另起算法。
- **聚类模式**：DEC private mode 2027 开启 UAX #29 extended grapheme clustering（`DECSET 2027`），`DECRST 2027` 恢复逐 scalar 兼容档，`DECRQM CSI ? 2027 $ p` 按标准分别回报 set/reset。单簇宽度由 `unicode-width 0.2` 的字符串规则判定并钳制到最多 2 cells；VS15/VS16、emoji modifier、ZWJ 与 regional-indicator 序列走同一路径。
- **默认档（ConPTY 实测裁决，2026-07-17）**：默认关闭 2027。Windows 10.0.26200.8875 / system ConPTY 的原始 UTF-8 转发完整，但 Console cursor 簿记对 family ZWJ 为 11、skin tone 为 4、combining 为 2、flag 为 4；发送 2027 后簿记不变，同时 `CSI ? 2027 h/l` 会原样转发。因此 BetterTerminal 保持 legacy 默认，协商成功的程序显式开启聚类，避免未协商程序与 ConPTY 再增加一种默认差异。证据与复现见 `docs/M1-width-correctness.md`。

## 3. 内容模型

### 3.1 内容生命周期事件表（v3.7：单一所有权版）

稳态冻结路径不变：primary full-screen normal scroll 把物理行交给 bt-transcript 的 normal
staging；soft-wrap 铭完整后定稿，4K staging 配额超限时最老候选以 `wrap-split` 强制定稿。
改变的是 resize 期间的可变段归属：**同一可变物理行任何时刻只能有一个持久 owner**。

**事务开始**：第一次实际 grid 尺寸变化清除 mutable selection；若存在上一 harvest 的唯一活动
candidate，先从 staging 取回并恢复到 vendor history；其余 normal staging 以 `wrap-split` 冻结，
随后打开 primary vendor grid 的原生 history（惰性增长；上限取原生
`Line=i32` 可寻址上限）。从此 resize 驱逐和 full-screen output scroll 只留在 vendor history；
adapter 的 removed-row 副本只是一次性的 anchor-rebase 事实，不写入 snapshot、settlement 或
transcript。alt screen 活跃时 owner 仍是停放的 primary grid。

**窗口/ConPTY coalescing**：winit 0.30.13 只提供 `WindowEvent::Resized`，没有 Win32
`WM_ENTERSIZEMOVE/WM_EXITSIZEMOVE` 等价的公开事件。每个事件都立即 resize swapchain 与本地
Term grid，保证 surface/live 视觉跟随；App 仅保存最后 `(cols, rows, pixels)`。连续 200ms
没有新 `Resized` 后才向 ConPTY 发这一个最终尺寸。若提交前又来事件，deadline 与最终值一起
覆盖。§3.3 pane divider 的 debounce 由此推广到窗口级。

**事务结束与收割**：最终 ConPTY 请求发出后，最后一批非空 PTY chunk 再静默 200ms 才结束；
新 resize 会撤销“已到最终请求”状态并继续同一 owner 事务。结束时按 oldest→newest 一次性取出
vendor history、恢复 history limit=0，再移交 normal transcript 管线。同一个 harvest 批次来自
同一份 vendor 网格，批内 WRAPLINE 因而是有序的因果延续，按 normal capture 重接。已闭合批尾
照常定稿；若批尾仍以 WRAPLINE 连续到 live 第 0 行，则整条批尾逻辑行保持为唯一 staging
candidate，下一 resize 事务按原生 row 顺序恢复进 vendor history。该 reverse-harvest 仅作用于
尚未 Frozen 的 candidate；所有已定稿行仍严格单向。全程不做
文本/样式/时间去重，因此真实重复输出必保留，repaint 也可能形成 0～几行干净重复副本。

**三条发布硬不变量**：

1. frame builder 对 history、normal staging、live、padding 每一平面检查 `row.cells == columns`
   且最终检查 `layout.width == columns`、`cells.len == anchors.len == columns*rows`；异宽行以
   `FrameProjectionError` 排除。
2. mutable row 的 owner 只能在 `transcript normal staging ↔ vendor native history` 边界转移，
   禁止 backing/snapshot/settlement 并存。
3. `LatestFrameSlot::publish`、IME composition、renderer `prepare_text_rows`、anchor/word/line
   selection 都先校验矩形并返回可恢复错误；禁止用 slice panic 表达坏输入。

**Trace/oracle**：每个事务按 ordinal + monotonic offset 记录 begin、本地 resize、最终 ConPTY
request、PTY chunk、adapter 行原因与物理宽度、vendor tail 行数、harvest 的宽度/WRAPLINE、每次
publish 的 `columns/rows/layout_columns/cells/anchors/viewport state/offset/cursor`、end。确定性回放必须逐事件相等，并覆盖“多轮模态拖拽 → 最终 repaint
scroll → 收割 → 滚轮上翻 → frame/selection/renderer 边界”。

| 事件 | v3.7 动作 |
|---|---|
| 稳态 full-screen normal scroll | 捕获到 normal staging；逻辑行闭合定稿，未闭合片段保持可变；4K 超限 `wrap-split` |
| resize 事务开始 | 上一批唯一活动 candidate 交还 vendor；其余 normal staging 冻结；vendor primary native history 成为完整可变尾部唯一 owner |
| 事务中的 local resize | 只 resize surface/Term，vendor 原生 reflow history+live；不写转录 |
| `Resized` 静默 200ms | 只向 ConPTY 提交最后尺寸，并开始等待输出静默 |
| 事务中的 full-screen output scroll | vendor history 保存真实终端事实；adapter event 只重定位 live anchor |
| 最终请求及 PTY 输出静默 200ms | 一次收割 vendor history；批内按 WRAPLINE 重接；闭合批尾定稿，连续到 live 顶部的活动批尾保持 staging；恢复 vendor scrollback=0 |
| 快速 shrink→grow 同一事务 | vendor 可从自身 history 拉回并重排；若最终 history 空则收割为空 |
| 多次独立手势/ConPTY repaint | 不承诺历史净零；允许每手势 0～几行干净重复，断言无跨行焊接、无字符丢失 |
| 滚动区域 IL/DL/局部上滚、alt screen scroll | 不进入 canonical primary history |
| 用户滚轮上翻 | 只动 viewport；frame/selection/render 三层都执行矩形门禁 |
| ED3 | 清 frozen + normal staging + 事务 vendor history；块/索引/缓存/锚点走同一删除降级管线 |
| RIS / DECCOLM | 清 normal staging 与事务 vendor history；已冻结历史按既定配置保留 |
| 配额淘汰 | 只由转录层执行，与 ED3 共用 tombstone/anchor 降级管线 |
| 无换行的最后一行 | 稳态留 live；事务期随 vendor tail；不凭 resize 猜测定稿 |

### 3.2 统一坐标：ContentAnchor（v3.2 补全）

```rust
enum ContentAnchor {
    History { id: TranscriptId, offset: GraphemeOffset, bias: Bias, gen: SourceGeneration },
    Staging { id: StagingId,    offset: GraphemeOffset, bias: Bias, gen: SourceGeneration },
    Live    { screen: ScreenId, point: GridPoint,       bias: Bias, gen: GridGeneration },
}
```

- **命名空间与全序（v3.2 修订）**：锚点按 Screen 分命名空间——只有 **primary screen** 参与文档全序：`History（转录文档序）< Staging（StagingId 捕获序）< Live(primary)（row, col）`；alt screen 的 Live 锚是独立命名空间，**不与文档序可比**（选区/搜索不跨 primary/alt）。跨平面比较即按此序。staging 定稿时 Staging 锚随映射表迁移为 History 锚（与持久锚点原子迁移同一事务）。
- **Affinity**：边界锚带 `Bias::Before | After`（行尾/块边界的粘性），选区端点、滚动锚均携带。
- **原子迁移：两步事务（v3.3 统一）**：**捕获时** Term actor 在捕获事务内重写持久锚点 Live→Staging；**定稿时**在定稿事务内重写 Staging→History（携带 §3.1 的映射表）。持久锚点 = 选区端点、滚动锚、无障碍节点、书签。**运行中的 worker 任务不重写**——任务携带 generation，完成时校验不匹配即丢弃（唯一机制，无同步重写承诺）。
- **resize 事务例外（v3.7）**：vendor-owned tail 没有并行的 `Staging` 表示。adapter 报出的物理移除只把受影响的 `Live` 锚重定位到当前 grid generation；事务结束收割时才创建新的 `StagingId`，随后沿正常定稿映射迁入 `History`。因此锚点迁移不会暗中构造第二份 mutable owner。
- **删除降级规则**：锚点所在区间被删（ED3/淘汰）→ 迁至**文档序上最近的后继**锚位（`bias=Before`）；无后继 → live screen 原点 `(0,0, Before)`。选区两端同时被删 → 选区清空。
- 转录规范化同 v3（trailing blank 截断、wide spacer、grapheme 边界表、OSC 8/shell mark 随冻结保留、逻辑行→物理片段层次）。

### 3.3 文档与视口投影

同 v3，一处修订（第三轮 MAJOR）：

```rust
LayoutKey   = (width, dpi, font_rev, theme_rev)          // 只管布局失效
DetectionRevision                                        // 解析器/检测器版本：变化触发“重检测”，
                                                         // 使装饰意图（DecorationLifecycle）失效重建，
                                                         // 而不仅是清布局缓存
LayoutCache 键控 (span, source_gen, detection_rev, LayoutKey)
```

Pane/PTY 所有权同 v3：一 session 一可输入 live viewport（owner 决定 ConPTY 尺寸），其余投影只读；IME/鼠标/焦点/damage 按 ViewportId 归属；`PaneContent = TerminalViewport | NativeHostedPanel` day-1 预留。pane divider 与 OS 窗口 resize 共用 §3.1 的语义：本地投影逐帧跟随，ConPTY 只在 200ms 静默边界接收最终尺寸；新事件覆盖待发送值与 deadline。

### 3.4 块来源抽象（v3.1 定策略）

```rust
enum BlockSource { Transcript(TranscriptSpan), Attachment { id, uri, content_rev } }
```

- Attachment 预览 = **打开时快照 + 显式刷新**：文件变更只点亮"已变更"徽标，用户点击才重渲染（消除 TOCTOU 与抖动的矛盾）；删除 → 占位卡片。
- 搜索语义：Attachment 块不进入文本搜索；复制返回文件路径。
- 网络路径（UNC/映射盘）默认不自动预览（显式确认）；解码资源上限同 v3。

## 4. 检测与装饰

### 4.1 双状态机 + 四版本（v3.1 修订状态图）

```
SourceLifecycle:     Live(含 staging) → Frozen → Tombstoned          // 单向，无 thaw
DecorationLifecycle: None → Pending → Ready | Failed | Suppressed
版本：SourceGeneration（内容/清除）· DetectionRevision（检测器版本→重检测）
      · LayoutKey（宽/DPI/字体/主题→重布局）· ViewGeneration（pane 投影）
```

- 撤销公式 = Suppressed（源码仍 Frozen）；reflow 只作废布局；检测器升级只作废装饰意图；均不触碰 SourceGeneration。
- 任务携带全部相关版本，完成校验不匹配即丢弃。

### 4.2 识别策略 / 4.3 等高 overlay 合成 / 4.4 测试策略

同 v3（LaTeX 默认块级强定界；CommonMark 真解析；overlay 裁剪+pointer-transparent+相交让位原文+无障碍暴露原文）。测试新增：反复 resize 抖动与人类节奏多手势 coalescing、vendor-tail 单 owner、收割 hard boundary、无焊接/无真实输出丢失/每手势有界干净重复、全平面矩形门禁、坏 frame 的 publish/selection/render 可恢复错误、trace 确定性回放、收割后滚轮上翻直达 renderer，以及配额淘汰、RIS/DECCOLM、alt、最后一行等生命周期矩阵项（对齐 G1）。

## 5. 数学渲染管线（M-1 spike）

同 v3（三路径、300+ 样本含恶意输入、进程隔离开销验证、缓存键含 detection_rev + LayoutKey）。

## 6. 会话与 agent 状态

同 v3（显式 side-channel 优先；任务栏聚合 = 窗口内最高注意力状态）。

## 7. UX 骨架

同 v3（布局三档手动固定 + 渐进披露默认；分屏 = 多投影 + 单 owner；egui/AccessKit/M2 去留门）。WebView2 补充（第三轮 MAJOR）：**external protocol 一律禁用**（`msteams:` 等弹确认都不给，直接拦截）；**web message bridge 默认关闭**（预览面板与终端无 JS 通信通道）；导航白名单默认仅 `http://localhost:*` 与 `http://127.0.0.1:*`，跳转其他 origin 需显式确认；下载/新窗口/权限请求默认拦截。

### 7.0 内容 × 形态模型（2026-07-17 用户提出并共同定稿——本节是 §7.1 全部碎片裁决的总纲）

**两条正交的轴。轴一:内容(是什么),三类一等公民,彼此平级**——**会话** Session(身份=profile+cwd+进程;状态=转录/live 网格/输入行)、**缓冲** Buffer(身份=文件路径 **或 无题(转录锚点)**——流式创生、尚无路径的内容即无题缓冲,「保存」=授予路径,同编辑器 untitled 传统;状态=内容/脏标/视图位置;含文本与图片,**终端里渲染的图片与预览里的同路径图片是同一份缓冲**,用户裁决勘误:不设独立"工件"类,内联是形态不是内容)、**场所** Folder(身份=root;状态=展开/选中/滚动)。**轴二:形态(装在哪),七级容器,彼此平级,按承诺递增**:瞬态面(hover)→ **内联锚定**(转录输出流里,跟随滚动;由输出流创生,不可拖入)→ 小窗(同窗口浮层,§7.1.2 模型)→ pane(树上座位)→ tab(工作区)→ 卡片(聚焦态待命密度)→ OS 窗口(M2+ 平台项)。

**三条法则(对所有内容×形态成立)**:① **状态属于内容,随行不分叉**——容器不持有状态(缓冲池/待命池/flyout 携带全套状态,皆此法则实例);② **拖拽换容器,落点定容器**——载荷=内容身份;边缘=pane、标签条=tab、拖出主窗=OS 窗(M2)、文件夹=存副本(即授予/复制路径);③ **一切限制是政策不是结构**——任何内容进任何形态在模型上可行,不开放的组合是显式政策记录(可翻案)。当前政策位:**会话的小窗形态默认关**(用户裁决;开启场景=P1-9 Quake、盯 agent 置顶小窗),缓冲/无题缓冲的小窗形态为**已知缺口待补**;会话恒只有一个可输入视图(§3.3),多投影只读。flyout 的 peek→pin→dock 三级 = 形态梯度「瞬态→小窗→pane」的通用晋升路径,非 files 专利。特有性质:会话——输入所有权/IME/进程生死/注意力;缓冲——脏与保存生命周期、一 tab 一文件一份、干净逐出;场所——无保存概念。

**压测补遗(2026-07-17 二轮,网页/视频/未来物候压测)**。分类判据一句话:**会吃输入、自己会动的=会话族;不吃输入、被动被看的=缓冲;用来找别的东西的=场所**。① **会话是族不是单类**:shell 会话、web 会话(活 WebView,受 v3.1 三禁约束)、AI 对话(P1-14)、直播/摄像头流皆族员,共享输入所有权/注意力/生命周期/卡片活内容;网页静态快照与 reader 渲染=缓冲(身份=URL);**视频文件=只读媒体缓冲,播放进度是内容状态随行不归零**(换形态接着放;同缓冲双视图=同一播放),直播流=会话族。② **所有权三层级**:内容属于 tab(现状,池随 tab)之上预留 **app 级全局宿主**(全局池)——AI 助手对话跨 tab 跟人走的归宿,P1-14 落地时拨开关不改架构。③ 总览/命令面板/通知中心**不是内容**,是观察模型本身的 chrome,不进矩阵(分界而非漏洞)。④ **界外清单(已知装不下,记档不设计)**:复合视图(diff 双缓冲一视图,元组身份破"一座位一内容")、内联交互组件(输出流里的可点组件使会话反成容器,内容套内容递归)、外部 HWND 嵌入(模型可算外来宿主会话,平台深水区)、多输入广播(跨内容动词,动词集无位)。

### 7.1 UI 状态与交互规格（v3.4 新增，原型已验证）

来源：`design/ui-mockup.html` 高保真原型 + 实现者视角审核（`docs/reviews/ui-mockup-review-2026-07-16.md`）+ 27 项逻辑排雷（`docs/reviews/ui-mockup-bughunt-2026-07-16.md`）。**本节是权威；原型是非规范性示例，两者有差异时以本节为准。** 所有数值为 DPI 无关逻辑像素（divider 视觉线为 1 逻辑像素,允许实现取整到物理像素以保锐利）。四条贯穿性纪律，写成行为不变量而非浏览器机制（每条都是原型里真实 bug 的墓志铭）：① **预览与提交共用同一个布局求解器**——蓝框展示的必须是松手后的真实矩形，两套几何必然漂移；② **手势期间命中目标保持身份、未变化的状态不得丢失**（选中/重命名/滚动位置不因无关重绘蒸发）；③ **弹出浮层由集中控制器互斥**——由打开方负责关闭其余，不依赖任何事件路由巧合；④ **实现为独立纯布局求解器,不模拟 CSS flex**（输入树+可用矩形+DPI+最小尺寸,输出叶子矩形）。指针流异常终止（pointer-cancel、失焦）时，进行中的拖拽/divider 调整必须无副作用回滚。⑤ **hover 原则**（用户裁决 2026-07-17,同日二次升格为产品核心差异点）：hover 不只是加速器,是本产品效率身份的一部分——多窗人人都有,把高频信息挂上滑动路径没人做。理论依据(KLM):交互成本的大头是操作**类型切换**处的心理准备(M≈1.35s),不是点击本身;hover 查询是单一动作块零边界,且**查询免提交**(无状态变更、无撤销负担,移开即散)故可自由探索。两条可执行细则:**a. 查询 hover 化、提交必点击**——一切"看"(预览/状态/布局示意)尽量 hover 可达,一切"改"(关闭/保存/换位)必须显式点击,提交摩擦是特性;**b. 交互成本预算**——高频任务 ≤ 2 次滑动 + 2 次点击,滑动/点击模式交替 ≤ 1 次,新功能入规格须自报预算账。约束不变:hover 无法被键盘替代,**任何信息或操作不得只经 hover 可达**,每个 hover 面必须有点击/键盘等价路径;hover 面自身守「意图延迟 + 不可交互 + 离开即散」。

**7.1.1 分屏布局。** 二叉树 `split{dir,ratio}|leaf`。常数：`MIN_PANE_W=260`、`MIN_PANE_H=120`、`FILES_W=240`、`FILES_W_MIN=170`、divider 视觉 1px/命中 7px、根 rim 48px、pane 边缘区 35%。**固定列语义**：files 叶按像素宽参与布局（不占比例份额）；row 内全固定子树 = 宽度求和+divider；**col 内全固定子树 = 成员最大宽**（纵向堆叠不丢固定性）；双固定 row 的尾侧吸收盈余（消灭死白，与预览同语义）。**Files 可拖到任意方向**（用户裁决）：处于 col 切分的纵向槽位时占满该槽全宽，固定宽只在 row 轴成立。divider 拖拽的夹取下限 = 两侧**子树需求**（同向求和+divider、异向取 max），不是单 pane 最小值。run 加入/离开成员时按 demand 重新均分（i3/Zellij 语义，如实承认覆盖手拖比例）。中心 drop：pane 拖拽 = 双向 swap、tab 拖拽 = replace（被替者回标签条），**蓝框内显示文字「Swap panes / Replace pane」**；整 tab 拖拽的预览画整棵子树的真实脚印。不可满足让步顺序：files 240→170 → 窗口最小宽由布局树需求决定 → OS 强制更小时保 focused pane、其余折叠占位，不静默销毁树。**文件拖出**（用户裁决 2026-07-17 两轮定稿,原型已验证）:文件树/浮窗的文件行是拖拽源,**载荷=文件身份(路径),内部拖拽只讲「视图」动词**——任意 pane 边缘/根 rim=按既有 split 语义裂出预览 pane(共享池缓冲,已开文件带着未保存修改到达,绝不分叉);预览 pane 中心=在该 pane 打开;**其余中心(含终端)一律诚实拒绝**。首版曾做「终端中心=插路径」,用户裁决砍除:pane/tab 拖拽的语言是「中心=此位置内容变化」,文本动词混进来使同一手势语义分叉。**插路径改由文件行右键菜单承载**(Open preview / Copy path / Insert path into terminal——插入带引号路径到焦点终端输入),显式、可发现、可键盘化。OS 资源管理器**外部拖入**终端时保留业界约定的插路径(与内部拖拽语言不冲突,M2+ 平台项,OLE 拖出同记);P1-14 AI 助手面是第三落点(**附内容**,如 Claude Code 粘图之于模型通道——PTY 只载文本,模型通道载内容,用户裁决确认 agent 时代必须有)。Esc 中途取消零副作用。**目录动词矩阵已裁决(用户 2026-07-18,原型已实证)**:目录行是拖拽源,载荷=**场所身份(root)**——任意 pane 边缘/根缘=裂出新 files pane 根在该目录;拖到标签条=新 files tab(即刻激活);**files pane 中心=把该 pane 重新扎根到此目录**(与"文件中心落预览=在此打开"对称,视图动词家族;展开集/选中清空,列宽保留);其余中心诚实拒绝;聚焦态下卡片列拒收(场所无卡席)、舞台照既有空间动词拒收。**预览固定右席(用户裁决 2026-07-18)**:预览 pane 默认创建(点开文件、小窗落 Dock)一律落 **root 级最右列**——单例内容配固定住址,不再贴着来源文件树;显式瞄准边缘的拖拽仍落所指。**两条补充裁决(2026-07-17 用户实测追加)**:① 文件/图片拖到**标签条 = 成为新 tab**(新工作区含单个预览 pane,缓冲生于新 tab 自己的池,插位钳在 pinned 分区之后);② 终端来源的拖拽有**「原地」取消区**——指针悬回来源图片的矩形时不出任何分屏 zone,松手即放回(否则源 pane 自己的边缘会截胡"算了"的松手)。**终端内联图片**(媒体交互原型,用户需求 2026-07-17):终端输出里的图片是**带路径身份的工件**——点击=开预览(入共享池);拖出=与文件行同一套视图动词,**另加一个动词:files pane 中心=「存入该文件夹」**(仍是空间动词,不破坏"中心=内容变化"语言);右键=文件菜单+Save as…。原生图片协议片(排在滚动/复制之后)照此交互规格实现,图片锚定在转录行上随内容滚动。

**7.1.2 文件树浮层（三级升级）。** hover 瞬态 peek（intent 180ms；离开宽限 220ms、左侧 420ms；命中容差 8px；触发器→浮层间距 6px；视口安全边距 8px；不抢键盘焦点、不切 tab、自动消散）→ 点击撕下为 pinned 浮窗（可拖拽/缩放，最小 200×150，默认宽 264 × 高 min(主窗口可用内容高的 62%, 460)；**只被 ×/Esc(顶层时)/Dock/再点触发器关闭**，切 tab、关无关 tab、新建 tab 均不动它；主窗口缩小自动重钳位——平移回视口、尺寸不变，与最小尺寸冲突时允许临时小于 200×150）→ Dock 成面板。**全窗口单例**：同时至多一个浮层；对另一触发器的操作重定向既有浮层。root 在召出时从**该触发器所属终端的 cwd** 捕获；撕下后浮窗与来源叶完全独立（来源 pane/tab 消失不影响它）。**Dock 落当前正在看的 tab**（用户裁决 2026-07-17：浮窗悬在哪就落哪，不跳回出生 tab）。Dock/pop-out 双向携带 root/展开集/选中/列宽/**滚动位置**。**首版窗口模型 = 同窗口合成层**，不出主窗口边界；系统子窗口留待真实需求证明。进出动画 120ms，reduced-motion 下关闭。

**7.1.3 文件树。** root 在打开时显式捕获，此后绝不自动跟随 cwd（生态共识：自动重根是最惊吓的行为）。键盘 = VS Code tree 契约（↑↓ 移动、→ 展开/进入、← 折叠/回父、Enter/Space 动作）。**文件上 Enter/双击 = 打开预览**（用户裁决）；预览的最小契约先钉死：**tab 内单实例 preview pane 复用**（再开另一文件换内容不增殖）、打开时键盘焦点不离开树（浏览连续性）、二进制/超大文件/网络路径显示「无预览」卡片而非尝试加载（网络路径默认不自动预览,同 §3.4 的附件纪律）。**文本类预览可就地轻编辑**（用户裁决 2026-07-17）：定位 = quick edit,**不是 IDE**——纯文本编辑、显式保存（Ctrl+S/按钮,原子写）、脏标记;不做 LSP/多光标/跨文件操作,复杂编辑走「在外部编辑器打开」;产品端需处理磁盘并发修改（编辑期间文件被改 → 提示）与超大文件只读降级。**多文件模型 = tab 级共享缓冲池,pane 是视图**（用户裁决 2026-07-17,推翻同日早先的「编辑即转正」,并把缓冲归属从 pane 上移到 tab）：**缓冲属于文件**——每个 tab 一个池,一个文件恰好一份缓冲;所有预览 pane 的文件名切换器（下拉,带各自脏点与开合箭头）列出同一份历史;**同一文件在两个 pane 打开即同一份缓冲**,编辑不可能分叉;未保存修改跨切换保留、**切换零提示零打断**。池是「带活缓冲的历史」而非第二套标签系统:干净且未被任何 pane 显示的缓冲有上限（原型 8）自动逐出、按需再生,脏缓冲与在显缓冲永不逐出。**pane 增殖只经显式图钉**（编辑不改变 pane 生命周期）。**hover 一瞥卡**（用户裁决 2026-07-17,原型已验证）:悬停文件行 350ms(与 tab hover 同常数)出只读一瞥卡——**构造上不可交互**(pointer-events 穿透,杜绝闪烁与指针劫持;想交互 = Enter/双击进真预览 pane 的信号);类型判定与拒绝规则**完全复用预览 pane**(unknown/二进制出「无预览」、网络路径不读);**文件已在共享池中打开时显示池缓冲**(含未保存修改+脏点——一瞥不说谎);离开/按下/滚动/拖拽即散,视口 8px 钳位、右侧放不下翻左侧;产品端 = 异步可取消的 ≤64KB 头部读取。目录行不弹。键盘入口(选中按 Space peek)**暂不做**——现行 Space 已绑打开动作,改绑需动已决项,留待裁决。**脏缓冲的三道确认门**:关闭 tab 内最后一个预览 pane、关闭 tab、关闭应用,均按名列出全部脏缓冲确认——隐藏状态不得无声蒸发。跨 tab 移动:pane 拆出/被顶出时携带其当前缓冲（同一对象）,若是原 tab 最后一个预览 pane 则整池随行;整 tab 合并时两池按「一文件一缓冲、脏者胜」归并。共享范围到 tab 为止,同文件跨 tab 的并发编辑留给产品端磁盘冲突检测。原生实现：虚拟化扁平列表从第一版就上（无阈值切换双实现）、目录枚举异步可取消（loading/permission-denied/已删除/root 不可达四种状态都有对应显示）、节点稳定 ID（重命名/目录删除后选中与展开按 ID 尽力恢复,失败则就近降级）、watcher 溢出重扫、UNC 枚举有超时。

**7.1.4 会话生命周期与持久化。** 恢复 = 位置+布局，**不存输出、不恢复进程**。序列化面（原型已验证对称性）：split{dir,ratio}（id 复活时重铸）、term 叶 seed{**稳定 profile_id**（不是标题、不是展示对象）,cwd,手动名}、files 叶{root,open,sel,width}、pinned、**active tab、focusIndex**；**未提交的重命名在序列化前提交**（blur 语义,输入到一半关窗不丢新名字）。Recent 条目 = 终端 seed **或 files 场所**（关闭纯 files tab 同样可撤销）,**去重键 = profile_id+cwd+手动名**（同位置不同名的 agent 保持独立条目）,含时间戳（P1-13 的 "N 分钟前"）。三扇门一个库：pin 自动回、unpinned 进 restore 提示（非模态、不倒计时自动恢复、窗口先可用）、Ctrl+Shift+T 撤销关闭。**restore 提示未答复时再关窗：未答复计划并回 lastSession**，不得丢失。**Pin 随内容迁移三规则**（P1-11 原文入格）：src 并入 tgt → tgt 继承 pin；pane 被中心 drop 顶回标签条 → 顶出者保留原 tab 的 pin 覆盖；**显式拖出成新 tab → 不继承**（新 tab 未被许诺过）。最后一个 pane 关闭 = 关闭该 tab；最后一个 tab 拒绝关闭（窗口关闭走 shut 流程）；无 pinned 可恢复时以默认 profile 的占位 shell 起步,restore 接受后占位（未被使用时）移除；纯 files tab pop-out 且它是最后一个 tab 时同样以占位 shell 补位。Restart shell（右键菜单）：杀进程原地重启（先温和退出、超时强杀整棵进程树,阈值实现定并写明）,保**最后一次可信上报的 cwd**（OSC 7;无上报则初始 cwd,再退 HOME——不承诺读活进程的真实 cwd）与手动名；busy 判定无 OSC 133 时保守视为 busy；**新进程环境按 P2-12 在 spawn 前从注册表重建**,不继承终端启动时的旧环境块;旧进程的挂起输出以 generation token 作废;重启前的转录保留、插入一条重启边界记录。「↑ 历史不丢」的 UI 文案**仅限检测到 PSReadLine 且其持久历史启用的 profile**,对 cmd/bash 不许诺。原生补充：版本化 schema、临时文件+原子替换、**写盘失败显式告警**（不假装 pin 成功）、正常退出/崩溃/系统关机三条路径区分（崩溃恢复的提示措辞不暗示进程还活着）、**逐叶降级**（未知 profile→默认；无效 cwd→HOME+提示；未知 leaf kind→占位 pane；非法 ratio→clamp；整树不因单叶损坏而丢）。

**7.1.5 焦点与命令路由。** 窗口级键盘焦点有唯一 `InputOwner = Terminal(viewport)|FilesTree|Rename|Menu|Dialog|None`（P2-5;Menu 涵盖右键/弹出菜单/combo,Dialog 涵盖设置/确认框/restore 提示）,不是全局键盘钩子+豁免表。**它与 §3.3 的「session 可输入投影 owner」是两个概念**：前者答「窗口里谁收键盘」,后者答「一个 session 的多个投影里哪个可输入」;字符只有在前者=Terminal 时才进入后者指定的投影。**Owner 迁移规则**：新建/恢复/合并产生的终端立即成为 owner（P2-5 的本体）;关闭 owner 所在 pane/tab → owner 移交给新的 focused pane;Menu/Dialog 打开即接管、关闭即归还打开前的 owner;Rename 提交/取消同理;窗口失焦不改 owner(回焦即恢复)。**IME 只绑定 owner=Terminal 时的可输入 viewport**。**Esc 分层路由，每按一次只处理最上一层**：进行中的拖拽/divider（取消，不提交 drop）→ Menu → Dialog → pinned 浮窗 → **owner=Terminal 时**原样透传 PTY（vim/fzf 不可破坏）;owner 为 FilesTree/Rename/None 时 Esc 由该 owner 消费或忽略,**绝不进 PTY**。菜单开启期间拥有键盘（字符不得漏进终端）；模态遮罩 z-order 高于一切弹出层与浮窗,且模态期间布局快捷键失效。快捷键绑定（含 Ctrl+B 与 tmux 前缀冲突）**留待统一快捷键审计**（P2-7,按 profile 可覆盖）。

**7.1.5b 会话状态分类学(用户裁决 2026-07-18,原型已实证)。** 状态属于**会话**,tab 聚合、卡片各显其own,三个面(横 tab/竖 tab/卡片/监视卡)共用同一 stateIcon 组件。七态:**busy**(图标呼吸,自有通道)| **进度**(图标外圈进度环,OSC 9;4 四相:normal 靛蓝/error 红/paused 黄/indeterminate 旋转虚线;百分比进 tooltip;WT 同款,winget/PS7.4+ 即发)| **未读·完成**(蓝点,OSC 133 退出码 0)| **未读·失败**(红点,退出码非 0——agent 时代"跑完了 vs 跑炸了"必须一眼可辨)| **等你回答**(橙点脉动,agent 阻塞在交互;bt-shellint 检测,P1-8 注意力队列本体;卡片同时橙框)| **bell**(橙点不脉动,BEL)| **dead**(图标变暗去色,tooltip 退出码)。**一点一断言**:点位冲突按严重度取一——等你回答 > 未读·失败 > bell > 未读·完成;进度环与呼吸/点共存(环是独立通道)。呼吸不带色、点才带色的既有纪律保持(running 与 finished-unread 是不同断言,不得同色抵达)。进度 tick 走定向刷新(per-session 更新所有已渲染 stateIcon),不触发全量 render,拖拽中不受扰;reduced-motion 关脉动与旋转。原生映射:OSC 9;4(进度)、BEL、OSC 133(退出码/busy 边界)、shellint 交互检测;聚合规则=tab 取成员最高严重度(原型先用 identity 会话+未读聚合近似,原生实现完整聚合)。**进程分层与 dead 策略(用户裁决 2026-07-18)**:一个 pane = 一棵进程树——**根进程**由 profile 决定(powershell/wsl/ssh/python REPL/直启 agent 皆可),**dead 态只看根**;shell 内跑的一切(python/cargo/claude)是**子进程**,其退出走 busy→未读·完成/失败(OSC 133 退出码),pane 不死。**closeOnExit 可配**(always/graceful/never,默认 graceful):干净退出(0)关 pane;非零/崩溃 → dead 停尸态(图标暗、tooltip 退出码、**转录原样保留**供验尸,右键 Restart 原地复活)。推论:agent 从 shell 启动(子进程,退出留 shell 现场)为推荐形态;直启 agent profile(根进程)合法但 agent 死即 pane dead;**无 shell 根进程没有 OSC 133**——busy/成败检测降级为输出流启发式+根退出码,状态机必须有此降级路径;Restart/关 pane 杀整棵进程树(Job Object),不留孤儿。进度环几何:25px(15px 方形 mark 对角半径 ~10.6px,环必须清出 12.5px 否则穿角——方图标实测勘误)。**注意力队列第一期(P1-8,用户批准+三轮勘误 2026-07-18,原型已实证)**:标题栏齿轮左侧一枚**安静小徽章**(5px 橙点+「N waiting」,21px 药丸,次级墨色 hover 升满;零等待=零占位卸载)。队列语义:**先到先服务**(等最久者最该被回答;时间戳用单调序号,mock 可确定重放)、**回答才消费**(在该会话按 Enter;看一眼阻塞的 agent 不解除阻塞)。动词:点徽章或 Ctrl+Shift+A(临时键位,归 P2-7)跳到等最久的会话,已在队列中的会话上再按=走到下一个,循环遍历;**聚焦态下跳转=换上舞台**(与点待命卡同一交易,uid 对调;舞台非终端席则落 tab 由卡片橙框指路)。设置「Waiting badge」**只关 UI 不关能力**(用户勘误:药丸令人心慌,但队列本身要留——关掉后 Ctrl+Shift+A 照常)。实现教训入册:`.attn-chip{display:flex}` 压过 UA 的 `[hidden]` 规则,显隐类断言必须验 getComputedStyle 而非属性(用户实测抓获)。**7.1.5c 命令标记轨(用户批准 2026-07-18,同日二轮勘误后定稿,原型已实证;minimap 第一期)。** 终端 pane **右缘垂直居中**一列**序数等距**刻度(Codex 同款:位置载序不载滚动几何,"是哪条"由一瞥卡回答),每条历史命令一个;**平滑峰效应**(用户四轮勘误定稿,含一次"高/宽"术语乌龙——横躺的条,用户的「高」=长轴):**粗细恒定 2px**,**长度**沿指针邻域呈平滑钟形峰(9→27px,峰值 3×,±3 刻度余弦衰减;右对齐列宽度只向左生长,纵向布局零位移零抖动),**被指的刻度颜色最深**(全不透明+accent 混 12% 黑加深)并携带一瞥卡,条上任意处点击跳最近刻度;**峰是选中序号的函数,不是指针像素的函数**(用户五轮勘误定稿:先定最近刻度,再按整数序号距离算钟形——任何时刻恰有一个明确的峰,指针跨中点时峰整刻度切换,宽度过渡使切换读作运动;指针停在两条之间不出现并列半高的含糊态)。**满轨降级(用户提问 2026-07-18,①已实证,②待用户拍)**:①密度压缩——间距随条数从 9px 收缩到 4px 地板(最近刻度数学不依赖命中 2px 小条,任意密度可操作);超地板后暂显最近容量条+溢出提示 tooltip;②**已裁定 A 并实证**(归并采样+鱼眼展开):超地板后每刻度=一段命令(k=⌈N/容量⌉),段内含失败常驻红(原生按退出码);**指向某段即原位展开为独立子刻度**(子刻度细一档 7px,组内滞回——悬在展开区内不折叠,移到别段换组,离开轨折回),峰落在子刻度上、一瞥与点击精确到单条;未展开段一瞥示「k commands · latest: …」、点击跳段内最新。B(最近窗口)弃;③铁律:Ctrl+Alt+↑/↓ 键盘遍历永远逐条全量,不受轨显示策略影响。**静止灰、hover 条区着色、错误刻度常驻红**(查询灰、信号红,与状态点哲学一致)。**幽灵蓝点结案(2026-07-18 三轮追凶)**:元凶=`.peek-list span` 通配规则特异性压过 `.unreaddot{display:none}`,示意图里无状态的点被强制显示——收窄为直接子代选择器;并补语义门:**progress 活跃 = 工作未完,不产生未读点**(下载完才谈未读,tabDotClass/stateDotClass 双处封门)。hover 刻度 → **一瞥卡**(命令单行等宽字,构造上不可交互,离开即散);点击 → 跳转至该命令行(平滑滚动+0.95s 高亮闪);**Ctrl+Alt+↑/↓ 键盘走刻度**(hover 铁律)。**布局示意图(tab peek)勘误之勘误(用户裁决)**:peek 名单**恢复 status**(每 pane 显示自己的活状态)——此前的错不在 peek 显示状态,而在 **tab 聚合说得比 pane 少**(pane 有点、tab 无点);修复=tab 点位取**成员严重度最大值**(tabDotClass:等你>失败>bell>未读,逐成员按各自 busy 门控),tab 永远不比 peek 沉默。原生映射:刻度=OSC 133 命令边界+转录逻辑行锚(G3),错误红=退出码非 0;跳转=Anchored(逻辑行);先轨后搜,搜索复用其跳转机制。**7.1.5d 会话内搜索(用户批准 2026-07-18,原型已实证)。** Ctrl+F 在焦点终端 pane 右上角出**驻留胶囊条**(搜索是停留态,占角不占台——与命令面板式大浮层相对):查询框、计数 n/N、Aa/ab/.* 三开关(大小写/整词/正则;非法正则就地红字+计数「—」,错误说在打字处)。Enter/Shift+Enter 走匹配(环回),F3/Shift+F3 在键盘还给终端的姿态下仍可走(Enter 归 shell,不可征用),Esc 关闭且视口留在原地;重开保留上次查询并全选(打字即换、Enter 即续)。当前命中 accent 实心(文字用 --termbg 反衬——两主题各自的终端底色恰是与自家 accent 对比的墨色,零新增主题令牌),其余命中 30% 半透明;**命中在屏内则打字不拽视口**,只有落屏外才滚动(1/3 视口处落位)。**结果轨 = 标记轨的第二内容源**(一轨两源,§7.1.5c 机制可复用性的实证):搜索激活期间匹配行以 accent 刻度**并入同一序数栈**(与命令刻度按行序归并),命令刻度退灰但保点击跳转,当前命中刻度不 hover 也常驻深色;聚合桶携最强成员信号(含匹配则亮、含当前则深——与 tab 点最大值同规则),密度压缩/鱼眼/一瞥/点击全套照用;**点匹配刻度 = 选中该命中**(计数随动),点命令刻度仍是跳命令。关闭即还轨,零残留。范围分层(用户裁决):第一期=会话内(回滚缓冲);跨会话全局搜索待原生转录索引(§3.1 canonical transcript,UI-UX §九),Clear scrollback 的 ED3 管线语义已为其预留。原型双限已注记(原生索引器不共享):跨样式 span 的命中不高亮(逐文本节点匹配)、整词=\b(ASCII 词界,CJK 整词需真分词);原生按逻辑行索引而非折行行。搜索开启期间 render 全量重建后胶囊/命中/结果轨原位重挂(焦点与光标走 pv-edit 同款保全契约)。快捷键最终归 P2-7 审计。**7.1.6 菜单与杂项。** **tab 溢出**(用户报 2026-07-18 两轮):横条窄档分两级(均为测量事实非数量启发式)——**tight(<140px)**:hover 控件(pin/files)退场、close 仅活动 tab,标题保住地盘(否则一 hover 三控件把标题挤成 0 宽,图标泄露);**常规宽度下 hover 控件静止时零占位**(用户三轮勘误 2026-07-18,推翻同日的 order 换位方案):DOM/阅读顺序保持 标题·徽标·files·pin·×,隐藏控件宽度收为 0(徽标自然贴 ×,无死空隙),hover 时按宽度动画滑入、徽标被连续推开(.16s ease,reduced-motion 关);pin.on(钉住态)常驻不收;**squeezed(<90px)**:收拢为居中图标;到 46px 地板后**条内横向滚动**,永不侵入标题栏按钮。竖栏 tab 行高**定死 30px flex:none**(列表滚动,行永不压缩——flex-shrink 默认值曾让行先被压矮到极限才轮到滚动),「New tab」行 sticky 钉底随时可及;卡片迷你体 pointer-events:none(嵌入内容的 tooltip/点击不得外泄——卡片是表面不是文档)。**tab 激活时机 = 按下即激活,带 ~180ms 宽限**(用户裁决 2026-07-18 两轮,Edge 同款+勘误,原型已验证):左键按下后 180ms 落定切换;宽限内拖出条外去分屏则**全程不切页**(旧的立即切换让瞄准目标闪失一瞬);按住不动或条内重排即提交激活(重排=对条上下文的承诺,无论计时器是否已到);快速点击松手即切,无感知损失。两条配套规则:**a. 离栏回切**——拖着已激活的 tab 离开标签条去瞄准布局时,视图切回出发时所在的 tab(按下说「选 A」,离栏说「把 A 放进我刚才那个布局」),合屏/替换手势因此保住目标;回栏再切回、取消拖拽守按下的承诺(激活停在被按的 tab)。**b. 聚焦态例外**——聚焦态中侧栏 tab 按下不激活(按下可能是入组拖拽的起手,先切走会拆掉它要瞄准的舞台),松手不拖才是点击切换。终端右键菜单序：Copy、Paste、Select all、──、Clear screen、Clear scrollback…、Restart shell…（高频无害在前,破坏性沉底带省略号）。**Clear screen ≠ Clear scrollback**：前者 = **终端本地执行 ED2+光标归位**（不向 PTY 注入任何输入、不触碰转录与 staging、清除 live 选区——被清的行走正常 scroll-out 进转录,之后仍可上滚/搜索）；后者 = **完整的 §3.1 ED3 删除管线**（转录、staging、块、索引、缓存、锚点降级、tombstone,一个都不少）,影响全局搜索,需确认。pinned tab 防误关是特性（先 unpin 再关）,可发现性靠图钉 tooltip 与右键菜单文案,且 pin/unpin/close 须有纯键盘入口（命令面板/快捷键,不能只活在 hover 上）。tab hover 350ms 出**布局示意图**（按 ratio 画的结构示意,明确非实时终端快照——无离屏纹理、无隐私泄漏）,在指针离开/拖拽开始/该 tab 关闭/窗口失焦时消散。拖拽预览的 refused 态（虚线灰框）表示布局放不下,drop 无副作用。主题 System/Light/Dark 跟随系统,高对比度模式跟随系统调色,首版不做整窗主题切换动画。**命令面板(P1-9 第一期,用户批准 2026-07-18,原型已实证)**:Ctrl+Shift+P(WT 同键,终端命令面板的事实标准;纯 Ctrl+P 属 readline 永不占用)呼出**无遮罩浮框**(上三分之一,世界保持可见——瞄准不是开模态),一张平列表两类源:**会话**(名字模糊匹配→直达,聚焦态走换舞台)+**动作**(新建各 profile/pin/关标签/重启 shell/查找/跳等待/主题/布局/聚焦/设置)。子序列模糊打分(连续命中与词首加分,命中字符 accent 高亮),↑↓ 环回、Enter 执行、Esc 走分层路由顶层、点外即关;面板改主题/布局与设置 combo 双向同步;聚焦项带 >4 终端 pane 资格门。**键位哲学(用户裁决)**:面板搜**意图**不搜内容(内容归 Ctrl+F/全局搜索);低频动作(pin/restart 等)**默认不发专属键**——终端键位稀缺且必须避让 TUI 程序,动作生在面板里,高频者经 P2-7 审计"毕业"成专属键,且键位 per-profile 可配。未来源(框不变源渐增):recent 会话(P1-13)、文件模糊路径、跨会话命令历史(待转录索引)、命令标记直跳。

**7.1.6b 串行/并行聚焦态（2026-07-17 方向已定,待原型上手确认后转正;demo=design/sidebar-focus-demo.html）。** 来源:折叠屏多窗口调研(vivo 串行/并行双注意力形态)+ 用户五问裁决。同一工作区(tab)的两种注意力形态:**并行态** = 现有分屏树全可见;**串行态(聚焦态)** = 一个 pane 撑满,其余成为左缘一列**实时卡片**(迷你真投影:按卡片宽度重投影转录尾部若干行,依托 §3.3 多投影;damage 驱动节流,不可见卡片暂停投影)——卡片保留名字/状态/实时内容,不做纯缩略图(终端会话视觉同质,纯缩略图不可辨认)。裁决五条:① **聚焦态是竖直文字侧栏模式的专属能力**(用户二次裁决 2026-07-17,推翻同日早先的"三布局各给一列":临时悬浮列是"没有固定住址的 UI 元素",与拖拽动词简化同一病;侧栏是会话导航唯一的家,聚焦态只是它的展开密度)——卡片列**嵌入侧栏当前 tab 之下**;horizontal/icon/收起模式下不可进入;**存续宽于进入**(用户勘误 2026-07-18):聚焦中**收起侧栏不退出**——收起是"给舞台腾地方",卡片随栏隐身、展开随栏归位,待命池原样存活;切到 horizontal/icon 布局才自动退出(家具没了,模式随之退场)。富内容行(公式/图片)在卡片里**摘要化为标签片**而非缩放(可读性是卡片的全部价值,压扁的公式是噪声,「∑ 公式」是信息)。单 pane tab 允许进聚焦(卡片列仅「+」)。原型已实证(2026-07-17):进出资格门、rail 内嵌锚定、树零变形、待命池跨进出持久、富内容摘要片。② **串行与并行是同一组 ≤4 窗口的两种看法**(用户三次裁决 2026-07-17,推翻"无上限待命池隐身存活"——隐藏状态违背本 UI 的一贯品味):聚焦态总窗口数(台上+卡片)**上限 4**,「+」到 4 禁用;**树上 pane >4 的 tab 不可进入聚焦**;**退出聚焦时「+」生的会话全部物化上树**(自动落座:最大 pane 沿较宽轴分裂,四座自然成 2×2),待命池清空——**没有任何东西在并行态隐身**。4 的依据=最小 pane 尺寸下典型屏的 2×2 诚实容量;要盯更多 agent 用更多 tab(tab 本为此设)。§7.0「会话属于 tab」作为架构保留(聚焦态内的待命即其实例),隐身持久化政策废止(法则三:限制是政策)。原型已实证:封顶/禁用/拒入/物化/清空全链路。③ **其它 tab 的卡片:手动展开可以,自动不展开**——点其它 tab 行箭头展开其卡片以监视,**展开≠切换**(Stage Manager 教训),注意力徽标仍在细行常驻。**原型已实证(2026-07-18)**,补三条细则:监视盒 = 聚焦列减去控制件(全部座位实时卡,无「+」无 4/4,cardSeq 排序只读不回写);**点监视卡 = 显式导航**(展开是看,点击是去——跳到该 tab 并聚焦该 pane),监视卡不可拖不可关(它是看进去的窗,不是把手);**卡片已展开的 tab 抑制 hover 布局示意图**(真身就在行下,弹窗纯属重复还挡卡——聚焦中的活动 tab 同理;用户报 2026-07-18);展开箭头走零占位滑入模式,active tab 不给(聚焦态服务它)。④ **files/预览 pane 不特殊化**:聚焦态下同为卡片(预览卡=文件名+脏标,files 卡=目录名),点卡换位规则一致;既有 flyout 通道(Ctrl+E 等)在聚焦态照常工作。⑤ 进出聚焦态:双击 pane 标题/快捷键(键位归 P2-7 审计);退出时并行树**原样恢复**(布局参数在聚焦期间不被触碰)。**进出走树的同一套 FLIP**(2026-07-18 落地):舞台 pane 在原槽位与全幅之间滑动(位移+缩放,内层反缩放保字号),退出时其余 pane 淡入回座;换台(点卡)= 新舞台淡入;卡片列在模式开启瞬间随动画入场(挂载动画只放一次,innerHTML 刷新不重播);reduced-motion 全关。注意力队列(P1-8)的物理形态 = 卡片/细行上的「等你」徽标,聚焦该会话即消费。交汇点无极调节(并行态,拖动 divider 交点同时调双轴)一并纳入。⑥ **聚焦态的拖拽语法**(用户裁决 2026-07-18,原型已验证):**舞台拒收空间动词**——聚焦态右侧是「当前卡片的投影舞台」不是树,tab/文件拖上去分屏 = 在投影上做树变更且要等退出才物化,是隔空改状态,一律诚实拒绝(refused 蒙层 + 文字指路);**卡片列是聚焦集的容器**(§7.0 落点定容器):拖 tab 入卡片列 = 该 tab 的会话入组为待命卡(与「+」同一法则:限 4、满员 deny、退出物化上树;pin 随内容归并;仅全终端 tab 可入——卡片是终端,含 files/预览 pane 的 tab 半入即撕裂,拒);**卡片拖出列 = 退组自成 tab**(入退同一动词:leaf 卡走 extractPaneToTab 从停放树取出,待命卡直接成新 tab;退组不退聚焦,舞台不受扰)。呈现细则(用户勘误 2026-07-18 二轮):**入组预览 = 列尾虚线替身卡**(所见即将得,一会话一卡),不给整列描框,舞台拒收蒙层的指路也用替身卡;**悬停「+」时按钮自身亮起**(它就是入口,替身卡叠上去反而话多),落「+」= 同一入组动作;**悬停卡片区时浮动芯片隐藏、被抓的 tab 归位槽内**——替身卡/亮起的加号就是「手中之物」,芯片再跟着是重影(用户报 2026-07-18);满员才用列级 deny 描边+4/4 转醒目;**卡片头部佩会话图标**(与 tab 同源 stateIcon,busy/unread 随行),不用抽象圆点——卡片是小号 tab,穿 tab 的衣服;**拖出预览槽位钳在卡片列之下**(卡片挂在其 tab 正下方,槽位不得插进 tab 与自家卡片之间),strip 预览用 stateIcon+标题的正装。⑦ **待命池收预览内容**(用户需求 2026-07-18 三轮,§7.1.6b ④「预览不特殊化」的推论):终端图片/文件拖到卡片列 = **以预览卡入组**(缓冲入 tab 共享池,待命项从纯会话 uid 扩为「会话|预览缓冲」两型),容量同 4 之门;退出聚焦时预览待命**物化为预览 pane**(与会话同法则);预览卡拖出列 = 自成预览 tab(缓冲对象随迁,编辑随行);**点击预览卡仅当舞台是预览位时换内容**(缓冲对调),终端舞台无它的座——闪烁拒绝而非无声。两处配套修正:**舞台在单叶树下必须保留 panehead**(头部承载身份与双击退出,树只剩一叶其余全待命时曾消失——multi 判定补 focusLeaf);**dock-preview 淡出期间保留 refused 灰色**(hidePreview 立刻摘类曾让残影闪回蓝色)。⑨ **容量按内容类分层 4+1+1**(用户提议+协调者背书 2026-07-18 五轮,细化②的一刀切 4):**终端 4 席**(2×2 诚实容量的本义就是终端 pane 的几何账;fc-cap「4/4」与「+」禁用只看终端席);**files 1 席**——固定 240px 列不占比例份额,聚焦中 Dock 文件树 = 落进该席,已占则**替换视图**(顺带堵死 Dock 绕过上限的漏洞:旧逻辑无条件 split 停放树);**预览 1 席单例**——继承 §7.1.3「tab 内单实例复用」:席空则入组,已占则**替换内容**(悬停时高亮占位卡示意"将被替换",不出替身卡),退出物化时优先复用树上的单例预览 pane 而非新增比例座位;进入门槛同步改为「树上**终端** pane >4 不可进入」。**卡片皆可关**(用户报"小窗关不掉"):卡头 hover 出 × ,动词=关内容(与 pane/tab 的 × 同语言)——leaf 卡=closePane、待命会话=杀会话(busy 确认)、待命预览=弃卡(脏确认;干净且无 pane 引用的缓冲随卡离池)。⑧ **卡片列三则**(用户裁决 2026-07-18 四轮):**列内拖动 = 仅换位**——绝不切台(与「展开≠切换」同族;拖完的 click 被吞,不误触换台);排位跨渲染持久(cardSeq),**退出物化按卡序落座**——你摆的列就是座次表;**换位是 tab 式跟手**(用户勘误 2026-07-18 三轮):被抓的卡贴着光标走(基线=按下点,不吃拖拽阈值),邻居在被压过一半时滑开(FLIP),拖拽期间零重渲染、DOM 即真序;**入组替身卡可站任意槽位**(指针命中处,不只列尾),落地按替身卡站的位置入 cardSeq;**卡片默认高度一律 72px**(终端/预览/files 同高,一列同刻度才是一件仪器);**files 卡有身子**——迷你树(复用 filesVisibleRows 前 4 行,app 字体、浅底,区别于终端卡的深底等宽),此前只有头(用户报)。

**7.1.7 规格债（M2 原生 UI 开工前必须闭合,承载于专门的 UI-SPEC 文档）。** 本节裁决了方向与语义;以下形式化工作有意后置——原型仍在演化（文件预览特性将再动树 schema）,现在穷举会返工：① 布局求解器的输入/输出与不变量级形式规格（fixed/flex 混合 row 的完整分配公式、含 fixed 子树的 rebalance 算法、ratio 序列化精度与 clamp、rim/edge 重叠优先级与角落选边、高度方向的让步链、「折叠占位」的几何/命中/焦点/展开条件）;② 持久化 schema v1 全字段定义（顶层版本、tab 顺序、类型与缺省、未知字段策略、open/sel 的稳定 ID 格式）;③ Restart shell 的完整进程契约（elevation/token 处理——最可能被平台收窄的承诺,见审核记录）;④ 极小窗口下浮窗约束冲突的完整优先级表。审核依据：`docs/reviews/`（2026-07-16/17 两轮）。

## 8. 依赖策略

同 v3 表格，关键修订：**alacritty_terminal 稳态配置 scrollback=0**。vendor seam 包含既有上滚事件钩子，以及窄事务操作：打开 primary native history、查询行数、在 coalesced final viewport 上用 vendor row/WRAPLINE/cursor 重新评估高度、一次性 `take_history(oldest→newest)`、清空并恢复 limit=0，以及只针对唯一未闭合 staging candidate 的 `restore_history(oldest→newest)`；没有可独立呈现的 transcript snapshot/backing 镜像，也不复制 reflow 算法。事务期 vendor grid 是 mutable tail 唯一权威；收割后转录层拥有 staging ID/配额/定稿权，vendor 只保留该候选的原生 row escrow 供下一事务无损交还。升级必须 diff `grid/resize.rs`/history 语义，跑 vendor 181 项与完整生命周期矩阵。

## 9. 里程碑

同 v3 的 M-1→M5+ 结构，三道退出门按 v3.1 修订：

- **G1 生命周期回放矩阵**（v3.7）：`scroll-out→normal staging→定稿→decorate→rewrite`、vendor-tail 单 owner、窗口/pane 最终尺寸 coalescing、变宽/变窄/变高原生 reflow、**变高后 ConPTY 全尺寸可寻址性**、变矮收割、连续真实输出守恒、六段 resize 风暴与多轮人类节奏手势。硬断言是**不崩、不焊接、不静默丢真实输出、frame 恒矩形**；允许每个独立手势 0～几行 vendor 排整齐的干净重复，不再断言 frozen 净增长 0。另含 WRAPLINE 边界空格、底部跟随、正常 staging 配额强制定稿、ED3、淘汰、alt 进/出、滚动区域、RIS/DECCOLM、最后一行、trace 双次确定性回放、收割后滚轮上翻直达 publish/selection/renderer——全过。
- **G2 多投影一致性**：同一转录在两个不同宽度视口同时显示，高度树/选区/滚动锚各自独立且正确。
- **G3 锚点与版本协议**（v3.3 统一术语）：持久锚点两步原子迁移（捕获时 Live→Staging，定稿时 Staging→History）+ ED3/淘汰降级正确；**Staging 锚定稿迁移**、**primary/alt 命名空间隔离**（跨屏比较被拒绝）验证；worker 任务 generation 失效丢弃零泄漏；四版本失效边界行为正确。

三门任一不过 → 按 R10 降级砍功能，不顺延。

## 10. 风险登记

v3 表保留，v3.7 重写 resize 风险：**R11' vendor native-history 依赖**——升级可能改变 reflow/history 顺序；vendor 面仅限事务开/取/清、唯一活动候选恢复与事件钩子，不复制算法，升级强制 diff `grid/resize.rs` 并跑 181 项 vendor 测试和 G1。**R13' ConPTY 不可判定 repaint**——公开字节流没有“重画身份”，不得以内容相等去重；同一 vendor harvest 批内只信 WRAPLINE，闭合批尾强制 wrap-split，唯有因果连续到 live 顶部的未闭合批尾可跨事务交还 vendor，并接受每手势有界干净重复。**R14 窗口模态边界不可见**——winit 0.30.13 无 enter/exit resize 信号，以 200ms quiet coalescing 代替，真实多轮拖拽为终审 oracle。**R15 长事务内存**——vendor history 受 native `Line=i32` 可寻址上限保护，持续输出会增长；final request + output quiet 保证正常手势及时收割，trace 记录 tail 行数。**R16 非矩形纵深失败**——builder、publish、composition、selection、renderer 五层均返回可恢复错误并有坏输入测试。

## 11. 已决问题（累计）

v1 Q1-Q4、v3 各决策（alt=停放、一 session 一可输入视口、折叠属视口、overlay 让位、任务栏聚合）、v3.1 决策（稳态 scrollback 所有权=转录层；原子迁移仅持久锚点；DetectionRevision 独立；Attachment=快照+显式刷新；WebView2 三禁）、v3.2 除 resize-grow 外的决策及 v3.4 UI 决策继续有效。演进史保留：v3.5 曾以用户实测推翻“不回填”，选择 reversible staging + live 联合 reflow；v3.6 又加入静默 settlement/内容判等以追求 resize 历史严格净零。**v3.7 用户裁决明确推翻这两版的净零承诺及三账本机制**：在 ConPTY 公开接口上，“真实输出零丢失 + 真实重复保留 + repaint 严格净零”信息论不可兼得；产品目标改为不崩、不焊接、不静默丢真实输出，事务期 vendor 单 owner，最终尺寸 coalescing，一次收割，允许每手势 0～几行干净重复。
