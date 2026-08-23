# 多窗 E + F 方案审阅

审阅对象：`docs/plans/multiwindow-ef/plan.md`（2026-08-23 送审稿）

审阅依据：`docs/spikes/spike-multiwindow.md`、`docs/DESIGN.md` §2.5/§2.7/§7.1.4/§7.1.6b′/§7.1.6h/§7.1.6k/§7.8–§7.11，以及当前 `crates/bt-app/src/{main,webhost,persist}.rs`、`crates/bt-platform/src/webview.rs`、`crates/bt-persist/src/session.rs`。

## 总结论

**必改后重审，当前不建议按稿开工。** E1 的产品裁决和 F1「空源窗不进 Recent」可以通过；其余三个审阅问题存在阻断项。最关键的是：

1. WebView2 composition controller 有官方支持的跨 HWND 重挂路径，稿中“唯一认 HWND，所以只能关引擎重建”的前提不成立；
2. Quit 的会话照片不能保存脏预览正文，因而“不弹确认，因为可逆”与现有脏门及持久化红线冲突；
3. 活 tab/pane 跨窗不是给 `NewWindowPlan` 加一个序列化 seed 就能完成，必须先解决失败原子性、窗口本地身份重寻址、PTY wake 与窗外宿主状态的迁移；
4. DESIGN 只让聚焦卡片列拒绝 **pane 源**，并未拒绝卡片自身这个 tab 源；送审稿把二者混在了一起。

下面的“必改”均指方案文本在实现前必须补齐，不是要求本次审阅直接改产品代码。

## 一、对末节五个问题的逐条结论

### 1. E1「全进程一个主题」是否有反例场景

**结论：通过。**

当前裁决与既有实现和 schema 一致：`THEME`、`CURSOR_STYLE` 是进程级状态，`WINDOW_CLASS_BACKGROUND` 更是 winit 共用窗口类的类级画刷；`SessionV1.theme/cursor_style` 也明确是顶层进程事实，而不是 `SessionWindowV1` 字段。现有需求里没有一条要求两窗各自深浅色。若以后需要 per-window theme，代价不只是搬字段，还包括独立窗口类/背景刷、主题相关缓存与设置语义，理应另立工单。

但 E1 的验收措辞需在后文第 6 条补齐：它标题同时点名光标样式，却只给主题写了传播红测；“同帧”也不能表述成两个 swapchain 的原子 present。

### 2. F1 网页座位是否只能「关引擎 → 新窗重建」

**结论：必改。**

稿中“唯一认 HWND 的是网页座位，所以必须关闭 controller 后重建”的推论不成立。WebView2 Win32 的 `ICoreWebView2Controller::put_ParentWindow` 明确用于更换父 HWND；官方对 composition controller 的说明更直接：如果应用把 WebView visual tree 移到另一扇窗口下面，应调用 `put_ParentWindow` 更新新的父 HWND。当前产品又恰好保留了两项所需对象：`WebHost` 持有 `ICoreWebView2Controller` 与 `ICoreWebView2CompositionController`，后者已经通过 `SetRootVisualTarget` 指到本窗的 per-seat visual。

官方依据：

- [ICoreWebView2Controller::put_ParentWindow](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)
- [ICoreWebView2Environment3::CreateCoreWebView2CompositionController](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment3)

方案应先加一个窄 spike，验证以下顺序及回滚，而不能先裁掉它：目标窗创建 per-seat visual；源窗隐藏并摘旧 visual；controller `put_ParentWindow(new_hwnd)`；composition controller `SetRootVisualTarget(new_visual)`；更新 bounds、DPI/rasterization、claimable chords 与焦点；目标 compositor commit；最后释放源 visual。验收至少要证明页面不发生 navigation，JS heap、history、scroll、表单输入、媒体状态仍在，鼠标/键盘转交目标 HWND，源窗不再留洞。

如果真机或受支持版本矩阵证明重挂不可靠，才把“按最后成功 URL 重建”定为降级路径；重建不能在 spike 前被写成唯一正路。降级还应诚实说明会丢失当前页内状态，而不只是“闪一次”。

### 3. F1 源窗空后「关闭且不留种子」是否冲突

**结论：通过。**

这不属于 DESIGN §2.7 的“用户关闭非最后一扇窗”，而属于一次移动事务对空容器的吸收。既有 §7.1.6k 已有完全同类的 `absorb_tab_into_strip` 判例：tab 因最后一枚 pane 被移走而消失，不走 `close_tab`，否则会把仍活着的内容写进 Recent 并关闭它。窗口层照搬这一语义是自洽的；空窗口也不符合 `SessionV1.windows[]` 的不变量，当然不应留下“空种子”。

