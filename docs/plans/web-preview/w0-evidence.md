# W0′ 取证:`plan.md` §2 十扇门的实机答案

日期 2026-08-20 · 分支 `webview2-w0-spike` · 探针 `spikes/webview2-w0/`(独立 workspace,不进产品依赖图)
本机 WebView2 Runtime **151.0.4129.93**(`GetAvailableCoreWebView2BrowserVersionString`);注册表报同一版本。

**这是取证单,不是产品单。** 没有改动任何产品 crate、`design/ui-mockup.html` 或 `dist\`。

复跑:

```
cd spikes/webview2-w0
cargo build --release
.\run-gates.ps1 -Shots <目录>     # 十扇门,分八个进程跑完,末尾打印判定表
cargo test                         # 22 条纯逻辑红测(§3 allowlist、§4 状态机、BINDINGS 转录)
```

每条读数都是 `gates.jsonl` 里的一行 JSON,下文引用的字段名就是那行的字段名。截图在 `docs/spikes/artifacts/webview2-w0/`。

**读数的适用范围:一台机器、一个 Runtime 版本。** 时间类的数(`CapturePreview` 61 ms、整窗 capture 15 ms、`ProcessFailed` 延迟 4 秒)是这台机器这一天的量级,不是跨机器的常数;结构类的结论(谁盖谁、哪个事件到不到、哪条 URL 拦不拦得住)才是可以搬走的。哪一条属于哪一类,下文逐条写明。

---

## 0. 总结论

**W2 六片条件 GO 成立,可以开工。** 十扇门 **10 扇全部 pass**,其中门 4 是**有条件 pass**(本机无数字化仪,真触屏驱动路径无设备不可测)。另有 3 项在门内单独标注"未测",都不是失败而是**没有设备或没有许可**:DRM 受保护流、GPU TDR、Runtime 自更新。1 项标注 **blocked,须人工测**:IME 候选窗的 pane 偏移。

方案有 **三处必须修订**、**六处必须新增约束**(§2)。最重的一条推翻了 §2.5 的前提:`AcceleratorKeyPressed` 不是"覆盖有限"——产品 `BINDINGS` 的 30 个和弦逐个实测,**30/30 全部可被宿主夺回**。第二重的一条否掉了 F2 的隐含假设:**WebView 隐藏时 `CapturePreview` 根本不返回**。第三重的一条改的是 §4 的收尾契约:**浏览器进程被杀死时 `BrowserProcessExited` 不一定到达**——同一段代码两次运行,一次 280 ms 就到,一次 25 秒没来。

---

## 1. 十扇门结论表

| # | 门 | 判定 | 机器证据 |
|---|---|---|---|
| 1 | visual 层叠 / clip / opacity / 圆角 / 动画原子性 | **pass** | 浮窗压进座位 42 120 px(几何理论 41 400);半透明采样 `[131,97,160]` 落在浮窗 `[47,85,232]` 与网页 `[234,114,71]` 之间;圆角 48px 后角落成洞而网页保留 526 718/526 776 px;动画 10 帧 `gap=0`,**0 帧撕裂**;座位内洞占比 0.012% |
| 2 | 像素矩阵 | **pass** | `CapturePreview` 中位 **61.1 ms**(58.9–82.6);整窗 WGC 中位 **15.5 ms**;**隐藏时 `CapturePreview` 超时不返回**;视频页两帧差 891 473/891 588 px、黑帧率 0.0;`PrintWindow` 拿得到网页 520 822 px;DRM 未测 |
| 3 | 完整鼠标 | **pass** | 除 `LEAVE` 外全部送达;坐标严丝合缝(client `(511,242)` − 座位原点 `(224,48)` ÷ 2.0 = 期望 CSS `(143.5,97.0)`,网页报 `(143,97)`);`CursorChanged` 报 32513/32512/32649/32646;右键起 `contextmenu`、中/X 键起 `auxclick`、四连点起 `dblclick`。**`LEAVE` 三种写法全被 `E_INVALIDARG` 拒绝**,替代路径已实测可用 |
| 4 | touch / pen / 多指 / OLE drop | **有条件 pass** | 合成 pen/touch 的 `pointerType` 正确;OLE 拖放 **`drop` 事件到达网页**,带 `[text/plain, text/uri-list, Files]`。**本机 `SM_DIGITIZER=0`、`SM_MAXIMUMTOUCHES=0` —— 真触屏驱动路径无设备不可测** |
| 5 | 键盘 / 焦点 / 快捷键实际绑定矩阵 | **pass** | **30/30 和弦 `AcceleratorKeyPressed` 触发且 `SetHandled(true)` 生效**,网页只见裸修饰键,宿主 HWND 0 条按键消息;Tab/Shift+Tab **不**进回调、在页内走控件,走到边界 `MoveFocusRequested reason=1 (NEXT)`;Esc 进回调;DPI 192→144 时 `devicePixelRatio` 自动 2.0→1.5。**IME pane 偏移 blocked,须人工测** |
| 6 | UIA | **pass** | `AutomationProvider` 可取;从宿主 HWND 走 UIA 树到达 `framework=Chrome` 的 `BrowserRootView`,名字是网页标题 `W0 probe page - Web content` |
| 7 | 生命周期 | **pass** | 一个 environment 两个 controller,**两者报同一个 browser pid**;杀 renderer → `ProcessFailed` 到达(延迟两次分别 4 145 ms / 167 ms),`Reload` 复原;杀 browser → `ProcessFailed` 每次都到,**`BrowserProcessExited` 一次 280 ms 到、一次 25 秒不到**;Runtime 缺失 **同步 0 ms** 失败于 `0x80070002`,而注册表仍谎报已装。自更新/GPU reset 未强制触发 |
| 8 | 后台账 | **pass** | 同一 6 秒窗口:可见 CPU **2 843 ms**、rAF **718 帧**;隐藏 CPU **16 ms**、rAF **0 帧**(节流彻底);私有内存仅从 211.3 MB 降到 205.5 MB;renderer 数 1 origin → **1**,2 origin → **2** |
| 9 | 安全门 | **pass** | 直连 / 重定向 / 页面脚本 三条路径 9 行红测**全部未抵达目标**;受控 file 入口:铸造的那一个 URL 加载成功,`C:\Windows\win.ini` 被 `NavigationStarting` 以 `FileScheme` 取消;下载被 `DownloadStarting` 取消;`window.open` 被 `NewWindowRequested` 接管 |
| 10 | 恢复门 | **pass** | 事件装齐前不首航(`ControllerPending` 状态下不发 `Navigate`);失败页不覆盖可恢复 URL;杀 browser 后 generation 1→2,**晚到的 controller 判 `CloseOrphanController`、晚到的 `NavigationCompleted` 判 `Ignore`**;重建后回到最后一个成功 URL 并加载成功 |

---

## 2. 需要改方案 / 改产品的发现

### 必须修订方案的三处

**① §2.5 的前提被推翻:`AcceleratorKeyPressed` 覆盖是完整的。**

方案写"AcceleratorKeyPressed 覆盖有限"。逐键实测 `bt_app::shortcuts::BINDINGS` 全部 30 个带和弦行——含 `Ctrl+Tab`、`Ctrl+Shift+Tab`、`Alt+Shift+-`、`Alt+Shift+=`、`Ctrl+,`、`F3`、`Shift+F3`——**每一行都触发了回调**,`SetHandled(true)` 之后网页只看得到裸 `Control`/`Shift`/`Alt`,看不到和弦键本身;宿主 HWND **一条按键消息都没收到**,证明这条路走的是回调而不是消息队列。

→ 切片 ①(platform host/input)的键盘接线**不需要任何"捞不回来的键"的兜底设计**:查 `BINDINGS` + `SetHandled` 就是全部。

**② F2 缩略图的取图时机被钉死:隐藏时 `CapturePreview` 不返回。**

`SetIsVisible(false)` 后调用 `CapturePreview`,10 秒超时内完成回调**从未到达**(三次独立运行复现)。方案 §1 写"隐藏 tab 是否可截**须实测**"——实测结果是**不可截**。

→ §5 片⑥ 的"缓存最后一次成功帧"不是优化而是**唯一可行实现**:只能在座位可见时抓,隐藏/失败时沿用上一帧。

**③ §4 的 UDF 收尾契约要加一条:崩溃路径上的 `BrowserProcessExited` 不可依赖。**

正常关闭时该事件可靠到达(每次运行的 `shutdown` 记录都是 `browser_process_exited: true`)。**崩溃路径上它是不确定的**:同一段代码、同一个动作(`TerminateProcess` 掉 browser 进程),两次完整运行给出两个结果——

| 运行 | `ProcessFailed` | `BrowserProcessExited` |
|---|---|---|
| 第一次 | 到达,`kind = 0` | **等满 25 秒未到** |
| 最终一次 | 到达 | 到达,**280 ms**,`kind = 1`(FAILED) |

两次的前置完全一样(都建了第二个 controller,两者报同一个 browser pid)。所以这不是"永远不来",而是**不能指望它来**。方案写"UDF 清理须等 BrowserProcessExited",照字面实现会在其中一半的崩溃里**挂到天荒地老**。

→ 收尾条件改为"`BrowserProcessExited` **或** `ProcessFailed(BROWSER_PROCESS_EXITED)`,并且带超时上限;超时后照样清理"。

### 必须新增的六条约束

**④ 视觉树挂载顺序必须写进切片 ①,而且和直觉相反。**

`IDCompositionVisual::AddVisual(visual, insertAbove = TRUE, reference = NULL)` 把视觉放到子列表**开头**,而 DirectComposition 子列表的**开头是最底层**。产品现在的 `bt_platform::Compositor`(`crates/bt-platform/src/lib.rs:670`)只有一个子视觉,这个参数今天没有后果;**web 视觉一进这棵树,它就决定谁盖谁**。

本探针第一次跑就踩了:浮窗被精确裁在座位左边界、座内内容完全消失——**和 2026-08-13 spike 里 child 模式 airspace 失败的照片一模一样**,而当时跑的是 composition 模式。判定 airspace 之前必须先确认挂载顺序,否则会把自己写错的参数读成引擎的能力上限。

正确写法:两个子视觉都用 `AddVisual(…, false, None)` 追加,得到 `[web, gpu]`,最后一个在最上。

**⑤ 网页像素带颜色变换,宿主像素不带——像素验收必须现场采样。**

同一张整窗截图里:宿主用 wgpu 画的矩形**逐字节原样返回**(`top_bar_nominal == top_bar_captured == [56,46,41]`);网页的 `#1b6ef3` 回来是 `[234,114,71]`(BGR),`#b7ff2e` 回来是 `[200,250,93]`。`CapturePreview` 与整窗 WGC **两个 oracle 互相一致**(按实拍色数 526 808 px,按 CSS 色数 0 px),所以 #5574 的黑帧在本机没有复现,但**两者都不等于 CSS**。

