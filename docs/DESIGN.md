# BetterTerminal 技术方案 v3.3（五轮 Codex 审核后）

> v3.3（回应第五轮）：消除三处规格矛盾——① 删除"冻结历史占据新增视觉空间"的残留表述
> （变高=live 屏自身变大，可见历史量不变）；② 变矮的淘汰顺序以 alacritty 实际报告为准并给出
> oracle；③ 锚点迁移统一为两步（捕获时 Live→Staging，定稿时 Staging→History）。

> 前置：`docs/PROBLEM-LIST.md`、`docs/RESEARCH-SUMMARY.md`、`docs/research/`。
> 审核记录：`docs/reviews/codex-review-design-v1.md` ~ `-v4.md`。
> v3.1 要点：scrollback 所有权落定（alacritty=0，转录层唯一）、删除 thaw、ContentAnchor
> 全序/affinity/降级、DetectionRevision 独立、背压容量契约、BlockSource 快照策略。
> v3.2 修订要点（回应第四轮）：① **resize-grow 语义如实落定**——不回填历史，live 屏底部加
> 空行（alacritty 无历史语义，**不是 tmux 语义**，v3.1 表述有误）；回填/thaw 作为 post-M0
> 可选增强单独立项；② staging 补 StagingId/跨界 resize 强制拆分/超限强制定稿；③ ContentAnchor
> 增 Staging 变体、按 Screen 分命名空间的全序；④ worker 溢出/最低份额/按数量限流；⑤ G1/G3 补 oracle。

## 1. 总体架构

### 1.1 核心设计不变量

1. **Canonical Transcript 是真相（对已冻结内容）**：冻结时构建规范转录（逻辑行文本 + soft-wrap 层次 + 样式/超链接 span + generation）。富内容是转录区间上的可撤销**装饰**。活动网格内容的真相是 grid 本身——两者由统一 `ContentAnchor` 桥接（§3.2）。
2. **双平面 + 单向冻结 + 历史所有权唯一**：alacritty 的内部 scrollback 设为 **0**，适配层在行滚出可寻址区时捕获进转录暂存区——**转录层是历史的唯一所有者、唯一配额权威**。冻结严格单向（Live → Frozen，无 thaw）。**resize 变高不回填历史**（v3.2 如实落定）：live 屏在底部增加空行（与 alacritty 无历史时的行为一致；ConPTY 上报的完整尺寸与实际可寻址缓冲严格一致），提示符会离开窗口底部直到新输出填充——这**不是 tmux 语义**（tmux 会把历史拉回可寻址屏），是本设计接受的 UX 取舍；视口保持底部锚定，**变高后新增的视口空间由变大的 live 屏自身占据（含其底部空行），可见历史量不变**。若 dogfood 证明该取舍不可接受，"受控回填"（等价 thaw 的所有权迁移，走 §3.2 的反向锚点迁移协议）作为 **post-M0 可选增强**单独立项，不进入 M-1 范围。活动平面固定行高，只允许等高装饰（§4.3）。
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

## 3. 内容模型

### 3.1 内容生命周期事件表（v3.2：单向冻结版）

冻结唯一路径：**primary screen 全屏正常上滚**使行离开可寻址区，适配层捕获该行进入**转录暂存区（staging）**；逻辑行（soft-wrap 链）在暂存区等待其全部物理片段离开可寻址区后，整条定稿冻结。

**Staging 规格（v3.2 补全）**：staging 属 bt-transcript，配额 4K 物理行，是 FreezeCandidate 的存放位置。每条暂存行持有 `StagingId`（捕获序单调）+ 捕获时的完整 cell 数据（样式、wrap 标记）+ 与 grid 尾部的续接关系；定稿时记录 `StagingId → (TranscriptId, offset)` 映射供锚点迁移。两条强制规则消除跨界不确定性：
- **跨界逻辑行遇宽度 resize → 强制拆分**：已移出片段立即定稿为独立逻辑行（带 `wrap-split` 标记），grid 内尾部成为新逻辑行的开头——staging 片段与 grid 尾部**永不联合 reflow**（stock alacritty 做不了联合 reflow，规则化比补丁化可靠；代价是横跨拆分点的公式不装饰，罕见可接受）。
- **staging 超限 → 强制定稿**：最老暂存行即使逻辑行不完整也定稿（同 `wrap-split` 标记），配额永不阻塞捕获。

| 事件 | 动作 |
|---|---|
| 全屏正常上滚移出行 | 捕获 → staging；逻辑行完整后定稿冻结入转录 |
| 逻辑行部分移出（soft-wrap 跨界） | 已移出片段留 staging **不定稿、不装饰**，直到尾部片段也移出；若尾部被改写，staging 片段随之更新（staging 仍属"可变"） |
| 滚动区域（DECSTBM）IL/DL/上滚 | 掉出行**不捕获**（与 VT 语义一致），直接丢弃 |
| 用户向上滚动 | 只动 viewport，绝不触发冻结/定稿 |
| resize 变宽/变窄 | 转录不变（投影重算）；活动网格由 alacritty reflow；staging 内片段按捕获时数据保持，跨界逻辑行按强制拆分规则处理；**历史无回流** |
| resize 变高 | **不回填**：live 屏底部加空行（alacritty 无历史语义；ConPTY 上报尺寸=实际可寻址缓冲），提示符暂离窗口底部属接受的取舍；视口底部锚定，可见历史量不变；受控回填为 post-M0 可选增强 |
| resize 变矮 | **以 alacritty 实际报告的移除集合为准**：光标下方空行被裁剪时直接丢弃（不捕获）；自顶部上滚移出的非空行捕获进 staging。G1 oracle：被捕获集合 ≡ 视觉上从顶部消失的非空行集合 |
| 反复 resize 抖动 | staging 定稿有 debounce 冷却；装饰任务在冷却内不启动 |
| ED3（清 scrollback） | 原子删除转录+staging+块+索引+缓存；锚点按 §3.2 降级；留 tombstone |
| 配额淘汰 | 与 ED3 同一删除管线（转录层是唯一配额，无双配额漂移） |
| 进入 alt screen | primary 停放（park），staging 冻结计时暂停，检测任务暂停 |
| 退出 alt screen | 恢复 primary generation；暂停任务 generation 校验后恢复或作废 |
| RIS / DECCOLM | 活动屏与 staging 候选作废；已冻结历史保留（可配置） |
| 无换行的最后一行 | 永不冻结；仅命令边界闭合后允许等高 inline 检测 |

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

