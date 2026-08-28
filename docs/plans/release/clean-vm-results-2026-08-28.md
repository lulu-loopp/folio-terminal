# 门 5 干净机冒烟:2026-08-28 这一轮的结果

`plan.md` 门 5 的原话是「Win10 1809+ 与 Win11 各一台无 VS/VC redist/WebView2 的 VM,从 zip 开始」。
两台机 2026-08-27 晚已经建好、各有一个 `clean` 快照,这一轮做的是**把冒烟真正跑通**,再把跑出来的
东西记在这里。手册是 `clean-vm.md`,工具是 `scripts/release/cleanvm/`。

---

## 0. 结论

**门 5 未过。**

- **工具侧**:进客户机的两份脚本对 Windows PowerShell 5.1 有六处不兼容,宿主端驱动脚本对 `vmrun`
  有三处错,全部已修,并加了一道新红门(§2)。
- **自动冒烟**:两台机全绿 —— `--version`、`--help` 与未知参数、冷启动到 `first_text_present`、
  sidecar ConPTY、窗口与显示器 DPI 一致、请它关就关退出 0、`diagnostics.log` 首行(§3)。
- **红的是产品,三条**(§5)。**第一条已修并已在同一台干净机上复跑通过**(2026-08-28 晚,
  `docs/DESIGN.md` §7.35;§5.1 的补记就在那一节里)。
  1. **Win11**:只要窗口上开过一个预览座,`WM_CLOSE` 就**关不掉** —— 五次测量都是 60 秒到点仍在、
     只能杀,连被门拦下、什么都没导航的那几次也一样。没有预览座的同一个窗口 91 毫秒就关了。
     **已修**:两个成因(关窗路径把自己上好的浏览器等待扔掉;一个托过 WebView2 的进程不能从
     `main` 里走出去),见 §5.1 的补记。
  2. **Win10(无 WebView2)**:预览座**整块什么都不画** —— 门 5 点名要拍的「缺席卡」拍不到,
     连导航门的拒绝卡也不出;`stderr` 一个字都没有。
  3. **Win10 同一批窗口**:外壳文字被画得比它的版面框大一截,标签、提示条、对话框三处全部溢出。

---

## 1. 怎么跑的

宿主 pwsh 7,**串行**(两台同时首启,客户机六分钟内答不上):

```powershell
pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 `
    -Vmx D:\VMs\folio-cleanvm\folio-win10\folio-win10.vmx `
    -Zip target\release-package\folio-0.1.0-windows-x64.zip `
    -Results <结果目录>\w10 -NoGui -KeepRunning

pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 `
    -Vmx D:\VMs\folio-cleanvm\folio-win11\folio-win11.vmx `
    -Zip target\release-package\folio-0.1.0-windows-x64.zip `
    -Results <结果目录>\w11 -VmPassword <加密口令> -NoGui -KeepRunning
```

`-KeepRunning` 是为了在同一个已登录会话里接着跑两个补充探针(§4):**回快照之后重新开机的机器
不会再自动登录**,`runProgramInGuest -interactive` 会答
`The specified guest user must be logged in interactively to perform this operation`。
Win11 的探针最后是单独一趟跑的(回快照 → `unpack` → 两个探针),因为头一次它们死在自己的
`Process.ExitCode` 上 —— 那正是 §5.1 的现象:窗口不关、`Kill` 之后立刻读退出码会抛。