→ 预览块的像素验收(F2 缩略图比对、截图回归)**不得拿 CSS 常量当期望值**,必须先从屏幕采一次基准色。探针的 `Calibration` 就是这么做的。

**⑥ `SendMouseInput(LEAVE)` 在本 Runtime 上不可用,鼠标离开要靠"移到边界外"。**

三种写法全部被 `E_INVALIDARG (0x80070057)` 拒绝:带座位坐标、带 `(0,0)`、以及紧跟一次 `MOVE` 之后再发。

**替代路径已实测可用**:把 `MOVE` 送到一个落在 WebView bounds 之外的点(探针用 `(24,8)`),网页收到 `mouseout`。切片 ① 的鼠标转发要按这条写,不要按 `LEAVE` 写,否则光标移进周围 chrome 时网页的 `:hover` 会卡住不散。

**⑦ `Alt+Shift` 是 Windows 的键盘布局切换键,而 `BINDINGS` 有两行在上面。**

`split-horizontal` = `Alt+Shift+-`、`split-vertical` = `Alt+Shift+=`。本机只装一种布局,实测 `changed: false`;**装了两种以上布局的用户,按这两个和弦会同时切换输入法布局**。这不是预览块的问题,是快捷键表的问题——记在这里,交给快捷键裁决处理。

