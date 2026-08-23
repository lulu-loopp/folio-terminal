# 多窗 E + F 方案 v2 第二轮复审

复审对象：`docs/plans/multiwindow-ef/plan.md` v2（2026-08-23）。

核对依据包括：`docs/DESIGN.md` §2.5、§7.1.3、§7.1.6e、§7.1.6i、§7.7–§7.11，`docs/spikes/spike-multiwindow.md`，`crates/bt-app/src/seats.rs`，`crates/bt-app/src/webhost.rs`，`crates/bt-platform/src/webview.rs`，`crates/bt-persist/src/session.rs` 及现有异步 worker 的 `LeafId` 路由。

## 总览

v2 已实质修掉首轮多数问题：应用级 Quit 事务、最终写盘失败可见、快照先于 teardown、Web 优先重挂、移交事务、PTY wake、聚焦列裁决、App 级 drag broker、混合 DPI 几何和 Web 有界收尾，方向均正确。

本轮确认：首轮 #11 关于“独 pane 没有 `⌄`”的事实前提确实过时，v2 对这一前提的驳回成立。但 F1a 的重挂回滚合同、F1b 的异步身份协议、Quit 的“保存失败”分支和 F2 的 spring deadline 仍未闭合，因此当前不能直接开工。

## 一、给审阅者（第二轮）的四问

### ① F1a 的 spike 验收单是否漏了重挂路径的已知坑

**结论：必改。**

`put_ParentWindow(new_hwnd)` + `SetRootVisualTarget(new_visual)` 是正确主线，但现有验收单还不足以支撑 F1b 所承诺的“任一步失败，源分毫不动”。必须补齐以下合同：

1. **先断旧 RootVisualTarget，再释放旧 visual。** 微软明确允许把 `RootVisualTarget` 设为 `nullptr` 以断开 WebView 与宿主 visual tree；设置新 target 后，宿主还必须在该 visual 所属的 device 上 `Commit`。因此顺序应至少覆盖：隐藏 controller → `put_RootVisualTarget(nullptr)` → 源 device commit → 换 `ParentWindow` → 设置目标 target → 目标 device commit。不能只从源树摘 visual 后仍让 controller 指着即将销毁的 COM visual。[ICoreWebView2CompositionController](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller)

2. **逐步写出补偿，而不是把重挂算作“commit 后无失败步骤”。** `put_ParentWindow`、`put_RootVisualTarget`、两边 compositor commit、bounds/presence 更新都可能失败。例如 parent 已换而新 target 失败时，必须证明能按“旧 HWND + 旧 target + 旧 bounds/presence”补偿回去；若补偿本身失败，不能继续声称源原样，必须关闭该 controller 并走有状态损失的重建降级，同时把失败说给用户/证据日志。F1b 的“一次 commit 后不得再有可失败步骤”与当前 F1a 顺序有直接矛盾，必须改成“prepare + 可补偿 platform handoff + model commit”，或由 spike 证明另一条真正无失败的边界。

3. **更新对象内部地址，不只是搬 map 项。** 现有 `WebSeat` 自身缓存 `seat: u64` 和 `hwnd`；后续 browser crash/rebuild 会用 `self.hwnd` 重新 `request_controller`，attach/detach 会用 `self.seat` 找 visual。迁移若只把 `WindowRuntime.web` 的表项 rekey，当前页也许能显示，但下一次 controller 重建会回旧 HWND/旧 visual。F1a/F1b 必须规定一个窄的 `WebSeat::rehost(new_seat, new_hwnd, ...)` 合同，并以“迁移后注入 browser process failure，页面在目标窗原位重建”为红测。

4. **补位置通知。** 换父窗并完成目标几何后调用 `NotifyParentWindowPositionChanged`；官方说明它独立于 Bounds，供 accessibility 与部分 WebView 对话框正确定位。[ICoreWebView2Controller](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)

5. **DPI 必须选定唯一所有者。** 当前代码未显式使用 `ICoreWebView2Controller3` 的 rasterization API。spike 应先读出并钉死 `ShouldDetectMonitorScaleChanges`：若保持 `true`，要等到/验证 controller 已按新 parent monitor 更新 `RasterizationScale`；若改为 `false`，则 Folio 必须在 parent/bounds 切换的明确时点写目标窗 DPI，并处理系统 text scale。还要钉住 `BoundsMode` 是 raw pixels 还是 rasterization-scale units，避免目标 bounds 被二次缩放。验收不能只看正文，要同时看 WebView 自带 context menu、tooltip/弹层，因为 rasterization scale 也管这些 UI。[ICoreWebView2Controller3](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller3)

6. **当前仓库没有“跨 environment”阻断。** `webhost.rs` 顶部与 `bt-platform::WebHost` 都表明 environment 是进程级共享缓存，controller 自身持有该 environment；所以本方案当前不是把 controller 从 environment A 搬到 B。spike 应把“单一 environment”写成前提并断言，而不是虚构跨 environment 支持。真正需要实证的是：每窗各有 compositor/device 时，一个活 composition controller 能否从源 device 的 visual 断开并挂到目标 device 的 visual；失败即按 v2 已写的重建降级。