通过有一个前提：目标窗已经成功接住活载荷，才可移除源窗。目标窗创建、Web 重挂或身份重寻址任一步失败时，源窗和载荷必须原地保持；不能先把源窗清空再尝试开目标窗。这个事务性缺口见第 5、8 条。

### 4. Quit 不弹确认是否成立

**结论：必改。**

“会话整体保留，所以 Quit 可逆”只对布局与可恢复位置成立，对脏预览正文不成立。`window_snapshot` 只持久化路径、URL、布局和预览池身份，不存正文、不存 dirty 位，这是 §7.1.4 的明确红线。现行 `WindowEvent::CloseRequested` 在进入 `close_window` 前会调用 `raise_dirty_gate(GateRequest::Shut)`，正是为了避免关闭窗口无声丢掉脏内容。Quit 若一律不确认，会绕开同一扇门并造成不可逆数据丢失。

方案必须改为应用级两阶段脏门：先扫描所有窗口的脏预览与尚未提交的编辑，统一给出保存/丢弃/取消的答案；只有全部窗口都可关闭时才进入 Quit 提交。没有脏内容时不需要额外的“确定退出”仪式，这一半论证仍可保留。不要逐窗弹多个对话框，也不要在用户取消后已经关掉前几扇窗。

### 5. 移交与 `windows[]`/debounce 是否有并发缝

**结论：必改。**

当前不存在多线程写盘竞态：所有窗口状态在同一 UI 线程更新，`SessionStore::flush_if_due` 只在 `about_to_wait` 的应用级时钟执行；`record` 只是用最新整份文档覆盖内存里的待写值。因此，只要整个移交在一次事件循环事务中完成，debounce 本身不会在两条语句中间抢写。

问题是送审稿没有定义这样的事务。当前 `NewWindowPlan` 只装 `SessionWindowV1`，走的是序列化恢复/重新创建内容，不能承载活 `TabState`；而现有 `extract_pane_into_new_tab`、`settle_seat_set_change`、开窗记录都会各自 `mark_session_dirty`。如果照“先从 A 摘、排一个新窗计划、下一门再交给 B”实现，就会产生载荷暂时无主或重复、开窗失败无法回滚、空源窗过早被移除的问题。

方案必须明确一个 App 级 transfer transaction：目标窗口壳与所有不可失败资源先创建成功；活载荷仍由源持有；一次 commit 中完成身份重寻址、外置状态迁移、源/目标 tab 表变更及空源窗吸收；最后同时刷新两扇窗的 `window_pictures`，只调用一次 `record_session`。任何 commit 前失败都只销毁目标空壳并保留源；commit 后不得再有可失败步骤。测试应注入创建 surface/compositor、Web 重挂等失败，断言 session 文档从未出现“载荷在零窗/两窗”或空窗口，Recent 也不增加。

## 二、方案之外的技术矛盾与遗漏失败模式

### 6. E1 漏了光标传播，且“同帧”不是可验合同

**结论：必改。**

E1 标题是“主题与光标样式”，正文和红测却只覆盖 `theme_mode`。当前 `apply_cursor_style` 改进程 static 后只给发起窗发布一帧；其它窗口最多在以后某次偶然 redraw 才读到新样式。应像主题/字体一样通过 `pending_application_change`（或等价单一通道）让每扇窗在同一 event-loop turn 结清，并增加两窗 cursor-style 热切红测。

“两扇窗同帧都换”也应改成“同一事件循环轮次内，两窗都应用相同 revision 并各自排入 redraw，返回 Wait 前没有旧状态窗口”。两个独立 swapchain/DWM present 无法承诺跨窗原子呈现；测试不应钉一个平台没有提供的同步语义。

### 7. Quit 不能直接“复用 `FolioApp::exiting`”，写盘失败语义也未定义

**结论：必改。**

`exiting` 是事件循环已经停止后的兜底回调，不是键盘动作可调用的退出事务。当前它和 `fail` 都用 `for_each_window(close_window(true))`，而该遍历遇到第一个错误就停止；`close_window(true)` 又把“拍快照”和“关 Web/PTY”交错在一起。因此前一窗 PTY shutdown 失败时，后面的窗口可能根本没拍照，却仍会清空窗口表并结束进程。这不满足“所有窗口先组成一份照片、一次原子写、然后退出”。

Quit 应有独立的 App 级阶段：脏门/重命名提交全部成功 → 对所有窗口只读拍照 → 组装最终 `SessionV1` → 同步且可判失败地原子写 → 成功后才关闭 controller/PTY 并 `event_loop.exit()`。现有 `SessionStore::close()` 返回 `()`，即使 `flush` 失败也会移除 `session.lock`；方案若坚持“写完再退出”，就必须让最终 flush 可观察失败并规定失败时保持窗口打开、显式告警。否则应把承诺改成“尽力写入后退出”，不能两种语义同时写。