**⑧ IME 的 pane 偏移无法自动化取证,必须人工测。**

焦点进网页表单后 `ImmGetContext` 拿得到 HIMC,但 `ImmGetCompositionWindow` 返回 false、`ptCurrentPos` 恒为 `(0,0)`——**没有正在进行的输入合成时,引擎不通过 IMM 暴露候选窗位置**。#5570 那个"候选窗跟不跟 pane 偏移"的问题,探针给不出答案。

→ 沿用 spike 04 的先例(CJK IME 三家 × 手动项),**W2 片① 的验收必须包含一份人工 IME 测试记录**:座位偏移后、跨 DPI 后、窗口移动后,候选窗是否贴着光标。探针已经把座位挪了 `160×90`、跨屏 192→144 各做了一次读数,人工测可以直接复用同一套动作。

**⑨ 隐藏即是洞:`SetIsVisible(false)` 后座位是纯黑,宿主必须自绘占位。**

隐藏后整窗截图里座位内部 890 439 px 全是洞色,网页像素 0。这个窗口是 `WS_EX_NOREDIRECTIONBITMAP`,窗口类背景刷没有落笔的地方,所以洞是 `[0,0,0]` 而不是类背景色。

→ W1 小样里的"加载失败 / 进程崩溃 / Runtime 缺失"三个失败态**不是可选装饰**,它们是座位在那些时刻唯一的内容。