7. **输入/焦点收尾要入验收。** 换父前向旧宿主结清 mouse leave/capture/button 状态，换父后用目标 client 坐标转发；页面原先有焦点时，明确焦点是随 tab 恢复到目标页，还是先回 Folio chrome 再由用户进入。还应覆盖 IME、Web context menu 和 accessibility 位置，而不只验证普通点击。

官方的核心依据是明确的：composition controller 创建时的 `parentWindow` 是接收并转发输入的 HWND；visual tree 移到另一窗口下时必须调用 `put_ParentWindow` 更新它。[ICoreWebView2Environment3::CreateCoreWebView2CompositionController](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment3)

### ② F1b 取“rekey + generation”而非全局 ID 的取舍

**结论：必改。不是判定该方向必然不可行，而是现稿的协议尚不足以消歧，且对改动面的比较不成立。**

当前 `LeafId` 只有 `{tab, seat}`，没有 window，也没有 migration generation；math/preview/files/git 等多条请求和完成路径已经广泛携带这个二元地址。现有 `WebMachine::generation` 只用于同一 WebSeat 的 environment/controller 生命周期，不能拿来充当跨窗内容身份代际。

若坚持 rekey，方案必须明确：

- generation 由谁发号。它必须是 App 级单调 epoch，或至少以稳定 `WindowKey` 为命名空间；“每叶从 0 开始的一枚 generation”仍会在两窗同号 `LeafId` 上撞车。
- 哪些请求/完成结构携带 `{window, LeafId, generation}`，不能只说“所有 worker completion 校验”。应逐项列出 math decoration、preview decode/read、files enumeration、git status/graph/checkout、web thumbnail decode，以及任何把 `LeafId` 捕获进闭包/队列的路径。
- rekey 时旧地址何时作废、新地址何时发布、源窗以后复用旧 ID 时为何不能收走旧完成；完成在事务前、事务中、事务后到达分别落哪里。
- tab 内部的所有 `SeatId`/`SplitId`、`WebSeat.seat`、以 seat 建键的 cache 与在途请求是否一并重写；不能只改树和 map key。

同时，v2 对“全局 ID 会动全仓”的否定缺少现仓证据。`SeatId` 的定义本来就是“one tab's tree”内唯一，而所有跨 worker 的叶地址已经带 `TabId`；若只把 `TabId` 的 mint 从每窗 `next_tab_id` 提升为 App 级不复用 ID，`SeatId` 仍可保持 tab 内局部，许多 worker 结构反而一字不用改。现代码里 `next_tab_id` 的生产点是有限的，恢复时也可由 App 重新发运行时 ID，磁盘本来就不保存 runtime seat id。

因此开工前必须做一个窄 inventory，量出两案真实 diff：

- A：App 级永不复用 `TabId`，移动整 tab 不 rekey；pane 先升格成新 tab 时从 App mint；
- B：保留局部 ID，但所有可能在途的地址升级为 `{WindowKey, LeafId, generation}`，并实现完整 rekey。

以“更小、可证明不会误投”为裁决标准重新选。若仍选 B，补齐上述协议即可，不要求为了形式统一强改成 A。红测除 v2 已写的“两窗故意同号”外，还必须让旧完成在源 ID 已被新叶复用后到达，并证明既不落新叶，也不被错误窗口先 drain 后丢弃。

### ③ 首轮 #11 的驳回是否成立

**结论：通过。驳回成立。**

已核实以下事实：

- `DESIGN.md` §7.1.6i 明写独 pane 右上角常驻角落幽灵 `⌄`，静息墨量 `.45`，点击或 hover 250ms 开同一张 pane 菜单；搜索胶囊占位时才整体让位。
- `Seats::seat_wears_ghost` 只让没有 pane head 的 Terminal seat 戴幽灵；画与命中共用这一谓词。
- `hit_pane_ghost` 返回的就是 `ChromeTarget::PaneMenu(seat)`；`pane_ghost_geometry` 同时供画、命中和测试使用。
- 代码测试钉住了独 pane 命中 pane menu、分屏 pane 不重复戴幽灵、静息墨为 `0.45`，以及搜索胶囊打开时画与命中一起让位。`docs/HANDOFF-2026-08-21.md` 也把 `78d3224` 记为该落地提交。

所以首轮 #11 中“独 pane 根本没有 `⌄` 可按”确为错误事实前提。F1 把 `Move pane to new window` 加进共享 pane 菜单后，分屏 pane 由 head `⌄` 抵达，独 pane 由 ghost `⌄` 抵达；要求 F1 因发现性而捆绑完成终端右键动词段，不再构成阻断。

