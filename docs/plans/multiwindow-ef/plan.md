# 多窗 E + F 方案 v2(2026-08-23,按 Codex 首轮审阅修订;首轮意见见 codex-review.md)

前情:多窗切片 A–D 已落地(路线见 `docs/spikes/spike-multiwindow.md`;DESIGN §2.5 一带是现在时)。本方案只谈 E 与 F。既定裁决不重开:恢复策略五条(memory `multiwindow-restore-policy`)、Quit `Ctrl+Shift+Q` 语义(2026-08-20)、聚焦卡片列拒 **pane 源**(§7.1.6b′ ②——**只拒 pane 源**,首稿把 tab 源也拒了,按审阅 #10 更正)。

修订总览(对照审阅编号):#2 网页座位改「重挂优先、重建降级」并前置窄 spike;#4/#7 Quit 补应用级两阶段脏门与可观察的写盘失败,不复用 `exiting`;#5/#8 移交升格为 App 级事务 + 身份重寻址裁决 + 窗外状态迁移清单;#9 PTY wake 重绑;#10 聚焦列只拒 pane 源;#11 用角落幽灵 `⌄` 这一既成事实作答(审阅前提已过时)并把右键动词段的欠账写实;#12 F2 补 App 级 drag broker;#13 几何写死单位与抓握点;#14 Quit 的 Web 收尾二选一取「等待阶段」;#6 E1 补光标传播、「同帧」改为可验合同。

---

## 片 E —— 进程级全局 + Quit

### E1. 「全进程一个主题」(审阅通过,按 #6 补齐)

裁决与理由同首稿(per-window 主题要 per-window 窗口类与缓存翻倍,无需求在等)。补齐两条:

- **光标样式同权**:`apply_cursor_style` 现只给发起窗发布一帧;改走与主题/字体同一条 `pending_application_change` 通道,每扇窗在同一 event-loop 轮次结清。红测:两窗 cursor-style 热切。
- **「同帧」改为可验合同**:同一事件循环轮次内,两窗都应用同一 revision 并各自排入 redraw,返回 Wait 前没有持旧状态的窗——不承诺跨 swapchain 的原子 present(平台没给的同步语义不钉)。

### E2. Quit(`Ctrl+Shift+Q`)——两阶段,不复用 `exiting`

审阅 #4/#7/#14 三条全部采纳。`exiting` 是循环停止后的兜底回调,`for_each_window(close_window(true))` 遇错即停、快照与 teardown 交错——**Quit 另立 App 级事务**,四个阶段:

1. **脏门(应用级,一次)**:扫描所有窗的脏预览与未提交编辑(重命名编辑器等),有则**一张**汇总卡(保存/丢弃/取消;不逐窗弹;取消 = 一扇窗都没动)。无脏内容则跳过——「对可逆动作不加确认仪式」这半论证只在这时成立,首稿把它说成无条件是错的。
2. **只读拍照**:对所有窗 `window_snapshot`,组装最终 `SessionV1`。此阶段不做任何 teardown。
3. **原子写,失败可观察**:`SessionStore` 的最终 flush 改为可判失败;失败 = 不退出、窗口保持打开、显式告警(`session.lock` 不伪装干净退出)。写成功后才进 4。
4. **收尾再退出**:进入 `Quitting` 阶段——窗口隐藏但事件循环**继续 pump**,直至所有 WebView controller 到 `Gone` 或总超时(片① 的 UDF 超时门这才真的在跑;首稿「走片①状态机」在立即 `exit()` 下是空话,#14 点破);PTY teardown 任一报错不影响已写入的照片;然后 `event_loop.exit()`。

红测(采纳审阅验收门 1/2):任一窗有脏预览时取消不改变任何窗;注入原子写失败 → 不退出;所有窗先完成快照再开始任何 teardown;`0 残留 msedgewebview2` 定义为**有界等待内归零**。

### E3. 小账核对

同首稿(标题栏几何两份副本 Q5#2 开工先查;`set_window_outer_rect` 已是现在时)。

---

## 片 F —— tear-out

### F0. 范围

- **F1 拖出**:tab 拖越窗界松手 = 指针处开新窗;**聚焦卡片列的卡(= tab)同权可拖出**(§7.1.6b′ 把卡定义成 tab 且已开放卡拖动;首稿引宽了裁决,更正)。**pane 源在卡列上照旧拒**(`hosts_pane_drop`)。pane 拖出窗界 = 先按 `StripExtract` 升格成 tab 再成窗,一条路。
- **F2 跨窗收养**:拖到另一扇 Folio 窗的 tab 条 = 移入。依赖 F1 的移交事务与 **App 级 drag broker**(见 F4),独立验收。
- **明写不做(v1)**:拖动中的活预览窗。

### F1a. 前置窄 spike:WebView2 重挂(审阅 #2,采纳)

`ICoreWebView2Controller::put_ParentWindow` 官方支持换父 HWND,composition controller 场景官方明说「visual tree 移到另一窗下应调它」。**spike 验证顺序与回滚**:目标窗建 per-seat visual → 源窗隐藏摘旧 visual → `put_ParentWindow(new_hwnd)` → `SetRootVisualTarget(new_visual)` → bounds/DPI/rasterization/claimable chords/焦点更新 → 目标 commit → 释放源 visual。验收:**不发生 navigation**,JS heap/history/scroll/表单/媒体状态全在,输入转交新 HWND,源窗不留洞。

- spike 通过 → **重挂是正路**;「按最后成功 URL 重建」降格为降级路径,且诚实写明降级丢页内状态(不是「闪一次」)。
- spike 失败(版本矩阵不可靠)→ 重建为正路,spike 证据入档。
- 缩略图(§7.11 的「最后一帧」)随座位迁移,或按明文规则作废——写死其一。

### F1b. 移交 = App 级事务(审阅 #5/#8,采纳)

首稿「一条移交函数」低估了。事务形状:

1. **目标先立**:新窗壳与一切不可失败资源(surface、compositor、visual)先创建成功;活载荷仍由源持有。失败 → 销毁空壳,源分毫不动(这也是审阅 #3「通过」的前提)。
2. **一次 commit**:身份重寻址、窗外状态迁移、源/目标 tab 表变更、空源窗吸收,在同一事件循环事务内完成;之后不得再有可失败步骤。
3. **一次记账**:两窗 `window_pictures` 同批刷新,`record_session` 只调一次(debounce 单线程,事务不跨 turn 就没有缝——审阅 #5 确认了这一半)。

**身份重寻址(裁决)**:`TabId` 每窗发、`SeatId` 每 tab 发,App 级 worker 回执只带 `LeafId{tab,seat}` 不带窗——直接搬表必然让在途回执落错窗(F2 必现)。**取「迁移时 rekey + 每叶 generation 作废在途回执」**:与既有 generation token 习语(webhost 状态机、preview key)同族,不动全仓 ID 制度;所有 worker completion 校验 generation,过期即丢。备选「App 级永不复用 ID」记为被否:改动面全仓,收益只在移交一刻。

**窗外状态迁移清单**(内容随载荷走,能重建的显式作废):`WindowRuntime.web` 的座位项(经 F1a 重挂或重建)、`web_thumbs` 最后一帧、preview watcher 订阅(`sync()` 天然跟着座位走,验证即可)、attention ticket、通知路由、一切按 TabId/SeatId 建键的缓存(逐项列举进切片工单)。

**PTY wake 重绑**(审阅 #9,采纳——首稿「shell 一毫秒不断」只对进程成立,不对唤醒成立):`LeafSession` 的 wake 握着**源窗**的 `AtomicBool`,源窗被吸收后输出永远叫不醒目标窗。移交时逐叶重绑 wake 目标(可换式 waker),红测照审阅原句:**源窗被吸收后,目标窗里静默 shell 的延迟输出能从全空闲事件循环自行唤醒并呈现**——只断言 scrollback 抓不到这个失败。

### F1c. 动词与门(审阅 #11,半采纳半驳回)

- **驳回其前提**:「独 pane 没有 `⌄` 可按」已过时——**角落幽灵 `⌄`(2026-08-21,`78d3224`,§7.1.6i)就是独 pane 的门**,开的是同一张 pane 菜单。所以 `Move pane to new window` 加进 pane 菜单后,独 pane 经幽灵 `⌄` **可达**,`Move pane to new tab` 在独 pane 上是空动作的挂账由新行接住。
- **采纳其正题**:DESIGN 1134 点名的「pane 独处时终端**右键**菜单的动词段」仍是独立一单,F1 不冒领;方案明写:右键段欠账原样挂着,F1 交付的门 = pane 菜单行(幽灵 `⌄` 与分屏头 `⌄` 同一张)+ 拖出手势,两扇门一条移交函数(f 单先例)。Files/Preview 独座位的动词面欠账(§7.1.6i 报过)也原样,不混入。

### F2 + F4. 跨窗收养与 App 级 drag broker(审阅 #12,采纳)

winit 的 Win32 capture 让源窗收到窗外 motion/up(F1 够用),但**目标窗收不到这条 pointer stream**——F2 的 spring gate、落点高亮不能在目标 `WindowRuntime` 里自然发生。**App 级 drag broker**:源 client 坐标 → 屏幕物理坐标 → 按 z-order/可见性找 Folio 目标窗 → 按目标 DPI 换回 target client 坐标 → 驱动目标窗的 `TabRun` 命中与 spring 状态;源持有捕获,broker 负责目标预览绘制与撤防。要定义:拖到目标窗正文(= 无落点)、目标窗悬停中被关闭、源窗失焦、Esc、显示器/DPI 变化——各自的结果逐条写进切片工单。

### F5. 新窗几何(审阅 #13,采纳)

屏幕命中与 work-area 一律物理坐标;期望尺寸 = 源窗**逻辑**外框换算到目标显示器 DPI;**保持抓握点相对 tab 的 offset** 再做 work-area clamp;走 `set_window_outer_rect`。混合 DPI(1.5×↔2.0× 双向)tear-out 实机验收各一次。

---

## 验收门(采纳审阅第三节全部八条)

1. Quit 脏门三态与原子写失败注入;2. 快照先于一切 teardown;3. F1/F2 在目标创建/surface/compositor/Web 重挂各注入失败,源与 Recent/session 原样;4. 源/目标故意同号 `TabId`/`SeatId`,在途与新回执各落各家;5. 吸收源窗后延迟 PTY 输出自行唤醒;6. Web 优先路径不导航且状态全在、降级显式记录;7. 聚焦列 card-tab 拖出通过、stage-pane 拒绝;8. 混合 DPI 双向 F1。

## 排期

E(E1+E2,可并行两线)→ F1a spike → F1b/F1c(主线)→ F2+F4。每片照仓规:红测先行、三门、DESIGN 现在时、实机证据。

## 给审阅者(第二轮)

首轮十条必改的落点已逐条标注在文中(#2 F1a、#4/#7/#14 E2、#5/#8 F1b、#9 F1b、#10 F0、#11 F1c、#12 F4、#13 F5、#6 E1)。请复核:①F1a 的 spike 验收单是否漏了重挂路径的已知坑(如跨 environment 不可重挂、DPI rasterization scale 的时序);②F1b 取「rekey + generation」而非全局 ID 的取舍;③#11 的驳回(角落幽灵 `⌄` 作为独 pane 的门)是否成立;④其余是否还有阻断项。

---

# v3 增补(2026-08-23,按第二轮复审;第二轮意见见 codex-confirm.md)

第二轮结论:架构与产品裁决全部通过(含 #11 驳回成立),剩四簇「把协议写到可注错可回滚的精度」。逐条增补:

## F1a 重挂合同(复审 ①,七条全采纳)

顺序改为:隐藏 controller → `put_RootVisualTarget(nullptr)` → 源 device commit → `put_ParentWindow(new_hwnd)` → 设目标 target → 目标 device commit → bounds/presence → `NotifyParentWindowPositionChanged`。**「prepare + 可补偿 platform handoff + model commit」三段**取代首稿「commit 后无失败步骤」:handoff 每一步失败都有写明的补偿(回旧 HWND + 旧 target + 旧 bounds);补偿也失败 → 关 controller 走有损重建降级并留证据日志,不再声称源原样。**`WebSeat::rehost(new_seat, new_hwnd, …)` 是窄合同**——`WebSeat` 自身缓存 `seat` 与 `hwnd`,只搬 map 项会让下一次 browser crash 重建回到旧窗;红测 = 迁移后注入 browser process failure,页面在**目标**窗原位重建。DPI 唯一所有者:spike 先读死 `ShouldDetectMonitorScaleChanges` 与 `BoundsMode` 并入档;验收含 WebView 自带 context menu/tooltip、IME、accessibility 位置。**单一 environment 写成前提断言**(本仓 environment 进程级共享,不存在跨 environment 搬迁)。输入/焦点收尾:换父前向旧宿主结清 leave/capture/按钮态;焦点随 tab 恢复到目标页(用户的手在拖它,意图明确)。

## F1b 身份:**改选 A 案**(复审 ② 要的清单已做,证据如下)

盘点(2026-08-23,main `bf9b209`):`next_tab_id` 全仓 18 处提及、真实铸号点约 5 处(新窗初始化、恢复路径、开 tab 增量);worker 回执(`SessionDecorationTask` 各变体)**本来就带 `leaf: LeafId{tab,seat}`**,drain 是 `for_each_window(apply_*_results)` 挨窗过滤。**A 案(`TabId` 铸号升 App 级、进程生命周期不复用;`SeatId` 保持 tab 内局部)diff 更小**:改约 5 个铸号点,回执结构一字不动,地址 `{全局唯一 TabId, tab 内 SeatId}` 自然全局无歧义,移交不 rekey、在途回执跟着 tab 走。B 案(rekey + generation)需给每条回执路径加 `{window, generation}` 并写完整作废协议,被否。**顺带的发现**:现状两窗同号 `TabId` + 挨窗 drain,worker 回执可能已经会误投——E 开工第一条红测先证明它是否已是活缺陷(是则 A 案顺带修掉,记档)。pane 升格成 tab 时从 App 铸新号,旧号的在途回执随源 tab 之死自然失配。恢复路径由 App 重新铸号(磁盘本不存 runtime id)。复审验收门第 4 条照钉:旧完成在源号已亡后到达,不落任何叶。

## Quit 保存分支(复审 ④-a,全采纳)

「保存」逐项走既有冲突检测与原子写(§7.1.3),全部成功才拍照进 teardown;任一项失败/冲突未裁 → 不退出、不藏窗、失败项列名;已保存的缓冲诚实变干净(不宣称整次零变化——那句只属于「取消」);「丢弃」只丢内存修改。重命名编辑器一类非文件缓冲:「保存」= commit 该编辑(与 Enter 同义),不阻退出;逐类列举进 E2 工单。

## F2 与杂项(复审 ④-b/c/d)

- **spring deadline**:App 级 drag broker 暴露 `next_deadline` 进事件循环 wait-until(与 `⌄`/拖拽 dwell 同款帧钟纪律);目标关闭/目标变更/Esc/capture lost 撤 deadline。红测 = 进入后零后续 motion,250ms 到点仍 spring-open。
- **broker 撤防清单**补:Win32 capture lost、源窗拖动中被关、锁屏/显示配置突变——统一结果 = 取消预览、清高亮、载荷归源。
- **缩略图规则写死**:live 重挂且页面/viewport 未变 → 最后一帧随座位重址;重建降级或目标 DPI/卡尺寸变 → 旧帧作占位并当场欠一张(不无限期把旧 DPI 图当新图)。F1b 开工前关闭。

## 验收门增补(复审第二节 7 条全部并入)

重挂四注错点(断旧 target / 换 parent / 设新 target / 目标 commit)各自补偿或显式降级;重挂后 browser-process failure 原位重建;100↔150↔200 双向含 context menu/IME/accessibility;同号在途回执不落新叶;Quit 保存注入磁盘冲突与原子写失败;跨窗 spring 无 motion 到点;幽灵 `⌄` 两路开菜单含新行且与拖出同一事务。