### 一条给 §0 的提醒(不是修订)

`window.open` 的 `IsUserInitiated`:探针用 `ExecuteScript` 触发 `window.open`,`NewWindowRequested` 报的是 **`is_user_initiated: true`**。这个标志反映的是"有没有用户激活",而不是"有没有人真的点了一下"——宿主自己注入的脚本会带上用户激活。产品不会这样用,但 §0 的"仅 user-initiated 转同 pane 导航"这条规则**不能被理解成一个防脚本的安全边界**,它只是行为分流。

---

## 3. 逐扇门证据

### 门 1 — visual 层叠、clip、opacity、圆角遮罩、动画原子性 · **pass**

复验 2026-08-13 的地基,在产品今天的树形状里重跑。

`alpha-modes`:`offered = [Auto, Inherit, Opaque, PostMultiplied, PreMultiplied]`,`chosen = PreMultiplied` —— 与原 spike 两行逐字一致。

`child-windows`:**1 个子窗口**,`Chrome_WidgetWin_0`,矩形 `0×0`,覆盖座位的子窗数 **0**。composition 模式下座位区域没有任何真子窗,airspace 无从谈起。

| 项 | 读数 |
|---|---|
| 座位占用 | 座位 888 520 px,洞色像素 107 px,**洞占比 0.012%**(网页自带半透明黑日志框贡献,非空洞) |
| 不透明浮窗压座位 | **42 120 px**(几何理论 41 400) |
| 半透明浮窗 55% | 采样 `[131,97,160]`,既非浮窗色也非网页色,**落在两者之间** → 逐像素真混合 |
| clip 到一半 | 网页像素 526 776 → 246 320,被裁掉的一半变成洞 445 218 px |
| 圆角遮罩 48px | 角落采样 `[0,0,0]` = 洞;网页像素 526 718 / 526 776 = **99.99% 保留** |
| opacity 0.45 | 采样从 `[234,114,71]` 变成 `[105,51,32]` |
| 动画原子性 | 座位左边缘扫 10 步,每步整窗截图 + 扫描线,**10/10 帧 `gap = 0`** |

截图:`g1-01-page-in-seat.png`、`g1-02-opaque-panel-over-web.png`、**`g1-03-translucent-panel.png`(半透明浮窗底下网页正文透出来)**、`g1-05-web-rounded-clip.png`、`g1-07-anim-05.png`。