**结果目录不在仓库里**,在这一轮的临时目录 `…\scratchpad\gate5\` 下:

| 目录 | 内容 |
| --- | --- |
| `w10\results\` `w11\results\` | `machine.txt`、`smoke.log`、`smoke\`(`startup.trace` / `pty.dump` / `diagnostics.log` / `window.png`)、`webview2-card.png`、`web.txt`、`no-console-notice.png`、`notice.txt`、`explorer-icon.png`、`explorer.txt` |
| `w10-probe\` `w11-probe\` | 网页座三张(3 秒 / 15 秒 / 强制重画一帧之后)、外壳打字与三次缩放五张、`probe.txt` |
| `w10-refusal\` `w11-refusal\` | `javascript:` / `file:` / `mailto:` / 普通 https 四张,加一张「资源管理器启动的拒绝」整屏、`refusal.txt` |

**截图不入库。** 客户机提示符里有账户名(`folio`,不是真人),按仓库规矩仍不放进来;这份文档写
路径与描述。

---

## 2. 工具侧:改了什么

### 2.1 六处 5.1 不兼容(`smoke.ps1` / `in-guest.ps1`)

干净机上只有 Windows PowerShell **5.1**(客户机自报 `PSVersion 5.1.19041.3803`)。这两份脚本是在
pwsh 7 上写的。下面每一条都是**在本机 5.1 上跑出来的原文**,不是推断。

**① 三参数 `Join-Path`** —— 门 5 次跑在 `smoke` 阶段当场死在这里:

```
smoke.ps1 : A positional parameter cannot be found that accepts argument '..'.
    + CategoryInfo          : InvalidArgument: (:) [smoke.ps1], ParameterBindingException
    + FullyQualifiedErrorId : PositionalParameterNotFound,smoke.ps1
```

改成两参数套两层,并把根路径从 `$PSScriptRoot`(退到 `$PSCommandPath`)取出来后**断言非空** ——
空根路径正是宿主那条 `smoke.ps1 is not at \scripts\release\smoke.ps1` 的来历。

**② `Start-Process -PassThru` 带重定向时读不到退出码** —— 修完 ① 之后的下一条红:

```
folio exited
At …\scripts\release\smoke.ps1:260 char:30
+ ... f ($folio.ExitCode -ne 0) { throw "folio exited $($folio.ExitCode)" }
```

最小复现(5.1):`Start-Process cmd.exe '/c exit 7' -PassThru -RedirectStandardOutput …` 之后
`WaitForExit` 答 `True` 而 `.ExitCode` 是空的;不带重定向则答 `7`。5.1 从没为这个 `Process` 打开过
进程句柄,而句柄要在进程还活着时才拿得到。起完就 `[void] $process.Handle`,在 7 上无副作用。

**③ 读 Folio 写的文件要 `-Encoding UTF8`** —— 这一条不报错,它污染证据。修 ① ② 之后 Win10 的
`smoke.log` 里是:

```
diagnostics.log: â”€â”€ Folio 0.1.0 (7c6d487de2) â€” run started 2026-08-27T23:16:36.857Z …
```

Folio 写的是无 BOM 的 UTF-8,`Get-Content` 在 7 上按 UTF-8 解、在 5.1 上按 ANSI 代码页解。加了
`-Encoding UTF8` 之后同一行是:

```
diagnostics.log: ── Folio 0.1.0 (7c6d487de2) — run started 2026-08-27T23:42:26.356Z, pid 5888 ──
```

**④ 找错了窗口** —— `web` 阶段在宿主上第一次拍回来的是一张 **26×26** 的图(宿主 200% 缩放,
逻辑上就是 13×13),`web.txt` 写着 `exit code: -1`(那个窗口收了 `WM_CLOSE` 不动,20 秒后被杀)。
探进程一查:folio 的进程有**两个**可见、无属主的顶层窗口,真窗口出生前三秒就有一个 13×13 的:

```
t=    0ms  h=68815410 vis=True owner0=True 13x13 class=Winit Thread Event Target title=""
t= 4500ms  h=26545688 vis=True owner0=True 960x600 class=Window Class title="PowerShell 7"
```

按 Alt-Tab 的规矩加一条判据就分得开:winit 那个带 `WS_EX_TOOLWINDOW`(`ex=0x080800A0`),Folio 的
窗口不带、而且带 `WS_EX_APPWINDOW`(`ex=0x00040110`)。`smoke.ps1` 一直没撞上它,靠的是枚举按
Z 序、刚显示的真窗口在上面 —— 那是运气不是规矩,所以两份脚本一起改。

**⑤ `SetForegroundWindow` 会被拒绝,而且是静默的。** 一个不在前台的进程调它只会答 `false`;截图
于是拍到窗口所在的那块屏幕上**压着的别的东西**(本机上拍到的是 VMware Workstation 的窗口)。现在
按文档的办法先 `AttachThreadInput` 到前台线程,再**核对** `GetForegroundWindow`,五次不成就在阶段
输出里喊一声。

**⑥ `-GuestHome` 空值** —— `in-guest.ps1` 的默认值改成脚本自己所在的目录(宿主本来就把它放在
`C:\folio-vm`),并断言非空。

### 2.2 三处 `vmrun`(`run-smoke-in-vm.ps1`)

**① 等待环节漏了 `-vp`。** 它自己拼命令行、没走公共的旗标构造,于是加密的 Win11 每五秒收到:

```
VMware Tools never came up; checkToolsState last said 'Error: Cannot open VM:
D:\VMs\folio-cleanvm\folio-win11\folio-win11.vmx, A password is required for this operation'
```

现在所有调用共用 `Get-VmrunFlags`。

**② `checkToolsState` 不能当判据。** 补上 `-vp` 之后它答的是 **`installed`** 而不是 `running` ——
而那台机此时早已自动登录、桌面画完(`captureScreen` 拍到了 Windows 11 桌面)、`fileExistsInGuest`
也答得上来。等待环节现在直接**等一次能用的 guest 操作**
(`fileExistsInGuest … C:\Windows\System32\cmd.exe`),因为后面每一步要的正是这个。

**③ 还要再等一次「能跑起来的 `-interactive` 程序」。** 五个阶段全走 `-interactive`,而自动登录比
Tools 晚几十秒到位,这中间 `vmrun` 答
`The specified guest user must be logged in interactively to perform this operation` ——
本轮有一次 `unpack` 就这么丢了。

> 顺带量到的一条 `vmrun` 事实:**它把客户机程序的参数当一条命令行发过去,第一段之后的分段不保留。**
> 同一台机上 `… cmd.exe /c ver`(三段)答 `Guest program exited with non-zero exit code: 1`,而
> `… cmd.exe "/c exit 0"`(一段)答 0。所以等待用的是后一种写法。

`new-vm.ps1` 装机轮询里的 `checkToolsState` 同样没带 `-vp`(第 809、831 行)。那里只打进度、不做
判据,本轮没动。

### 2.3 新加的红门

`run-smoke-in-vm.ps1` 在复制之前,除了既有的 BOM 门,还用宿主自己的 `powershell.exe`(就是 5.1)
把两份进客户机的脚本各解析一遍,有错就拒绝。**已验证会红** —— 往 `smoke.ps1` 尾巴加一行不闭合的
字符串,宿主端当场答 `line 365, column 14: The string is missing the terminator: ".`,一个虚机都
没开。