### 8. 活载荷的身份不是只搬 `TabState`：现有 ID 与 worker 路由是阻断项

**结论：必改。**

`TabId` 是每窗口计数器发的；`SeatId` 更是每 tab 自己发的，DESIGN §7.1.6k 已用“两张 tab 都可能有 `SeatId(3)`”钉死。与此同时：

- App 级 math/files/preview worker 的请求/结果只带 `LeafId { tab, seat }` 或 `TabId`，不带 `WindowId`；`AppEvent::*Ready` 又按窗口顺序遍历，而第一个窗口会先 drain 共享 receiver；
- `WindowRuntime.web` 是窗口级 map，却只以 `SeatId` 为键；
- `NewWindowPlan` 当前只有持久化的 `SessionWindowV1`，没有活对象载荷。

所以 F 不能把“整 tab 移交函数”理解成 `Vec<TabState>` 的 remove/insert。方案必须先裁定稳定地址：要么把窗口身份纳入所有异步地址（例如 `{window, tab, seat, generation}`），要么把 tab/seat 改成 App 级永不复用 ID；若在移交时 rekey，则所有在途 worker completion 必须用 generation 作废或随迁，绝不能落到目标窗中碰巧同号的另一叶。这个问题在 F2 把 tab 插入已有目标窗时是必现而非边角。

还应把以下窗外状态列成迁移清单，而不是只写 `TabState`：`WindowRuntime.web`/per-seat visual、`web_thumbs` 的“最后一次在玻璃上的样子”、preview watcher 订阅、焦点/attention ticket、通知路由及所有按 TabId/SeatId 建键的缓存。能安全重建的要显式作废；属于内容的必须随载荷走。

### 9. PTY 虽不认 HWND，但它的 wake 合并器认源窗口

**结论：必改。**

“ConPTY 句柄不认 HWND，所以 shell 一毫秒不断”只说明进程不会因换窗而必须重启，不足以证明移交完成。每个 `LeafSession` 创建 PTY 时拿到的是源 `WindowRuntime::pty_wake.wake()`；该 wake 内含源窗自己的 `AtomicBool`。移到另一扇窗后，目标 `drain_pty` 只会 lower 目标窗的 bit。源窗若被吸收，旧 bit 第一次 raise 后可能永远保持 true，后续输出不再 post `AppEvent::PtyOutput`，空闲事件循环就会睡过该 PTY。

移交必须重绑每个活 PTY 的 wake，或把 PTY wake 合并器提升为 App 级；并以“源窗被吸收后，目标窗里的静默 shell 延迟输出仍能自行叫醒并呈现”为红测/实机验收。仅断言 scrollback 字节仍在抓不到这个失败。

### 10. 聚焦卡片列只拒 pane 源，方案却把 tab 源也拒了

**结论：必改。**

F0 的“拖着一个 tab（从水平条/竖栏；聚焦卡片列不接）”及测试 `the_focus_column_refuses_the_tear_out` 与现行 DESIGN 冲突。§7.1.6b′ 已把卡片定义成 tab，并已开放卡片拖动排序；它拒绝的是 `TabRun::hosts_pane_drop == false`，即从舞台拿来的 **pane** 不能在卡列上获得 StripExtract/StripAdopt offer。卡片自身作为 tab 源并未被拒绝。

F1 应让“从卡片列拖 tab 出窗”与横/竖 tab 条同义；只保留“从舞台拖 pane 到卡列不生成 tab/窗”。若产品确实想禁止前者，必须作为一条新的显式政策重新裁决，不能引用 §7.1.6b′ ②作为理由。测试也应拆成 tab 源通过与 pane 源拒绝两条。

### 11. `Move pane to new window` 没有结清 DESIGN 点名等待 F 的独 pane表面

**结论：必改。**

DESIGN §7.1.6i（约 1134 行）的挂账不是普通 `⌄` 菜单是否多一行，而是“pane 独处时终端右键菜单的动词段”；独 pane 不画 pane head，因而根本没有 `⌄` 可按。该段明确说 `Move pane to new tab` 在独 pane 上为空动作，它的诚实兄弟 `Move pane to new window` 等 F 一并裁。

送审稿只说“pane 菜单（⌄）加一行”，并让独 pane 的 `Move pane to new tab` 继续隐藏/禁用，没有回答独 pane 右键段是否出现 `Move pane to new window`，因此没有结清被点名的裁决。F1 必须明确：终端独 pane 的右键菜单是否增加该行、它与 `⌄` 行/手势是否共用同一移交函数；Files/Preview 因没有 `⌄` 时又有哪些可达门。可以裁定暂不加，但必须把理由和欠账状态写清，不能声称“一并裁”后仍留空。

### 12. 跨窗拖拽需要 App 级 broker；目标窗不会收到这次 pointer stream

**结论：必改。**