### 门 2 — 像素矩阵 · **pass**

| 通道 | 得到什么 | 实测 |
|---|---|---|
| `CapturePreview` | **视口** PNG,1146×778(座位 1146×777),`contains_host_border: false` | 中位 **61.1 ms**,范围 58.9–82.6 ms(8 次) |
| 整窗 WGC | 最终合成帧,1378×889,含非客户区 | 中位 **15.5 ms**(等下一帧,受刷新率下界约束) |
| `PrintWindow(PW_RENDERFULLCONTENT)` | 客户区位图,**含网页内容** 520 822 px,含宿主边框 7 456 px | 单次 |

- **隐藏时 `CapturePreview`:超时不返回**(三次运行复现)。见 §2②。
- **视频 / 每帧变化的表面**:两次 `CapturePreview` 相差 891 473 / 891 588 px,**黑帧率 0.0** —— 本机未复现 #5574 的黑帧。
- **DRM:未测。** 受保护媒体表面需要有许可的 EME 流,离线拿不到,canvas 不是替身。这一行的留白是诚实的留白。
- 截图:`g2-capturepreview-00.png`(只有视口,没有宿主边框)、`g2-printwindow.png`、`g2-window-while-webview-hidden.png`(隐藏后的洞)。

### 门 3 — 完整鼠标 · **pass**

22 行逐条发、逐条问网页收到了什么:`move / left-down / left-up /` 真双击四连 `/ right / middle / x1 / x2 / wheel ×2 / horizontal-wheel /` LEAVE 三种写法 `/ move-outside`。

- **坐标对得上**:送 client `(511,242)`,座位原点 `(224,48)`,scale 2.0 → 期望 CSS `(143.5, 97.0)`,网页报 `(143, 97)`。这条决定"点哪儿网页就以为点哪儿":`BoundsMode = USE_RAW_PIXELS` + 视觉偏移 + `client − 座位原点` 三者一致。
- 右键 up 起 `contextmenu`;中键 / X1 / X2 起 `auxclick`;down-up-doubleclick-up 四连起 `dblclick`;水平滚轮起 `wheel`。
- `CursorChanged` 实测触发,`SystemCursorId` 报 32513(IBEAM)、32512(ARROW)、32649(HAND)、32646(SIZEALL)——光标形状必须由这个事件驱动,不能猜。
- 选择拖拽产生了选区。
- **`LEAVE` 见 §2⑥**,替代路径已实测。

### 门 4 — touch / pen / 多指 / OLE drag-drop · **有条件 pass**

- **本机 `SM_DIGITIZER = 0`、`SM_MAXIMUMTOUCHES = 0`:没有数字化仪。** 驱动产生的 `WM_POINTER` 真路径**无设备不可测**,按单子要求记"无设备",不记 pass 也不记 fail。
- 合成 `SendPointerInput` 全部被接受(`send_errors: []`),网页收到的 `pointerType` 是 `pen` / `touch`,并伴随 `touchstart/touchmove/touchend`。
- **多指的真实限制**:`ICoreWebView2PointerInfo` 每个接触点要一个独立 `pointerId`,探针只铸了一个,所以"两指同时"这格测的是管道不是并发。**W2 片① 必须按接触点建 PointerInfo**,这条记在案。
- **OLE drag-drop 通了**:探针实现了一个最小 `IDataObject`(CF_HDROP + CF_UNICODETEXT),经 `ICoreWebView2CompositionController3::DragEnter/DragOver/Drop` 送进去,网页收到 **`drop`**,`dataTransfer.types` 是 `[text/plain, text/uri-list, Files]`。
  途中有一条值得记的坑:`effect` 是**入参兼出参**,进去的值是"拖拽源允许哪些效果"。传 0(`DROPEFFECT_NONE`)时网页正确地拒绝,`drop` 退化成 `dragleave`——那是探针在拒绝自己的拖放,不是引擎不支持。

### 门 5 — 键盘 / 焦点 / 快捷键实际绑定矩阵 · **pass**