**解析器只管语法**:上面 ① 那条 `Join-Path` 是绑定错误、解析得干干净净。所以改这两份文件的办法
写进了 `clean-vm.md` §3.4b —— 先在宿主上对着解出来的 zip 跑一遍:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/release/smoke.ps1 `
    -Exe <解压出来的>\folio.exe -Artifacts <某个临时目录>
```

本轮它是全绿的(隔离 `APPDATA` / `LOCALAPPDATA` / `TEMP`,`BT_PTY_DUMP` 开)。

---

## 3. 自动冒烟:两台机的结果

| | Win10 | Win11 |
| --- | --- | --- |
| 机器 | `FOLIO-WIN10`,Windows 10 Pro 10.0.19045 build 19045,64 位 | `FOLIO-WIN11`,Windows 11 Enterprise Evaluation 10.0.26200 build 26200,64 位 |
| WebView2 | **none** —— 没有 EdgeUpdate 客户端键,也没有 EdgeWebView 目录 | `pv=151.0.4129.107`,`C:\Program Files (x86)\Microsoft\EdgeWebView\Application` 下同版本 |
| zip | `folio-0.1.0-windows-x64.zip`,39,246,402 字节,sha256 `c70d1a88…8ebe229a` | 同一个字节、同一个哈希 |
| 解出来 | 七个文件,大小逐个对上 | 同 |
| VERSIONINFO | 八个字段齐,`Folio` / `0.1.0` / `folio.exe` | 同 |
| `--version` | `Folio 0.1.0 (7c6d487de2)`,退出 0,与 PE 资源一致 | 同 |
| `--help` / 未知参数 | 0 / 2,拒绝里报出参数名 | 同 |
| 冷启动 | `runtime_ready=407ms` `background_visible=465ms` `first_text_present=3353ms` | `runtime_ready=937ms` `background_visible=1019ms` `first_text_present=6000ms` |
| ConPTY | `source=sidecar version=1.25.260710002-preview dll=C:\folio-vm\folio\conpty.dll` | 同 |
| DPI | 窗口与显示器都是 96(100%) | 同 |
| 关闭(无网页) | 请它关就关,退出 0 | 同 |
| `diagnostics.log` 首行 | 与 `--version` 同一行 build 标识 | 同 |
| 无 console 的消息框 | 出,标题 `Folio`,正文 `There is no --panic-selftest option.` + 用法块 | 同 |
| `--panic-selftest` | 退出 2 —— 本构建没有受控 panic 入口,这句话仍成立 | 同 |
| 资源管理器图标 | `folio.exe` 戴着自己的图标,七项 | 同 |
| 网页座 | **空的**(§5.2) | 3 秒内载入 example.com,地址栏与标题都对 |
| `web` 阶段退出码 | 0 | **-1**(§5.1) |

