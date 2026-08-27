# 门 5「干净机验证」执行手册(2026-08-27)

`plan.md` 门 5 的原话:「Win10 1809+ 与 Win11 各一台无 VS/VC redist/WebView2 的 VM,从 zip
开始:`--version`、PE 图标/VERSIONINFO、shell 输入、resize、ConPTY 来源、WebView2 缺席卡、受控
panic 的可见提示;有 WebView2 的 VM 验顶层 gate 与四个拒绝面。截图入库。」

这份文档是那一句的施工图。工具在 `scripts/release/cleanvm/`:

| 文件 | 干什么 |
| --- | --- |
| `autounattend-win11.xml` | Win11 无人值守应答文件(模板,带占位符) |
| `autounattend-win10.xml` | Win10 无人值守应答文件(模板,带占位符) |
| `new-answer-iso.ps1` | 把应答文件做成一张只装一个文件的小 ISO |
| `run-smoke-in-vm.ps1` | 宿主端驱动:回快照 → 开机 → 拷进去 → 跑 → 拷回来 → 关机 |
| `in-guest.ps1` | 客户机端五个阶段:unpack / smoke / web / notice / explorer |

**本次没做的事(明确说清):没有启动任何虚机、没有下载任何 ISO、没有改 `.gitignore`、没有改任何
`.rs`。** 虚机由用户建,ISO 由用户下。

---

## 1. ISO:官方下载页与核实结果

所有链接都在 **2026-08-27** 当天抓取核实。下面每条都标了核实方式和结论。

### 1.1 Windows 11 企业版评估 —— 页面在,可用

- <https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise>
- **核实结果(2026-08-27 抓取页面正文)**:页面在,提供两个产品:
  - **Windows 11 Enterprise, version 25H2** —— x64 ISO 与 Arm64 ISO;
  - **Windows 11 Enterprise LTSC 2024** —— 仅 x64 ISO。
  - 只有 ISO,没有 VHD。
- 评估期 **90 天**;页面原文:「A product key is not required for this software」。90 天后行为页面
  也写了(桌面变黑、持续提示未激活、每小时关机)——对一台跑几次冒烟就删的虚机无所谓。
- **取哪个**:`Windows 11 Enterprise, version 25H2` 的 **x64 ISO**。LTSC 2024 是另一条服务线,
  不是普通用户装的东西,门 5 要的是「像真机」。
- 下载要先用 Microsoft 账号登记(页面自述)。

### 1.2 Windows 10 企业版评估 —— **已下架**,这是本次核实最重要的发现

- <https://www.microsoft.com/en-us/evalcenter/evaluate-windows-10-enterprise> ——
  **2026-08-27 抓取:HTTP 301,重定向到**
  <https://blogs.windows.com/windowsexperience/2024/10/31/how-to-prepare-for-windows-10-end-of-support-by-moving-to-windows-11-today/>
  (一篇「迁移到 Windows 11」的博文)。页面不再存在。
- <https://www.microsoft.com/en-us/evalcenter/evaluate-windows-10-enterprise-ltsc-2021> ——
  **2026-08-27 抓取:HTTP 404**。
- <https://www.microsoft.com/en-us/evalcenter/> —— **2026-08-27 抓取正文:Windows 客户端评估产品
  只剩 Windows 11 Enterprise 一项,列表里没有任何 Windows 10 版本。**

Windows 10 于 2025-10-14 结束支持,评估中心的 Windows 10 条目随之撤掉。**门 5 原文假定的
「Win10 22H2 企业评估版 ISO」这条路已经没有了。**

### 1.3 Win10 干净机的替代路线(按推荐顺序)

**A(推荐)消费版多版本 ISO —— 官方页面仍在**

- <https://www.microsoft.com/en-us/software-download/windows10ISO>
- **核实结果(2026-08-27 抓取页面正文)**:页面在,提供 **Windows 10 多版本 ISO(含 Home 与
  Pro)**,40 余种语言,32/64 位;页面同时挂着「2025-10-14 之后不再提供免费更新/技术支持/安全修复」
  的醒目告示。企业版需另行到 Microsoft 365 管理中心取,页面自述。