先有一个对照:**焦点还在宿主时**发 `Ctrl+Shift+N`,宿主 HWND 收到三条 keydown(`[17, 16, 78]` = Ctrl / Shift / N),一条不少。**焦点进网页后**,`GetFocus` 变成那个 `0×0` 的 `Chrome_WidgetWin_0`,宿主 HWND 从此一条按键消息都收不到——2026-08-13 那条"visual hosting 只搬渲染,键盘和 IME 仍走一个真 HWND"的发现在这里复验。下面这张表量的就是"聋掉之后还能捞回多少"。

**30 行和弦矩阵,每行一次注入、一次记账:**

| 结果 | 行数 |
|---|---|
| `AcceleratorKeyPressed` 触发 | **30 / 30** |
| `SetHandled(true)` 生效(网页看不到和弦键) | **30 / 30** |
| 宿主 HWND 直接收到按键消息 | **0 / 30** |
| 网页另外看到的键 | 只有裸 `Control` / `Shift` / `Alt` |

**Tab 合同实测成立**:Tab 与 Shift+Tab **不**触发加速键回调(`accelerator_events: 0`),在页内走控件(`focusin`);连按到边界后 **`MoveFocusRequested` 触发,`reason = 1` = `COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT`**,宿主 `SetHandled(true)` 把焦点收回。这正是方案 §2.5 写的那条合同。

**Esc** 触发加速键回调(keydown + keyup 两条),所以"esc 归宿主管"在预览块里同样成立。

**DPI**:窗口从 192 DPI 屏搬到 144 DPI 屏,`WM_DPICHANGED = 144`,网页 `devicePixelRatio` 自动 `2.0 → 1.5`、视口 `573×389 → 767×528`,宿主一行 DPI 代码没写。

**IME 见 §2⑧(blocked,须人工测);键盘布局见 §2⑦。**

### 门 6 — UIA · **pass**

- `ICoreWebView2CompositionController2::AutomationProvider` 可取(`available: true`)。
- 从宿主 HWND 用 `IUIAutomation` 客户端往下走(Narrator 走的同一条路),树里出现:
  `BtSpikeW0Host (Win32)` → `Chrome_WidgetWin_1 (Win32) "W0 probe page"` → `BrowserRootView (**Chrome**) "W0 probe page - Web content"`。
  框架列表 `["Win32", "Chrome"]`,**网页标题出现在节点名里**——网页树确实挂进了窗口的 UIA 树。
- 走树有节点预算(600)与深度上限(8),所以这格证明的是**可达**,不是完整目录。Chromium 的无障碍树是懒建的,探针走两次、中间等 2 秒,第一次"叫醒",第二次读数。

### 门 7 — 生命周期 · **pass**

| 项 | 读数 |
|---|---|
| 进程级共享 environment | 第二次 `environment()` **没有新建**;两个 controller 报**同一个 browser pid** |
| 杀 renderer | `ProcessFailed` 到达,`kind = 1`(RENDER_PROCESS_EXITED)、`reason = 3`、`exit_code = 57005`;**延迟两次运行分别是 4 145 ms 与 167 ms**——量级不稳定,产品不能拿它当超时基准;随后 `Reload` 成功回到原页面 |
| 杀 browser | `ProcessFailed` 每次都到;`BrowserProcessExited` **一次 280 ms 到达、一次 25 秒未到** → 见 §2③ |
| 正常关闭 | 每次运行的 `shutdown` 都是 `browser_process_exited: true`;唯一的 false 出现在杀过 browser 的那一组 |
| Runtime 缺失 | 子进程里把 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` 指向不存在的目录:API 报 `0x80070002`,**注册表仍谎报 151.0.4129.93**,`CreateCoreWebView2EnvironmentWithOptions` **同步失败,耗时 0 ms** |
| Runtime 自更新 | 处理器已挂,本次运行未触发。**无法按需触发**——`NewBrowserVersionAvailable` 要 Evergreen 在进程运行期间装新版,强行制造等于改动这台机器 |
| GPU reset | **未测**:TDR 需要驱动级设备移除,用户态探针没有受支持的触发方式 |

"注册表会撒谎,API 不会"这条 2026-08-13 的结论,在本次实测中原样复现。

### 门 8 — 后台账 · **pass**

同一个 6 秒采样窗口,同一张页面:

| | CPU | 私有内存 | `requestAnimationFrame` 帧数 |
|---|---|---|---|
| 可见 | **2 843 ms** | 211.3 MB | **718** |
| 隐藏 | **16 ms** | 205.5 MB | **0** |

- **计时器节流是彻底的**:隐藏后 rAF 一帧不发,CPU 掉到 1/177。
- **内存几乎不还**:211.3 → 205.5 MB,只降 2.7%。一个隐藏的预览块**省的是 CPU,不是内存**;若干个隐藏预览的内存账要按"每个都常驻"来算。
- **site isolation**:一个 origin 时 renderer **1** 个;`https://example.com` 作为第二 origin 加载成功后 renderer **2** 个。