`in-guest.ps1` 五个阶段两台机都零非零退出。

**改完之后又整跑了一遍**,两台机各一次、用的就是提交进来的这三份脚本,`run-smoke-in-vm.ps1` 都以
0 结束(结果在 `v10\` 与 `v11\`)。所以「六处 5.1 不兼容 + 三处 `vmrun`」这一串改动本身是被端到端
验过的,不只是被解析器验过。

---

## 4. 手工三项:本轮用探针补上了

`clean-vm.md` §4.2/§4.3 把「shell 输入」「resize」「四个拒绝面」列成手工项。这一轮写了两个一次性
探针挂在运行后面(不在仓库里,在这一轮的临时目录),把三项变成有证据的:

- **打字**(两台都做):`SendKeys` 送 `echo folio-typed-this` 与 `cd C:\folio-vm`。两条都回显,提示符
  从 `PS C:\Users\folio>` 变成 `PS C:\folio-vm>`,命令标记的下划线也画出来了
  (`shell-b-typed.png`)。
- **缩放**(两台都做):`SetWindowPos` 到 1400×900、再到 700×420、再回 960×600。内容跟着重排、
  不花屏、回到原尺寸后内容仍在(`shell-c/d/e-*.png`)。700 宽时提示条右侧三件东西
  (「Add to $PROFILE」「Don't show again」和关闭的 ×)开始互相压边,是个小问题。
- **拒绝面**:`BT_WEB_DEV` 分别设成 `javascript:alert(1)`、`file:///C:/Windows/win.ini`、
  `mailto:someone@example.com`,加一个普通 https 作对照。**Win11 上三面里出了两张卡**:

  | `BT_WEB_DEV` | 座上是什么 |
  | --- | --- |
  | `file:///C:/Windows/win.ini` | 一张卡:地球图标 + 「…do not open in a preview.」+ 地址 `C:\Windows\win.ini` + 「Copy address」 |
  | `mailto:someone@example.com` | 同一张卡的形状,地址是 `mailto:someone@example.com` |
  | `javascript:alert(1)` | **没有卡** —— 座是空座占位(「…ted path to preview it here」),地址栏空白。这一条被悄悄丢掉了 |
  | `https://example.com/`(对照) | 页面正常 |

  **下载那一面本轮没做** —— 它要一个真的会触发下载的地址。Win10 上四张长得一模一样:座是空的
  (§5.2)。

  拍到的两张卡有一半压在 PSReadLine 那张对话框后面 —— 是证据质量问题,不是产品问题;要正式的
  截图得先把对话框按掉。

另外把「资源管理器启动的拒绝会不会先开一个窗口」问清楚了:**不会**。那次启动的 folio 进程只有
一个顶层窗口,411×270,就是那个消息框(`refusal-explorer.png`)。

---

## 5. 发现的产品缺陷

### 5.1 (红)开过预览座的窗口关不掉 —— Win11

同一台机、同一个二进制、同一次会话,两次启动只差一个 `BT_WEB_DEV`:

```
web launch  : closed on WM_CLOSE = False after 60044 ms   exit code -1(被杀)
shell launch: closed on WM_CLOSE = True  after 91 ms      exit code 0
```

**不是「页面载入了才这样」。** 拒绝面那一组四次启动,连被门拦下、什么都没导航的那几条在内,
全都一样:

```
javascript : closed=False after 60052 ms
file       : closed=False after 60004 ms
mailto     : closed=False after 60013 ms
ordinary   : closed=False after 59999 ms
```

冒烟自己的 `web` 阶段(20 秒预算)两轮也都记下同一件事:`w11\results\web.txt` 的
`exit code : -1`。而且那个没关掉的窗口会**留在屏幕上**继续挡后面的阶段 ——
`w11\results\no-console-notice.png` 和 `explorer-icon.png` 的背景里都能看见它。

Win10 上同样的四次启动 60–80 毫秒就关了。两台机之间唯一的差别是**这台有 WebView2 运行时,那台
没有**;而在有运行时的那台上,**凡是开过预览座的启动一次都没关掉**,包括地址被门拦下、什么都没
导航的那三次。所以嫌疑落在座向引擎要环境这条路上,不在「页面载入过」。

用户按下窗口的 × 之后窗口不走,这是发布阻塞项。

#### 补记(2026-08-28 晚):已定因、已修、已在同一台机上复跑

**这一节量的是「进程有没有退出」,不是「窗有没有消失」**(探针用的是 `Process.WaitForExit`),而
那正是解开它的第一把钥匙:那些「没关掉」的进程其实**已经写下了退出码 0**,只是从没离开进程表。
完整的复现记录、两个成因、修法与红门在 `docs/DESIGN.md` **§7.35**;这里只留结果。

| | 窗离开屏幕 | 退出码写下 | 离开进程表 | `HWND` 销毁 | 它起的六个浏览器进程 |
|---|---:|---:|---:|---:|---:|
| 这一轮的构建 `7c6d487de2` | 26 ms | 600 ms | **240 秒内从未** | **240 秒内从未** | **240 秒后仍剩五个** |
| 修完之后 | **47 ms** | 6.803 s | **6.803 s** | **6.803 s** | 6.803 s 全部走完 |

复跑用的是 `scripts/release/cleanvm/run-smoke-in-vm.ps1`,同一台 Win11、`clean` 快照、新打的
`folio-0.1.0-windows-x64.zip`:整趟以 0 结束,`results\web.txt` 的 `exit code` 从 **-1 变成 0**,
`smoke.log` 写着「it closed when asked, and exited 0」。那六秒是引擎自己拆进程树要的时间(修前在
开发机上 folio 298 毫秒就死了,它的六个 `msedgewebview2` 仍然花到 6.8 秒才走完),读者看见的关窗
仍然是几十毫秒。

顺带:本机(开发机)上 §7.34 挂账的「进程退了 `HWND` 不死」**同时消失** —— 同一段复现里窗、进程、
浏览器全部在 6.6 秒一起走完。

### 5.2 (红)没有 WebView2 的机器上,预览座整块不画 —— Win10

`BT_WEB_DEV` 开出预览座,座的头部画了页面图标,**座身完全是空的**。等到 15 秒还是空的;再把窗口
拉宽 60 像素强制重画一帧,仍然是空的。这一路 `stderr` **一个字节都没有** —— `main.rs` 在
`WebOutcome::Fault` 上是 `eprintln!("BT_WEB {text}")`,所以那条失败根本没走到。

门 5 明写要拍的「Microsoft Edge WebView2 Runtime is not installed. / Download the runtime」缺席卡,
**在这台机上拍不到**。

不只是缺席卡:同一台机上把 `BT_WEB_DEV` 换成 `javascript:` / `file:` / `mailto:`,座也是同样的空。
导航门的拒绝卡是 Folio 自己画的、跟引擎无关,它也没出现。所以现象是「这台机上这个座什么都不画」,
而不是「缺席卡这一张写错了」。

证据:`w10-probe\web-a-3s.png`、`web-b-15s.png`、`web-c-after-forced-frame.png`、
`w10-refusal\refusal-*.png`;对照 `w11-probe\web-a-3s.png`(同样的窗口尺寸、同样的对话框,页面
3 秒内就在)。

### 5.3 (红)同一台机上,外壳文字比它的版面框大一截 —— Win10

和 5.2 同框出现,值得单列,因为它可能是同一个原因的另一面:Win10 + 有预览座的那些窗口里,**外壳
(chrome)的文字被画得明显大于版面为它留的框**,而终端网格的字号一点没变
(`PS C:\Users\folio>` 在两种情况下都是 x=8..170)。看得见的后果:

- 标签「Windows PowerShel」画到标签之外、压住关闭的 ×;
- 提示条的正文与右边两个动作错位,关闭的 × 跑到正文中间;
- PSReadLine 那张对话框:圆角面板和两个按钮在正确的位置,**标题与正文整块画在面板右边**、被窗口
  边缘切掉;
- 窗口再宽 60 像素之后,标题栏的最大化与关闭按钮直接被挤出可视区。

**它只在这台机上、只在开了预览座的窗口里出现。** 同一台 Win10 不开预览座的启动(打字/缩放那一组)
外壳完全正常;Win11 开着同一个预览座、同一张对话框,外壳也完全正常。可疑的分界正是
**WebView2 环境创建失败**这件事本身。

证据:`w10-probe\web-b-15s.png`(15 秒,不是动画中间帧)对 `w10-probe\shell-b-typed.png`,再对
`w11-probe\web-b-15s.png`。

### 5.4 ~~(小)窗口一变宽,已经载入的页面就不见了 —— Win11~~ **撤销:不是缺陷**

`w11-probe\web-c-after-forced-frame.png`:窗口从 960 拉到 1020 之后**三秒**,地址栏还写着
`https://example.com/`、标题还是 `Example Domain`,而页面那一块是空的。

**2026-08-28 查明:这是 §7.7 的裁决在正常工作,不是缩放的事。** 那张图里 PSReadLine 的邀请卡正立
在窗上,而**一张盖住整扇窗的卡会把页从玻璃上撤下来**(`a_modal_covers_the_window` →
`WebPresence::Hidden`);地址栏和标题是 chrome,不受影响,所以看上去就是「头还在、页没了」。它的
**前一张** `web-b-15s.png`(缩放之前)已经是同一个样子,所以缩放与它无关。要拍一张会随窗变宽而
重排的页,得先把那张卡按掉。

### 5.5 (小)消息框里没有 build 标识,而证据文件说它有

`notice.txt` 写着「Read in the picture: … the build line is the one --version prints」。两台机拍到的
框里只有拒绝那一句和用法块,**没有版本行**。要么把那一行加进去,要么把 `notice.txt` 的话改准。

### 5.6 (小)提示条在窄窗口上互相压边

两台机的 700×420 那一张都是。

### 5.7 (待裁)`javascript:` 这一面没有卡

`file:` 与 `mailto:` 各出一张「不在预览里打开」的卡,`javascript:alert(1)` 什么都不出:座回到空座
占位、地址栏空白(§4)。门确实拦住了(没有导航、没有 `stderr`),但读的人得不到「它被拒了」这句
话。`plan.md` 门 5 写的是「四个拒绝面各出各的卡」,所以要么补这一张,要么改那句话。

---

## 6. 手册与工具之外的两处环境事实

- **两台客户机的桌面是 1024×768,不是 `clean-vm.md` §2.1 写的 1920×1080。** `.vmx` 里的
  `svga.maxWidth` / `svga.maxHeight` 没有变成实际分辨率。后果不只是难看:Folio 的首窗 960×600
  开在 (104,104),右边 40 像素**掉到屏幕外**,所以每一张整窗截图右侧都是一条空白、关闭按钮不在
  图里。要么把客户机分辨率真的调到 1920×1080,要么把这条写进手册。
- **回快照后重新开机的机器不再自动登录**,`-interactive` 一律被拒。补充探针必须挂在一次带
  `-KeepRunning` 的完整运行后面。两条都已写进 `clean-vm.md` §3.4c。

---

## 7. `plan.md` 门 5 验收项 × 冒烟覆盖

| 门 5 的话 | 谁在验 | 覆盖 |
| --- | --- | --- |
| Win10 1809+ 与 Win11 各一台 | `machine.txt` 记版本与 build | 全 |
| 无 VS / VC redist / WebView2 | WebView2 有查(注册表 + 目录 + 让 Folio 自己答);**VS 与 VC 运行库没有清点**,只靠「这台机除 VMware Tools 外没装过东西」和「exe 真的跑起来了」 | 部分 |
| 从 zip 开始 | `unpack`:sha256、字节数、七个文件逐个 | 全 |
| `--version` | `smoke` 第 1 条(经管道读,并与 PE 资源比对) | 全 |
| PE 图标 | `explorer-icon.png` | 全 |
| VERSIONINFO | `machine.txt` 八字段 + `explorer.txt` 全字段 | 全 |
| shell 输入 | 冒烟只到 `first_text_present`;本轮由探针补(§4) | 全(靠探针) |
| resize | 冒烟不做;本轮由探针补(§4) | 全(靠探针) |
| ConPTY 来源 | `smoke` 第 4 条 | 全 |
| WebView2 缺席卡 | `web` 阶段拍,**本轮红**(§5.2) | 覆盖,未过 |
| 受控 panic 的可见提示 | `notice` 阶段验的是**同一条通道的另一种触发**(未知参数经资源管理器启动 → 消息框),不是真 panic。「release 无受控 panic 入口」这件事本身被 `--panic-selftest` 退出 2 检验了 | 部分 |
| 有 WebView2 的 VM 验顶层 gate 与四个拒绝面 | 探针跑了三面:`file:` 与 `mailto:` **各出一张卡**,`javascript:` **没有卡**(座是空座占位);**下载那一面没做**(§4) | 部分 |
| 截图入库 | 截图齐了;**本轮按规约不入库** | 挂账 |

冒烟**完全没碰**、而任务单里点名的一项:**SmartScreen**。`vmrun copyFileFromHostToGuest` 送进去的
文件没有 Mark-of-the-Web,SmartScreen 天然不会出现;要拍它得在客户机里用浏览器真下一次 zip
(Win10 那台没有网卡,只能在 Win11 上做)。

---

## 8. 挂账

1. **§5.1 已修并已复跑通过**(2026-08-28 晚,`docs/DESIGN.md` §7.35)。**§5.2、§5.3 仍是发布阻塞
   项**,都在没有 WebView2 的那台 Win10 上,要先修再谈门 5 过不过。**§5.4 已撤销**,不是缺陷。
2. `javascript:` 那一面没有卡(§4),以及下载那一面根本没测。前者得先定是产品缺陷还是
   `plan.md` 的说法要改。
3. SmartScreen 一张,仍缺。
4. 200% 缩放下的第二轮(`clean-vm.md` §2.1 建议在 Win11 上做)本轮没做。
5. 客户机分辨率 1024×768 与手册不一致(§6),截图都被切掉右边一条。
6. `new-vm.ps1` 的 `checkToolsState` 同样漏 `-vp`(§2.2 末)。
7. 证据入库与 `.gitignore` 例外行(`clean-vm.md` §7.1)本轮没做。