- 装 **Windows 10 Pro,不填密钥、不激活**。未激活的 Windows 10 能装、能开机、能跑终端;它会在
  设置里唠叨个性化功能不可用,而门 5 一条都不量这个。
- 两点二手信息,**未验证**(第三方报道,非微软页面):① 在 Windows 上访问该页会被引导去
  Media Creation Tool,得用非 Windows 的 User-Agent 才直接拿到 ISO 选择界面;② 微软在
  2025-10-10 确认 Media Creation Tool 有问题,不能再造 Windows 10 启动介质。**结论:直接走
  ISO,别走 MCT。** 如果页面只给你 MCT,换浏览器 UA 再试。
- 版本对不对得上:多版本 ISO 的最终版本就是 22H2(build 19045),满足门 5 的「1809+」。

**B 官方但要钱:Visual Studio 订阅**

<https://my.visualstudio.com> 的下载区仍是取 Windows 10 企业版镜像的官方渠道(第三方讨论指向
这里,**未验证**,需付费订阅)。除非门 5 后续冒出「必须是 Enterprise SKU 才有的行为」,否则不必。

**C 你手上已有的 Win10 安装介质**

有就用,记下版本号和 SHA256,写进证据里。

**不推荐:archive.org 上的评估版镜像。** 有人在传,但那不是微软发布的字节,没有官方哈希可比对。
一个用来证明「干净机上能跑」的实验,底座不能是来路不明的镜像。

**Folio 只需要 Pro 还是 Enterprise?** 门 5 关心的是「无 VS、无 VC 运行库、无 WebView2 的
Windows」,不是 SKU。Pro 完全够。若哪天要验企业策略相关行为,再单独立机。

---

## 2. VMware 建机参数

宿主实测(本机 2026-08-27 探测):

```
VMware Workstation 17.6.4 build-24832109
vmrun version 1.17.0 build-24832109
C:\Program Files (x86)\VMware\VMware Workstation\vmrun.exe
```

`run-smoke-in-vm.ps1` 就是按这个默认位置探测的(另外还会看 Player 的目录、注册表 `InstallPath`、
以及 `PATH`)。

### 2.1 两台机的参数表

| 项 | Win11 机 | Win10 机 |
| --- | --- | --- |
| 客户机操作系统 | Windows 11 x64 | Windows 10 x64 |
| 虚拟机名 / 目录名 | `folio-win11` | `folio-win10` |
| 处理器 | 2 核(给得起就 4) | 2 核 |
| 内存 | 6 GB(下限 4) | 4 GB |
| 磁盘 | 64 GB,拆分成多个文件,**不**预分配 | 60 GB,同上 |
| 固件 | **UEFI + Secure Boot** | UEFI(Secure Boot 开关随意) |
| TPM | **加 vTPM(见 2.2)** | 不加 |
| 显示 | 关掉「自动调整客户机分辨率」,固定 **1920×1080**,缩放 100% | 同 |
| 网络 | NAT,**连接**(要验「有引擎」路径) | NAT,**断开连接**(见 2.4) |
| 声卡 / 打印机 / USB | 全部移除 | 全部移除 |
| VMware Tools | **必装**(见 2.3) | **必装** |
| 快照名 | `clean` | `clean` |

磁盘不预分配是为了别在宿主上白占 124 GB;拆分文件是为了将来复制/删除快。

**显示分辨率为什么是 1920×1080:** 冒烟脚本要给窗口整窗截图,产品截图规范(`docs/screenshots/README.md`)
是 1600×1000 逻辑像素 @100%。1920×1080 的桌面刚好放得下,且不会因为客户机分辨率随宿主窗口漂移而
让两次截图尺寸不同。**「自动调整客户机分辨率」必须关掉**,否则每次开窗大小都不一样。

**要不要再验 200% DPI:** `smoke.ps1` 第 5 条断言的是「窗口 DPI == 显示器 DPI」而不是「等于 96」,
所以把某一台的客户机缩放设成 200% 再跑一遍,同一条断言就是 200% 的断言。建议在 Win11 机上做第二轮:
`clean` 快照恢复 → 系统设置里改缩放 200% → 注销重登 → 再跑一次,证据另存一份。