不过，驳回只针对该前提与阻断结论，不会抹掉 §7.1.6i 1134 行的独立欠账：独 pane 的终端右键动词段仍未落地。v2 已诚实保留这笔账，没有冒领，处理正确。F1 的红测应直接钉“独 pane 的 ghost hover/点击所开的同一菜单含 `Move pane to new window`，执行后走与拖出相同的 App 级移交函数”。

### ④ 其余是否还有阻断项

#### ④-a Quit 汇总脏门的“保存”分支

**结论：必改。**

v2 已规定一张应用级“保存/丢弃/取消”卡，但只写了取消的原子性，没有写保存失败与磁盘冲突。§7.1.3 的预览保存本身是原子写，且编辑期外部改盘必须提示；多窗汇总保存不可能把多个独立文件变成一个文件系统事务。因此必须写明：

- “保存”逐项走既有冲突检测与原子写；全部成功后才拍 Session 并进入 teardown。
- 任一项失败/冲突未裁决即不退出、不隐藏窗口、显式列出失败项；已成功保存的缓冲可诚实变干净，未成功的继续为脏，不能宣称整次操作零变化。
- “丢弃”只丢内存修改再进入拍照；“取消”才是全应用零变化。
- 重命名编辑器等非文件缓冲的未提交编辑，要分别定义“保存”究竟是 commit、结束编辑还是仍阻止退出，不能用一个按钮名掩盖不同事务。

否则写盘失败虽已可见，用户真正的内容仍可能在汇总门后无合同地丢失。

#### ④-b F2 spring gate 必须有 deadline/wake

**结论：必改。**

broker 写了“驱动目标窗 spring 状态”，但没有说明指针进入目标 tab 后停住、此后不再产生 motion 时，谁在 250ms 到点唤醒事件循环。若只在 motion/up 上 `observe`，真正的“停住”恰好永远不会成熟。应让 App 级 drag broker 暴露 `next_deadline`，通过 event-loop wait-until/显式 wake 在到点时重跑命中；目标窗关闭、目标改变、Esc、capture lost 时撤销 deadline。红测要用“进入后零后续 motion，250ms 到点仍 spring-open”。

#### ④-c capture 被系统夺走与源窗消失

**结论：建议。**

v2 已列目标窗关闭、源窗失焦、Esc 和 DPI 变化，但还应把 Win32 capture lost（如系统/别的控件夺走 capture）、源窗在拖动中被关闭、会话锁屏/显示配置突变列成 broker 撤防输入。统一结果应是取消预览、清目标高亮、载荷仍归源；这可并入 F2 工单，不必改变总体架构。

#### ④-d Web 缩略图迁移规则必须在开工前二选一

**结论：建议。**

F1a 仍写“随座位迁移，或按明文规则作废——写死其一”。排期已经把 F1a 作为前置 spike，建议在 spike 出档时即裁：若 live controller 重挂且页面/viewport 未变，最后一帧跟随并以新 seat key 重址；若走重建降级或目标 DPI/卡尺寸改变，则保留旧帧作占位并立即欠一张，不能无期限把旧 DPI 图当新图。此项应在 F1b 开工前关闭，但不单独推翻方案。

## 二、复审后的最低增补验收门

**结论：必改。** v2 的八条门保留，并至少增补：

1. Web 重挂在“旧 target 已断”“parent 已换”“新 target 已设”“目标 commit”四个点分别注入失败；每点要么完整补偿回源且页内状态仍在，要么明确进入有损重建降级，绝不留下双挂、悬空 visual 或源/目标同时认为自己拥有 controller。
2. 重挂后注入 Web browser-process failure，controller 使用目标 `hwnd`、目标 `seat`、目标 compositor 原位重建。
3. 100%↔150%↔200% 双向重挂，验证正文、context menu/tooltip、输入坐标、IME、accessibility/对话框位置与 `RasterizationScale`/`BoundsMode` 合同。
4. 两窗同号 `LeafId`，旧源完成在 ID 被复用后才到达；不得落入新叶，也不得被错误窗口 drain 丢失。若改取 App 级 `TabId`，则钉进程生命周期内不复用与恢复后重新 mint。
5. Quit 汇总“保存”中注入一项磁盘冲突、一项原子写失败：应用不退出，失败项仍脏并可见，已成功项状态诚实；取消仍保证任何窗零变化。
6. 跨窗 spring 在最后一次 motion 后不再有输入，deadline 到点仍切 tab；capture lost/目标关闭会撤掉 deadline 与所有高亮。
7. 独 pane 的角落幽灵以 click 与 250ms hover 两路打开共享 pane 菜单，菜单含 `Move pane to new window`，执行路径与拖出调用同一事务。

## 总判定

**必改后再审。**

#11 的驳回通过，v2 的总体架构也可继续；阻断项不是重新选方向，而是把 platform handoff、异步地址和失败分支写到可实现、可注错、可回滚的程度。完成上述必改项后，可进行一次窄复核，不需要重开已经通过的产品裁决。