### 门 9 — 安全门 · **pass**

§3 的规则先作为纯函数红测(`cargo test`,18 行拒绝矩阵 + 绿行 + "钉不是授权"),再拿实机打。

**实机 9 行,三条路径,全部未抵达目标:**

| 路径 | 目标 | 拦在哪 |
|---|---|---|
| 直连 | `javascript:location='https://evil.example/'` | 引擎**执行了脚本**,`NavigationStarting` 收到的是脚本产生的 `https://evil.example/` |
| 直连 | `file:///C:/Windows/win.ini` | `NavigationStarting` 取消 |
| 直连 | `edge://settings` | `NavigationStarting` 取消 |
| 直连 | `view-source:https://example.com/` | 引擎**改写**成 `https://example.com/`,`view-source:` 语义未生效 |
| 直连 | `data:text/html,<h1>injected` | `NavigationStarting` 取消 |
| 302 重定向 | `edge://settings` | 引擎拒绝跨 scheme 重定向(`RedirectFailed`),目标**从未进入 `NavigationStarting`**,也从未提交 |
| 302 重定向 | `file:///C:/Windows/win.ini` | 同上 |
| 页面脚本 `location.href` | `file:///C:/Windows/win.ini` | 引擎在 `NavigationStarting` 之前就拒绝,页面留在原地 |
| 页面脚本 `location.href` | `edge://settings` | 同上 |

**`javascript:` 这一行是本门最重要的读数,而且不能读成"拦住了":**
`Navigate("javascript:…")` 不报错、也不会以 `javascript:` 的身份到达 `NavigationStarting`——**引擎直接对当前文档执行了这段脚本**,只有脚本随后发起的那次导航才走 allowlist。所以:

> `NavigationStarting` **不是** `javascript:` 的兜底。`policy::address_bar` 里的那条拒绝是唯一的门,凡是能走到 `Navigate()` 的入口(地址输入、钉、命令面板、切换器)都必须先过它。

**受控 file 入口**:由 canonicalize 后的路径铸出的那一个 `file:///…` 加载成功;从那张页面再去 `file:///C:/Windows/win.ini`,`NavigationStarting` 以 `refusal: FileScheme` 取消,`Source` 停在原处。UNC 拒绝与 `..` 逃逸在纯测里覆盖。

**其余两扇门**:`/download` 触发 `DownloadStarting` 并被取消;`window.open` 被 `NewWindowRequested` 接管(见 §2 末尾的提醒)。

### 门 10 — 恢复门 · **pass**

§4 状态机先作为纯逻辑红测(9 条),再**用本次运行真实产生的事件流驱动同一台状态机**——所以模型与引擎的分歧会在这里暴露,而不是在 mock 里。