### 2.2 Win11 的 TPM / Secure Boot 怎么过:用 vTPM,不用 LabConfig

**做法:** 先给虚机加密(Workstation 17 Pro 支持「只加密支持 TPM 所需的文件」的快速加密,不必整盘
加密),再在虚机设置里添加 **Trusted Platform Module** 设备;固件选 UEFI 并勾 Secure Boot。加密口令
记在你自己的密码管理器里,并在跑 `run-smoke-in-vm.ps1` 时用 `-VmPassword` 传给 `vmrun`。

**为什么不用注册表绕过(`LabConfig\BypassTPMCheck` 一类):**

1. vTPM 是 Broadcom 自己文档给的受支持路径(<https://knowledge.broadcom.com/external/article/315650/installing-windows-11-as-a-guest-os-on-v.html>,
   2026-08-27 抓取:要求 TPM 支持、UEFI/Secure Boot、并说明加密后添加 vTPM 是 Pro/Fusion 的做法;
   Player 不支持 vTPM);绕过检查是绕过微软公开声明的安装要求。
2. **更要紧的是这台机的意义**:门 5 要的是「像用户的真机」。真机有 TPM 2.0、开着 Secure Boot;一台
   靠改注册表骗过检查装出来的 Windows 11,和用户手里那台不是同一种机器,拿它做的结论也就不是同一个
   结论。花五分钟点两下加密,比给证据留一个「这机器不合法」的注脚划算。

Workstation 17 的「快速加密」只加密支撑 TPM 所需的部分文件,性能影响可忽略(VMware 17 发布说明,
2026-08-27 经检索确认该选项存在;**具体性能数字未验证**)。

### 2.3 VMware Tools 必须装,而且它是唯一允许装的东西

`vmrun` 的 **guest 操作全部依赖 Tools**:`copyFileFromHostToGuest`、`copyFileFromGuestToHost`、
`runProgramInGuest`、`checkToolsState`、`captureScreen` 都是发给虚机内 Tools 服务的请求。没有 Tools,
这套脚本只能开机关机,别的一件都干不成。

「不装 VS/VC 运行库/WebView2 之外的任何东西」这条纪律和装 Tools 不冲突:Tools 是虚拟硬件的驱动,
不是开发工具链,也不带 VC++ 运行库以外的用户态负担。但它**确实**是一件装上去的东西,所以
`in-guest.ps1` 的 `unpack` 阶段会把 Windows 版本、build 号、安装日期一起写进 `machine.txt`,谁看证据
都知道这台机上有什么。

> 若哪天想要一台连 Tools 都没有的机器,那就只能人手在虚机窗口里操作 + `captureScreen` 截图。
> 门 5 不值得为这个多花一天。

### 2.4 Win10 机为什么要断网 —— 这条最容易翻车

Windows 10 的消费设备**是** WebView2 运行时的推送目标:微软通过 Edge Update 向 Windows 10
(2018 年 4 月更新及以后的 Home/Pro)分发 Evergreen WebView2 运行时,并在自家文档里说,推送完成后
开发者可以「比较可靠地」认为 Windows 10 上有 WebView2
(<https://blogs.windows.com/msedgedev/2022/06/27/delivering-the-microsoft-edge-webview2-runtime-to-windows-10-consumers/>、
<https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution>,2026-08-27 检索确认)。

也就是说:**一台联网的、放着不管的 Win10 干净机,可能自己长出 WebView2,然后「缺席卡」就拍不到了。**

三道防线,按强度:

1. **网卡断开连接**(虚机设置里取消「已连接」和「开机时连接」)。这一条最硬。
2. 应答文件在 specialize 阶段写的策略键:关自动更新、禁止连 Windows Update 的互联网位置、
   `HKLM\SOFTWARE\Policies\Microsoft\EdgeUpdate` 下把 WebView2 的应用 id
   `Install{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` 设成 0(该 id 与该策略名的用法见微软 Edge Update
   策略文档与社区实践,2026-08-27 检索确认;**注意它只挡新装,不会卸掉已有的**)。
3. **看,而不是假定**:`in-guest.ps1` 的 `unpack` 阶段先把注册表三处 `pv` 和三处
   `Microsoft\EdgeWebView\Application` 目录的实际内容抄进 `machine.txt`,再让 Folio 自己回答。