当前 `Drag` 与 `pointer_position` 都住在源 `WindowRuntime`。winit 在按下后用 Win32 capture 让源窗继续收到窗外 motion/button-up，这对 F1 是可用基础；代价是目标窗口不会同时收到这条 pointer stream。F2 的 spring gate、目标 tab 激活、落点高亮和最终 adopt 因而不能各自在目标 `WindowRuntime` 里“自然发生”。

方案应指定 App 级 drag broker：把源 client 坐标转换为屏幕物理坐标；按 z-order/可见性找 Folio 目标窗；再按目标窗自己的 DPI 把点换回 target client 坐标，调用目标窗的 `TabRun` 命中与 spring 状态；源仍持有捕获，broker 负责让目标绘制预览并在取消/捕获丢失/目标关闭时撤防。还要定义拖到另一 Folio 窗正文、目标窗在悬停中关闭、源窗失焦、Esc、显示器/DPI 变化时的结果。F2 目前一句“状态不共用”不足以构造这条链。

### 13. 新窗几何缺少跨 DPI 的单位与抓握点规则

**结论：建议。**

“外框大小抄源窗，位置=指针处（夹进工作区）”没有说明抄物理像素还是逻辑尺寸，也没有说明指针是新窗左上角还是保持抓住 tab 时的 offset。跨 1.5×/2.0× 显示器时两种选择会得到明显不同的窗；这正是 multiwindow spike 已强调每窗 DPI 独立的场景。

建议写死：屏幕命中与 monitor work area 一律用物理屏幕坐标；窗口期望尺寸按源窗逻辑外框换算到目标 monitor DPI（或明确选择保物理尺寸并说明理由）；保持抓握点相对标题/tab 的 offset，再做整窗 work-area clamp；最后仍走 `set_window_outer_rect`。补一条混合 DPI tear-out 实机验收。

### 14. Web 退出验收与当前状态机实际运行方式不一致

**结论：必改。**

方案称 Quit “走片①关闭状态机，UDF 收尾超时门是唯一兜底”，但当前 `Runtime::close_window` 调 `web.close()` 后马上销毁窗口/退出循环，并明确写着“这扇窗不会留下来等状态机”；`WebSeat::tick` 的超时门只有事件循环继续 pump 时才会触发。因此显式 Quit 若立即 `event_loop.exit()`，并没有真的走完那道超时门。

方案要二选一并写实：要么增加 `Quitting/WaitingForWeb` 阶段，窗口隐藏但事件循环继续运行至 controller 全 Gone 或总超时，再 finish/exit；要么明确依赖宿主进程退出带走 WebView2 子进程，不再声称 UDF 状态机完成。`0 残留 msedgewebview2` 也应定义为 Folio 退出后在有界等待内归零，而非退出瞬间采样。多页面共享 environment 时还应测试所有 controller 都被请求关闭，而不是第一张页面的退出事件让全局收尾提前完成。

## 三、建议修改后的最低验收门

**结论：建议。** 在现有红测之外，至少补以下门，能把上述阻断点变成可执行合同：

1. Quit：两窗中任一有脏预览时，取消不改变任何窗；确认保存/丢弃后才一次写全；注入 session 原子写失败时不退出且 `session.lock` 不伪装成干净退出。
2. Quit：所有窗口先完成快照，再开始任何 PTY/Web teardown；任一子进程 shutdown 报错不影响其它窗口已经写入最终照片。
3. F1/F2 事务：分别在目标窗口创建、surface、compositor、Web 重挂处注入失败，源载荷/PTY/Recent/session 均原样。
4. 身份：源/目标故意制造相同 `TabId` 与 `SeatId`，在途 math/files/preview 回执只能落到原内容；移交后新回执只能落到目标内容。
5. PTY：吸收源窗后，延迟输出能从全空闲 event loop 自行唤醒目标窗。
6. Web：优先路径不导航且保留 history/scroll/form/JS state；降级路径只回最后成功 URL并显式记录降级；最后帧缩略图随座位迁移或按明文规则作废。
7. 聚焦列：card-tab tear-out 通过、stage-pane 到 card column 继续拒绝。
8. 混合 DPI：1.5× → 2.0× 与反向各做一次 F1，外框、抓握点、work-area clamp、输入命中正确。

## 最终判定

**必改。** E1 的全进程主题裁决与 F1 的空源窗吸收语义可保留；Web controller 路线、Quit 脏门/提交事务、跨窗活对象身份与失败原子性、聚焦卡片列政策、独 pane 菜单挂账需改完再送审。上述问题未结清前，直接按当前排期把 F1/F2 当“一条移交函数”实现，最可能出现的不是动画瑕疵，而是脏内容丢失、PTY 不再唤醒、异步回执落错窗或目标开窗失败后活载荷无主。
