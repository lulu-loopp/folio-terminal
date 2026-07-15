## 总体结论

v2 不是简单的文字应付：上一轮要求的大方向基本都被实质采纳，尤其是 damage/present、有界资源、M-1 前移和 Q1–Q4。

但双平面与转录模型尚未形成闭合的所有权协议。我认为目前有三个 BLOCKER：

1. resize/ED3/alternate screen 下的冻结、解冻、删除语义未定义；
2. 活动 grid 与 frozen transcript 之间没有统一坐标和原子迁移协议，四态状态机还混合了两个正交状态；
3. `HistoryBlock` 保存单一 `layout/px_h`，无法支持分屏、总览等多视口、多宽度同时布局。

因此：

> **可以进入 M-1，但只能作为风险原型/spike 实施；不能把 v2 视为架构已通过并直接铺主干。**  
> 下述 BLOCKER 必须成为 M-1 的首批决策项，并在 M-1 结束前关闭，否则不能进入 M0。

这里的 BLOCKER 阻断的是“原型晋升为正式架构”，不是阻止编写用于验证假设的一次性原型。

---

## 1. 对 v1 主要意见逐条验收

| v1 意见 | 验收结果 | 判断 |
|---|---|---|
| 双平面模型 | v2 明确规定活动平面固定行高、历史平面冻结后变高，并把原型前移到 M-1。这是实质采纳。 | **部分通过，残留 BLOCKER**：没有定义内容在两个平面之间的完整生命周期，尤其 resize 拉回历史、ED3、alternate screen。 |
| Canonical transcript 取代 `StableRowId` | 物理 row identity 已从公共模型移除，补了逻辑文本、soft-wrap、样式、revision、tombstone、SourceMap，方向正确。[DESIGN §3.1](D:/Developer/BetterTerminal/docs/DESIGN.md:62) | **部分通过，残留 BLOCKER**：transcript 只覆盖 frozen history，活动区仍以 grid 为真相；跨两者的选择、搜索、锚点和冻结迁移没有统一身份。 |
| 四态稳定性状态机 | 不再把 200ms 直接叫 Stable；任务携带 span/revision，过期结果丢弃。[DESIGN §4.1](D:/Developer/BetterTerminal/docs/DESIGN.md:93) | **部分通过，残留 BLOCKER**：`Frozen/Decorated` 把“源码生命周期”和“装饰生命周期”混成一条状态链；部分转移在双平面假设下自相矛盾。 |
| 有界并发 | 有界 channel、UI coalescing、优先级、取消、上传/缓存/scrollback 配额、CPU 预留均已写入。[DESIGN §1.3](D:/Developer/BetterTerminal/docs/DESIGN.md:40) | **基本通过，残留 MAJOR**：缺少不可丢 VT 流的满队列策略、超时的真正强制手段以及具体公平性/容量契约。 |
| Damage/present 语义 | 已准确改成 damage 只驱动唤醒和 scene 更新，每次完整合成；surface capability、device lost、IME/选区等触发也覆盖了。[DESIGN 第 18 行](D:/Developer/BetterTerminal/docs/DESIGN.md:18) | **通过**。只有未来 WebView2 不属于同一 wgpu 帧这一增量问题。 |
| M-1 前移 | 双平面闭环、数学管线、IME/CJK、延迟、洪水和取消全部前移。[DESIGN §9](D:/Developer/BetterTerminal/docs/DESIGN.md:159) | **基本通过，残留 MAJOR**：验收矩阵没有覆盖本文几个关键边界；延迟只测到 present submit，没有光子侧基线。 |
| Q1 行内 `$...$` 默认关 | 已采纳，保守档只启用强定界，agent 输出/aggressive 才开放。[DESIGN 第 107–111 行](D:/Developer/BetterTerminal/docs/DESIGN.md:107) | **通过，MINOR**：问题清单 P0-1 仍把 `$...$` 与块级公式并列，需求文本没有同步默认策略。 |
| Q2 egui 去留门 | AccessKit 第一天启用、状态接口隔离、终端自建 accessibility tree、M2 去留门均已写入。 | **通过**。 |
| Q3 Markdown 富排版而非位图 | 已采用 DisplayList + SourceMap + inline artifact，并限制 MVP 范围。 | **部分通过，残留 MAJOR**：单一 `RichLayoutId/px_h` 仍是单视口模型；多宽度时架构失效。 |
| Q4 仅历史恢复、不保进程 | 已明确 M4 只恢复布局和可选 scrollback，不暗示任务存活。 | **通过，MINOR**：持久化默认值仍写成“默认关闭或短限额”，最终策略未定。 |