第 3 条里「让 Folio 自己回答」不是客套:`crates/bt-app/src/webhost.rs` 明写着注册表会说谎而
loader 不会(门 7 抓到过注册表声称有运行时而 loader 拿不出来)。所以**最终判据是那张卡本身**,
注册表只是旁证。

反过来,**Win11 自带 WebView2**(微软文档:Evergreen 运行时随 Windows 11 预装),所以 Win11 机不需要
装任何东西就能验「有引擎」那条路。

---

## 3. 无人值守安装:两张 ISO 的做法

### 3.1 原理

Windows Setup 在画第一个页面之前,按固定顺序找应答文件。顺序表的**第 5 条是「可移动只读介质,按盘符
顺序,在盘根目录」,文件名必须是 `Autounattend.xml`**
(<https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/windows-setup-automation-overview>,
2026-08-27 抓取原文核对)。

所以做法是:**给虚机挂两个 CD/DVD 设备** —— 第一个是微软的安装 ISO(原样,字节不动,哈希还能对),
第二个是我们做的一张只装一个文件的小 ISO。不需要 U 盘直通,不需要软盘镜像,更不需要重打安装 ISO。

### 3.2 生成那张小 ISO

```powershell
# 在仓库根目录
pwsh -File scripts/release/cleanvm/new-answer-iso.ps1 -AnswerFile autounattend-win11.xml
pwsh -File scripts/release/cleanvm/new-answer-iso.ps1 -AnswerFile autounattend-win10.xml
```

产物默认落在 `target/cleanvm/autounattend-win11.iso` / `autounattend-win10.iso`(`target/` 不入库)。

**本机实测输出(2026-08-27):**

```
autounattend-win10 : computer FOLIO-WIN10, time zone China Standard Time, account folio
D:\...\target\cleanvm\autounattend-win10.iso — 63,488 bytes
attach it to the VM as a second CD/DVD device, connected at power on.
```

挂载回来验过:根目录一个 `Autounattend.xml`(10,270 字节,大小写正确),占位符全部替换掉,密码两处
(本地账户与自动登录)都填上了。

脚本用的是 Windows 自带的 IMAPI2 光盘刻录服务,**不需要装 Windows ADK**。装了 ADK 的机器上等价的
一行是:

```
oscdimg -u2 -m <staging 目录> <输出.iso>
```

参数:`-Password`(默认 `folio`)、`-ComputerName`(默认按模板名推 `FOLIO-WIN11` / `FOLIO-WIN10`)、
`-TimeZone`(默认 `China Standard Time`,`tzutil /l` 列全部)、`-Output`。带 `-WhatIf` 只打印不写盘。

### 3.3 账号与口令

- 本地管理员账户 **`folio`**,口令 **`folio`**。
- 口令**不在仓库里**:两个 `.xml` 模板写的是 `@@PASSWORD@@`,`new-answer-iso.ps1` 在做 ISO 的时候
  才替换。这样门 0 的 gitleaks/trufflehog 全历史扫描不会在跟踪文件里撞见一个凭据,而这份文档说的
  是一台随手删掉的虚机的口令。
- 换口令就 `-Password <新的>`,同时给 `run-smoke-in-vm.ps1` 传 `-GuestPassword <新的>`。

### 3.4 应答文件里做了什么

两份都是:英文 en-US、接受许可、**擦掉磁盘 0** 重新按 UEFI/GPT 分区(EFI 260 MB + MSR 16 MB + 其余给
Windows)、装到第 3 个分区、跳过 OOBE 的隐私/网络/微软账号页、建本地账户 `folio` 并自动登录、时区、
关自动更新、挡 Edge Update 装 WebView2、建 `C:\folio-vm`、把资源管理器的扩展名显示打开。

**自动登录是必需的,不是图省事**:`vmrun runProgramInGuest -interactive` 要一个有桌面的已登录会话,
而后面每个阶段都要开窗口再拍照 —— 画在没人看的工作站上的窗口,拍出来是黑的。

两处必须自己核对的字段:

- **映像名**。Win11 模板写的是 `Windows 11 Enterprise Evaluation`;Win10 模板写的是 `Windows 10 Pro`。
  ISO 里到底叫什么,挂载后用
  `dism /Get-ImageInfo /ImageFile:<盘符>:\sources\install.wim` 问一次。名字对不上,Setup 会停在选版本
  那一页等一个不存在的人。
- **磁盘 0**。虚机只有一块盘时它就是 0。别把这两个 xml 拿到真机上跑。

**两份应答文件都标着「未验证」**:它们是照微软 unattend 参考写的,还没有对着 ISO 跑过。门 5 的第一次
建机就是它们的第一次运行。第一次装如果卡在某一页,截图记下卡在哪一页,回来改对应的一段 —— 别改一堆。

### 3.5 建机三步(留给用户的操作)

1. **建机**:按 2.1 的表建两台。Win11 那台先加密再加 vTPM(2.2)。声卡打印机 USB 都删掉。
   Win10 那台把网卡的「已连接 / 开机时连接」两个勾都去掉。
2. **挂两张盘 + 开机装**:CD1 = 微软安装 ISO,CD2 = `target/cleanvm/autounattend-winXX.iso`,
   两个都勾「开机时连接」。开机,让它自己装完到桌面。装完手工装 VMware Tools,重启。
3. **打快照**:确认桌面干净、Folio 从未在这台机上跑过、网络状态是你想要的,然后打一个名叫
   **`clean`** 的快照。名字必须是 `clean` —— 脚本的默认值,也是这台机唯一该有的那个快照。

---

## 4. 每台机的验证清单

跑法:

```powershell
# 先有一个包(门 2 的产物)
pwsh -File scripts/release/package.ps1

# 再驱动虚机(-WhatIf 先看一遍要跑什么)
pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx D:\vm\folio-win10\folio-win10.vmx -WhatIf
pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx D:\vm\folio-win10\folio-win10.vmx
pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx D:\vm\folio-win11\folio-win11.vmx -VmPassword <加密口令>
```

证据落在 `target/cleanvm/<虚机名>-<时间戳>/results/`。

### 4.1 两台机都要过的(自动)

| ✓ | 项 | 谁在验 | 证据文件 |
| --- | --- | --- | --- |
| ☐ | zip 解出来就是七个文件,大小对得上 | `unpack` | `machine.txt` |
| ☐ | 这台 Windows 是什么(版本/build/架构/装机日期) | `unpack` | `machine.txt` |
| ☐ | `folio.exe` 的 VERSIONINFO 八个字段 | `unpack` | `machine.txt` |
| ☐ | 这台机有没有 WebView2(注册表 + 目录,旁证) | `unpack` | `machine.txt` |
| ☐ | `--version` 从管道能读到,退出 0,与 PE 资源一致 | `smoke` | `smoke.log` |
| ☐ | `--help` 退出 0;未知参数退出 2 且报出参数名 | `smoke` | `smoke.log` |
| ☐ | 冷启动走到 `first_text_present`(开窗 → 建设备 → 起 shell → 有字) | `smoke` | `smoke/startup.trace` |
| ☐ | **ConPTY 来自 exe 旁边的 sidecar,不是系统的** | `smoke` | `smoke/startup.trace` |
| ☐ | 窗口 DPI 与显示器 DPI 相等 | `smoke` | `smoke.log` |
| ☐ | 请它关它就关,退出码 0 | `smoke` | `smoke.log` |
| ☐ | `diagnostics.log` 首行就是 `--version` 那一行 | `smoke` | `smoke/diagnostics.log` |
| ☐ | 无 console 启动时的可见提示(消息框) | `notice` | `no-console-notice.png`、`notice.txt` |
| ☐ | release 里确实没有受控 panic 入口 | `notice` | `notice.txt` |
| ☐ | 资源管理器里的图标 | `explorer` | `explorer-icon.png` |

### 4.2 Win10 机专属(无引擎)

