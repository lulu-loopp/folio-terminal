# W0′ 复验:`plan.md` §2 十扇门的第二次实机取证

日期 2026-08-22 · 分支 `worktree-agent-a907c55e28e525683`(基线 `7077181`)· 探针 `spikes/webview2-w0/`
本机 WebView2 Runtime **151.0.4129.101**(`GetAvailableCoreWebView2BrowserVersionString`;注册表 `HKLM\WOW6432Node` 报同一版本)
上一次取证 `w0-evidence.md`(2026-08-20)跑在 **151.0.4129.93** 上 —— **Evergreen 在两次取证之间把这台机器升了级**,见 §5。

**这是复验单,不是产品单。** 没有改动任何产品 crate、`design/ui-mockup.html`,也没有碰 `dist\`。改动只在 `spikes/webview2-w0/`。

复跑:

```
cd spikes/webview2-w0
cargo build --release
.\run-gates.ps1 -Shots <目录>          # 十扇门,八个进程,末尾打印判定表
.\run-gates.ps1 -Shots <目录> -Groups @('5')   # 单扇门
cargo test                              # 32 条纯逻辑合同测试
```

四次记录在案的运行:

| 运行 | 内容 | 附件 |
|---|---|---|
| run 1 | **未改一行**的已提交探针,十扇门全跑一遍 —— 复验本身 | `attachments/run1-baseline-gates.jsonl` |
| run 2 | 补洞后的探针,十扇门全跑一遍 | `attachments/run2-extended-gates.jsonl` |
| run 5 | 门 4 / 门 5(修完聚焦次序与接触点 id 后) | `attachments/run5-gates-4-and-5.jsonl` |
| run 6 | 门 5(把合成的 autorepeat 改成有间隔的按键之后) | `attachments/run6-gate-5-autorepeat.jsonl` |

每条读数都是 jsonl 里的一行,下文引用的字段名就是那一行的字段名。

**读数的适用范围仍是一台机器、一个 Runtime 版本。** 时间类的数是这台机器这一天的量级;结构类的结论(谁盖谁、哪个事件到不到、哪条 URL 拦不拦得住、哪个键归谁)才是可以搬走的。

---

## 0. 总结论

**十扇门 8 扇 pass、1 扇 pass-with-caveat 之外还有 1 扇 fail。**

- **复验成功**:2026-08-20 记下的结构类结论**逐条复现**,一条没翻 —— 30/30 和弦被宿主夺回、隐藏时 `CapturePreview` 不返回、`SendMouseInput(LEAVE)` 三种写法全被拒、`AddVisual` 次序、注册表撒谎而 API 不撒谎、隐藏座位是纯黑洞、`BrowserProcessExited` 在崩溃路径上时来时不来。
- **门 9 判 fail**,而且不是安全洞:§3 的 allowlist 把 `about:` 无条件拒在 `NavigationStarting`,而实测**引擎自己会为 `about:blank` 触发 `NavigationStarting`**,§4 又把 `about:blank` 写进正常流程。照 §3 字面实现,产品会取消自己发起的导航。**W2 片② 必须给"宿主自己铸的 about:blank"开一个口子。**
- **门 10 判 pass-with-caveat**:状态机本身过,但过的是**修订后**的状态机 —— 方案 §4 缺两条,一条是 UDF 收尾的超时门(`w0-evidence.md` §2③ 记了却没写进模型),另一条这次才发现:`ProcessFailed` 在 `Closing` 态下会让原模型**把用户已经关掉的 pane 重建出来**。两条都已写成合同测试。
- **补上了 5 个洞**:门 5 的"网页自己的键"整半张表(方案与上一版取证都只测了 Folio 拿走的那半)、门 5 的剪贴板真往返、门 2 的 EME 管道、门 4 的每接触点独立 `PointerInfo`、门 7 的自更新**恢复动作**(触发器仍不可强制,但要做的事这次真做了一遍)。
- **探针自己被抓到三处会说谎的地方**,全部已修 —— 见 §6。其中一处让上一版取证的门 7 结论**是运气**:它杀的是别的 controller 的 renderer,却在等自己的 `ProcessFailed`,而判词无论如何都印"到了"。

---

## 1. 十扇门结论表

| # | 门 | 判定 | 机器证据(run 2 除非另注) |
|---|---|---|---|
| 1 | visual 层叠 / clip / opacity / 圆角 / 动画原子性 | **pass** | 浮窗压座位 42 120 px(理论 41 400);半透明采样 `[131,97,160]` 落在浮窗 `[47,85,232]` 与网页 `[234,114,71]` 之间;圆角后角落成洞、网页保留 526 692/526 729 px;clip 到一半 526 729→246 343;10 帧动画 `frames_with_a_gap = 0`;座位内洞占比 0.012%;`child-windows` 覆盖座位数 **0** |
| 2 | 像素矩阵 | **pass-with-caveat** | `CapturePreview` 中位 **52.7 ms**(47.4–54.1);整窗 WGC 中位 **13.7 ms**;**隐藏时 `CapturePreview` 超时不返回**(第 4、5 次独立复现);视频页两帧差 891 461/891 588 px、黑帧率 0.0;`PrintWindow` 拿到网页 520 795 px。**新增**:EME 三家 key system 全部可取(§3.2)。**caveat = 受保护表面仍未测**,离线拿不到有许可的流 |
| 3 | 完整鼠标 | **pass** | 除 `LEAVE` 外全部送达;坐标严丝合缝(client `(511,242)` − 座位原点 `(224,48)` ÷ 2.0 → 网页报 `(143,97)`);`CursorChanged` 报 32513/32512/32649;`LEAVE` 三种写法仍全被 `E_INVALIDARG` 拒,`move-outside-bounds` 替代路径仍可用 |
| 4 | touch / pen / 多指 / OLE drop | **pass-with-caveat** | 合成 pen/touch 的 `pointerType` 正确;OLE `drop` 到达网页,`[text/plain, text/uri-list, Files]`。**补洞**:两个接触点各带自己的 `pointerId`,网页看到 **id 5 与 6**、`TouchList` 最大 **2**(run 5)。**caveat = 驱动路径仍未走通**:本机 `SM_DIGITIZER=0`,`InitializeTouchInjection` 成功而 `InjectTouchInput` 每一帧都 `E_INVALIDARG`(单接触点与双接触点都一样) |
| 5 | 键盘 / 焦点 / 快捷键实际绑定矩阵 | **pass-with-caveat** | 30/30 和弦复现(§3.1);**新增 14 行"网页自己的键"矩阵(§2)**;剪贴板真往返 Ctrl+A/C/V 全绿(run 5、run 6);Tab 边界 `MoveFocusRequested reason=1`。**caveat 三条**:IME 候选窗仍须人工测;跨 DPI 今天**不可复验**(本机只剩 1 台显示器);**autorepeat 无法用 SendInput 合成**(§2.3) |
| 6 | UIA | **pass** | `AutomationProvider` 可取;从宿主 HWND 走 UIA 树到 `Chrome_WidgetWin_1 "W0 probe page"`,`frameworks = ["Win32","Chrome"]`,网页标题出现在节点名里 |
| 7 | 生命周期 | **pass-with-caveat** | 一个 environment 两个 controller、同一 browser pid;杀 renderer → `ProcessFailed` **9 ms** 到达、`Reload` 复原(**修掉挑错进程的 bug 之后**,§6.1);杀 browser → 两个事件都到、366 ms;Runtime 缺失同步 0 ms 失败于 `0x80070002` 而注册表仍谎报;**新增:自更新的恢复动作真跑了一遍** —— browser pid `116016 → 124360`、HWND 不变、1 344 ms 回到最后一个成功 URL。**caveat = 触发器仍不可强制**(`NewBrowserVersionAvailable`)、**GPU reset 仍未测** |
| 8 | 后台账 | **pass** | 同一 6 秒窗口:可见 CPU **1 811 ms**、rAF **718 帧**;隐藏 CPU **0 ms**、rAF **0 帧**;私有内存 219.2 → 207.0 MB(只降 5.6%);renderer 数 1 origin → **1**,2 origin → **2**。**本次两个 origin 全在环回**(`localhost` vs `127.0.0.1`),不再访问外网 |
| 9 | 安全门 | **fail** | 拒绝矩阵本身全绿:9 行实机三条路径全部未抵达目标,18 行纯测红矩阵 + 非空洞见证全过,受控 file 入口只放行铸造的那一个。**洞在 `about:`**:`NavigationStarting` **确实为 `about:blank` 触发**(`navigation_starting_fired: true`,`uri: "about:blank"`),而 `policy::navigation_starting("about:blank")` 返回 `Err(BrowserInternalScheme)` —— 照 §3 字面装上强制器,产品会取消自己的导航。见 §4.1 |
| 10 | 恢复门 | **pass-with-caveat** | 事件装齐前不首航;失败页不覆盖可恢复 URL;杀 browser → generation 1→2,晚到 controller 判 `CloseOrphanController`、晚到 `NavigationCompleted` 判 `Ignore`;重建回到最后一个成功 URL。**caveat = 过的是修订后的模型**:方案 §4 缺 UDF 收尾超时门与 `Closing` 态下的 `ProcessFailed` 消歧,两条都在 §4.2 |

---

## 2. 第 5 扇门:逐键矩阵

### 2.1 Folio 拿走的那半(复验,30/30 与 2026-08-20 逐字一致)

30 行 `BINDINGS` 和弦,每行一次注入一次记账(run 1 与 run 2 结果相同):

| 结果 | 行数 |
|---|---|
| `AcceleratorKeyPressed` 触发 | **30 / 30** |
| `SetHandled(true)` 生效(网页看不到和弦键本身) | **30 / 30** |
| 宿主 HWND 直接收到按键消息 | **0 / 30** |
| 网页另外看到的键 | 只有裸 `Control` / `Shift` / `Alt` |

`Ctrl+Tab`、`Ctrl+Shift+Tab`、`Alt+Shift+-`、`Alt+Shift+=`、`Ctrl+,`、`F3`、`Shift+F3` 全在内。

### 2.2 网页自己的那半(**本片新增**,run 5)

这半张表方案没写、上一版取证也没测。它量的是**反过来的问题**:Folio 不要的键,网页拿得到吗。`in BINDINGS = no` 而 `who got it ≠ page` 的行,就是预览块提供不了的能力。

| 和弦 | 在 BINDINGS | 加速键回调触发 | 回调 kind | 宿主 `SetHandled` | 网页收到键本身 | **谁收到了** |
|---|---|---|---|---|---|---|
| Ctrl+C | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+V | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+X | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+A | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+Z | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+Y | no | yes | `[0, 1]` | no | yes | **网页** |
| **Ctrl+Shift+Z**(对照行,在 BINDINGS) | yes | yes | `[0, 1]` | **yes** | no | **宿主** |
| Alt(裸) | no | yes | `[2, 3, 3]` | no | yes | **网页** |
| F5 | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+R | no | yes | `[0, 1]` | no | yes | **网页**(引擎同时刷新了页面) |
| F12 | no | yes | `[0, 1]` | no | yes | **网页** |
| Ctrl+P | no | yes | `[0, 1]` | no | yes | **网页** |
| **Alt+Left** | no | yes | `[2, 3]` | no | **no** | **两边都没有 —— 引擎自己吃了** |
| **Alt+Right** | no | yes | `[2, 3]` | no | **no** | **两边都没有 —— 引擎自己吃了** |

`kind`:0 = KEY_DOWN,1 = KEY_UP,2 = SYSTEM_KEY_DOWN,3 = SYSTEM_KEY_UP。

三条读法:

1. **keyup 的答案在 `kind` 列里**:每一行都有 `1`(或 `3`),即 `AcceleratorKeyPressed` **按下和松开都触发**。宿主要握住一个和弦再放开是做得到的。
2. **裸 Alt 不会掉进 Win32 菜单循环**:它以 `SYSTEM_KEY_DOWN` 走回调、以裸 `Alt` 到网页,没有一条按键消息落到宿主 HWND。
3. **`Alt+Left` / `Alt+Right` 是引擎的后退/前进**,回调看得见、`SetHandled` 没设、网页什么也没收到 —— 也就是说**它们在预览块里已经是后退/前进了**,W2 片④ 要么接受这个既成事实,要么在回调里 `SetHandled(true)` 抢回来。这是一条产品裁决,不是实现细节。

### 2.3 剪贴板真往返(run 5 / run 6)

键盘事件到没到,和事情有没有发生,是两件事。所以这一格量的是剪贴板本身(测完把用户原有的内容放回去):

| 步骤 | 读数 |
|---|---|
| 宿主写入网页输入框 | `"w0p clipboard round trip"` |
| `Ctrl+A` 之后网页报选中长度 | **24**(= 全长),`select_all_worked: true` |
| `Ctrl+C` 之后宿主读剪贴板 | **`"w0p clipboard round trip"`**,`copy_worked: true` |
| 宿主写剪贴板 → `Ctrl+V` → 网页报字段值 | **`"w0p pasted by the host"`**,`paste_worked: true` |

### 2.4 autorepeat:**用 SendInput 合成不出来**

| 读数 | 值 |
|---|---|
| 单次裸按键 `K`(对照) | `SendInput` 收 2 条,**加速键回调 0 次**,网页收到 1 条键事件 |
| 连按 `K` 六次 + 松开(间隔 35 ms) | `SendInput` 收 7 条,**加速键回调 0 次** |
| 连按 `F5` 六次 + 松开(间隔 35 ms) | `SendInput` 收 7 条,**加速键回调 1 次,kind = `[1]`(只有 KEY_UP)**,`repeat` 标志 `false` |

两条结论,一条是引擎的、一条是方法的:

- **裸可打印键根本不进 `AcceleratorKeyPressed`。** 对照行证明了这一点:网页收到了 `K`,回调一次没响。`AcceleratorKeyPressed` 只管"可能是加速键"的键,一个不带修饰键的字母不是。→ **预览块获得焦点时,Folio 绑不了裸字母键。** `BINDINGS` 今天没有这样一行(只有 `F3` / `Shift+F3`),所以不亏;但将来加一行会**静默地永不触发**,这条要写进快捷键表的注意事项。
- **`SendInput` 造不出 autorepeat。** 对一个已经按下的键再发一次 KEYDOWN 不是一次状态变化,系统不产生重复;真正的 autorepeat 来自键盘驱动的重复计时器。所以这一行**测不了**,和 IME 候选窗一样属于人工项。探针已经把 `PhysicalKeyStatus.RepeatCount > 1` 接到 `Accelerator.repeat` 上,真人按住一个 `F3` 就能读出答案 —— 一行人工记录的事。

---

## 3. 本片补上的洞

### 3.1 门 5 的另外半张表 —— 见 §2.2 / §2.3 / §2.4

方案 §2.5 与 `w0-evidence.md` 都只问了"宿主能捞回多少",从没问过"网页拿得到多少"。补完之后的答案很干净:**除了 `Alt+Left`/`Alt+Right` 被引擎吃掉,网页需要的键一个不少**,复制粘贴是真的能复制粘贴。

### 3.2 门 2 的 DRM:表面仍不可测,**管道已经可测而且是通的**

上一版把整格记成"not tested",理由是"受保护表面需要有许可的 EME 流"。这个理由对**表面**成立,但从来不构成不问**管道**的理由 —— 而管道离线就能问,并且决定预览 pane 能不能播一段付费视频。

| key system | `requestMediaKeySystemAccess` | `createMediaKeys` | `video.setMediaKeys` |
|---|---|---|---|
| `org.w3.clearkey` | **ok** | **ok** | **ok** |
| `com.widevine.alpha` | **ok** | **ok** | **ok** |
| `com.microsoft.playready.recommendation` | **ok** | **ok** | **ok** |

`isSecureContext: true`(`http://localhost` 是安全上下文)、`hasEme: true`、`hasMediaCapabilities: true`。
**仍未测的是表面**:有许可的流合成出来到底是画面还是黑帧(#5574 的那个问题),离线没有替身。

### 3.3 门 4 的每接触点一个 `PointerInfo`

上一版把这条记成"探针的局限,不是引擎的"。现在是读数:两个接触点在**同一 frame** 里各带自己的 `pointerId` 送进去,网页报 `pointerId = [5, 6]`、`TouchList` 最大 **2**、`send_errors: []`。→ **W2 片① 按接触点建 `PointerInfo` 是做得通的**,捏合手势有路。

真触屏驱动路径仍未走通:`InitializeTouchInjection(2, TOUCH_FEEDBACK_NONE)` 成功,但 `InjectTouchInput` 对 down / update / up 三帧一律 `E_INVALIDARG`,**单接触点也一样**(所以不是多指的问题)。最可能的解释是这台机器 `SM_DIGITIZER = 0`,根本没有可注入的触摸输入栈 —— 这是推断,不是证明,如实标注。

### 3.4 门 7 的自更新:触发器仍不可强制,**恢复动作跑通了**

`NewBrowserVersionAvailable` 要 Evergreen 在进程运行期间装新版,强行制造等于改动这台机器。但那个事件真到了之后**要做什么**是完全可以做的,而且正是容易做错的部分。探针把整套动作跑了一遍:

```
关掉所有 controller → 等 browser 走(三扇门中任一 + 超时)
→ 丢掉进程级缓存的 environment → 在同一个 HWND 上新建 environment 与 controller
→ 导航回最后一个成功 URL
```

| 读数 | 值 |
|---|---|
| `recovery_exercised` | **true** |
| browser pid before → after | **116016 → 124360**(真换了一棵进程树) |
| host HWND before → after | `0x63089c` → `0x63089c`(**窗口没重启**) |
| `reloaded_the_last_good_url` | **true**,落在 `http://localhost:9121/` |
| 耗时 | **1 344 ms** |

**里面有一条不写下来会踩的**:新 environment 必须在**丢掉进程级缓存**之后才建 —— 在旧 environment 上建新 controller,就是在旧版浏览器上建,更新对谁都没生效。而且旧 browser 还占着 UDF 时建新 environment **不会报错**,它只是**永远不回调**。

### 3.5 门 9 的 `about:blank` —— 见 §4.1

### 3.6 门 10 的 UDF 收尾三扇门与版本变更 —— 见 §4.2

---

## 4. fail 与 caveat 的展开

### 4.1 【fail】门 9:§3 的 `about:` 条款会取消产品自己的导航

**洞在哪。** `policy::navigation_starting` 把 `about:` 归进 `BrowserInternalScheme` 一律拒绝。实机测下来:

```json
{"policy_says": "Err(BrowserInternalScheme)",
 "navigation_starting_fired": true,
 "navigation_starting": [{"uri": "about:blank", "user_initiated": true, "cancelled": false}],
 "source_after": "about:blank"}
```

`NavigationStarting` **确实**为 `about:blank` 触发。而方案 §4 把 `about:blank` 写进正常流程("失败页/about:blank/取消不覆盖"可恢复 URL),§0 的 popup 分流也会碰到 `about:blank` 的新窗。照 §3 字面把强制器装上,**产品会取消自己发起的导航**,座位停在旧页面上。

**W2 哪一片必须绕、怎么绕。** **片②(navigation / security / UDF)**。一行的事,但必须是显式的一行:

- `navigation_starting` 多收一个"宿主自己铸的目标"参数(和受控 file 入口的 `sanctioned_file` 完全同构):宿主铸的 `about:blank` 放行,页面脚本或地址栏来的 `about:` 照拒。
- **地址栏那道门不动** —— `address_bar("about:blank")` 继续 `Refuse`,人不能手打 `about:` 进来。
- 合同测试已经把今天的行为钉住了(`about_blank_is_refused_by_the_same_door_paragraph_four_navigates_through`),改行为的那天必须改这条测试,也就是必须有人**决定**改。

### 4.2 【caveat】门 10:过的是修订后的 §4,方案还缺两条

**缺的第一条 —— UDF 收尾没有超时门。** `w0-evidence.md` §2③ 已经记下"崩溃路径上 `BrowserProcessExited` 不可依赖",但**模型没改**,`AwaitBrowserExitBeforeCleanup` 依旧是个无限等待。本片八组 shutdown 的实测:

| 结果 | 组数 | 耗时 |
|---|---|---|
| `BrowserProcessExited` 到达 | 6 | 271–390 ms |
| `BrowserProcessExited` 到达(慢) | 1 | **6 588 ms** |
| **两个事件都没到,等满超时** | **1** | 10 004 ms |

而且要更正 §2③ 的一个细节:**正常关闭路径上 `ProcessFailed` 不会来**(八组 `process_failed_browser_exited` 全是 `false`)。所以"两事件之一"里的第二扇门在优雅关闭时不开,**超时是唯一兜底**。

**缺的第二条 —— `Closing` 态下的 `ProcessFailed` 会复活已关的 pane。** 这是本片新发现的:同一个 `ProcessFailed` 在活着的 pane 上意思是"重建",在正在关闭的 pane 上意思是"文件夹归你了",只有状态能分开这两件事。原模型两种都走重建分支 —— 用户关掉的 pane 会自己回来。

两条都已写成合同测试(§5),并在 gate 10 的实机运行里驱动过。**W2 片③(preview / session model)落地时直接搬。**

### 4.3 【caveat】门 2 / 门 4 / 门 5 / 门 7 的"设备或许可不在"

| 项 | 状态 | 为什么 |
|---|---|---|
| 受保护媒体表面 | 未测 | 需要有许可的 EME 流;离线没有替身。**管道已证可用**(§3.2) |
| 真触屏驱动路径 | 未测 | `SM_DIGITIZER = 0`;`InjectTouchInput` 一律 `E_INVALIDARG`(单指亦然) |
| IME 候选窗跟不跟 pane | **blocked,须人工测** | 焦点进网页后 `ImmGetCompositionWindow` 返回 false、`ptCurrentPos` 恒 `(0,0)`;没有正在进行的合成时引擎不经 IMM 暴露候选窗位置。**与 2026-08-20 结论一致** |
| 跨 DPI(192 → 144) | **今天不可复验** | 本机 `monitors: 1`;2026-08-20 那次有两台。上一版的 `devicePixelRatio 2.0 → 1.5`、视口 `573×389 → 767×528` **原样保留为 08-20 的读数**,本片不重复主张 |
| autorepeat | 测不了(方法所限) | `SendInput` 对已按下的键再发 KEYDOWN 不产生重复;须人工按住一个键。见 §2.4 |
| Runtime 自更新触发器 | 未强制 | 见 §3.4;**恢复动作已跑通** |
| GPU reset / TDR | 未测 | 需驱动级设备移除,用户态没有受支持的触发方式 |

---

## 5. 与 2026-08-13 spike / 2026-08-20 W0′ 的差异

### 5.1 直接复用、没有重做的

`spikes/webview2-w0/` 整套探针(DComp 树、wgpu 预乘 overlay、WebView2 托管与全部事件、WGC/PrintWindow/扫描线/采样、最小 `IDataObject`、环回服务器、`BINDINGS` 转录、`run-gates.ps1` 的分组与三个坑)全部原样复用。2026-08-13 spike 的地基结论(DComp visual 同窗叠放、透明洞、基础鼠标、焦点、IME、DPI、Runtime 探测)也没有重做,只在门 1 的基线复验里再确认了一次。

### 5.2 复现了、逐条对得上的

`alpha-modes` 五项与 `PreMultiplied`;`child-windows` 1 个 `Chrome_WidgetWin_0` / 覆盖座位 0;座位洞占比 0.012%;浮窗压座位 42 120 px;半透明采样 `[131,97,160]`;圆角保留 526 692/526 729;10 帧无撕裂;隐藏时 `CapturePreview` 不返回;隐藏后座位 890 439 px 全洞、网页 0 px;`LEAVE` 三写法全拒 + `move-outside` 可用;坐标 `(143,97)`;OLE `drop` 三种格式;30/30 和弦;Tab 边界 `reason = 1`;UIA 走到 `Chrome` 框架;一 environment 两 controller 同 pid;注册表撒谎而 API 不撒谎;隐藏 rAF 0 帧;1 origin → 1 renderer、2 origin → 2 renderer;门 9 九行三路径全拦;门 10 generation 淘汰晚到 callback。

### 5.3 数字变了的(都是时间/资源类,不动结论)

| 项 | 2026-08-20 | 2026-08-22 |
|---|---|---|
| Runtime | 151.0.4129.**93** | 151.0.4129.**101** |
| `CapturePreview` 中位 | 61.1 ms | **52.7 ms** |
| 整窗 WGC 中位 | 15.5 ms | **13.7 ms** |
| 可见 6 秒 CPU | 2 843 ms | **1 811 ms** |
| 私有内存(可见 → 隐藏) | 211.3 → 205.5 MB | **219.2 → 207.0 MB** |
| 杀 renderer 后 `ProcessFailed` 延迟 | 4 145 ms / 167 ms | **9 ms**(挑对进程之后) |
| 杀 browser 后两事件 | 一次 280 ms / 一次 25 s 不到 | **366 ms,两事件都到** |

**Runtime 版本这一行值得单独说**:Evergreen 在两次取证之间(08-20 → 08-22)把这台机器从 .93 升到了 .101。也就是说 `NewBrowserVersionAvailable` 要等的那件事**真的会发生、而且两天一次**;不可强制的只是"在进程运行期间"这个条件。这让 §3.4 的恢复动作从"以防万一"变成"一定会用上"。

### 5.4 本片新增的(上一版没有的行)

门 2 `protected-media`;门 4 `two-contacts` / `injected-touch`;门 5 `page-owned-key` × 14 / `keyup-and-autorepeat` / `clipboard-round-trip`;门 7 `self-update-rebuild` / `renderer-choice` / `page-before-renderer-kill`;门 9 `about-blank`;门 10 `new-browser-version` 与 `udf-cleanup-ordering` 的三扇门;`shutdown` 记录里的 `process_failed_browser_exited` / `waited_ms` / `timed_out`。

---

## 6. 探针自己被抓到的三处

复验的价值有一半在这里:同一段代码跑第二遍,三个地方的读数和判词对不上。

### 6.1 门 7 的判词与它的测量无关

run 1 的日志里 `renderer-killed` 写着 `process_failed_fired: false`、`latency_ms: 20006`(等满超时)、`records: []`,而同一次运行的判词照印 **"a renderer kill produced ProcessFailed and reloaded"**。

原因:`procs::census` 里此时有**两个** renderer —— 第二个 controller 带来了自己的。探针取的是列表里第一个,然后去等**自己这个 host** 的 `ProcessFailed`,那是别人的事件。上一版取证之所以是 pass,是因为那次恰好排到了对的那个。

已修:先重新导航本 host 再取一次 census,并优先挑**第二个 controller 出现之前就存在**的 renderer(`renderer-choice` 这一行把候选与选择都记下来);判词改成由测量拼出来,renderer 那一格没读到就整扇门降成 `pass-with-caveat`。修完之后 `ProcessFailed` **9 ms** 到达。

### 6.2 `Probe::shutdown` 只在事件到达时删 UDF

原代码 `if exited { remove_dir_all(...) }` —— 正是 §2③ 说过不能照字面写的那句话,写在探针自己身上。后果:等不到事件的那次运行,一整个浏览器 profile 留在磁盘上。已改成"三扇门任一 + 有界等待,到时无论如何都删",并把 `waited_ms` / `timed_out` 记进 `shutdown` 行。

### 6.3 探针写完 `done` 之后不退出

run 1 之后 `bt-spike-webview2-w0.exe` **八个进程全部还在**(一组一个),各自都已写完 `done` 并调用了 `std::process::exit(0)`,却仍然驻留,占着自己的二进制文件(下一次 `cargo build` 因此 `Access is denied`)与自己的 UDF。`exit` 会跑 at-exit 链和每个模块的 `DLL_PROCESS_DETACH`,而 WebView2 loader 在一个已经不再泵消息的 apartment 线程上拆卸,就停在那里。

已改成 `TerminateProcess(GetCurrentProcess(), 0)`:该有序释放的东西(关 controller、等 browser、删 UDF)在 `Probe::shutdown` 里已经做完了。run 2 及之后每次跑完都是 **0 个残留进程、0 个残留 UDF**。

### 6.4 顺带清掉的一条纪律欠账

门 8 的第二 origin 原来打的是 `https://example.com` —— 一个真实的外网请求,写在一个自己的模块注释说"nothing here reaches the network"的探针里。已改成环回的 `http://127.0.0.1:port`(与 `localhost` 同一个 socket、对 Chromium 是**不同的 site**),renderer 计数读数不变:1 origin → 1,2 origin → 2。

---

## 7. 合同测试清单(第 9 / 10 扇门)

`cargo test` 32 条,全绿。第 9/10 扇门今天还没有产品代码,所以这些是**可执行的合同**,W2 落地时原样平移。

### 7.1 §3 URL 规则 → 平移到 **`bt-app`(片②)的 navigation policy 模块**

| 测试 | 红证(不满足规则会怎样) | 绿证 |
|---|---|---|
| `address_bar_red_matrix` | 18 行攻击串逐条必须 `Refuse`,变体全覆盖(`javascript:` / `data:` / `blob:` / `vbscript:` / `file:` / `view-source:` / `devtools:` / `edge:` / `chrome:` / `about:` / `ftp:` / `ws:` / `wss:` / `mailto:` / `tel:` / userinfo / 空串) | 18/18 `Refuse` 且 `Refusal` 变体逐条对得上 |
| **`the_red_matrix_is_not_vacuous`**(本片新增) | 先证明"只查 `javascript:` 前缀"这种偷懒写法会**放过 8 行里的 6 行**,再要求 `address_bar` 与 `navigation_starting` 一行不放 | 偷懒写法放过 ≥6 行;真规则 0 行放过 |
| **`obfuscated_scheme_spellings_never_become_a_navigation`**(本片新增) | 9 种混淆写法(大小写、前导 Tab/CR/LF、scheme 内嵌 Tab/换行、前导 NUL、`%6a` 编码、全大写)一律不得成为 `Navigate` | 9/9 落在 `Search` 或 `Refuse`,无一 `Navigate` |
| `address_bar_green_rows` | 好行不能被误杀:`localhost:3000` 不是 scheme、`example.com` 补 `http://`、自然语言进搜索 | 5/5 |
| `navigation_starting_is_the_same_rule_again` | 地址栏拦得住的,重定向与页面脚本也要拦得住 | 4/4 |
| `only_the_minted_file_url_passes` | 只有铸造的那一个 `file:` 过;大小写等价;`..` 逃逸不过 | 4/4 |
| `unc_paths_are_refused_at_the_mint` | `\\server\share` 与 `\\?\UNC\...` 在铸造处就拒;`#`/空格正确百分号编码 | 3/3 |
| `a_pin_gets_no_special_pass` | 钉里的字符串走同一道门 | 2/2 |
| `loopback_is_syntax_only` | 纯语法判定:RFC1918、本机名、`localhost.evil.com`、`128.0.0.1` 都不是环回 | 6 真 6 假 |
| `unspecified_host_is_rewritten_keeping_everything_else` | `0.0.0.0` → `127.0.0.1`,端口/路径/query/fragment 一字不动 | 3/3 |
| `switcher_identity_keeps_query_and_fragment` | 去重键含 query 与 fragment,默认端口归一 | 3/3 |
| **`about_blank_is_refused_by_the_same_door_paragraph_four_navigates_through`**(本片新增) | **钉住今天这个 fail**:`about:blank` 两道门都拒。改行为必须改这条测试 | 2/2(记录的是缺陷,不是正确性) |

实机红证在 `run2-extended-gates.jsonl` 的 `gate 9 live-matrix`(九行三路径)、`javascript-scheme`、`controlled-file-entry`、`downloads-and-popups`、`about-blank`。

### 7.2 §4 恢复状态机 → 平移到 **`bt-app`(片③)的 preview/session model**

| 测试 | 红证 | 绿证 |
|---|---|---|
| `nothing_navigates_before_every_handler_is_on` | controller 到手后仍是 `ControllerPending`,不产出 `Navigate` | `on_events_installed` 之后才 `Navigate` |
| `a_late_environment_callback_from_a_closed_pane_is_dropped` | 关掉的 pane 的 environment 回调必须 `Ignore` | ✓ |
| `a_late_controller_from_an_abandoned_generation_is_closed_not_adopted` | 晚到的 controller 必须 `CloseOrphanController`(收养 = 留一棵没人指的浏览器进程树) | ✓,且新 generation 不受影响 |
| `desired_url_is_last_write_wins_and_only_one_navigation_results` | 三次 request 只导航一次,导航到最后一次 | ✓ |
| `a_failed_page_does_not_overwrite_a_recoverable_url` | 失败页、`about:blank` 都不许覆盖 | ✓ |
| `a_browser_crash_comes_back_to_the_last_good_url` | 崩溃后重建回最后一个**成功**的 URL,不是最后一次 request | ✓ |
| `a_renderer_crash_only_reloads` | generation 不动、状态仍 `Ready` | ✓ |
| `a_navigation_completed_from_a_stale_generation_cannot_write_the_recoverable_url` | 旧 generation 的成功导航不许写会话 | ✓ |
| `closing_waits_for_the_browser_to_exit_before_the_udf_is_touched` | `close` → `AwaitBrowserExitBeforeCleanup` | ✓,随后 `CleanupUserDataFolder` |
| **`a_browser_that_never_says_it_exited_is_cleaned_up_when_the_wait_runs_out`**(本片新增) | **方案 §4 的字面写法过不了这一条**:两个事件都不来时必须靠超时清理 | `on_cleanup_deadline` → `CleanupUserDataFolder` |
| **`process_failed_is_the_second_door_to_cleanup`**(本片新增) | `ProcessFailed(BROWSER_PROCESS_EXITED)` 也开同一扇门 | ✓ |
| **`the_user_data_folder_is_cleaned_exactly_once`**(本片新增) | 三扇门任意组合只清一次 | 后续三次全 `Ignore` |
| **`a_dying_browser_does_not_resurrect_a_closed_pane`**(本片新增) | **原模型过不了**:`Closing` 态收到 `ProcessFailed` 会 `RebuildFromScratch` | 不重建、generation 不动、状态仍 `Closing` |
| **`a_new_runtime_version_rebuilds_the_seat_at_the_last_good_url`**(本片新增) | `NewBrowserVersionAvailable` → `RebuildForNewVersion`,新 generation,回到最后一个成功 URL | ✓ |
| **`a_controller_from_before_the_version_change_is_closed_not_adopted`**(本片新增) | 版本切换前发出的 controller 回调必须关掉 | `CloseOrphanController` |
| **`a_version_change_during_close_is_nobodys_business`**(本片新增) | 正在关的 pane 不因版本变更复活 | `Ignore` |

实机绿证在 `run2-extended-gates.jsonl` 的 `gate 10 events-before-first-flight` / `failed-page-does-not-overwrite` / `browser-crash` / `rebuild` / `udf-cleanup-ordering` / `new-browser-version`,以及 `gate 7 self-update-rebuild`(同一套动作跑在真引擎上)。

### 7.3 `BINDINGS` 转录的四条

`the_transcription_is_the_size_the_product_asserts` / `no_row_uses_ctrl_alt` / `every_chord_is_unique` / `claims_matches_the_table` —— 保证探针量的是今天的产品表(34 行 = 30 和弦 + 4 无键)。这四条**不平移**,它们是探针与产品之间的对账。

---

## 8. W2 六片判定(在 `w0-evidence.md` §4 之上的增量)

| 片 | 判定 | 本片改动了什么 |
|---|---|---|
| ① platform host / input | **GO** | 片单加两条:**每接触点一个 `PointerInfo` 已证可行**(不再是"记在案");**`Alt+Left`/`Alt+Right` 归引擎**,要不要抢回来是产品裁决。人工验收清单从"IME 一项"变成"IME + autorepeat 两项" |
| ② navigation / security / UDF | **GO,但带一条必须的口子** | **`about:blank` 必须走宿主铸造豁免**(§4.1),否则产品取消自己的导航。UDF 收尾按三扇门 + 超时,且**优雅关闭时 `ProcessFailed` 不来,超时是唯一兜底** |
| ③ preview / session model | **GO** | §4 状态机要**先补两条**再落地(§4.2),两条都已有测试 |
| ④ chrome commands | **GO** | 门 5 现在给了完整的双向合同:宿主拿得回 30 个和弦,网页拿得到复制粘贴撤销全套。`F12`/`Ctrl+L` 若要加进 `BINDINGS`,记得 `Ctrl+R`/`F5` 今天是网页的 |
| ⑤ file / PDF / watch | **GO** | 不变 |
| ⑥ F2 capture(独立性能片) | **GO,范围仍收窄** | 数字更新为 `CapturePreview` 中位 52.7 ms;**隐藏时不可截这一条第 4、5 次复现**,范围不变 |

---

## 9. 附件

| 文件 | 是什么 |
|---|---|
| `attachments/run1-baseline-gates.jsonl` | 未改一行的已提交探针,十扇门全跑 —— 复验本身,也是 §6 三处缺陷的原始证据 |
| `attachments/run2-extended-gates.jsonl` | 补洞后的探针,十扇门全跑 |
| `attachments/run5-gates-4-and-5.jsonl` | 门 4 / 门 5 定稿读数(逐键矩阵、剪贴板、两接触点、触摸注入) |
| `attachments/run6-gate-5-autorepeat.jsonl` | 门 5 的 autorepeat 用有间隔的按键再测一次 |
| `attachments/g1-03-translucent-panel.png` | 半透明浮窗底下网页正文透出来 |
| `attachments/g2-capturepreview-00.png` | `CapturePreview` 只有视口,没有宿主边框 |
| `attachments/g2-window-while-webview-hidden.png` | 隐藏后座位是纯黑洞 |
