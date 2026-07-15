## 1. 逐条判定

| 项目 | 判定 | 剩余缺口 |
|---|---|---|
| BLOCKER 1：内容生命周期 | **partially closed** | 事件表已覆盖主要事件，但 `alacritty scrollback=0 或受控接管`尚未二选一；同时 §8 又依赖 alacritty history pull。还缺 FreezeCandidate 的存放位置、部分 soft-wrap 片段如何 thaw，以及变宽 reflow 导致历史回流时的规则。 |
| BLOCKER 2：ContentAnchor | **partially closed** | 已有统一类型和迁移时机，但跨 `History/Live` 的全序、选区端点 affinity、删除后的“归零”具体锚点未定义；运行中的 worker task 也无法被 Term 线程原子“重写”，应明确为失效后丢弃或通过 relocation map 转换。 |
| BLOCKER 3：双状态机/三版本 | **partially closed** | 两维状态和三版本已拆开；但状态图中的 thaw 路径不成立，应明确为 `Frozen → Thawed → Live`，并规定再次冻结时 TranscriptId/SourceGeneration 如何处理。另将 `parser_rev` 放入 LayoutKey，会把语义重新检测错误地等同于布局失效。 |
| 分屏与 PTY owner | **closed** | `HistoryDocument/ViewportProjection` 分层、每视口高度树及单一可输入 owner 已形成完整架构约束。owner 转移可留到 M4 细化。 |
| §1.3 背压与抢占 | **partially closed** | 阻塞背压、独立取消通道和进程级 kill 已闭合；仍需明确同一 session 的单线程串行保证、队列容量/轮转 quantum。 |
| §4.3 overlay | **closed** | 裁剪、命中、失效、光标/选区/IME 和 accessibility 均已闭合。 |
| §3.4 BlockSource | **partially closed** | 来源类型、快照和资源上限已有；文件变化后的“重 render”与“打开时快照”存在冲突，且搜索语义、网络路径策略未定义。 |
| §7 WebView2 边界 | **partially closed** | HWND 合成、弹层、DPI、焦点及导航权限已覆盖；还缺 external protocol 与 web-message 的明确禁用/白名单规则。 |
| §9 G1–G3 | **partially closed** | 三门结构正确，但 G1 未覆盖变宽回流、反复 freeze/thaw、配额淘汰、RIS/DECCOLM/最后一行；G3 又依赖尚未闭合的 thaw 和异步锚点协议。 |

## 2. v3 新协议引入的问题

- **BLOCKER：scrollback 所有权自相矛盾。** §3.1 允许关闭 alacritty scrollback，§8 却依赖其 history pull；这会直接决定 thaw 是否可能执行。
- **MAJOR：状态图与正文不一致。** thaw 的来源、目标以及再次冻结的身份规则不明确。
- **MAJOR：原子迁移范围过度承诺。** 持久锚点可以原子迁移，但正在 worker 中运行的任务只能通过 generation 失效，不能同步重写。
- **MAJOR：parser revision 被归入布局版本。** 解析器变化可能改变块边界和装饰意图，需要独立的 decoration/detection revision，而不只是清布局缓存。

## 3. 最终结论

**尚未达到该标准。**

v3 已足以作为 M-1 风险原型的设计输入，但还不能作为确定性的 M-1 实施规格；尤其 scrollback/thaw 所有权、跨平面锚点顺序及状态转换仍会导致不同实现得出不同语义。上述 BLOCKER 关闭并补齐 G1/G3 后，才可采用“通过三门即可进入 M0”的结论。