| ✓ | 项 | 证据 |
| --- | --- | --- |
| ☐ | `machine.txt` 的 WebView2 一节是 `none` | `machine.txt` |
| ☐ | **WebView2 缺席降级卡**:一句「Microsoft Edge WebView2 Runtime is not installed.」+ 一个「Download the runtime」按钮 | `webview2-card.png` |
| ☐ | 卡不是「引擎没起来」那一张(那张在有运行时的机器上才该出现,出现在这里就是本门红了) | `webview2-card.png`、`web.txt` |
| ☐ | shell 里能打字、能回显(手工:虚机窗口里敲两行) | 手工截图 |
| ☐ | 拉窗口改大小,内容跟着重排不花屏(手工) | 手工截图 |

### 4.3 Win11 机专属(有引擎)

| ✓ | 项 | 证据 |
| --- | --- | --- |
| ☐ | `machine.txt` 的 WebView2 一节报出一个版本号 | `machine.txt` |
| ☐ | `BT_WEB_DEV` 那一页真的出来了(联网时),或者出的是「命中了主机但没加载成」的卡(断网时)—— 两者都证明引擎起来了 | `webview2-card.png` |
| ☐ | 顶层导航 gate:`javascript:`、`file:///C:/Windows/win.ini`、`mailto:someone@example.com`、一个下载 —— 四个拒绝面各出各的卡 | 见下 |
| ☐ | 200% 缩放下再跑一遍冒烟(第二轮) | 第二份 `smoke.log` |
| ☐ | shell 输入 / resize(手工) | 手工截图 |

**四个拒绝面怎么触发。** `BT_WEB_DEV=<url>` 是这个构建里唯一不需要动鼠标就能开出一张网页座的门
(`webhost::development_target`),而导航门(`webnav::address_bar`)在 `webhost.rs` 内部被调用。因此
理论上把 `BT_WEB_DEV` 换成上面四个值分别启动一次,就能逐个拍到拒绝卡:

```powershell
# 在虚机里,每次一个,起窗 → 看卡 → 关窗
$env:BT_WEB_DEV = 'javascript:alert(1)'          # 脚本/内联 scheme
$env:BT_WEB_DEV = 'file:///C:/Windows/win.ini'   # file: scheme
$env:BT_WEB_DEV = 'mailto:someone@example.com'   # 外部 scheme
# 下载那一面需要一个真的会触发下载的地址,得联网
```

**这四条尚未在本机验证过**(没有跑过 release 二进制去看每一张卡长什么样)。到虚机上逐条试,把实际
出现的卡面记进证据;如果某一条出的不是拒绝卡而是别的东西,那本身就是门 5 该抓的发现,不是脚本的
bug。下载那一面需要网络,放在 Win11 联网的那一轮做。

---

## 5. 截图清单

自动产出(每台机一套):

| 文件 | 拍的是什么 |
| --- | --- |
| `smoke/window.png` | 冷启动后的窗口整窗 —— 有 shell 提示符就是「跑起来了」 |
| `webview2-card.png` | 网页座上的那张卡(Win10 是缺席卡,Win11 是页面/加载失败卡) |
| `no-console-notice.png` | 没有 console 的启动被拒时弹出的消息框 |
| `explorer-icon.png` | 文件夹窗口,超大图标,`folio.exe` 戴着自己的图标 |
| `guest-screen.png` | 宿主端 `vmrun captureScreen` 独立拍的一张(不经过客户机) |

手工补(两台各一次,拍虚机窗口即可):

| 文件 | 拍的是什么 |
| --- | --- |
| `shell-typing.png` | 在窗口里敲了两行命令、有回显 |
| `resize.png` | 把窗口拉大/拉小之后的样子 |
| `properties.png` | `folio.exe` 右键属性 → 详细信息页(版本号、产品名、版权) |
| `refusal-*.png` | Win11 上四个拒绝面各一张 |

**截图纪律**:客户机 100% 缩放、不缩放图片文件。图里不许出现真实姓名、真实路径、真实仓库 ——
虚机里本来就只有 `C:\folio-vm` 和一个叫 `folio` 的账户,这一条基本自动满足,但拍属性页之前看一眼
标题栏。

---

## 6. 受控 panic:现在没有入口,这是现状与建议

### 6.1 查证结果

**release 构建里没有任何受控 panic 的入口。** 两处证据:

- `crates/bt-app/src/hang_watch.rs` —— `BT_HANG_SELFTEST` 被 `#[cfg(debug_assertions)]` 切开,release
  半边的文档原文就是「Release builds do not read `BT_HANG_SELFTEST`」。`DESIGN.md` 也把理由写在明处:
  一个能被环境变量卡死十秒的 `folio.exe` 是一份带说明书的拒绝服务。这条设计是对的,不该改。
- `crates/bt-app/src/cli.rs` 的 `parse` —— 认得的参数只有 `--cwd`、`--profile`、`--`、
  `-Embedding`/`/Embedding`、`--help`/`-h`/`/?`、`--version`。没有任何一个会引发 panic。

### 6.2 建议(只建议,本次不实现)

加一个**隐藏参数 `--panic-selftest`**:

- 不出现在 `--help` 的用法块里(所以叫隐藏);
- 唯一的行为是在窗口起来之前 `panic!` 一句写死的话;
- **不**受任何环境变量控制 —— 环境变量能被别的进程设置,一个命令行参数只能由启动它的人打出来,
  这正是 `BT_HANG_SELFTEST` 当年被切掉 release 半边的那条理由的反面;
- 于是 `install_panic_log_hook` 的整条链路(写 `%TEMP%\folio-panic.log` → 交回上一个 hook →
  `announce_panic` 弹框)在 release 上第一次变成可验证的。

代价:一个参数、一行 panic、一条测试。收益:门 5 这一格从「靠推理」变成「拍得到」。

### 6.3 在它存在之前,门 5 这一格怎么填

`in-guest.ps1` 的 `notice` 阶段做的是**同一条通道、不同的触发**,并且把这件事写在证据里:

- 先跑一次 `folio.exe --panic-selftest`。**预期退出码 2**(未知参数)—— 这条不是摆设,它是把
  「本构建没有受控 panic 入口」变成一句被检验过的话;哪天真加了这个参数,这一行会自己变红,提醒
  改写这个阶段。
- 再用一个指向 `folio.exe --panic-selftest` 的**快捷方式**,经资源管理器启动。资源管理器不带
  console,`bt_platform::write_to_console` 在那里答 `false`,于是走 `message_box` —— 和
  `announce_panic` 崩溃时用的是同一个调用、同样两个出口、同样的先后顺序
  (`main.rs` 的 `announce_panic` 与 `report_at_the_front_door` 并排写着这条规矩)。
- 拍下那个框:框上有产品名、有那句话、有 `--version` 打印的同一行 build 标识。

**这证明的是「双击启动的 Folio 出事时不会一声不吭地消失」,不是「一次真 panic 长这样」。** 证据文件
`notice.txt` 自己就是这么写的,别在回填时把话说大。

真想在虚机上手工制造一次崩溃,可试(**全部未验证**,当成探索而不是清单):把 `conpty.dll` 从
目录里挪走、把 `%APPDATA%\Folio\session.json` 换成一段乱字节、把 `folio.exe` 拷到只读目录再跑。
如果哪一条真的崩了,那既是这一格的证据,**也是一个 bug**,得先修再发。

---

## 7. 结果怎么回填 `plan.md` 门 5

### 7.1 证据入库

`run-smoke-in-vm.ps1` 把东西放在 `target/cleanvm/`(不入库)。要入库的那几张,拷到:

```
docs/plans/release/clean-vm-evidence/
    win10-machine.txt          win11-machine.txt
    win10-smoke.log            win11-smoke.log
    win10-webview2-card.png    win11-webview2-card.png
    win10-no-console-notice.png
    win10-explorer-icon.png
    ...
```

**`.gitignore` 需要加一行,本工具包没有加**(本次任务的文件白名单里没有它):

```
!docs/plans/release/clean-vm-evidence/*.png
```

仓库根 `.gitignore` 第 6 行是 `*.png`,后面跟着一串 `!` 例外;新证据目录照那个样子添一行即可。
`.txt` 和 `.log` 不受影响,直接就能提交。

这些截图**不进** `docs/screenshots/`:那个目录是 README 用的产品照,有 1600×1000 的尺寸红门
(`scripts/check-screenshots.ps1`)和一张清单表,虚机证据混进去会把那道门弄红。