| 条款 | 读数 |
|---|---|
| 事件全装完才首航 | controller 到手后状态是 `ControllerPending`,`on_events_installed` 之后才产出 `Navigate` |
| 失败页不覆盖可恢复 URL | 先成功加载 `http://localhost:9335/`,再去 `http://127.0.0.1:9/never` 失败;`recoverable_url` 仍是前者 |
| generation 淘汰晚到 callback | 杀 browser → generation 1→2;**晚到的 controller → `CloseOrphanController`**、**晚到的 `NavigationCompleted` → `Ignore`** |
| browser crash 重建 | 新 generation 建新 controller,导航到最后一个成功 URL,`success: true` |
| renderer crash 只 Reload | 见门 7:`Reload` 后回到原页面,generation 不变 |
| UDF 清理等 `BrowserProcessExited` | 状态机给出 `AwaitBrowserExitBeforeCleanup`;**实机上这个事件在崩溃路径时来时不来** → §2③ |

---

## 4. W2 六片的开工判定

| 片 | 判定 | 依据与前置 |
|---|---|---|
| ① platform host / input | **GO** | 门 1/3/4/5 全过。开工前把 §2④(挂载顺序)、§2⑥(LEAVE 替代路径)、门 4 的"每接触点一个 PointerInfo"写进片单;验收必须含一份**人工 IME 记录**(§2⑧) |
| ② navigation / security / UDF | **GO** | 门 9/7 全过。§3 规则原样可用,但必须写上 §3 门 9 那条:`javascript:` 只有地址栏这一道门,`NavigationStarting` 不兜底;UDF 收尾按 §2③ 改成"两个事件之一 + 超时" |
| ③ preview / session model | **GO** | 门 10 全过,§4 状态机可原样落地(含 generation token) |
| ④ chrome commands | **GO** | 门 5 给了完整键盘合同;缩放、查找、DevTools 都是 WebView2 现成 API,未在本单取证范围内 |
| ⑤ file / PDF / watch | **GO** | 门 9 的受控 file 入口实测通过(铸造 URL 放行、其他 file 拒绝) |
| ⑥ F2 capture(独立性能片) | **GO,但范围收窄** | 门 2 给了硬数:`CapturePreview` 中位 61 ms 且**隐藏时不可用**。这一片只能做"可见时低频抓 + 缓存最后成功帧";**活缩略图与逐帧远程投影仍然没有受支持路径**,维持方案 §1 的推论不变 |

**没有一扇门要求方案推翻重来。** 需要的是 §2 那九条落到片单里——三处修订(①②③)是文字改动,六条新增约束是实现规则。

---

## 5. 附:探针本身

| 东西 | 位置 |
|---|---|
| 十扇门主程序 | `spikes/webview2-w0/src/main.rs` |
| DirectComposition 树(root / web / gpu) | `src/dcomp.rs` |
| wgpu 预乘 overlay | `src/gfx.rs` |
| WebView2 托管与全部事件 | `src/host.rs` |
| 鼠标 / 指针转发 | `src/host.rs`(`MouseEvent`、`send_pointer`) |
| 最小 `IDataObject`(拖放载荷) | `src/dataobject.rs` |
| WGC / PrintWindow / 扫描线 / 采样 | `src/capture.rs` |
| §3 URL 规则 + 红测 | `src/policy.rs` |
| §4 恢复状态机 + 红测 | `src/machine.rs` |
| `BINDINGS` 30 行转录 + 红测 | `src/bindings.rs` |
| 环回服务器(302 / 下载 / 第二 origin) | `src/server.rs` |
| 取证页 | `assets/probe.html`、`assets/video.html` |
| 跑法 | `run-gates.ps1` |

探针是独立 workspace,`unsafe_code = "allow"`(DComp / WebView2 / UIA / WGC 全是裸 COM),**不进任何产品 crate 的依赖图**。`cargo fmt --check` 干净,`cargo clippy` 干净,`cargo test` 22 条全绿。

跑法上有三个坑值得留给下一个人,`run-gates.ps1` 里都写了原因:门 7 与门 10 故意杀浏览器进程,所以**每组门单开一个进程**;探针的 stdout 会被 WebView2 子进程继承,所以**等它结束不能用管道、`Start-Process -Wait` 或 `WaitForExit`**,要等日志里那行 `{"event":"done"}`;两组之间要等上一棵浏览器进程树放开 UDF,否则新 environment 的 controller 回调**永远不来**。
