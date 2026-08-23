# 多窗 E + F 方案 v3 窄复核

复核范围仅限 `plan.md` 的“v3 增补”是否闭合第二轮指出的四簇必改，以及增补是否引入新矛盾。未重开已经通过的产品裁决。

## 1. F1a WebView2 重挂合同

**结论：通过。**

v3 已闭合第二轮要求的七项：

- 顺序明确为断旧 `RootVisualTarget`、源 commit、换 `ParentWindow`、设新 target、目标 commit、更新 bounds/presence、通知父窗位置变化。
- 用“prepare + 可补偿 platform handoff + model commit”取代了 v2 中“commit 后无失败步骤”的不成立表述。
- 中途失败有回旧 HWND/target/bounds 的补偿，补偿失败则关闭 controller 并明确走有损重建，不再虚称源原样。
- `WebSeat::rehost` 明确更新对象内部的 `seat`、`hwnd`，并用迁移后 browser-process failure 原位重建作红测。
- DPI owner、`ShouldDetectMonitorScaleChanges`、`BoundsMode`、WebView 自带 UI、IME 与 accessibility 均进入 spike/验收。
- 进程单一 environment 被写成前提断言，不再把不存在的跨 environment 迁移当能力。
- 输入按钮态、capture、leave 与焦点交接均有明确结果。

该簇没有新引入的阻断矛盾。spike 出档时仍须把 DPI 最终取值写成明确二选一：自动侦测为真则 WebView2 是 owner；否则 Folio 写 `RasterizationScale`。这已属于 v3 所述 spike 的产出，不要求在 spike 前预判机器答案。

## 2. F1b App 级 `TabId`（A 案）

**结论：仍需修改。**

A 案本身选对了：`TabId` 进程级唯一且不复用，`SeatId` 保持 tab 内局部，整 tab 跨窗时无需 rekey，确实比给所有回执加 `{window, generation}` 小。但 v3 的盘点证据同时暴露了一个未被方案处理的路由事实：

- `AppEvent::{MathReady,FilesReady,PreviewReady,GitReady}` 确实依次调用 `for_each_window(runtime.apply_*_results)`；
- 但每个 `apply_*_results` 不是“只看属于本窗的结果并把其余留给下一窗”，而是在该 runtime 内对**共享 receiver**反复 `try_recv()` 直到空；
- `apply_preview_results`、`apply_files_results`、`apply_git_results` 对本窗找不到的 `TabId` 直接 `continue`，math 路径同样已经把 completion 从共享队列取走。

因此第一扇被遍历的窗口会把整条共享队列消费完。现状同号时可能误投；改成全局唯一 `TabId` 后，不属于第一窗的结果虽然不会误投，却仍会被第一窗消费后丢弃，目标窗随后无结果可收。也就是说，全局 ID 只把“可能误投”变成“确定可能误丢”，不能单靠改约五个铸号点修复。

### 必须补的路由合同

把 worker 完成的 drain 提升到 App 级，只 drain 一次，再按全局 `TabId` 找到唯一 owner window 后投递；或先在 App 级按 owner 分桶进入每窗 mailbox，再让 runtime 应用。浮窗等不以 tab 为 owner 的 host 也必须在这次盘点里保持其既有 epoch/窗口路由，不能被强塞进 `TabId` 分支。

E 的第一条红测不应再写“先证明它是否已是活缺陷”：上述代码结构已经足以证明消费模型有缺陷。红测应直接钉：结果排成“窗 B、窗 A、窗 B”的顺序，App 一次 drain 后三项各落唯一 owner，窗口遍历顺序反转结果不变，任何窗口都不能吞掉别窗结果。

### pane 升格换号的活性仍缺一半

整 tab 跨窗能让在途回执随全局 `TabId` 走；但 pane 从多 pane tab 抽出并升格为新 tab 时，移动的 `LeafSession` 会获得新 `TabId`，它已经发出的任务仍携带旧 `LeafId`。源 tab 此时通常**没有死**，所以 v3 的“旧号的在途回执随源 tab 之死自然失配”并不成立。

安全上，旧回执必须被丢弃；活性上，还必须清掉随 `LeafSession` 搬走的 pending/in-flight 标记并以新 `LeafId` 重新索取，否则旧回执被正确丢弃后，新 tab 可能永远认为工作仍在飞而不再发问。应补一个窄合同与红测：从仍有兄弟 pane 的 tab 抽出一叶，在旧任务未完成时升格；旧完成不落任何叶，新叶以新 ID 重发并最终完成。

## 3. Quit 保存分支

**结论：通过。**

v3 已区分：

- 保存逐项走冲突检测与原子写，全部成功才进入拍照/teardown；
- 任一失败时不退出、不隐藏，列出失败项；
- 已成功保存的项诚实变干净，只有“取消”承诺全应用零变化；
- 丢弃只处理内存修改；
- 非文件编辑按类型定义 commit 语义并列入 E2 工单。

这与 §7.1.3 的脏缓冲、磁盘冲突和原子保存裁决一致，没有新增矛盾。

## 4. F2 spring deadline、撤防与缩略图

**结论：通过。**

App 级 broker 已有 `next_deadline` 并接入 event-loop wait-until，覆盖“进入后零 motion 仍到点”；目标变化、关闭、Esc、capture lost 等均撤钟。系统夺 capture、源窗关闭、锁屏和显示配置变化也有统一撤防结果。缩略图对 live 重挂、重建降级、DPI/尺寸变化的规则已经写死，不再留二选一到实现期。

该簇与 v2 的 App 级 broker、混合 DPI 和 Web 缩略图所有权没有冲突。

## 5. 新引入的文字矛盾

**结论：仍需修改。**

v3 已明确“改选 A 案”，实质上覆盖 v2 F1b 的“rekey + generation”；但 v2 验收门第 4 条仍要求“源/目标故意同号 `TabId`/`SeatId`，在途与新回执各落各家”，v3 又写“复审验收门第 4 条照钉”及“旧完成不落任何叶”。A 案下两个 live tab 不应再能取得同号 `TabId`，这三句不是同一个验收合同。

应明确标注 v3 覆盖 v2 F1b 及原验收门第 4 条，并换成两条：

1. App allocator 在恢复、新窗、新 tab、pane 升格所有铸号点上进程内永不复用，跨窗移动保持原 `TabId`；
2. dead `LeafId` 的旧完成被丢弃，live 全局 ID 的完成由 App router 精确投递，pane 换号后会重新索取。

## 总判定

**仍需修改。**

F1a、Quit、F2 三簇已经闭合；#11 及既有产品裁决无需再审。剩余修改只在 A 案身份簇：补 App 级单次 drain/路由、补 pane 升格换号后的取消与重发，并用 v3 明文覆盖 v2 的旧身份验收门。完成这三处后即可通过，无需第三次扩大审阅范围。

v4 确认：身份簇三处均已按窄复核要求闭合，总判定：**通过**。