---

## 2. 双平面与转录模型的新问题

### BLOCKER 1：冻结边界不是单向的，当前模型却按单向设计

[DESIGN 第 15 行](D:/Developer/BetterTerminal/docs/DESIGN.md:15)称内容滚出活动窗口后冻结，“此后”才进入变高世界。但 alacritty 在增加终端行数时会从 scrollback 拉行回可见区，并同步下移当前及保存光标；这是其明确实现行为。[alacritty resize 源码](https://docs.rs/alacritty_terminal/latest/src/alacritty_terminal/grid/resize.rs.html)

于是会发生：

1. 一行滚出屏幕，被转录并渲染成变高块；
2. pane/窗口变高；
3. alacritty 把同一行拉回固定网格；
4. PTY 又可能通过光标移动改写它；
5. 历史平面同时还保留其变高版本。

v2 没有定义 `thaw`，也没有说明如何避免双重所有权。必须在 M-1 明确选择一种策略：

- 保留可逆 live-history 尾部，只有越过某个不可返回边界才冻结；
- resize 拉回时原子解冻、撤销富布局并迁回 grid；
- 或完全接管 scrollback/resize 投影，不采用 alacritty 默认的 history pull 行为。

其中第一种没有天然有限的“绝对不可返回距离”，所以不能只写一个固定保留行数就算解决。

同一生命周期表还必须覆盖：

- **ED3/CSI 3 J**：应原子删除 primary scrollback 对应的 transcript、块、搜索索引、选择端点和缓存，而不只是笼统地“source 失效”。锚点落在被删区时要有确定的后继落点。
- **普通配额淘汰**：必须与 ED3 区分。alacritty 内部淘汰是否也删除 canonical transcript，目前不清楚；否则两个 scrollback 配额会漂移。
- **alternate screen**：进入 alt 时 primary 活动屏应被“停放”而非冻结；alt 通常不产生历史转录；退出时恢复原 primary generation。候选检测任务是暂停、取消还是继续，也需明确。
- **RIS、DECCOLM、全屏清除、终端重置**：分别清什么、保留什么。
- **滚动区域、IL/DL、reverse index**：局部滚动掉出的行不一定进入正常 scrollback，不能把所有“离开某个矩形”的行都冻结。

### BLOCKER 2：所谓 canonical truth 实际仍是双真相

目前 canonical transcript 只在冻结时创建，而活动平面的真实内容仍是 alacritty grid。因此整个会话并不存在单一 canonical 坐标系。

缺失的关键类型类似：

```text
ContentAnchor =
    History(TranscriptId, source_offset)
  | Live(ScreenId, GridPoint, grid_generation)
```

以及从 `Live → History` 时的原子迁移协议。否则以下场景无确定语义：

- 选择从历史块跨到活动屏；
- 搜索结果同时落在 frozen 与 live 内容；
- 冻结发生时鼠标选择、滚动锚点、可访问性节点如何迁移；
- 用户停在旧历史位置时，底部继续有行冻结；
- resize 或 ED3 后 anchor 如何降级。

此外，`FrozenLine { text, soft_wrapped, styles }` 还需要规范化规则：

- trailing blank、被擦除 cell 是否进入源码；
- wide-cell spacer、组合字符、grapheme 与 UTF-8 byte offset 的映射；
- OSC 8 hyperlink、shell mark 等语义元数据是否保留；
- “逻辑行”和“soft-wrapped 物理片段”的层次关系。

只有 byte range 不足以直接支持终端 cell、grapheme、富布局 fragment 三者之间的双向选择。

### BLOCKER 3：四态状态机混合了两个不同维度

当前图是：

```text
Mutable → Candidate → Frozen → Decorated
```

但：

- `Frozen` 是源码是否还可被 PTY 改写；
- `Decorated` 是某个视图是否已有排版结果。

一个 frozen span 可以同时有 Plain、Pending、Ready、Failed、用户关闭装饰等多种展示状态；撤销公式后它仍然是 Frozen，不会回 Mutable。反过来，resize 导致的 reflow 只应使 layout 过期，不应改变 canonical source revision。

建议拆成两个状态机：

```text
SourceLifecycle:
Live → FreezeCandidate → Frozen → Tombstoned
                   ↘ Thawed（若采用可逆 resize）

DecorationLifecycle:
None → Pending → Ready | Failed | Suppressed
```

同时拆开版本：

- `SourceGeneration`：内容、清除、淘汰、thaw；
- `LayoutKey`：宽度、DPI、字体、主题、renderer/parser 版本；
- `ViewGeneration`：pane 尺寸和投影；
- reflow 不应修改 source revision。

当前“reflow 后 revision 不匹配即丢弃”的写法容易把不可变源码和可变布局混为一谈。

### MAJOR：冻结触发条件仍是信号清单，不是算法

[DESIGN 第 103–105 行](D:/Developer/BetterTerminal/docs/DESIGN.md:103)列出了 soft-wrap 完整性、clear、resize 等信号，但没有事件到动作的转换表。

至少要明确：

- 只有 primary 全屏正常上滚才创建 history，还是还有其他路径；
- 一个逻辑行跨越活动/历史边界时，是等待整条逻辑行离开，还是允许部分冻结；
- shell 输出块闭合只能提高检测置信度，不能使仍可寻址的行提前进入变高平面；
- 用户向上滚动只改变 viewport，绝不能触发冻结；
- 没有换行的最后一行何时成为完整检测单元。

M-1 回放验收应加入上述事件组合，而不只是“alt-screen 零触发”。

### MAJOR：等高 inline overlay 与输入系统没有合成规则

“等高”只约束高度，不保证公式宽度能塞入原 cell 区域。必须明确 overlay：

- 严格裁剪到原 source cell rectangle，不改变 cell advance；
- pointer-transparent，命中测试仍选择原始网格文本；
- mutation、resize、screen swap 后按 grid generation 立即失效；
- 与选择、光标、IME preedit 相交时是隐藏 overlay，还是规定确定的绘制层级；
- accessibility 暴露原文及可选公式语义，不能只暴露栅格图。

最稳妥的 MVP 是：选择、可见光标或 IME preedit 与 overlay 相交时临时显示原文。

### MAJOR：并发有界化还缺少背压语义

`PTY reader → Term` 是不可丢、不可 coalesce 的 VT 字节流；只有 `Term → UI` 才能 latest-state 合并。因此应明确：

- reader 队列满时阻塞并把背压传给子进程，还是使用固定上限的 spill buffer；
- shutdown/cancel 信号不能被一个阻塞的 send 卡死；
- 同一会话解析必须严格有序，多会话洪水需要公平调度；
- “解析超时”不能只在任务返回后检查时间。对于可能卡住的数学/脚本引擎，需要可抢占进程、Job Object 或明确的合作式中断；
- `物理核-2` 在 1–2 核环境需要下限和降级规则。

---

## 3. 增量范围对架构的影响

### P1-11 分屏

#### BLOCKER：块高度和布局不能存放在共享文档模型中

当前定义：

```text
HistoryBlock::Rich {
    layout: RichLayoutId,
    state: Ready(px_h)
}
```

隐含同一块只有一个宽度和一个高度。[DESIGN 第 75–86 行](D:/Developer/BetterTerminal/docs/DESIGN.md:75)

但以下场景都会同时需要不同投影：

- 主 pane 与总览缩略图同时显示同一会话；
- 同一会话被克隆到两个 pane；
- pane 拖动分隔线时不断改变宽度；
- 不同显示器/DPI；
- 一个视图在历史中，另一个视图跟随 live tail。

因此不能只是“每 pane 一个滚动位置”。应拆成：

```text
HistoryDocument
  └─ 只保存 transcript span、语义块、装饰意图

ViewportProjection(ViewportId)
  ├─ width / DPI / font revision
  ├─ per-block layout
  ├─ per-viewport height SumTree
  ├─ scroll anchor / selection
  └─ visible range
```

布局缓存至少按 `(span, source_generation, width, DPI, font/theme/parser revision)` 键控。`px_h` 和高度 SumTree 应属于 viewport projection，而不是共享 `HistoryBlock`。

#### MAJOR：pane 与 PTY geometry 的所有权需明确

一个 live PTY 同时只能有一组 `rows × cols`。如果同一会话可出现在两个不同尺寸 pane：

- 哪个 pane 决定 ConPTY 尺寸；
- 其他 pane 是只读重排、裁剪还是 letterbox；
- 两个 pane 是否都能输入；
- 拖动分隔线是否连续发送 resize；
- resize 引起的 history pull/thaw 如何处理。

建议第一版规定“一个 session 只能有一个可输入 live viewport；其他投影只读”，否则双平面问题会成倍复杂。

IME 候选框、鼠标报告、焦点、选择和 damage 也都必须归属 `ViewportId/PaneId`，不能只按 session 保存。

### P3-6 文件预览块

#### MAJOR：它不是 transcript 的可撤销视图

[PROBLEM-LIST P3-6](D:/Developer/BetterTerminal/docs/PROBLEM-LIST.md:57)要求图片、CSV、Markdown 文件预览块，但现有 Rich block 必须锚定 `TranscriptSpan`。文件预览还有独立、可变化的外部数据源。

需要至少抽象：

```text
BlockSource =
    Transcript(TranscriptSpan)
  | Attachment(AttachmentId, uri, content_revision)
```

并定义：

- 文件变化、删除和 TOCTOU 后如何失效；
- 大文件、网络路径、图片解码器的资源/安全上限；
- 复制和搜索时返回路径、原文件内容还是占位文本；
- 如果预览在当前 prompt 旁立即出现，如何遵守活动平面禁止变高的规则。

如果产品实际想要的是独立预览 pane，就不应继续称为“文件预览块”。

### P3-7 Web 预览面板

#### MAJOR：独立面板是正确方向，但 HWND 合成边界必须提前预留

窗口化 WebView2 可以作为 child HWND 存在，但它不是 wgpu scene node。其 bounds 相对 parent HWND，窗口移动、大小、可见性和焦点都需单独同步；父窗口移动还需显式通知，涉及可访问性和浏览器对话框。[Microsoft WebView2 Controller 文档](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)

主要坑包括：

- wgpu 绘制的命令面板、toast、拖拽遮罩不能自然覆盖原生 WebView child；需要临时隐藏 child、使用独立 popup HWND，或选择 composition hosting；
- windowed hosting 较简单，由 OS 处理输入；composition hosting 可统一视觉层级，但应用必须转发鼠标、触控、坐标变换并承担更多可访问性工作。[Microsoft hosting modes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting)
- pane resize/DPI/移动需更新 bounds、zoom、可见性和 parent-position 通知；
- WebView 获焦后快捷键、Tab 导航、IME 和全局命令面板的路由需重新定义；
- 总览缩略图不能假定从 wgpu framebuffer 得到 WebView 内容；
- 必须限制导航、下载、新窗口、权限、外部协议和 web message；“只预览 localhost”也需要明确 origin/端口跳转策略。

因此 `PaneContent` 应从现在就允许 `TerminalViewport | NativeHostedPanel`，但无需在 M-1 实现 WebView2。

### P3-8～10

- **P3-8 链接/路径——MAJOR**：OSC 8 hyperlink identity 需要进入 frozen transcript 元数据和 SourceMap；否则冻结后链接会丢失。路径启发式还需按 source offset 命中。
- **P3-9 Diff——MINOR**：基本适配 RichLayout，但折叠状态应属于 viewport，而非共享 transcript；复制仍必须走 SourceMap。
- **P3-10 任务栏进度——MINOR**：OSC 9;4 已在 `bt-shellint` 范围内，但多 pane/多 session 同窗时需要定义窗口级聚合策略，例如 focused、最高进度或最高注意力状态。

---

## M-1 必须增加的退出门

建议把以下三项作为 M-1 的硬性 go/no-go 条件：

1. 完整通过 `scroll-out → decorate → resize-grow/thaw → rewrite`、ED3、alt enter/exit、局部滚动区域、soft-wrap 跨边界的回放矩阵。
2. 同一 transcript 在两个不同宽度 viewport 中同时显示，拥有独立高度树、选择和滚动锚点。
3. source lifecycle、decoration lifecycle、layout generation 分离；选择和 anchor 能在 live/frozen 边界原子迁移。

最终判断：**v2 可以启动 M-1 风险验证，但当前仍不能作为可直接产品化的实现规格；三个 BLOCKER 关闭后，才可判定 M-1 架构路线成立并进入 M0。**

本次未修改任何文件。