Pane/PTY 所有权同 v3：一 session 一可输入 live viewport（owner 决定 ConPTY 尺寸，拖分隔线 debounce 发 resize），其余投影只读；IME/鼠标/焦点/damage 按 ViewportId 归属；`PaneContent = TerminalViewport | NativeHostedPanel` day-1 预留。

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

同 v3（LaTeX 默认块级强定界；CommonMark 真解析；overlay 裁剪+pointer-transparent+相交让位原文+无障碍暴露原文）。测试新增：反复 resize 抖动、配额淘汰、RIS/DECCOLM、最后一行、staging 部分定稿等生命周期矩阵项（对齐 G1）。

## 5. 数学渲染管线（M-1 spike）

同 v3（三路径、300+ 样本含恶意输入、进程隔离开销验证、缓存键含 detection_rev + LayoutKey）。

## 6. 会话与 agent 状态

同 v3（显式 side-channel 优先；任务栏聚合 = 窗口内最高注意力状态）。

## 7. UX 骨架

同 v3（布局三档手动固定 + 渐进披露默认；分屏 = 多投影 + 单 owner；egui/AccessKit/M2 去留门）。WebView2 补充（第三轮 MAJOR）：**external protocol 一律禁用**（`msteams:` 等弹确认都不给，直接拦截）；**web message bridge 默认关闭**（预览面板与终端无 JS 通信通道）；导航白名单默认仅 `http://localhost:*` 与 `http://127.0.0.1:*`，跳转其他 origin 需显式确认；下载/新窗口/权限请求默认拦截。

## 8. 依赖策略

同 v3 表格，关键修订：**alacritty_terminal 配置 scrollback=0**，不使用、不依赖其 history/pull/reflow-history 行为；适配层捕获上滚移出行（评估现有 API/事件是否足够，不足则 vendor 补丁，补丁面=捕获钩子一处）；该行为契约用生命周期矩阵钉死，升级必须全过。

## 9. 里程碑

同 v3 的 M-1→M5+ 结构，三道退出门按 v3.1 修订：

- **G1 生命周期回放矩阵**（v3.2 扩）：`scroll-out→staging→定稿→decorate→rewrite 检验`、变宽/变窄/变高 reflow、**变高后 ConPTY 全尺寸可寻址性**（CUI 程序在新尺寸下全屏绘制正确）、**变矮的实际淘汰分支**、反复 resize 抖动、**跨界逻辑行 resize 强制拆分**、**staging 超限强制定稿**、ED3、配额淘汰、alt 进/出、滚动区域、RIS/DECCOLM、最后一行——全过。
- **G2 多投影一致性**：同一转录在两个不同宽度视口同时显示，高度树/选区/滚动锚各自独立且正确。
- **G3 锚点与版本协议**（v3.3 统一术语）：持久锚点两步原子迁移（捕获时 Live→Staging，定稿时 Staging→History）+ ED3/淘汰降级正确；**Staging 锚定稿迁移**、**primary/alt 命名空间隔离**（跨屏比较被拒绝）验证；worker 任务 generation 失效丢弃零泄漏；四版本失效边界行为正确。

三门任一不过 → 按 R10 降级砍功能，不顺延。

## 10. 风险登记

v3 表保留，修订：~~R11 thaw 抖动~~ → **R11' 上滚捕获依赖 alacritty 内部行为**（scrollback=0 下移出行的捕获钩子可能需 vendor 补丁并随上游演进维护）——缓解：钩子面最小化 + 行为测试钉死。新增 **R13 staging 语义复杂度**（部分定稿/改写更新）——缓解：staging 逻辑纯函数化，语料矩阵覆盖。

## 11. 已决问题（累计）

v1 Q1-Q4、v3 各决策（alt=停放、一 session 一可输入视口、折叠属视口、overlay 让位、任务栏聚合）、v3.1 决策（scrollback 所有权=转录层唯一；原子迁移仅持久锚点；DetectionRevision 独立；Attachment=快照+显式刷新；WebView2 三禁），v3.2 新决：**resize 变高=底部加空行不回填（非 tmux 语义，如实承认；受控回填为 post-M0 可选增强）**；**staging 跨界 resize 强制拆分、超限强制定稿**；**锚点按 Screen 分命名空间，Staging 为独立锚变体**；**worker 溢出=替换→拒绝+retry-on-idle，渲染任务按数量限流**。