### 7.2 `plan.md` 门 5 改成什么样

跑完之后,把 `plan.md` 的门 5 段落换成结论 + 指针,形如:

```markdown
## 门 5 干净机(必须,发布前最后一道)
**已过 / 未过**(日期)。两台无 VS/VC redist 的 VM,从 `folio-0.1.0-windows-x64.zip` 开始:

- Win10 22H2 Pro(build 19045.xxxx,无 WebView2):冒烟七条全绿,缺席卡如
  `clean-vm-evidence/win10-webview2-card.png`。
- Win11 25H2 Enterprise Eval(build 26xxx,自带 WebView2 x.y.z):冒烟七条全绿,
  顶层 gate 与四个拒绝面见 `clean-vm-evidence/win11-refusal-*.png`。
- 受控 panic:release 无入口,本轮验的是同通道的可见提示
  (`clean-vm-evidence/win10-no-console-notice.png`);建议的 `--panic-selftest`
  见 `clean-vm.md` §6,**挂账到 v0.2**(或本版补,待裁)。
- 手册与工具:`docs/plans/release/clean-vm.md`、`scripts/release/cleanvm/`。
```

**一条红了就整门红**,别在门 5 里写「基本通过」。

---

## 8. 已知空白与未验证项(一处列全)

| 项 | 状态 |
| --- | --- |
| 两份 `autounattend-*.xml` | **未验证** —— 照微软 unattend 参考写,没对着 ISO 跑过 |
| Win10 多版本 ISO 里 `Windows 10 Pro` 这个映像名 | **未验证** —— 装之前用 `dism /Get-ImageInfo` 核 |
| Win10 ISO 页面在 Windows 浏览器上会被导去 MCT / MCT 自 2025-10 起造不了介质 | **未验证** —— 二手报道,不是微软页面原话 |
| Visual Studio 订阅仍有 Win10 企业版镜像 | **未验证** —— 二手讨论 |
| Workstation 17 「快速加密」的性能影响 | **未验证** —— 只确认了该选项存在 |
| 四个拒绝面能否都用 `BT_WEB_DEV` 触发 | **未验证** —— 到虚机上逐条试 |
| `vmrun captureScreen` 在 `nogui` 启动下是否照样出图 | **未验证** —— 脚本里这一步标了 `-Tolerant`,失败不中断整轮 |
| release 里手工制造真 panic 的办法 | **未验证** —— §6.3 列的三条是探索方向 |
| 已验证:Win11 企业评估页 / Win10 评估页已下架 / evalcenter 客户端列表 / Win10 ISO 页 | 2026-08-27 逐页抓取 |
| 已验证:宿主 Workstation 17.6.4 + vmrun 1.17.0 路径 | 本机探测 |
| 已验证:`new-answer-iso.ps1` 能造出可挂载、内容正确的 ISO | 本机跑过并挂载核对 |
| 已验证:三个 `.ps1` 语法、两个 `.xml` 格式、`run-smoke-in-vm.ps1` 的 `-WhatIf` 全流程 | 本机跑过 |

### 参考链接(全部 2026-08-27 抓取)

- Windows 11 企业版评估:<https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise>
- 评估中心首页:<https://www.microsoft.com/en-us/evalcenter/>
- Windows 10 ISO 下载页:<https://www.microsoft.com/en-us/software-download/windows10ISO>
- Windows Setup 自动化总览(隐式应答文件搜索顺序):
  <https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/windows-setup-automation-overview>
- 自动化 Windows Setup:
  <https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/automate-windows-setup>
- VMware 上装 Windows 11 客户机(TPM/UEFI/加密):
  <https://knowledge.broadcom.com/external/article/315650/installing-windows-11-as-a-guest-os-on-v.html>
- WebView2 分发(Win11 预装 / Win10 推送):
  <https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution>
- 向 Windows 10 消费者投递 WebView2 运行时:
  <https://blogs.windows.com/msedgedev/2022/06/27/delivering-the-microsoft-edge-webview2-runtime-to-windows-10-consumers/>
- Microsoft Edge Update 策略文档:
  <https://learn.microsoft.com/en-us/deployedge/microsoft-edge-update-policies>
