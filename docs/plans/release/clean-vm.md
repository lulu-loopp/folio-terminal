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
| `new-install-iso.ps1` | 把安装 ISO 重制成 EFI 免提示引导的那一张(§3.1a) |
| `new-vm.ps1` | **建机**:写 `.vmx`、建盘、装 Windows、打 `clean` 快照(§3.5) |
| `run-smoke-in-vm.ps1` | 宿主端驱动:回快照 → 开机 → 拷进去 → 跑 → 拷回来 → 关机 |
| `in-guest.ps1` | 客户机端五个阶段:unpack / smoke / web / notice / explorer |
| `guest-files.ps1` | 拷进客户机的清单从脚本自己的文字里读出(`smoke.ps1` 点源的文件及其旁边的文件),并在宿主上验 BOM、用 5.1 解析 |
| `updater/` | 更新器的断电验收(§4.4):装候选版、逐行驱动、客户机端监视/按键/收集、macOS 三个脚本 |

ISO 由用户下,**虚机由 `new-vm.ps1` 建**(Win11 的 vTPM 除外,见 §2.2a)。

> **写这份文档的那一轮没有启动过任何虚机**,所以下面凡是标「未验证」的都是那一轮的状态。
> 2026-08-27 晚建机时验掉了一批,最要紧的一条是 §3.1a:安装 ISO 必须重制成 EFI 免提示引导,
> 否则两台机会在「按任意键从光盘引导」上白等到超时。§8 的表按实际结果更新过。

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
| 内存 | 8 GB(`new-vm.ps1` 默认;下限 4) | 4 GB(`-Memory 4096`) |
| 磁盘 | 64 GB,拆分成多个文件,**不**预分配 | 60 GB,同上 |
| 固件 | **UEFI + Secure Boot** | UEFI(Secure Boot 开关随意) |
| TPM | **加 vTPM(见 2.2、2.2a)** | 不加 |
| 显示 | 关掉「自动调整客户机分辨率」,固定 **1920×1080**,缩放 100% | 同 |
| 网络 | NAT,**连接**(要验「有引擎」路径) | **没有网卡**(`ethernet0.present = "FALSE"`,见 2.4) |
| 声卡 / 打印机 / USB | 全部移除 | 全部移除 |
| VMware Tools | **必装**(应答文件自己装,见 2.3、3.5.4) | **必装**(同) |
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

Workstation 17 的「快速加密」只加密支撑 TPM 所需的部分文件,性能影响可忽略(Broadcom 官方文档
<https://techdocs.broadcom.com/us/en/vmware-cis/desktop-hypervisors/workstation-pro/17-0/using-vmware-workstation-pro/configuring-and-managing-virtual-machines/encrypting-and-restricting-virtual-machines-ws-guide.html>,
2026-08-27 抓取原文:两个选项分别是「Fast VM Encryption (Only the files needed to support a TPM are
encrypted)」与全盘加密,并写着「Workstation Pro does not provide a way to retrieve the passwords if
you lose them」;**具体性能数字未验证**)。

### 2.2a 免加密 vTPM(`managedvm.autoAddVTPM`)核实结果:**不走这条路**

网上流传的一行 `managedvm.autoAddVTPM = "software"` 号称能免整机加密拿到 vTPM。**2026-08-27 逐条
核实,结论是本门不用它,默认不写。** 核实结果:

| 问的是什么 | 结果 | 来源(2026-08-27 抓取) |
| --- | --- | --- |
| 键名与值 | `managedvm.autoAddVTPM = "software"`,拼写属实 | <https://www.vimalin.com/blog/what-you-should-know-about-vmwares-experimental-vtpm/> |
| 最低版本 | Workstation **16.2** / Fusion 12.2 起 | 同上 |
| **官方文档有没有这个键** | **没有**。Workstation Pro 17 的加密文档整页不提 vTPM,也不提这个键 | 上面那条 Broadcom 加密文档 |
| 是否真的「免加密」 | 否。磁盘不加密,但 `.vmsd` / `.nvram` / `.vmsn` / `.vmss` / `.vmem` / vmdk 描述符都被加密,**口令是 Workstation 自己生成的,机主没有** | vimalin 同上 |
| **`vmrun` 能不能开这台机** | **报告说不能**:`vmrun -T ws start` 答 `The operation is not supported`;VM 被标成 encrypted 而口令没人知道,`-vp` 也就无从谈起 | <https://communities.vmware.com/t5/VMware-Workstation-Pro/How-to-1-click-start-VMs-with-16-2-TPM/m-p/2927197>(经检索确认该线程内容,页面本身已 301 到 community.broadcom.com);vimalin 同上 |
| 加密后 `-vp` 走正常口令能不能开 | 能:`vmrun -vp <口令> start <vmx> nogui` | <https://communities.vmware.com/t5/VMware-Workstation-Pro/Issue-vmrun-Running-with-vp-encryptedVirtualMachinePassword/td-p/2893108> |

**最后一行是判决理由。** 门 5 从头到尾是 `vmrun`:回快照、开机、拷文件、跑五个阶段、拷回来。一台
`vmrun` 开不了的机器不是这道门能用的机器。所以:

- **默认(`-VTpm encrypted`,Win11 自动选中)**:走 §2.2 的正路 —— 自选口令加密 + 加 TPM 设备,
  `run-smoke-in-vm.ps1 -VmPassword` 早就是为它写的。代价是加密和加设备**没有命令行**(本机
  `vmcli` 的模块只有 Chipset / ConfigParams / Disk / Ethernet / Guest / HGFS / MKS / Nvme / Power /
  Sata / Serial / Snapshot / Tools / VM / VMTemplate / VProbes,没有一个碰加密),于是 Win11 机在
  建完 `.vmx` 之后停一次,让人点三下,再 `-Stage install` 接上。
- **`-VTpm software`**:照写那一行,给愿意在 17.6.4 上亲自验一次「`vmrun start` 是不是还坏」的人
  留的。**本机没有验过**——本次任务不建真虚机。
- **`-VTpm none`**:不加任何 TPM。Win11 Setup 会拒,留着是为了别人想验别的东西时不必改脚本。

> 上面那条 vimalin 还提到一件跟本门无关但值得记的事:加这个 vTPM 前会做一次「磁盘剩余空间够不够
> 整个虚拟盘大小」的检查,空间不够时 vTPM 只加了一半,虚机就坏了。**未验证**。

### 2.3 VMware Tools 必须装,而且它是唯一允许装的东西

`vmrun` 的 **guest 操作全部依赖 Tools**:`copyFileFromHostToGuest`、`copyFileFromGuestToHost`、
`runProgramInGuest`、`fileExistsInGuest`、`getGuestIPAddress`、`captureScreen` 都在 `vmrun` 自己的
用法表里挂在 **GUEST OS COMMANDS** 下,全是发给虚机内 Tools 服务的请求(本机 `vmrun` 1.17.0 的用法
原文,2026-08-27 打印核对)。没有 Tools,这套脚本只能开机关机,别的一件都干不成。

`checkToolsState` 和 `installTools` 挂在 **GENERAL COMMANDS** 下 —— 前者能在没有 Tools 的机器上答
「没有」,所以它能当进度指示,**不能**当装机完成的判据。

**Tools 现在由应答文件自己装**(§3.5):`new-vm.ps1` 把 Workstation 安装目录里的 `windows.iso` 挂成
第三张光盘,`FirstLogonCommands` 找到它、静默装、重启。人不用碰。

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

1. **根本没有网卡**。`new-vm.ps1` 给 Win10 机写的是 `ethernet0.present = "FALSE"`,比取消「已连接」
   两个勾更硬:没有的东西点不回来。guest 操作走 VMCI 后门不走网络,这台机照样能被 `vmrun` 驱动。
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

### 3.1a EFI 无人值守必须免提示引导 —— 2026-08-27 事故

**这一节是用两台机白跑两个半小时换来的,放在这里是因为它比应答文件更早决定成败。**

#### 事故

两台机(`D:\VMs\folio-cleanvm\folio-win10`、`folio-win11`)开机后一直没进安装。`vmware.log`
从第一分钟起就写着答案:

```
2026-08-27T18:17:46.520Z vcpu-0 Guest: About to do EFI boot: EFI VMware Virtual SATA CDROM Drive (0.0)
2026-08-27T18:17:49.606Z vcpu-0 Guest: Status upon boot failure: Time out
2026-08-27T18:17:49.623Z vcpu-0 Guest: Status upon boot failure: No Media
2026-08-27T18:17:49.644Z vcpu-0 [msg.Backdoor.OsNotFound] No operating system was found.
```

两行之间三秒。那三秒是 `efi\microsoft\boot\efisys.bin` 里的 `cdboot.efi` 在画
「Press any key to boot from CD or DVD」等人按键。无人值守的意思正是没人按。等不到就放弃光盘,
固件落到空盘上,于是「没有操作系统」。

#### 证据链(全部本机 2026-08-27 实测)

| 问的是什么 | 怎么问的 | 答案 |
| --- | --- | --- |
| 微软盘上是不是有免提示的那张 | 列 ISO 目录 | 有:`efi\microsoft\boot\efisys_noprompt.bin`,与 `efisys.bin` 同为 1,474,560 字节,内容不同 |
| 原盘引导项指的是哪一张 | 读 El Torito 引导目录,把它指向的块地址上的 1,474,560 字节做 SHA256 | 两张盘都等于各自的 `efisys.bin` —— 就是会提示的那一张 |
| 免提示那张真的不提示吗 | 拿同一套内容做两张只有引导映像不同的小 ISO,分别开机 | `efisys.bin` 那张:3.2 秒后 `Status upon boot failure: Time out`;`efisys_noprompt.bin` 那张:0.18 秒后 `Not Found`(盘上本来就没有系统)。**「等三秒」这件事消失了** |
| 重制后的整盘能不能引导 | 两台机各挂重制盘开机 | `About to do EFI boot` 之后不再有 `Time out` / `No Media` / `OsNotFound`,直接进 Setup |

#### 解法

`new-install-iso.ps1`:取出 ISO 内容,用 IMAPI2 重建一张一模一样但**引导项指向
`efisys_noprompt.bin`** 的盘,落在 `target/cleanvm/<原名>-noprompt.iso`。

```powershell
pwsh -File scripts/release/cleanvm/new-install-iso.ps1 -Iso D:\iso\Win10_22H2.iso
```

`new-vm.ps1` 的 build 阶段自己会调它,所以平时不必单独跑。它有两种「什么都不做」:盘本来就免提示
(读引导目录跟盘上自带的 `efisys_noprompt.bin` 比一次哈希),或者这个源做过的重制盘还在
(输出旁边有一行记着源的名字、大小、写入时间)。两种情况都直接把该用的路径交出来。

**本机实测(2026-08-27):**

| 盘 | 源大小 | 重制耗时 | 重制后大小 |
| --- | --- | --- | --- |
| `Win10_22H2.iso` | 4,893,900,800 | 21 秒(取出 6 秒 / 写出 3 秒) | 4,893,900,800(100.0%) |
| Win11 25H2 企业评估版 | 7,092,807,680 | 20 秒(取出 10 秒 / 写出 5 秒) | 7,093,157,888(100.0%) |

#### 免提示引导带出来的第二件事:**光驱不能排在硬盘前面**

这一条是重制盘装到一半才暴露的,写在这里因为它和上面是同一件事的两面。

一直以来光驱能排第一而不出事,靠的正是那个「按任意键」:第二次开机没人按,提示超时,固件就落到
Setup 刚写好的硬盘上。**把提示拿掉,这条退路也一起没了。** `bios.bootOrder = "cdrom,hdd"` 配免提示
盘的结果是每次开机都从光盘走,Setup 第一次重启又回到 Setup:

```
Windows Setup
It looks like you started an upgrade and booted from installation media.
If you want to continue with the upgrade, remove the media from your PC and click Yes.
```

Win11 那台 2026-08-27 就卡在这个框上四十分钟:映像已经铺完(`.vmdk` 涨到 14.16 GB)、机器重启、
然后从光盘又进了一次 Setup。

**所以引导顺序装机时也得是 `hdd,cdrom`。** 第一次开机盘是空的,固件答 `No Media` 之后自己往下走到
光驱;本机日志逐字是:

```
Guest: Status upon boot failure: No Media
Guest: Status upon boot failure: No Media
Guest: About to do EFI boot: EFI VMware Virtual SATA CDROM Drive (0.0)
```

之后每次开机硬盘上都有 Windows Boot Manager,硬盘赢。`new-vm.ps1` 现在两个阶段都写 `hdd,cdrom`。

#### 几个当时踩到的点,写下来免得再踩

- **取出内容不能靠 `Mount-DiskImage`**:本机它报「需要提权」。一个用来准备「干净机」的脚本要管理员
  才能跑本身就荒唐。脚本按顺序找 `7z`(本机 `C:\Program Files\7-Zip\7z.exe`),再找带 `pycdlib`
  的 Python;两个都没有就把两条安装命令和 `oscdimg` 的等价写法一起打出来。**`pycdlib` 那条本机
  没有走过,未验证。**
- **文件系统只能是 UDF。** Win11 的 `sources\install.wim` 有 6,225,286,811 字节,ISO9660 装不下。
  这也正是微软自己的做法:那张原盘的 ISO9660 目录树里只有一个 `README.TXT`,真内容全在 UDF 上。
- **`Emulation` 必须在 `AssignBootImage` 之后设。** `AssignBootImage` 看见 1,474,560 字节就自己判成
  1.44 MB 软盘仿真;先设的 `Emulation = 0` 会被它覆盖,引导目录写出去是 media type 2。顺序对了才
  是 media type 0 + 2880 扇区,跟微软原盘一致。
- **IMAPI2 不认长路径。** 超过 259 字符它只答一句「找不到指定的路径」,不说是哪个文件。Win11 盘里
  `sources\replacementmanifests\` 下有 148 字符的相对路径,而那张 ISO 的文件名本身就有 91 字符 ——
  以它命名的暂存目录直接越线。脚本因此把暂存目录起名 `staging-<源路径哈希前八位>`,并在展开后
  自己量一遍最长路径,超了就报出是哪个文件。
- **`BootImageOptionsArray` 在 PowerShell 里设不进去**:`IFileSystemImage3` 的这个属性要
  `SAFEARRAY(IUnknown*)`,.NET 送过去的是 `SAFEARRAY(VARIANT)`,一律回 `E_NOINTERFACE`(直接赋值、
  `InvokeMember`、按读回来的元素类型建数组,三种都试过)。所以脚本用单数的 `BootImageOptions`,
  写出来的是「校验项平台 0xEF + 默认项就是 EFI 映像」,而不是原盘那种「默认项 BIOS + 分节头
  0xEF」。**这一形态在本机 VMware EFI 固件(VMW201.00V.24006586)上实测能引导** —— 上表第三行的
  两张小 ISO 就是为验它做的。

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
D:\...\target\cleanvm\autounattend-win10.iso — 65,536 bytes
attach it to the VM as a second CD/DVD device, connected at power on.
```

挂载回来验过:根目录一个 `Autounattend.xml`(12,689 字节,大小写正确),占位符全部替换掉,密码两处
(本地账户与自动登录)都填上了,§3.5.4 新加的三条 `SynchronousCommand` 逐字原样(引号、`>` 重定向
都没被 XML 转义吃掉)。

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

**第三处,2026-08-27 第一次真装才发现的:`<ProductKey>` 里必须有 `<Key>`。**

Win10 那台在装机第一分钟就停在一个消息框上,半小时没动,磁盘一个字节都没写(`.vmdk` 还是初始的
8.5 MB,说明 windowsPE 那一趟连 `DiskConfiguration` 都没走到):

```
Windows Setup
  Windows cannot read the <ProductKey> setting from the unattend answer file.
```

原来写的是只有 `<WillShowUI>Never</WillShowUI>` 而没有 `<Key>` 的 `<ProductKey>`,Setup 认不出来。
补一个**空的** `<Key></Key>` 就过了 —— 空元素才是「明确地不给密钥」。Win11 那份没有 `<Key>` 也照样
装,因为评估版镜像本来就不问密钥;所以两份文件在这一处不同,而且应该继续不同。真要给密钥,微软
公开的 Windows 10 Pro 版本选择用 KMS client setup key 是 `W269N-WFGWX-YVC9B-4J6C9-T83GX`,它只选
版本,不激活任何东西。

**第四处,同一天再发现的:OOBE 的「地区」页不归 `<OOBE>` 管。**

两台机在 Setup 全程静默之后,双双停在 OOBE 第一页:

```
Let's start with region. Is this right?          (Win10)
Is this the right country or region?             (Win11)
```

`<OOBE>` 里那一串 `Hide…Page` 管不到它。管它的是 **`oobeSystem` 趟里的
`Microsoft-Windows-International-Core`** 组件(`InputLocale` / `SystemLocale` / `UILanguage` /
`UserLocale`)。原来两份文件的 `oobeSystem` 里只有 `Microsoft-Windows-Shell-Setup`。

注意别和 `windowsPE` 趟里那个名字几乎一样的
`Microsoft-Windows-International-Core-**WinPE**` 搞混:后者只替 Setup 自己回答,前者替 Setup 装出来
的那台机器回答。两个都要有。

**教训:装机卡住先看屏幕。** 没有 Tools 的机器 `vmrun captureScreen` 用不了(它要 guest 登录),
但 `vmware.exe <vmx>` 能把正在跑的机器挂进 Workstation 窗口,再对那个窗口截图就看见了。这一步比
任何推理都快 —— 上面这四条里有三条是一张截图当场定的案,而在有截图之前,同样的问题已经猜了半小时。

**两份应答文件原本都标着「未验证」**:它们是照微软 unattend 参考写的。2026-08-27 是它们的第一次
运行,上面这一条就是那次跑出来的。第一次装如果卡在某一页,截图记下卡在哪一页,回来改对应的一段
—— 别改一堆。

### 3.5 建机:用户只提供 ISO,其余 `new-vm.ps1` 做

原来这里写着「建机三步(留给用户的操作)」:在 Workstation 向导里建机、挂两张盘看着装、手工装
Tools 再打快照。**那三步现在是 `scripts/release/cleanvm/new-vm.ps1`。** 留给人的只剩把 ISO 下下来,
告诉脚本它在哪。

#### 3.5.1 两条命令

```powershell
# Win10 机:一条命令从零到 clean 快照,中间不用管
pwsh -File scripts/release/cleanvm/new-vm.ps1 -Name win10 `
    -Iso D:\iso\Win10_22H2_English_x64.iso -Memory 4096 -DiskGB 60

# Win11 机:建完 .vmx 停一次(vTPM,见 §2.2a),点三下,再接上
pwsh -File scripts/release/cleanvm/new-vm.ps1 -Name win11 `
    -Iso D:\iso\Win11_25H2_EnterpriseEval_x64.iso
#   → 按屏幕上打出来的三步加密 + 加 TPM,然后:
pwsh -File scripts/release/cleanvm/new-vm.ps1 -Name win11 -Stage install -VmPassword <加密口令>
```

**给已经建好的机换安装盘**:`-Stage install` 也收 `-Iso`,它改写 `.vmx` 的 `sata0:0.fileName`、
把光驱设成开机连接、引导顺序设回 `cdrom,hdd`,然后照常装。§3.1a 那次事故就是这么收的场 ——
两台机不用重建:

```powershell
pwsh -File scripts/release/cleanvm/new-vm.ps1 -Name win10 -Stage install `
    -Iso D:\Developer\BetterTerminal\target\cleanvm\Win10_22H2-noprompt.iso -InstallTimeoutMinutes 120
```

**Win11 的 `.vmx` 是 partial 加密的,手改明文键照样认。** 那台机的文件里有
`vmx.encryptionType = "partial"` 和 `encryption.keySafe` / `encryption.data` 两行,别的键都是明文。
在副本上改掉 `sata0:0.fileName` 之后 `vmrun -T ws -vp <口令> listSnapshots` 照答不误,而同一条命令
换个错口令仍然答 `Incorrect password` —— 说明文件还是真在被解密,不是被放行。真机上开机后
`vmware.log` 的 `DICT sata0:0.fileName` 与 `CDROM: Connecting sata0:0 to …` 两行也印着改后的路径。
(本机 2026-08-27 实测。)

**先带 `-WhatIf` 跑一遍**:它把每一步和**将要写入的 `.vmx` 全文**打出来,一个文件都不建,ISO 都不
必存在。

参数:`-Name win11|win10`、`-Iso`、`-VmRoot`(默认 `D:\VMs\folio-cleanvm`)、`-Memory 8192`、
`-Cpus 2`、`-DiskGB 64`、`-Stage build|install|all`、`-VTpm auto|none|software|encrypted`、
`-VmPassword`、`-GuestUser/-GuestPassword`(默认 `folio`/`folio`)、`-AnswerIso`、`-ForceAnswerIso`、
`-ToolsIso`、`-HardwareVersion 21`、`-InstallTimeoutMinutes 90`、`-KeepMedia`、`-SkipImageCheck`、
`-VmrunPath`、`-VdiskManagerPath`、`-WhatIf`。

**幂等靠拒绝,不靠覆盖**:`<VmRoot>\folio-<name>` 已存在时 build 阶段直接报错退出,提示要么删目录
要么改 `-VmRoot` 要么 `-Stage install` 接着已有的那台。一次半覆盖比不覆盖坏得多。

#### 3.5.2 脚本做了什么

1. **找工具**:`vmrun.exe`、`vmware-vdiskmanager.exe`、`windows.iso`,三者都按 `run-smoke-in-vm.ps1`
   同一套办法探测(Workstation 目录 → Player 目录 → 注册表 `InstallPath` → `PATH`)。
2. **核 ISO 的映像名**:挂载安装 ISO,`Get-WindowsImage` 读 `sources\install.wim`/`install.esd`,和
   应答文件里的 `/IMAGE/NAME`(Win11 `Windows 11 Enterprise Evaluation`、Win10 `Windows 10 Pro`)
   比一次。对不上就**当场停**,并把 ISO 里真实有的版本名列出来 —— 这一条挡的是最贵的一种失败:名字
   写错时 Setup 不报错,它画出选版本那一页等一个不存在的人,而脚本要轮询九十分钟才发现。挂载 ISO
   要管理员权限,不是管理员就打一行「没核成」继续走(`-SkipImageCheck` 关掉它)。
3. **重制安装盘**:调 `new-install-iso.ps1`(§3.1a),把 `sata0:0` 指向免提示引导的那一张。盘本来
   就免提示、或这个源做过的重制盘还在,它就什么都不做。
4. **应答 ISO**:`target/cleanvm/autounattend-<name>.iso` 不存在就调 `new-answer-iso.ps1` 现做
   (改过 `.xml` 之后用 `-ForceAnswerIso` 重做)。
5. **建盘**:`vmware-vdiskmanager -c -s <N>GB -a nvme -t 1 <vmdk>`。`-t 1` = 可增长且按 2 GB 拆文件。
6. **写 `.vmx`**:见 3.5.3。
7. **开机 + 轮询 + 快照**:`vmrun start … nogui` → 轮询哨兵 → `stop soft` → 等它离开 `vmrun list` →
   把三个光驱在 `.vmx` 里改成「开机不连接」、引导顺序改回 `hdd,cdrom` → `vmrun snapshot <vmx> clean`。

**快照是关机态的,而且必须是。** `run-smoke-in-vm.ps1` 的第一步是 `revertToSnapshot` 紧接着 `start`;
回到一个「运行中」的快照之后再 `start`,`vmrun` 是报错而不是无操作。

**光驱要断开**,否则每次回快照都要在「press any key to boot from CD」上耗掉十秒,而且这台「干净机」
的快照里永远插着一张微软安装盘。`-KeepMedia` 保留它们。

#### 3.5.3 `.vmx` 里的关键几行

全文用 `-WhatIf` 看。这里只说要解释的:

| 键 | 值 | 为什么 |
| --- | --- | --- |
| `virtualHW.version` | `21` | **本机实测**:17.6.4 的 `vmcli VM Create` 自己写的就是 21。`-HardwareVersion 20` 是要在更老的 Workstation 上开这台机时的退路 |
| `guestOS` | `windows11-64` / **`windows9-64`** | **这一条最容易写错**:Workstation **没有** `windows10-64`。本机 `vmcli VM Create -g windows10-64` 直接报「Invalid argument」并列出合法值,里面是 `windows9-64`;安装目录的 `isoimages_manifest.txt` 也只有 `windows9-64` 和 `windows11-64` 两条映射到 `windows.iso` |
| `firmware` / `uefi.secureBoot.enabled` | `efi` / `TRUE` | 两台机都是,§2.1 |
| `bios.bootOrder` | **两个阶段都是 `hdd,cdrom`** | 键名**未经官方文档核实**(第三方引用一致,EFI 下也拼 `bios.`)。但行为已实测:免提示盘配 `cdrom,hdd` 会让 Setup 每次重启又回到 Setup(§3.1a 第二件事);空盘答 `No Media`,固件自己落到光驱 |
| `nvme0:0` | 系统盘 | Win10/Win11 都自带 NVMe 内置驱动,Setup 不用加载任何东西就能看见盘;Workstation 给 Win11 客户机默认也是 NVMe |
| `sata0:0` / `sata0:1` / `sata0:2` | **免提示重制的**安装 ISO(§3.1a)/ 应答 ISO / **Tools ISO** | 三张,都 `present = TRUE` + `startConnected = TRUE`。第三张是 §3.5.4 的关键 |
| `ethernet0.virtualDev` | `e1000e` | 内置驱动,Tools 装之前就有网卡;而且 Broadcom 那条静默安装的注意事项警告 `REBOOT=R` 在 vmxnet3 上会当场断网 |
| `ethernet0.present`(Win10) | `FALSE` | 比 §2.4 的「取消勾选已连接」更硬:根本没有网卡,谁也点不回来。guest 操作走 VMCI 后门不走网络,所以这台机照样能被 `vmrun` 驱动 |
| `svga.autodetect` `svga.maxWidth` `svga.maxHeight` `svga.vramSize` | `FALSE` / `1920` / `1080` / `16777216` | 固定 1920×1080。1920×1080×4 = 8,294,400 < 16 MB,且 16777216 能被 65536 整除(Windows 客户机的要求)。键名见 Broadcom KB 313896 <https://knowledge.broadcom.com/external/article/313896/adding-video-resolution-modes-to-windows.html>(2026-08-27 抓取) |
| `mks.enable3d` | **`TRUE`** | Folio 用 GPU 画;关掉 3D 得到的证据是关于产品不走的那条路的 |
| `sound/usb/ehci/usb_xhci/serial0/parallel0/floppy0` | 全 `FALSE` | §2.1 「声卡打印机 USB 全部移除」 |
| `isolation.tools.*` | 全 `FALSE`(即默认:不隔离) | 写出来是为了文件自己说清这台机允许什么 |
| `msg.autoAnswer` | `TRUE` | 无头启动的前提:ISO 挪过位置之类的问题不能变成没人看的模态框 |
| `uuid.action` | `create` | 第一次开机那句「moved or copied?」当场有答案 |
| `pciBridge*` / `hpet0` / `svga.present` / `vmci0` | 照抄 | 本机 `vmcli` 给新机器写的就是这些 |

#### 3.5.4 「装完了没有」怎么知道:文件哨兵 + 让客户机自己装 Tools

这是这个脚本唯一一处需要设计的地方。

**先说走不通的:** `vmrun` 的 guest 命令全部依赖 Tools(§2.3),装机阶段没有 Tools,所以
`fileExistsInGuest`、`listProcessesInGuest`、`getGuestIPAddress` 一个都问不出来。`checkToolsState`
不依赖 Tools,但它只会一直答「没有」,答不出「Windows 装完了」。

**走通的这条:让客户机自己装 Tools,再自己说一声。**

1. `new-vm.ps1` 把 Workstation 安装目录里的 `windows.iso` 挂成 `sata0:2`。**本机实测**:
   `C:\Program Files (x86)\VMware\VMware Workstation\windows.iso`,113,586,176 字节,根目录是
   `setup.exe`(111 MB)、`VMwareToolsUpgrader.exe`、`manifest.txt`(`monolithic.version = "12.5.3"`)、
   `autorun.inf`、`autorun.ico`、`certified.txt`、`Program Files\`。**注意没有 `setup64.exe`** ——
   17.6.4 的这张盘上只有一个 `setup.exe`。
2. 两份应答文件的 `FirstLogonCommands` 加了三条(本次改动):
   - **Order 3** 找到那张盘并静默装:
     ```
     cmd /c for %d in (D E F … Z) do @if exist %d:\VMwareToolsUpgrader.exe start /wait %d:\setup.exe /S /v"/qn REBOOT=R"
     ```
     认盘认的是 `VMwareToolsUpgrader.exe` 而不是 `setup.exe` —— **微软安装 ISO 的根目录也有一个
     `setup.exe`**,认错了就是把 Windows Setup 又开一遍。开关是 Broadcom 文档的原话
     `setup.exe /S /v"/qn REBOOT=R"`(<https://knowledge.broadcom.com/external/article/376237/installing-vmware-tools-on-a-vmware-vsph.html>,
     2026-08-27 抓取;`REBOOT=R` 压掉安装器自己的重启,好让后面两条还能跑)。
   - **Order 4** 用 `RunOnce` 预约哨兵:重启之后才写 `C:\folio-vm\oobe-done.txt`。**顺序是要紧的**
     —— 哨兵要是在重启之前就有了,宿主会在机器正往下关的时候以为它好了。
   - **Order 5** `shutdown /r /t 20`。这次重启才是把 Tools 驱动装到位的那一下。
3. 宿主端轮询 `vmrun -gu -gp fileExistsInGuest <vmx> C:\folio-vm\oobe-done.txt`,15 秒一次。
   **这一条问的其实是两件事**:能问出答案说明 Tools 起来了,答案是「有」说明装机走完了。一个动作
   两个事实。每分钟顺带打一行 `checkToolsState` 当进度。
4. 超时(默认 90 分钟)不是一句「失败」:它打出 `checkToolsState` 的最后回答、`vmrun captureScreen`
   的命令,以及三种典型现场各自意味着什么(卡在 Setup 某一页 / 有桌面没 Tools / 有 Tools 没哨兵)。

**Tools 是这台机上唯一允许装的东西**,§2.3 已经说过理由,`in-guest.ps1` 的 `unpack` 阶段会把它写进
`machine.txt`,谁看证据都知道。

#### 3.5.5 建完之后

脚本最后自己打出来:`.vmx` 路径、`listSnapshots` 的结果,和下一步

```powershell
pwsh -File scripts/release/package.ps1
pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx <打出来的路径> -WhatIf
pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx <打出来的路径>
```

快照名固定是 **`clean`** —— `run-smoke-in-vm.ps1` 的默认值,也是这台机唯一该有的那个快照。

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

### 4.4 更新器(0.4.6 起)

自更新的断电验收:给虚机装上一个有更新能力的签名候选版 A,让它从本地源下载并应用第二个
候选版 B,在每一个持久化边界上硬断电(或持有一个文件),观察重启后下一位操作者做了什么。
`self-update-2026-09-16.md` (b).2 的 W 表(Windows)与 M 表(macOS)定义了每一行的持久状态、
磁盘内容和下一位操作者的动作;下面两张表的「预期」一栏写的是**已发布的规则**(与 (b).2 原文
不同的地方逐行注明)。

驱动工具在 `scripts/release/cleanvm/updater/`(2026-09-29 起;此前的 `hard-reset-in-vm.ps1`
与 `in-guest-updater.ps1` 已删除,见本节末「这套脚本从哪来」):

| 文件 | 干什么 |
| --- | --- |
| `install-candidate.ps1` | 回 `clean` → 拷入 A 的 zip 与 `in-guest.ps1` → 只跑 `unpack` 阶段 → 关机 → 打快照(如 `a-installed`)。Folio 不启动 |
| `run-row.ps1` | 宿主端驱动:对 `-Row` 的每一行回快照 → 开机 → 拷入客户机脚本与源 → 起监视与按键 → 断电或定时收集 → 记录下一位操作者 |
| `rows.ps1` | 行表(数据):每一行的按键计划、监视参数、收集时刻 |
| `vm-door.ps1` | 所有 `vmrun` 调用走的一扇门:找 `vmrun`、`-vp`/`-gp` 打印成 `<hidden>`、等客户机能跑交互程序 |
| `guest/watch.ps1` | 客户机端:盯日志阶段,到达目标行时冻结全部 `folio.exe` 并写哨兵;或按行持有文件 |
| `guest/waits.ps1` | 客户机端:监视器的有界等待(W4 的 Run 值、毫秒级行的自旋、被结束的 trial 退出),每个界限的数值与理由写在文件头 |
| `guest/keys.ps1` | 客户机端:启动 Folio,经 `ui-probe.ps1` 按更新卡的键,每次按键前后截图 |
| `guest/collect.ps1` | 客户机端:安装目录清单(带 SHA-256)、日志、`diagnostics.log`、Run 键、进程,打成 zip |
| `mac/setup.sh`、`mac/row.sh`、`mac/teardown.sh` | macOS 的同一件事(见「macOS 步骤」) |
| `../guest-files.ps1` | 拷进客户机的脚本先在宿主上验 BOM 并用 5.1 解析(§3.4a、§3.4b);`run-smoke-in-vm.ps1` 也用它 |

宿主用的是 VMware Workstation 的 `vmrun`(和 `run-smoke-in-vm.ps1` 同一套探测与调用方式),
不是 Hyper-V。设计笔记 E-7 写的是 Hyper-V;实际的客户机是 VMware,硬断电等价于
`vmrun stop <vmx> hard`(切断虚拟电源,不发 ACPI 关机请求)。**口令只走参数**
(`-VmPassword`,与 `run-smoke-in-vm.ps1` 相同),脚本不读任何口令文件。

#### 前提

1. 两个候选版(A 和 B)都已签名、公证、带更新能力(`FOLIO_UPDATER=on`);A 还必须是更新
   卡已打开的构建(`OFFERS_ENABLED_WINDOWS` 自 U-31 起、`OFFERS_ENABLED_MACOS` 自 U-32 起)——`--update-feed`
   只换来源,不打开卡片。B 的版本号高于 A,且 B 的发布清单里 `min_updater` 不高于 A 的版本
   (否则 Prepare 拒绝,卡片只说 *The update is not verified.*,见 U-31 D-13)。B 不必发布到 GitHub。
2. A 已由 `install-candidate.ps1` 装进客户机的 `C:\folio-vm\folio\` 并打了快照;每一行都从这个
   快照开始。
3. 后继版本 B 经命令行参数 `--update-feed <file-URL>` 注入(U-30b)。带这个参数启动的那个
   进程,检查新版时读本地文件夹里的 `releases.json`,按「更新」时从同一文件夹拷贝两个文件,
   不连 github.com;之后的校验和、归档读取与签名者检查一概不变——B 必须与 A 同一签名者签名,
   测试根签的包在 Prepare 的身份检查被拒(`Stop::Identity`)。参数不落盘:不带它重启就回到
   github.com。宿主端步骤:
   1. **构建 B**(版本号高于 A):按 `docs/WINDOWS-CI-RELEASE.md` 第 1–3 步——
      `gh workflow run build-release.yml --ref <分支> -f ref=<B 的完整提交> -f updater=true`;
      在所有者本机用 `scripts/release/fetch-ci-build.ps1 … -Apply` 取回,再
      `scripts/release/package.ps1 -Binary target/ci-signing -Sign`(内部调用 `sign.ps1`,
      用所有者本机的签名会话)。产物在 `target/release-package/`:
      `folio-<B>-windows-x64.zip` 和 `SHA256SUMS.txt`。
   2. **建源文件夹**:宿主上新建 `feed\`,放入上面两个文件,再写 `feed\releases.json`
      (GitHub 发布列表的形状,以下字段缺一不可;`size` 是文件字节数;`draft: true` 的条目
      不算):

      ```json
      [
        {
          "tag_name": "v<B>",
          "name": "Folio <B>",
          "draft": false,
          "prerelease": false,
          "assets": [
            { "name": "folio-<B>-windows-x64.zip",
              "browser_download_url": "file:///C:/feed/folio-<B>-windows-x64.zip",
              "size": 12345678 },
            { "name": "SHA256SUMS.txt",
              "browser_download_url": "file:///C:/feed/SHA256SUMS.txt",
              "size": 1234 }
          ]
        }
      ]
      ```
   3. **拷进客户机** `C:\feed\`:`run-row.ps1 -Feed <宿主上的 feed 文件夹>` 每一行都逐个文件拷入。
   4. **带参数启动 A**:`guest/keys.ps1` 的第一步 `launchfeed` 以
      `C:\folio-vm\folio\folio.exe --update-feed file:///C:/feed/` 启动它。

   **看什么**:`diagnostics.log` 启动段有一行 `update feed: file:///C:/feed/`;更新卡在最近
   活动的普通窗口弹出,写 v<B>;按「更新」后日志 `H\journal.json`(H 是
   `<安装目录>\.folio-update`;`update_txn.rs` `Home::journal`)经过 `Allocated` → `Prepared`;
   按「重启」后 `Handoff` → `Armed` → `Moving` → `Trial` → `Committed` → `Retired`,安装目录里的
   文件换成 B 的。这些阶段就是下表各行的断电点。试运行、应用者、恢复进程都不带这个参数,
   也不需要:下载在 Prepare 就完成了。**一次 `--update-feed` 运行之后,`update-check.json` 记住了
   源的答复**:此后 24 小时内的普通启动仍会提示 v<B>,按下去去问 github.com(U-31 D-10)。

   macOS 同理:资产是 `Folio-<B>-macos-arm64.dmg` 和 `SHA256SUMS-macos.txt`,由
   `mac/setup.sh` 放进演练文件夹的 `feed/` 并写好 `releases.json`,URL 形如
   `file://<演练文件夹>/feed/Folio-<B>-macos-arm64.dmg`。

#### Windows 步骤(W1–W15)

跑法:

```powershell
# 1. 装 A,打快照(每换一个候选版做一次)
pwsh -File scripts/release/cleanvm/updater/install-candidate.ps1 `
    -Vmx <win11.vmx> -VmPassword <口令> -Zip <folio-<A>-windows-x64.zip> -Snapshot a-installed

# 2. 先看一遍(-WhatIf 不碰虚机,打印每一条 vmrun)
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 `
    -Vmx <win11.vmx> -VmPassword <口令> -Snapshot a-installed -Feed <feed 文件夹> -Row W1 -WhatIf

# 3. 真跑:一行、几行(逗号分隔)或全部(all = 行表顺序)
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 `
    -Vmx <win11.vmx> -VmPassword <口令> -Snapshot a-installed -Feed <feed 文件夹> -Row W1,W2
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 `
    -Vmx <win11.vmx> -VmPassword <口令> -Snapshot a-installed -Feed <feed 文件夹> -Row all
```

证据落在 `target/cleanvm/<虚机名>-<时间戳>/updater/<行>/`:每次收集一个文件夹
(`after-logon`、`after-start1`、`after-start2`,或活行的 `t<秒>`、`then`),里面是
`install-listing.txt`(每个文件的 SHA-256,`folio.exe` 带 FileVersion)、`journal.json`、
`diagnostics.log`、`run-key.reg`、`processes.txt`、`updater-*.txt`(监视日志、日志历史、
按键日志、哨兵、断电那一刻的记录 `updater-atmarker-<行>.txt`)和 `shots\`;旁边是宿主的
`screen-<收集>.png` 和 `run.log`。整轮全部 21 行约 4–5 小时。

**一行是怎么跑的**(每一条都是 U-31 实跑过的做法,括号里是当时的发现编号):

- **两种行。** 断电行(W1–W13):按键计划一直走到监视器看见该行的状态,冻结、写哨兵;宿主
  `vmrun stop hard`,不回快照直接开机,登录后 45 秒收集一次(Run 入口已跑),然后普通启动一次、
  关窗再普通启动一次,各收集一次。活行(happy、W14、W14long、W15、E7、rollback、console1/2)
  不断电,在驱动启动后的固定秒数收集,有的再跑第二段按键计划(happy 的两次普通启动、E7 的
  放开后再按「重启」)。
- **更新入口。** 表中的「更新」和「重启」默认仍指自动弹出的更新卡。若卡片已收起而该行要
  从 `Verified` 继续,入口是「设置 → 关于 → 版本」的「更新并重启」,不再是「常规」页的一行;
  自动检查开关就在版本下一行,但这些 W 行不改它。
- **监视器绝不长时间打开日志。** `journal.json` 上任何一个打开的句柄都会让写入方的
  rename-over 失败(`Access is denied`);第一版监视器每 25 ms 读一次,把应用者弄死在
  Armed → Moving(H-6)。现在只在目录项(时间、大小、创建时间,`FindFirstFile` 不开文件)变了
  才读一次并立刻关闭;活行在读到 `Handoff` 之后就再也不打开它(`-StopReadingAt Handoff`),
  rollback 行从头到尾不打开(`-NoRead`)。
- **冻结。** 到达该行状态的一瞬间挂起全部 `folio.exe`(`NtSuspendProcess`),写一份刷过盘的记录,
  再写哨兵;宿主 1–5 秒后断电。只持续几毫秒的状态(W4–W9、W11–W13)在前一阶段读到后不睡眠
  地自旋(W4 盯 Run 键,W8 盯 receipt,其余盯目录项)。**代价**:这几秒里客户机的惰性写回照常
  进行,所以产品漏掉的一次 flush 会被掩盖,而不会暴露。
- **驱动在隐藏控制台里、`-noWait` 启动。** 旧脚本不带 `-noWait`,宿主先干等监视器自己的
  600 秒超时(H-2);用 `-activeWindow` 起的 PowerShell 会被客户机默认的 Windows Terminal
  开成一扇压在 Folio 上面的窗,ui-probe 于是拒绝截图和按键。现在一律
  `conhost.exe powershell.exe -WindowStyle Hidden`。
- **按键。** Tab 点亮的是**最先画的**那个动词——提示卡上是 *Skip*,重启卡上是 *Later*
  (`update_card.rs`,`enter_presses_nothing_until_the_ring_is_lit`)。所以「更新」和「重启」是
  **Shift+Tab、Enter**(H-4);每次按键前后各拍一张环的截图。Windows 没给前台的窗(重启后常见,
  任务栏按钮在闪)先用一次真点击拿前台:窗口 +(60, −60),ui-probe 先核像素归属再点(H-8)。
  W13 的「取消」要落在几秒的下载里,用预先加载进进程的 ui-probe 类发键(H-9)。
- **首启的两张卡。** 干净快照上每次冷启动都先出 *Welcome to Folio*,再出 PSReadLine 邀请,然后
  才是更新卡;计划用两次 Escape(`later`,即 *Not now*)关掉它们,什么都不应用(H-5)。
- **一台客户机同一时间只有一个驱动。** 停掉后台任务只停 shell 不停子进程,而 bash 边跑边读
  脚本:U-31 有一段时间两个驱动同时在开同一台虚机,那段时间的每一行都作废(H-7)。

| ✓ | 行 | 到达方式(`rows.ps1`) | 断电 / 持有 | 预期结果(已发布规则;U-31 A7 实测) |
| --- | --- | --- | --- | --- |
| ☐ | happy | 更新 → 重启 | 无 | 一条 `wrote [Armed, Moving, Trial, Committed, Retired]`;trial(B)就是那扇窗;`folio.exe` = B;Run 值没了;之后两次普通启动都不出卡,`H\<txn>` 与日志已删(只剩 `admission`、`lock`) |
| ☐ | W1 | 更新,不等下载完成 | 日志 `Allocated` | 登录:无入口,原样;第一次普通启动 `an unfinished download of an earlier launch is cleared`,`H\<txn>` 和日志删掉;旧版正常启动 |
| ☐ | W2 | 下载校验完成,不按「重启」 | `Prepared` | 第一次启动 `was prepared at an earlier launch and is verified again` → 重启卡,`deferred_launches 1`;第二次启动 `discarded at its second launch` |
| ☐ | W3 | 按「重启」 | `Handoff`,Run 值未写 | 登录:无入口;第一次启动 R 把日志退回 `Prepared`(`Reverted`),打开旧版并出重启卡。**与 (b).2 原文不同**:原文说第一个锁持有者从 W4 开始应用;已发布规则是退回(DESIGN 2026-09-27,U-23) |
| ☐ | W4 | 同上 | `Handoff`,Run 值已写、`Armed` 未落盘(自旋盯 Run 键) | 登录 R:删 Run 值,退回 `Prepared`,无文件被移动,打开旧版出重启卡 |
| ☐ | W5 | 同上 | `Armed`,0 次移动 | 登录 R:`Reverted` → `Prepared`,删 Run 值,重启卡。**与 (b).2 原文不同**:不是「admission → Moving」,而是退回 |
| ☐ | W6 | 同上 | `Moving`(部分文件已移) | I1′ 成立;登录 R:`RollbackIntent, RolledBack, Retired`,旧版按摘要复原,删 Run 值,以 `--update-failed` 打开;卡 *The update was interrupted before the new version started. / Previous version restored.*;诊断行 `BT_UPDATE_RECOVER the update was interrupted before the new version started` |
| ☐ | W7 | 同上 | `Trial`,无 receipt | 登录 R:`the trial <pid> gave no receipt` → 回滚 → `--update-failed` |
| ☐ | W8 | 同上 | `Trial`,receipt 已写(自旋盯 receipt) | 登录 R:`Committed, Retired`,`nobody is waiting; nothing was started`(登录时不开窗,U-31 D-12 待裁);普通启动打开 B |
| ☐ | W9 | 同上,trial 一出现就被结束 | `RollbackIntent` | 登录 R:`RolledBack, Retired`,`--update-failed` |
| ☐ | W10 | 同上,且新的 `folio.msix` 被不共享地持有 | `Stuck` | 断电后持有消失;登录 R:`RolledBack, Retired`,`--update-failed`。持有仍在时恢复在移回前停止(`Stuck`),原因 `` `folio.msix` in the install could not be read or moved out: held open by another program ``;卡片是 *Update incomplete.* 并指出文件夹 |
| ☐ | W11 | 同 W9 | `RolledBack` | 登录 R:`Retired`,删 Run 值,`--update-failed` |
| ☐ | W12 | 同 W3 | `Committed`,receipt 在、Run 值还在 | 登录 R:`Retired`,不开窗(D-12);B 已装;下一次启动删 `H\<txn>` |
| ☐ | W13 | 更新后 1–2 秒内取消 | `Abandoned` | 无日志、无 `H\<txn>`、无 Run 值;旧版完整。W13 的第一次普通启动不带 `--update-feed`,源的回答不保留,不出更新卡;检查直接问 github.com |
| ☐ | W14 | 更新 → 重启;Run 值一出现,监视器以 `Read`、共享 `Read`(无 delete、无 write)打开 `H\journal.json` 3 秒 | 不断电 | rename 重试扛过去:`wrote [Armed, Moving, Trial, Committed, Retired]`,trial 是那扇窗 |
| ☐ | W14long | 同上,持有 120 秒。**前提:A 为 0.4.7 或更高**(A = 0.4.6 时是已知问题:新版以未记录状态运行,直到下次启动或登录完成更新;本行不执行) | 不断电;句柄持有覆盖 applier 的 `TrialBegan` 写入及其回滚声明 | 句柄持有期间:`diagnostics.log` 包含 `the trial <pid> could not be recorded`,然后 `is asked to quit`,然后 `started …\folio.exe`;唯一的 Folio 窗口是以 trial 启动的新版,卡片显示「Update incomplete.」并指出文件夹;日志停留在 `Moving`;Run 值仍存在。释放句柄后,trial 的 watchdog 到期时(该 trial 启动后 102 s、204 s、408 s、816 s;至多四次,前一次恢复仍在运行时不启动下一次)或在下次启动/登录时立即完成:`BT_UPDATE_TRIAL … it is handed back to …\rescue\folio.exe (ready)`(仅 watchdog 路径),然后 `BT_UPDATE_RECOVER the trial <pid> runs and its receipt names it, and the journal does not record it: it is recorded`。句柄仍持有时,人手动启动触发的恢复记录 `BT_UPDATE_RECOVER deferred: …` 且不启动第二个进程 trial 窗口不会被自行结束;唯一可以结束从未变为 ready 的 trial 的是持有事务的恢复进程。日志最终为 `Retired{Committed}`;Run 值 `FolioUpdate-<txn8>` 已删除;安装目录为 B 版;唯一的窗口是句柄持有期间启动的 trial,保留不变,卡片现显示「Updated.」;持有期间在该窗口做的更改(如设置)保留。释放句柄后普通启动 Folio 立即到达相同终态,不再启动 B 的第二个进程 |
| ☐ | W14m | 更新 → 重启;Run 值一出现(暂存集合已验证过),监视器以 `Read`、共享 `Read`(无 delete、无 write)打开 `H\<txn>\set\folio.exe`:(a) 一直持有到 `Moving` 写入之后再过 3 秒(从 `Moving` 起算,持有就不会在移入之前结束);(b) 60 秒 | 不断电;句柄持有覆盖 applier 把 `folio.exe` 移入安装目录的那一步(T-UPDATE-MOVE-HELD-RETRY) | (a) 移动被重试扛过去:`diagnostics.log` 有一行或多行 `` BT_UPDATE_APPLY `folio.exe` held by another program for … s: …; asked again in … ms ``,然后 `wrote [Armed, Moving, Trial, Committed, Retired]`,trial 是那扇窗,安装目录为 B 版。(b) 十秒窗口过后 `` … past the 10 s window: …; the move is given up ``,然后 `` BT_UPDATE_APPLY the move of `folio.exe` failed: another program held it open past the window: … ``,`wrote [Armed, Moving, RollbackIntent, RolledBack, Retired]`;旧版以 `--update-failed <journal> --update-journal-held <错误> --update-held-file folio.exe` 打开,卡片标题是 *Another program held folio.exe open: <Windows 的错误>*(中文界面同为英文,zh 待译),下面是 *Previous version restored.*;Run 值已删,安装目录为 A 版 |
| ☐ | W15 | 从 `Prepared` 起以 `Read`、共享 `None` 持有 `H\<txn>\rescue\folio.exe` 40 秒,再按「重启」 | 不断电 | `its applier could not be started` → `Abandoned`,无文件被移动;旧 Folio `leaving after an update: started …`,普通方式打开一次。(启动仅迟缓、应答未在 15 秒 `HANDOFF_DEADLINE` 内到达的路径由单元测试 `a_hand_over_with_no_answer_in_time_still_opens_folio_once` 覆盖) |
| — | W16 / U-35 | **Not staged by hand; covered by the harness.** The cell needs a holder that leaves `Moving` with B live, its own trial neither launched nor recorded, and then has both B and the rescue refused by the operating system. Denying execute on B does not reach it: P's own trial launch fails first and P declares and performs the rollback before its exit guard runs. The only release-build road there is a journal write P cannot make (W14-like), and then the reservation's own write fails too. | — | Pinned instead by `update_apply::exit_guard_tests::u35_the_last_trial_requires_two_os_launch_refusals` (the 27-cell created / refused / unacknowledged table) and, in the Windows sandbox, `update_apply_windows::tests::u35_two_windows_launch_refusals_reserve_exactly_one_last_trial` (journal `TrialStarting` with the reserved nonce at the third process creation), `u35_a_ready_last_trial_commits_itself_without_any_rescue_process`, `u35_an_unready_last_trial_commits_nothing_and_the_recovery_ends_it` and `u35_the_guard_and_a_start_agree_on_the_reserved_trial`. Windows only; macOS has no such cell. |
| ☐ | E7 | 从 `Prepared` 起不共享地持有 `THIRD-PARTY-NOTICES.md` 200 秒,按「重启」 | 不断电 | `nothing is moved: held open by another process`,`wrote [Armed, Prepared]`,删 Run 值,旧版重新打开(约 60 秒无窗,D-15);放开后重启卡从暂存集合再按一次 → 提交 |
| ☐ | rollback | 更新 → 重启;trial 一启动就被结束 | 不断电 | `the trial <pid> ended without a receipt` → `RollbackIntent, RolledBack, Retired`;旧版按摘要复原;`--update-failed` 卡 *The new version did not start. / Previous version restored.* |
| ☐ | console1、console2 | 按键驱动在按下「重启」后退出(启动 Folio 的控制台随之消失) | 不断电 | 交接与提交照常,trial 是那扇窗(与 U-31 D-5 无关的对照) |

每一行结束时都应当有一扇 Folio 窗(D-12 的 W8/W12 登录时除外,之后的普通启动打开)。

#### macOS 步骤(M1–M11)

没有可处置的 macOS 虚机(§8)。U-32 在所有者的 Mac mini 上,以所有者自己的账号跑了这张表
(**「option B」**,所有者 2026-09-28 书面授权);除演练文件夹外只写三处,且都会复原:

- `~/Library/Application Support/Folio`:第一次 setup 时用 `ditto` 整个备份,每一行开始与结束、
  以及 teardown 时从备份复原;
- `~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<txn8>.plist`:每一行和 teardown 删除;
- `$TMPDIR/folio-<uid>/`:Folio 自己的单实例 socket。

**断电的替身比断电弱。** 所谓「切」是对演练文件夹里每个进程先 `SIGSTOP` 再 `kill -9`;内核的
页缓存还在,所以没有一行能证明缺了 `fsync`/`F_FULLFSYNC` 或 rename 被撕裂。凡是带切的行,
证据一律写「synthetic-process evidence, weaker than a power cut」。

**前提。**

1. A 与 B 的 dmg、B 的 `SHA256SUMS-macos.txt` 已在 Mac 上(文件名必须是
   `Folio-<版本>-macos-arm64.dmg`,版本从文件名读);A 已签名、公证、`spctl` 接受。
2. 控制台会话**已登录且未锁屏**,演练期间没人用这台 Mac:按键和截图都作用在真屏幕上;
   锁屏会让每一行停在第一次按键(「not frontmost; nothing sent」)。
3. 没有演练文件夹以外的 Folio 在跑。每个脚本自己查,查到就以退出码 3 停下;请人退出它,
   脚本绝不结束它。
4. 这个 ssh 会话能发键(辅助功能与 Automation → System Events 已授权);setup 用一个只读的
   no-op 记录结果,不弹授权框。

**演练文件夹**是 `$FOLIO_REHEARSAL`,默认 `~/folio-rehearsal`,三个脚本读同一个值:

| 路径 | 是什么 |
| --- | --- |
| `a-pristine/Folio.app` | A,从镜像 `ditto` 出来一次;每一行都放一份新的副本到 `app/` |
| `app/Folio.app` | 安装;它的 home H 是 `app/.Folio.app.folio-update` |
| `H/journal.json` | 日志(不是 `H/<txn>/journal.json`) |
| `H/<txn>/{stage,rescue,owner,health-<nonce>,mnt}` | 一个事务的文件 |
| `feed/` | B 的 dmg、sums 文件与 `releases.json` |
| `versions` | A、B 的版本(setup 写,row 读) |
| `data-before/` | 数据文件夹的备份(`Folio/`,或没有时的 `ABSENT`) |
| `evidence/` | `setup.txt`、`<行>.log`、`<行>/…`、`teardown.txt` |

**跑法**(从 Windows 经 ssh;`<mac>` 是 Mac 的 ssh 主机名,`<dir>` 是 Mac 上放脚本的文件夹;
ssh 字符串里不写变量,行名照字面替换):

```text
scp scripts/release/cleanvm/updater/mac/setup.sh scripts/release/cleanvm/updater/mac/row.sh scripts/release/cleanvm/updater/mac/teardown.sh <mac>:<dir>/
ssh <mac> 'bash <dir>/setup.sh <A.dmg> <B.dmg> <B 的 SHA256SUMS-macos.txt>'          # 以 SETUP DONE 结束
ssh <mac> 'nohup bash <dir>/row.sh <行> > ~/folio-rehearsal/evidence/<行>.log 2>&1 < /dev/null &'
ssh <mac> 'tail -n 4 ~/folio-rehearsal/evidence/<行>.log'                              # 末行 ROW <行> END (<结果>)
ssh <mac> 'bash <dir>/teardown.sh'                                                      # 以 TEARDOWN DONE 结束
scp -r <mac>:folio-rehearsal/evidence <宿主上的证据文件夹>
```

脚本在仓库里是 LF;经别的途径拷过去的,先 `tr -d '\r'`。**一次只跑一行**(有演练进程在跑时
row 拒绝开始);一行 3–8 分钟,19 行约 2 小时;重跑一行会把旧证据改名为 `<行>.prev-<时间>`。
`--cut named` 只杀该行点名的角色、其余 `SIGCONT`;`--no-clean` 留下进程、plist、`app/` 与数据
文件夹供手看。`<行>/RESULT.txt` 是 `done`、`row not reached: …`(监视器看到时状态已过,
没有切,冻结的进程已继续,路照走并记录;重跑)、`no offer card`、`never Ready` 或
`keys refused`。退出码:3 前提、4 不能发键、5 A 没启动。

每一行做的事:复原数据文件夹、放一份新的 A → `open -n -a <文件夹>/app/Folio.app --args
--update-feed file://<文件夹>/feed/` 启动 O → 等 `diagnostics.log` 出现 `is offered` →
经 `osascript`/System Events 按键,每次先把 O 设为前台并按 pid 核实(更新/重启 = Shift+Tab、
Enter;Later = Escape;取消 = Tab、Enter;退出 = Cmd+Q)→ 监视器(`evidence/<行>/watch.py`)
每 2 ms 看一次 `H/journal.json` 的目录项(inode、mtime、size),变了才读(macOS 上读者不会挡住
rename,与 W14 不同),到达该行条件时 `SIGSTOP` 文件夹里的每个进程,写 `at-cut.txt`、`cut.txt`
→ 切:`kill -9`,`cut.txt` 只对真的送达了信号的 pid 记 `KILLED` → 下一位操作者:普通
`open -n -a`,或登录替身 `launchctl bootstrap gui/<uid> <plist>`(launchd 拒绝时退而手动运行
plist 的 `ProgramArguments`,日志写明是哪一种)→ 每一步收集证据 → 清理(只结束本文件夹的进程,
`bootout` 本行 bootstrap 过的标签,删指向本文件夹的 plist,卸载挂载,复原 `app/` 与数据文件夹)。

| ✓ | 行 | 怎么到 | 切 | 之后 | 预期(已发布规则;U-32 第 4 轮实测) |
| --- | --- | --- | --- | --- | --- |
| ☐ | happy | 更新 → 就绪 → 重启 | 无 | 稳定;截 trial;Cmd+Q;普通启动 ×2 | `Armed, Moving, Trial, Committed, Retired`;trial(B)是那扇窗;装的是 B;无 plist;之后不出卡,`H/<txn>` 与日志已删 |
| ☐ | M1 | 更新 | `Allocated`(O) | 普通启动;Cmd+Q;普通启动 | 清掉:`H/<txn>` 与日志没了,无挂载,仍是 A,又出提示卡 |
| ☐ | M2 | 更新 → 就绪 → Later → Cmd+Q | 无 | 普通启动;Cmd+Q;普通启动 | 第一次:重启卡,`deferred_launches 1`,无检查行;第二次:`discarded at its second launch` |
| ☐ | M3 | 更新 → 就绪 → 重启 | `Handoff`(O,以及 P 若已存在) | 同上 | R 退回 `Prepared`,打开 A 并出重启卡 |
| ☐ | M4 | 同上 | `Armed`(P;plist 已在) | 登录替身;Cmd+Q;普通启动 | 删 plist,`Prepared`,打开 A。**本构建上 Armed → Moving ≤ 24 ms,2 ms 轮询加 SIGSTOP 抓不住(U-32 三次未到达)**;同一恢复分支由 M5 覆盖,Armed 特有的持久结果见 U-32 第 1 轮 M4 |
| ☐ | M5 | 同上 | `Moving`,仍是 A(P) | 登录替身;Cmd+Q;普通启动 | 删 plist,`Reverted` → `Prepared`,A 打开并出重启卡(设计笔记称此阶段 `Exchanging`;日志里是 `Moving`,活跃身份决定走向) |
| ☐ | M6 | 同上 | `Moving`,已是 B(P) | 普通启动;稳定;Cmd+Q;普通启动 | R 以 `--update-trial` 启动 B(`Trial` 由 R 记)→ receipt → `Committed, Retired` |
| ☐ | M7 | 同上 | `Trial`,无 `health-*`(P、N) | 登录替身;稳定;Cmd+Q;普通启动 | `RollbackIntent` → 换回 → `RolledBack`;A 以 `--update-failed` 打开,*Previous version restored.*;窗口在登录后存活(plist 带 `AbandonProcessGroup`) |
| ☐ | M8 | 同上 | `Trial`,`health-<nonce>` 已在(P;`--cut all` 时 N 也杀) | 登录替身;稳定;普通启动 | `Committed, Retired`;登录时不开窗(D-12);普通启动打开 B |
| ☐ | rollback (M9) | 同上;`Trial` 一记下监视器就 `SIGKILL` N | 仅 N | 稳定 | P:`RollbackIntent` → 换回 → `RolledBack`;A 以 `--update-failed` 重开,*The new version did not start. / Previous version restored.* |
| ☐ | M9cut | 同 rollback | `RollbackIntent`(P) | 登录替身;稳定 | R 完成回滚;`--update-failed` 窗 |
| ☐ | stuck-a (M10a) | 同 rollback;N 被杀后 `H/<txn>/stage` 改为只读 | 无 | 等 `Stuck`;复原 `stage`;Cmd+Q;普通启动;稳定 | P:换回被拒 → `Stuck`(attempts 1)→ P 以 B 起一次**记录在案的重试**(`Stuck.retrial`)→ 其 receipt 向前提交 → `Committed, Retired`;窗口是 B,卡片 *Folio <B> / Updated.*。**与旧表不同**(U-32 计划缺陷 8):B 健康时 R 重试 → `RolledBack` 这条路不会发生 |
| ☐ | stuck-b (M10b) | 同 stuck-a | `Stuck` 且 P 尚未记下自己的重试(`stuck-noretrial`;P) | 普通启动(`stage` 仍只读);盯重试;稳定;复原 `stage` | R 见 `Stuck` 且 B 在位 → 以 B 起记录在案的重试(`RetrialBegan`,attempts 不变)→ receipt → `Committed`;启动时卡片 *Update incomplete.*(`shots/retrial-at-launch.png`),提交后 *Updated.*(`shots/retrial-after-commit.png`)。若 P 的重试已先记下,本行报 `row not reached`,重跑 |
| ☐ | M11-rolledback | 同 rollback | `RolledBack`(P) | 普通启动;Cmd+Q;普通启动 | `Retired`;终态落盘后删 plist |
| ☐ | M11-abandoned | 更新后约 1 秒内取消(下载卡上 Tab 点亮的是 Cancel) | 无 | 稳定;普通启动;Cmd+Q;普通启动 | `Abandoned`,随后退役。若取消落地前下载已完成,Tab 点亮的是就绪卡的 *Later*,日志是 `Prepared`:重跑 |
| ☐ | M11-committed | 更新 → 就绪 → 重启 | `Committed`(P 与 trial) | 普通启动;Cmd+Q;普通启动 | `Retired`;删 plist;B |
| ☐ | R-W15 | 就绪后对 `H/<txn>/rescue/Folio.app/Contents/MacOS/folio` `chmod 000`,重启,等 O 离开,复原 | 无 | 稳定 | `its applier could not be started` → `Abandoned`;O 普通启动 A 一次。(macOS 上打开着文件不会挡 exec,所以只用 `chmod 000`) |
| ☐ | R-D5 | 同 R-W15,另对已安装的 `folio` 也 `chmod 000` | 无 | 复原 | O 启动自己被拒;系统提示框 *Folio / Update incomplete. <文件夹>*;`no start after the update was delivered`。提示框在其进程结束后仍留在屏幕上(U-32 缺陷 4,未关) |
| ☐ | D14 | 更新 → 就绪 → 重启;`Moving` 读到 B 时停下所有进程、把 H 改为只读(`chmod a-w <H>`),约 20 秒后复原。**前提:A 为 0.4.7 或更高**(A = 0.4.6 时是已知问题:新版以未记录状态运行,直到下次启动或登录完成更新;本行不执行) | 不断电;只读 home 覆盖 applier 的 `TrialBegan` 写入及其回滚声明 | 等路走完;截图;复原 H;普通启动;稳定(trial 窗不必退出:新进程被交给它,不再回滚) | 只读期间:`diagnostics.log` 包含 `the trial <pid> could not be recorded`、`is asked to quit` 和以 trial 打开的新 bundle(「Update incomplete.」并指出文件夹);日志停留在 `Exchanging`;LaunchAgent plist 存在。若该 trial 的 receipt 在只读期间被拒:`receipt … not written …; the watch writes it again`,然后 `receipt … written by the trial's watch`。恢复后,trial 的 watchdog 到期时(该 trial 启动后 102 s、204 s、408 s、816 s;至多四次,前一次恢复仍在运行时不启动下一次)或 `open -n` / 登录时立即完成:`BT_UPDATE_RECOVER the trial <pid> runs and its receipt names it, and the journal does not record it: it is recorded`。home 仍为只读时,`open -n` 触发的恢复记录 `BT_UPDATE_RECOVER deferred: …` 且不启动第二个进程 `Committed` 然后 `Retired`;plist 已删除;旧 bundle 在 `stage/`、rescue 克隆已删除;窗口为 trial,卡片显示「Updated.」;该窗口打开时 `open -n` 不会启动留存的第二个副本,新版不被回滚;trial 窗口不会被自行结束;唯一可以结束从未变为 ready 的 trial 的是持有事务的恢复进程(本行替代排演的缺陷 9) |

M 表的原始定义见 (b).2 的 macOS 表;上表与它的差别只在 M4/M5/M6 的阶段名(日志里是 `Moving`)
与 M10 的两行。所有者自己的两步——§7.4(E-12:所有者本机、Gatekeeper 开着)与 cask 检查
`scripts/release/check-cask-hooks.sh`——不在这些脚本里。

macOS 上,trial 在被观察前结束(M9 在 `Trial` 之前被杀)不再耗费 90 秒:applier 看到启动结束(`the trial ended before it could be seen`)后立即回滚。此条适用于候选版 A 为 0.4.7 或更高;被信号杀死的 `open` 视为未知(applier 照常等待 trial 至截止时间)。

#### E1-b: the journal escape hatch (RV, RW; 0.4.8) <!-- zh: pending T-JOURNAL-ESCAPE-HATCH E1-b -->

What a build does with an update journal it cannot read whole (DESIGN 2026-10-08, E1-a1/E1-a2):
the rows reverse the usual pair. **A** is the build under test (it carries the hatch); **B** is a
rehearsal successor built from a throwaway branch that is never merged or shipped: it reads one
test-only word per journal vocabulary as the word it replaces (class `rehearsal_destructive`,
outcome `rehearsal_rolled_back`, header `v` 2, phase `RehearsalRolledBack`, layout
`RehearsalMembers`, adapter `RehearsalOurs`, `Retired.outcome` `RehearsalRolledBack`) and writes
its receipt as v2; **C** is B with the next version number. **A-mut** is A built from a second
throwaway branch with the hatch's arms put back to their pre-E1 behaviour (the start's unknown
header continued plainly with no card, the held-writes continue removed, the Windows exit guard's
`Installed{failed:false}`, the holders' `Left`/`Refused`, the door's header-alone exit). B and C
are signed with the release identity (the B -> C update needs it); A and A-mut need not be.

Snapshots: `a-installed` (A), `amut-installed` (A-mut), `b-installed` (B), each by
`install-candidate.ps1`. The planted rows plant from journals B wrote itself: `rolled-back.json`,
the journal at RV-W11's marker, and `retired.json`, its `Retired{RolledBack}`.

```powershell
$pass = Get-Content <the VM password file>   # into a variable, never printed
# RV: B installed, the feed holds C, A's archive laid over the install at the W state
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 -Vmx <folio-win11.vmx> -VmPassword $pass `
    -Snapshot b-installed -Feed <feed of C> -OverlayZip <folio-A-windows-x64.zip> -Row RV-W11,RV-W6
# the controls (R2-17): B installed meets the planted journal and settles it
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 ... -Snapshot b-installed -Feed <feed of C> `
    -TemplateDir <folder> -RescueB <B's folio.exe> -Row BC-class,BC-phase
# RW: A installed; then the same rows on A-mut with -Mutant
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 ... -Snapshot a-installed -Feed <feed of C> `
    -TemplateDir <folder> -RescueB <B's folio.exe> -Row RW-class-B,RW-class-none,RW-phase-B,RW-phase-none,RW-class-A,RW-phase-A
pwsh -File scripts/release/cleanvm/updater/run-row.ps1 ... -Snapshot amut-installed -Mutant ...
```

**An RV row** is a W row of the table above run on B -> C, with one difference at the marker
(`guest\watch.ps1 -OverlayZip`): the frozen processes are ended (each within `$KillWithinMs`; a
running or suspended `folio.exe` keeps its image open and cannot be written over), every member of
A's archive is written over the install and flushed, `.folio-update` is left as it is, and only
then the marker and the cut. Then logon (+45 s), start1, start2, as for every cut row.

**A planted row** runs no update: the host writes `H\journal.json` from B's journal with one word
replaced (`rows.ps1` `Get-PlantedJournal`; `written_by` stays B's version), `guest\plant.ps1` puts
it in place with the rescue copy the row names, and Folio is started plainly twice. The first run
is answered beforehand (`keys.ps1 answerfirstrun`), each start's card is read by the operating
system's OCR over Folio's own window (`guest\card.ps1`, the capture kept beside the text), and the
close is `WM_CLOSE` to Folio's `Window Class` window. The verdict is `Get-PlantedVerdict`, cell by
cell, written to `<row>\verdict.txt`.

| ✓ | row | word / rescue | expected (A, the hatch) | expected (A-mut: the mutation column) |
| --- | --- | --- | --- | --- |
| ☐ | RV-W1 … RV-W12 | B's own journal; A laid over | A never deletes or rewrites a journal it did not settle: a change between two collections has a rescue's `BT_UPDATE_RECOVER … recovery ended` line in that interval, or a `terminal`, `preparing` or `deferred` header before it (A1); a Folio window at the end. **RV-W4** (Handoff with the Run value, before Armed) is not reached by the harness: twice the journal already read `Moving` when the watcher froze on the Run value (the window is milliseconds wide); it goes to 0.4.9 (E1-b-rest) | — |
| ☐ | RV-W6 | as above, cut at `Moving` | B's rescue finds neither whole set live → `Stuck`, and opens its rescue copy, which shows *Update incomplete.* with the folder (T-RESCUE-COPY-CARD) — when B carries that fix; a B built before it opens the rescue copy with no card | — |
| ☐ | BC-class … BC-retired-outcome | each word; B's rescue (B installed) | B reads its word and settles: `recovery ended RolledBack`, the journal gone, *Previous version restored.* | — |
| ☐ | RW-class-B, RW-outcome-B, RW-v-B | header word; B's rescue | A hands it to B's rescue (`an update is being finished by …\rescue\folio.exe`), which settles it as above | A continues plainly, nothing handed over, no card (`… is left as it is`) |
| ☐ | RW-class-none, -held (and outcome, v) | header word; no rescue / B's held share-none | held-writes continue: `could not be started; Folio starts with its writes held`, the journal byte for byte, *An update by a newer Folio is not finished.* … *Changes made in this session are not kept.* | as above |
| ☐ | RW-class-A (and outcome, v) | header word; A placed as the rescue (R2-19) | A as the recovery stands aside (`recovery ended StoodAside(`), the exit guard opens the rescue copy, never the installed build plainly, and that window shows the newer Folio's card (T-RESCUE-COPY-CARD: the rescue copy reads the real home's journal; before it, the rescue copy read its own home and showed no card) | as above |
| ☐ | RW-phase-B, RW-layout-B, RW-adapter-B | body word; B's rescue | the header hands it over; B settles it | the same (the control) |
| ☐ | RW-phase-none, -held (and layout, adapter) | body word; no rescue / held | held-writes continue; the card is the newer Folio's whenever A cannot read the journal whole (`update_startup::unfinished`), with the not-kept sentence | continues plainly (`Folio starts without it`), no card |
| ☐ | RW-phase-A (and layout, adapter) | body word; A as the rescue | as RW-class-A: stands aside, the rescue copy's window with the newer Folio's card | `recovery ended Left(`, the rescue copy's window, no card |
| ☐ | RW-retired-outcome-B | `Retired.outcome` (terminal header) | A retires it on its frozen header (A1): the journal gone, nothing handed over, no card (a plain start) | the same |

| ☐ | RW-receipt-v2 | live: A installed, A -> B through the feed (A and B signed by the same identity; first run answered) | B as the trial writes its receipt as v2; A's holder never accepts it: `BT_UPDATE_APPLY the receipt: version 2 is not one this build reads`, no receipt by the deadline, rollback (`RolledBack`), the journal gone, A restored with *The new version did not start. / Previous version restored.* (cells read from the `t240` and `then` collections) | — |

Not yet staged here: the macOS rows (`mac/row.sh`; B and C signed and notarised on the Mac).

#### 每一行检查什么(通用)

对每一行,重启后(或每个收集时刻)收集以下证据:

1. **日志阶段**:`H\journal.json`(或 `H/journal.json`)的当前阶段——不是 `H\<txn>\journal.json`。
2. **安装目录文件集**:Windows 带 SHA-256(`install-listing.txt`),macOS 是 `ls -laR`。
3. **`diagnostics.log`**:`BT_UPDATE_START` / `BT_UPDATE_APPLY` / `BT_UPDATE_RECOVER` 行(每条
   目前写两遍,D-11)。
4. **Run 值**(Windows):`reg export HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`。
5. **LaunchAgent plist**(macOS):`~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-*.plist`。
6. **结束时有没有一扇 Folio 窗**,以及它的卡片(截图)。

#### 这套脚本从哪来

2026-09-27/28 的 U-31(Windows,候选版 A1–A7)和 U-32(macOS,两轮)没法用仓库里原来的
`hard-reset-in-vm.ps1` / `in-guest-updater.ps1`:监视器找的是 `H\<txn>\journal.json`,构建写的
是 `H\journal.json`,所以哪一行的哨兵都写不出来(H-1);宿主不带 `-noWait` 地等监视器(H-2);
`ui-probe.ps1` 没有 BOM,放在一层目录下还会解析失败(H-3);文档说的 `Tab` 会按到 Skip/Later
(H-4)。两次演练各自写了一套能用的,证据在 `tickets-046\reports\U-31-checklist.md`、
`RC-046-vm-recheck.md` 与 `U-32-rehearsal.md`。T-CLEANVM-SCRIPTS(0.4.7)把这两套搬进
`updater/`,删掉旧的两份脚本,只留一条路;搬的时候改了:行表收进 `rows.ps1`、口令只走参数、
机器路径与账号名去掉、ui-probe 放在哪一层都能用;macOS 那套的 H2(未到达时冻结的进程不
继续)、H3(没送达信号也记 `KILLED`)、H4(没有前台进程时 no-op 报错)、H6(D14 不退出 trial
窗)修掉,stuck-a/b 的预期按 U-32 计划缺陷 8 改正。**这份仓库版本尚未整轮跑过**:下一次演练
是它的第一次。

**2026-10-15(H6)演练脚本的改动。**

- **macOS 截图只截 Folio 的窗。** 全屏截图曾拍到这台 Mac 上一扇远程桌面窗口和上面的密钥。`row.sh` 的
  `capture_folio_window` 经 `osascript -l JavaScript`(`CGWindowListCopyWindowInfo`,只读窗口号与所属进程,
  不读标题与像素)找本文件夹进程最前面的一扇屏上窗,用 `screencapture -l <窗口号>` 只截它;找不到就不截,
  在该行日志里写一行 `REFUSED`,绝不退回全屏。没有本文件夹进程时(`after-cleanup`)与 R-D5 的系统提示框
  (不属于 Folio 进程)因此如实拒绝。`row.sh --capture-dry-run <应用名>` 只找窗、从不截图。
- **`-KillNewInstallProcess` 在句柄上等。** 原先 `Stop-Process -Force` 之后既不等也不核实就记「ended」;
  现在第一次看到这个进程时就打开并持有它的句柄(句柄开着,pid 不会被别的进程重用),路径核对、`Kill()`
  与等待退出都落在这一个进程上;界限 `$KillWithinMs` = 2 秒(产品的 `CHILD_EXIT_BUDGET`),到界仍在运行则该行
  `FAILED` 并写出 pid,不再往下走。
- **W4 不再空转。** 原先 120 秒墙钟、到时不判失败,主循环读到仍是 `Handoff` 又进同一自旋,直到监视器的总超时(切电行是 `-MarkerTimeoutSeconds`,
  默认 600 秒);
  现在 `Wait-RunValue` 等 Run 值,界限 `$W4RunWithinMs` = 90 秒(`old_within` 60 秒加 30 秒),日志目录项变了
  而 Run 值没来即结束等待,到界该行 `FAILED` 并写出最后所见。其余毫秒级行的自旋同理,界限 `$SpinWithinMs`
  = 180 秒。三个界限的推导在 `guest/waits.ps1` 文件头。
- **失败传到宿主。** `run-row.ps1` 把切电行的哨兵拷回(`marker.txt`),并在每次收集回来的监视器日志里找
  `ROW <行> FAILED`;哨兵缺失或为 `MISSED`/`NOT_REACHED`/`FAILED`、或日志里有失败行,该行在宿主上记为
  FAILED。最后打印各行结果表,有失败则以退出码 1 结束。

---

### 4.5 winget local manifest: two clients (every release) <!-- zh: pending T-WINGET-TWO-CLIENTS -->

`docs/RELEASING.md` "Validating before submitting" is the rule; this is how the row is run.
It runs on **`folio-win11` only**: the install fetches `InstallerUrl` from the release
page, and `folio-win10` has no network adapter (§2.4). Both cells start from the `clean`
snapshot and run in one session, image client first, because the second cell changes the
client.

| Cell | Client | How the client gets there | Expected shape |
| --- | --- | --- | --- |
| **W-a** | the image's App Installer, as it is (`clean` carried **1.6.10121** on 2026-10-08) | nothing: record `winget --version` | `folio` resolves to `%LOCALAPPDATA%\Microsoft\WinGet\Links\folio.exe`, a symbolic link whose target is `<package folder>\folio-<version>\folio.exe`; user `PATH` gains `WinGet\Links`; `diagnostics.log` says `Folio was started as …\WinGet\Links\folio.exe and runs from …\folio-<version>\folio.exe; its own files are looked for there` |
| **W-b** | the current App Installer (**1.29.290** on 2026-10-08) | updated in the guest, steps below; record `winget --version` | `folio` resolves to `<package folder>\folio-<version>\folio.exe`; user `PATH` gains that folder; nothing named `folio.exe` in `WinGet\Links`; no line about where Folio was started |

Both cells, in addition: install exits 0; `folio --version` prints this release's version;
`conpty.dll`, `OpenConsole.exe` and `folio.msix` are in `<package folder>\folio-<version>\`;
Folio started by `folio` with one pane open has that folder's `conpty.dll` in its module list
(`(Get-Process folio).Modules | Where-Object ModuleName -eq 'conpty.dll'`) and **no**
`ConPTY: this pane runs on the Windows inbox ConPTY` line in `diagnostics.log`;
`winget uninstall WeiyiShi.Folio` leaves no package folder, no `PATH` entry and no link.

Recorded, not judged: winget's `Archive scan detected malware. Proceeding due to
--ignore-local-archive-malware-scan` line, and `ZoneId=3` in every extracted file's
`Zone.Identifier` stream (RELEASING.md, Troubleshooting).

Red cells:

- **W-b with the link shape**: `ArchiveBinariesDependOnPath` no longer takes effect on the
  current client. The manifest is not submitted until that is understood.
- **Either cell on the inbox ConPTY**: Folio does not find its own files through the road
  that cell exercises. Up to 0.4.7 W-a was exactly this (the 0.4.7 post-publish row:
  `…\WinGet\Links\conpty.dll is missing`); from 0.4.8 it is a regression.
- **W-a with the folder shape** is not red: it is what winget does on a machine where it cannot
  make a symbolic link. The row says which shape it met. When the image's own client is 1.9 or
  newer, W-a cannot reach the link road; the row says so and W-a expects W-b's shape.

Steps (PowerShell in the guest; the two settings need an elevated one):

1. Copy `packaging/winget/manifests/w/WeiyiShi/Folio/<version>/` into the guest.
2. Elevated: `winget settings --enable LocalManifestFiles` and
   `winget settings --enable LocalArchiveMalwareScanOverride`.
3. **W-a**: `winget --version`; `winget install --manifest <folder> --ignore-local-archive-malware-scan`;
   in a new shell (the `PATH` change reaches only new ones), `Get-Command folio -All`, the
   user `PATH`, the `WinGet\Links` listing with each entry's target, the package folder's
   listing, `folio --version`; start `folio`, wait for the pane's prompt, read the module list
   and the new `diagnostics.log` lines, close it; `winget uninstall WeiyiShi.Folio`; the
   three listings again.
4. **Update App Installer** (measured 2026-10-08: the bundle alone is refused with
   `0x80073CF3`, the framework `Microsoft.WindowsAppRuntime.1.8` missing):
   - download `https://aka.ms/getwinget` (the App Installer bundle) and
     `https://github.com/microsoft/winget-cli/releases/latest/download/DesktopAppInstaller_Dependencies.zip`;
   - `Add-AppxPackage` each `x64` package in the dependencies archive, then the bundle;
   - `winget --version` again, and step 2 again (the 0.4.7 run enabled both settings again on
     the new client).
5. **W-b**: step 3's commands, unchanged.

Evidence: `vm-live\winget\a-image\` and `vm-live\winget\b-current\`, each with the act logs,
the elevated install's output and the shots. Report the two client versions on the row's
line: `PASS on <current> / PASS on <image's>`, or which cell went red and its exact
`diagnostics.log` line.

### 4.6 Windows PowerShell under a lockdown (every release) <!-- zh: pending T-INTEGRATION-INJECT-2-BUILD -->

The rule is RULES §38: a Windows PowerShell 5.1 pane is given Folio's PSReadLine only when the
edition answered `FullLanguage`; on a machine whose 5.1 runs constrained the pane is exactly the
Shell-integration-off pane. A lockdown cannot be entered from a test on the development machine
(`__PSLockdownPolicy` is read from the machine's block only, and writing it needs an
administrator), so these two rows are where it is proved. **`folio-win11`, an elevated shell in
the guest, the `clean` snapshot first; run by the coordinator at release time.**

| Row | Make the lockdown | Check |
| --- | --- | --- |
| **V0** — the debug lockdown (cheap) | `[Environment]::SetEnvironmentVariable('__PSLockdownPolicy','4','Machine')`, sign out and in; `powershell -NoProfile -Command $ExecutionContext.SessionState.LanguageMode` says `ConstrainedLanguage` | Folio started, the first Windows PowerShell pane of the launch and a second one: `$env:PSModulePath` holds no `psmodules` entry; `(Get-Module PSReadLine).ModuleBase` is the in-box 2.0.0; colouring and `Ctrl+R` history search work (the in-box editor); `diagnostics.log` has the one "runs constrained" line and, for the first pane, the "waited N ms" line. Revert: delete the variable, or the snapshot. |
| **V1** — a real WDAC lockdown | `$x="$env:TEMP\clm.xml"; Copy-Item "$env:windir\schemas\CodeIntegrity\ExamplePolicies\DefaultWindows_Enforced.xml" $x`; admit Folio (unsigned): `New-CIPolicy -Level Hash -UserPEs -ScanPath '<Folio install folder>' -FilePath "$env:TEMP\folio.xml"`, `Merge-CIPolicy -PolicyPaths $x,"$env:TEMP\folio.xml" -OutputFilePath $x`; `ConvertFrom-CIPolicy -XmlFilePath $x -BinaryFilePath "$env:TEMP\clm.cip"`; `CiTool.exe --update-policy "$env:TEMP\clm.cip"`; `CiTool.exe --list-policies` shows it enforced; a new 5.1 says `ConstrainedLanguage`. Check `Get-Command New-CIPolicy` first (the ConfigCI module). | The rows of V0. A second pass with `AllowMicrosoft.xml` (audit mode deleted: `Set-RuleOption -FilePath $x -Option 3 -Delete`) records the same. Remove: `CiTool.exe --remove-policy <PolicyID>`, or the snapshot. |
### 4.7 Load row: the window thread under a machine-wide CPU load (T-WINDOW-THREAD-UNDER-LOAD) <!-- zh: pending T-WINDOW-THREAD-UNDER-LOAD -->

The owner's report of 2026-10-18 on a machine with nothing else running. `folio-win11`,
`clean` snapshot, a candidate's `folio.exe`, `conpty.dll` and `OpenConsole.exe` (a release
build's `target\release`), copied in; no install.

```powershell
$pass = Get-Content <the VM password file>   # into a variable, never printed
pwsh -File scripts/release/cleanvm/run-load-row.ps1 -Vmx <folio-win11.vmx> -VmPassword $pass `
    -Candidate <target\release of the candidate> -Tag before
```

`run-load-row.ps1` reverts, starts `nogui`, copies the candidate and `guest\load-row.ps1`,
`guest\cpu-hog.ps1`, `ui-probe.ps1` in, runs the row in the logged-on session and copies out
`load-<Tag>.txt`, `-perf.txt` (Folio's stderr under `BT_PERF_TRACE=1`), `-diagnostics.log`,
`-hog.txt`; `read-load-row.ps1` prints the cells (`cells-<Tag>.txt`). In the guest: four rows
with a box rule printed in the first tab (a frame candidate, as an agent's TUI is), a second
tab opened over it, then three phases of `-Seconds` (60) each — **baseline**, **hog**
(`cpu-hog.ps1`: 2 x logical processors spinners at the Normal priority class), **after** —
with the person's cycle every two seconds: `1..400|%{$_}` typed and Enter, three wheel notches
up and three down.

| Cell | Read from | Expected (the fix) | Red |
| --- | --- | --- | --- |
| turns/s | the budget lines' `turn=` over the phase | the turn rate of the work: tens to a few hundred a second | thousands a second (the loop spins) |
| stale turns, max, sources | `stale us=` lines and their `suppressed=` | none past a few `SearchWalk`/`ImeCursor` turns | a `stale` line a second naming a clock, `ForegroundProgram` in the unmodified build |
| delay max | `delay_us=` on the `wall` lines | under one frame in baseline and after; the hog phase says what the guest's scheduler gives | — (reported, not judged) |
| present cadence | `since_previous_us` of `BT_PERF_TRACE present` | about the display's frame while the pane prints | presents stop while input is sent |
| echo / notch / output | `event_to_present_us` by `source=` | within a frame or two in every phase | seconds |
| close | `close ok` / `FAILED close` in `load-<Tag>.txt` (with the processes still standing and `load-<Tag>-hang-reports.zip`) | Folio exits within 30 s of WM_CLOSE | still running: the row's failure |

**The row runs with the first-run card already answered.** Before it launches the candidate, the guest script places `guest/load-row-settings.json` as the clean account's `%APPDATA%\Folio\settings.json`: a schema-25 document, every value the shipped default but the PowerShell invitation (`Declined`). The candidate's own migrations carry it to its version, and the v31 → v32 step writes `first_run_card: Shown`, the product's rule that a returning reader never meets the card. The welcome card and the invitation are therefore not over the pane, and they are not under test here; the first-run card is a row of its own. A guest that already holds a `settings.json` is left as it is.

A phase whose driver cycles never reached Folio is printed `NOT EXERCISED`, and its cells are not a result. The guest is told not to sleep before the row (`powercfg` standby, monitor and hibernate timeouts on AC to 0). The row runs `-NoWait`, and the host waits for `load-<Tag>-done.txt` (bounded at 3 × `-Seconds` + 480 s (the driver waits up to 180 s for the foreground)).

Run it twice: `-Tag before` on the build at the instrumentation commit (the named fold and the `stale_us`/`source=` cells, no fix), `-Tag after` on the fix; the report carries both tables.

**Recorded 2026-10-21** (folio-win11, 2 logical processors, so the hog runs 4 Normal-priority spinners). before3 is the instrumentation build without the fixes (7d00c2ed); after10 is the fix (1b911da7, with main 453c0c7c), on the harness at 81779f7a:

| cell | before3 (7d00c2ed) | after10 (1b911da7) |
|---|---|---|
| exercised | yes: the boxed tab printed, 18 / 17 / 19 cycles | yes: the driver chose `class=Window Class title=folio`; 19 / 18 / 20 cycles; keyboard presents 247 / 244 / 259 |
| turns/s (baseline / hog / after) | ≥103 / ≥127 / ≥97 | ≥83 / ≥152 / ≥55 |
| delay max | 0 / 0 / 26.5 ms | 0 / 0 / 0 |
| stale turns (count, max) | 289, 399 ms / 374, 3.4 ms / 397, 437 ms | 229, 532 ms / 549, 462 ms / 262, 526 ms |
| stale sources | LiveStability, StripAnimation, RefusedFrame (1–5 ms; one 42 ms), ShellCaret, ProgramsEar; TerminalThumbs ~0.4 s twice a phase | LiveStability, RefusedFrame, StripAnimation, TerminalThumbs (~0.5 s), ShellCaret (after) |
| ForegroundProgram spin | none: the boxed tab was not an armed candidate | none (and the entry is no longer booked) |
| keyboard echo p50 / p95 | 5.5 / 25 → 21.8 / 50 → 6.6 / 21 ms | 3.9 / 11.8 → 15.9 / 36.9 → 5.0 / 13.3 ms |
| present cadence p50 | 8.3 → 18.9 → 9.9 ms | 5.9 → 17.8 → 16.0 ms |
| shell output p50 | 3.1 → 3.3 → 4.2 ms | 2.3 → 3.1 → 3.6 ms |
| close | ok | ok: `WM_CLOSE posted -> True`, Folio exited |

**What the row measures, and what it does not.**
- **CPU starvation.** Under the hog, keys echo about 4× slower at the median (3.9 → 15.9 ms; 5.5 → 21.8 ms on before3) and recover after. The present cadence settles at about one per vblank. Shell output barely moves. No wake is ever late (`delay max` 0 in every phase on the fix). That is the per-turn cost residual, measured and not fixed in T-WINDOW-THREAD-UNDER-LOAD.
- **The stale turns** are the single-extra-turn class: clocks that came due inside the turn that booked them, at 100-odd turns a second. They are not a spin. One shape is recorded for follow-up: `TerminalThumbs` booking an instant about 0.5 s already past, once or twice a phase.
- **The owner's spin is not reproduced here.** That needs a background tab whose pane keeps redrawing a boxed frame (an armed foreground-program candidate), and this row's boxed tab is printed once. That shape is §4.7b, a 0.4.9 row (row 26), with a trace line that the cadence armed.

**The ten runs that measured nothing, and why** (each fixed in the harness this row lands with):

| runs | cause | fix |
|---|---|---|
| before2, after, after3, after4 | the guest suspended itself (ACPI S1, 13–21 min after boot) while a slow logon held `Wait-GuestReady` | `no sleep` steps (`powercfg` standby / monitor / hibernate on AC to 0); the readiness wait prints what it waited for and how long |
| after5 | the `no sleep` step handed powercfg its words as one argument | through `cmd /c`, not tolerant |
| the 10-20 run, after6 | the driver was refused the foreground; the first-run card stayed up | the row waits up to 180 s for the keyboard, else `FAILED foreground`; the first run is answered before the launch |
| main453, after7 | the foreground was taken but not the keyboard (`input_seen=false` all run) | the driver checks the foreground thread's focus window (`GetGUIThreadInfo`) and clicks a verified pixel otherwise; the reader marks `KEYS NOT SEEN BY FOLIO` |
| after8 | the desktop held the foreground; the click point found no window | an Alt tap before the foreground is asked; a click at the window's verified centre; the close recorded on every road |
| after6, main453, after7, after9 (the close) | `Process.CloseMainWindow()` and the probe's `AppWindow` both picked winit's `Winit Thread Event Target` popup, so the row typed into and closed a window that draws nothing | the row drives and posts WM_CLOSE to the visible window of winit's application class (`Window Class`); every top-level window is logged |

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

### 3.4a 进客户机的脚本必须带 UTF-8 BOM(2026-08-27 门 5 首跑事故)

客户机里跑 `smoke.ps1` 与 `in-guest.ps1` 的是 **Windows PowerShell 5.1**,它读无 BOM 的文件按 ANSI 代码页解;两份脚本都有非 ASCII 文本(一处消息里的破折号就够了),按 ANSI 读进去的字节把一个带引号的字符串拆坏,连带整个 `switch` 解析失败——首跑在 `unpack` 阶段以「Guest program exited with non-zero exit code: 1」结束,宿主上看不到原因,把 `-Phase unpack` 的输出重定向到文件再 `copyFileFromGuestToHost` 回来才见到 `Unexpected token`。现在两份脚本带 BOM,`run-smoke-in-vm.ps1` 在复制之前先验 BOM,缺就在宿主上拒绝。宿主侧脚本跑在 pwsh 7(默认 UTF-8),不受影响。

### 3.4b 进客户机的脚本还得是 5.1 的方言(2026-08-28 门 5 次跑事故)

BOM 修好之后 `unpack` 过了,**`smoke` 阶段当场死在下一行**:

```
smoke.ps1 : A positional parameter cannot be found that accepts argument '..'.
    + CategoryInfo          : InvalidArgument: (:) [smoke.ps1], ParameterBindingException
    + FullyQualifiedErrorId : PositionalParameterNotFound,smoke.ps1
```

`Join-Path $PSScriptRoot '..' '..'` 在 pwsh 7 里是三段路径,在 5.1 里是「多给了一个位置参数」。
干净机上只有 5.1,所以**凡是要进客户机的脚本,都按 5.1 写**:两参数 `Join-Path` 套两层,不用
`??` / `?:` / `-AsByteStream` / `Split-Path -LeafBase` 一类 7 独有的东西。

同一轮在本机 5.1 上跑出来的还有三条,都不是语法而是行为差异:

| 现象 | 成因 | 现在怎么写 |
| --- | --- | --- |
| `folio exited `(退出码是空的) | 5.1 对**带重定向的** `Start-Process -PassThru` 返回的 `Process` 从没打开过进程句柄,进程走了以后 `.ExitCode` 永远答 `$null` | 起完就 `[void] $process.Handle` |
| 证据里的 `diagnostics.log` 首行是 `â”€â”€ Folio 0.1.0` | Folio 写的是无 BOM 的 UTF-8;`Get-Content` 在 7 里按 UTF-8 解,在 5.1 里按 ANSI 代码页解 | 凡读 Folio 写的文件一律 `-Encoding UTF8` |
| `web` 阶段拍回一张 26×26 的「卡」,窗口还 `WM_CLOSE` 不动 | winit 有一个**常驻可见、无属主的 13×13** 消息窗(类名 `Winit Thread Event Target`),它比真窗口早三秒出生,「可见且无属主」的搜索先撞上它 | 按 Alt-Tab 的规矩再加一条:排掉 `WS_EX_TOOLWINDOW`(winit 那个 `ex=0x080800A0`,Folio 的窗口是 `ex=0x00040110`) |

**红门也随之多了一道**:`run-smoke-in-vm.ps1` 复制之前,除了验 BOM,还用宿主自己的
`powershell.exe`(就是 5.1)把两份脚本各解析一遍,不过就拒绝。解析器只管语法——上面那条
`Join-Path` 是**绑定**错误,解析得干干净净——所以改这两份文件的办法是先在宿主上对着解出来的 zip
跑一遍:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/release/smoke.ps1 `
    -Exe <解压出来的>\folio.exe -Artifacts <某个临时目录>
```

### 3.4c `vmrun`:`-vp` 要给全,Tools 状态不能当判据,还要等到能交互(2026-08-28)

同一轮里 Win11 那台连跑不起来,四条都在 `run-smoke-in-vm.ps1` 的等待环节:

1. 等待环节自己拼了命令行,**漏了 `-vp`**,于是加密的 Win11 每五秒收到一句
   `A password is required for this operation`,等满六分钟按「Tools 没起来」失败。现在所有
   `vmrun` 调用共用同一个 `Get-VmrunFlags`。
2. 补上 `-vp` 之后 `checkToolsState` 答的是 **`installed` 而不是 `running`**,而那台机此时早已
   自动登录、桌面画完、`captureScreen` 与 `fileExistsInGuest` 都答得上来。§2.3 早写着
   `checkToolsState` 只能当进度指示;现在等待环节直接**等一次能用的 guest 操作**
   (`fileExistsInGuest … C:\Windows\System32\cmd.exe`),因为后面每一步要的正是这个。
3. 那还不够:五个阶段全走 `-interactive`(§3.4「自动登录是必需的」),而自动登录比 Tools 晚**几十秒**
   到位,这中间 `vmrun` 答的是

   ```
   Error: The specified guest user must be logged in interactively to perform this operation
   ```

   ——本轮有一次 `unpack` 就这么丢了。所以等待分两段:先等一次能用的 guest 操作,再等一个
   **能跑起来的 `-interactive` 程序**(`cmd.exe "/c exit 0"`)。
4. 上面那对引号是要紧的:**`vmrun` 把客户机程序的参数当一条命令行发过去,第一段之后的分段不保留。**
   本机实测同一台机上 `… cmd.exe /c ver`(写成三段)答
   `Guest program exited with non-zero exit code: 1`,而 `… cmd.exe "/c exit 0"`(一段)答 0。

**另一件同源的事:回快照之后重新开机的机器不会再自动登录**,`-interactive` 一律被拒。想在冒烟之后
接着在同一台机上手工做点什么,得给那一轮加 `-KeepRunning`,而不是事后再 `start` 一次。

> `new-vm.ps1` 的装机轮询里 `checkToolsState` 同样没带 `-vp`(第 809、831 行)。那里只是打进度,
> 不影响判据,**本轮没改**。

### 3.4d 出厂策略:两台机的 `clean` 快照回到 `Restricted`(2026-08-30)

#### 为什么

两份应答文件的 `FirstLogonCommands` 里原来有一条:

```
powershell -NoProfile -Command "Set-ExecutionPolicy -Scope LocalMachine -ExecutionPolicy RemoteSigned -Force"
```

理由写的是「让宿主能在这台机里驱动 PowerShell」。**宿主从来不需要它**:`run-smoke-in-vm.ps1` 每一次
进客户机的调用都是 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File …`(`Invoke-GuestPowerShell`,
该文件第 245 行),而 `-ExecutionPolicy` 是**只管它自己启动的那一个进程**的设定,不问机器策略要任何
许可。`in-guest.ps1` 里 `& $script` 调 `smoke.ps1` 走的是同一个进程的 Process 作用域,同样在
`Bypass` 之下。所以那一行删掉,**烟测本身一个字都不受影响**。

它实际做的是另一件事:**把门 5 的两台机从「用户手上那台 Windows」上挪开了**。Windows 客户端出厂的
执行策略是 `Restricted`,而 `docs/DESIGN.md` §7.47 正是「新电脑上 PSReadLine 开关点了没反应、
一言不发」——那条缺陷的成因就是这个出厂默认。**发布路径上没有一台机器走过 `Restricted` 那条路**,
于是这一类缺陷门 5 根本看不见。这就是改的理由。

#### 快照怎么改的

两台机各一趟,**没有重装**:回 `clean` → 开机 → 等一次能用的 guest 操作(§3.4c,不是 sleep)→
以 `folio` 身份跑一段脚本 → 关机 → 原地重拍同名 `clean`。逐条:

```powershell
vmrun -T ws [-vp <口令>] revertToSnapshot <vmx> clean
vmrun -T ws [-vp <口令>] start <vmx> nogui
# readiness:fileExistsInGuest C:\Windows\System32\cmd.exe,五秒一次,和 run-smoke-in-vm.ps1 同一个探针
vmrun -T ws [-vp <口令>] -gu folio -gp folio copyFileFromHostToGuest <vmx> <脚本> C:\folio-vm\factory-policy.ps1
vmrun -T ws [-vp <口令>] -gu folio -gp folio runProgramInGuest <vmx> `
    C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\folio-vm\factory-policy.ps1
vmrun -T ws [-vp <口令>] -gu folio -gp folio copyFileFromGuestToHost <vmx> C:\folio-vm\execpolicy.txt D:\VMs\tmp\<机名>-execpolicy.txt
vmrun -T ws [-vp <口令>] -gu folio -gp folio deleteFileInGuest <vmx> C:\folio-vm\factory-policy.ps1
vmrun -T ws [-vp <口令>] stop <vmx> soft
vmrun -T ws [-vp <口令>] snapshot <vmx> clean-restricted
vmrun -T ws [-vp <口令>] deleteSnapshot <vmx> clean
vmrun -T ws [-vp <口令>] snapshot <vmx> clean
vmrun -T ws [-vp <口令>] deleteSnapshot <vmx> clean-restricted
vmrun -T ws [-vp <口令>] listSnapshots <vmx>
```

放进去的两个文件在关机前用 `deleteFileInGuest` 删掉,快照里不留我们的东西;最后那一条
`listSnapshots` 必须只答一行 `clean`。

**`vmrun` 没有 rename**,所以「同名重拍」只能是删了再拍。中间那个 `clean-restricted` 是为了不出现
「一个快照都没有」的时刻:删父快照 VMware 会把它并进子快照,顺序对了三步之后 `clean` 就是当前这个
状态。快照名最终仍是 **`clean`** —— `run-smoke-in-vm.ps1` 按这个名字找,§3.5.5 也是这么写的。

**客户机里跑的那一段**(一次性,不入库;5.1 方言 + UTF-8 BOM,§3.4a/§3.4b),关键只有一行:

```powershell
Set-ExecutionPolicy -Scope LocalMachine -ExecutionPolicy Undefined -Force
```

`Undefined` 而不是 `Restricted`:出厂状态是**注册表里根本没有那个值**,而不是有一个写着 `Restricted`
的值。改完 `HKLM\SOFTWARE\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell` 下的 `ExecutionPolicy`
消失,五个作用域全部 `Undefined`,`Get-ExecutionPolicy` 于是答内置默认 `Restricted`。

两条读法上的讲究,不注意就会量到假数:

- **要在一个不带 `-ExecutionPolicy` 旗标的子进程里读。** 跑这段脚本的进程自己是
  `-ExecutionPolicy Bypass` 起来的,`Get-ExecutionPolicy -List` 的 Process 一行会答 `Bypass`;而且
  那个旗标是靠环境变量 `PSExecutionPolicyPreference` 传给子进程的,所以要先
  `Remove-Item Env:PSExecutionPolicyPreference` 再起子 `powershell.exe`。
- **写 HKLM 要完整令牌。** `runProgramInGuest` **不带** `-interactive` 时,Tools 服务(SYSTEM)用
  `folio` 的完整令牌起进程,脚本自报 `elevated : True`,`Set-ExecutionPolicy -Scope LocalMachine`
  一次就过。桌面上那个自动登录的 `folio` 拿的是被过滤的令牌。

顺带一条 5.1 的写法坑:`powershell.exe -Command '… $_.Scope.ToString() + "=" + …'` 里那对双引号会被
命令行吃掉,当场是语法错误;写成 `+ [char]61 +` 才对。

#### 改完之后两台机是什么样(2026-08-30 实测)

| | `folio-win10` | `folio-win11` |
| --- | --- | --- |
| 机器 | `FOLIO-WIN10`,Windows 10 Pro 10.0.19045,PS 5.1.19041.3803 | `FOLIO-WIN11`,Windows 11 Enterprise Evaluation 10.0.26200,PS 5.1.26100.6584 |
| 改前 | LocalMachine `RemoteSigned`,其余四个 `Undefined`,有效 `RemoteSigned`,注册表 `RemoteSigned` | 同 |
| 改后 | 五个作用域全 `Undefined`,有效 **`Restricted`**,注册表值不存在 | 同 |
| 快照 | `listSnapshots` 只剩 `clean` | 同 |

#### 又一条 `vmrun` 坑:加密机上 `deleteSnapshot` 会谎报失败

Win11 那台(partial 加密,§3.5.1)两次 `deleteSnapshot` 都答

```
Error: Cannot read the virtual machine configuration file
```

并以 `-1` 退出 —— **而快照其实已经删掉了**,紧接着的 `listSnapshots` 答得干干净净。Win10 那台
(不加密)四步一次没报错。所以在加密机上,`deleteSnapshot` 的判据是 `listSnapshots` 的结果,
不是它的退出码。


## 8. 已知空白与未验证项(一处列全)

| 项 | 状态 |
| --- | --- |
| 两份 `autounattend-*.xml` | **2026-08-27 第一次真跑,改了两处**。① Win10 的 `<ProductKey>` 缺 `<Key>`,Setup 当场停在消息框上;② 两份的 `oobeSystem` 都缺 `Microsoft-Windows-International-Core`,两台机都停在 OOBE 地区页。均见 §3.4 |
| `FirstLogonCommands` 新加的三条(装 Tools / RunOnce 哨兵 / 重启) | **未验证** —— 语法与开关有来源(§3.5.4),但没在真机上跑过 |
| Win11 25H2 的 OOBE 在**联网**时是否还认 `HideOnlineAccountScreens` 而不强推微软账号 | **未验证** —— 近期 Win11 有此报告;Win10 那台没网,不受影响。第一次装 Win11 若停在账号页,这是第一嫌疑 |
| `new-vm.ps1` 写出的 `.vmx` 能不能真的开机装完 | **部分已验证(2026-08-27 晚)** —— 两台机都从这个 `.vmx` 引导进了 Setup;前提是安装盘按 §3.1a 重制过 |
| **原样的微软安装 ISO 在无人值守下能不能引导** | **已验证为不能** —— §3.1a。`efisys.bin` 的 `cdboot.efi` 等按键,三秒后 `Status upon boot failure: Time out` |
| **重制成 `efisys_noprompt.bin` 引导后能不能引导** | **已验证能** —— §3.1a。两台机 `About to do EFI boot` 之后不再有 `Time out` / `OsNotFound` |
| **partial 加密的 `.vmx` 手改明文键之后 `vmrun` 还认不认** | **已验证认** —— §3.5.1。副本上改 `sata0:0.fileName` 后 `-vp` 只读命令照答,错口令仍答 `Incorrect password`;真机 `vmware.log` 印着改后的路径 |
| IMAPI2 的 `BootImageOptionsArray` 能不能在 PowerShell 里设 | **已验证不能** —— 一律 `E_NOINTERFACE`,三种写法都试过(§3.1a 末尾)。脚本改用单数的 `BootImageOptions` |
| `new-install-iso.ps1` 的 `pycdlib` 取出分支 | **未验证** —— 本机装着 7-Zip,这一支从没被选中过 |
| `bios.bootOrder` 在 EFI 固件下的确切键名 | **未验证** —— 第三方引用一致,官方文档没找到;盘是空的时候不影响首次引导 |
| `managedvm.autoAddVTPM = "software"` 在 17.6.4 上是否仍然让 `vmrun start` 失败 | **未验证** —— 报告见 §2.2a,本机没试;`-VTpm software` 是留给试它的开关 |
| 加 vTPM 前的「剩余空间要够整盘大小」检查 | **未验证** —— 二手报道(§2.2a 末尾) |
| **已验证(本机 2026-08-27 探测)**:`virtualHW.version = 21` 是 17.6.4 自己写的 | `vmcli VM Create` 产物 |
| **已验证**:Workstation 无 `windows10-64`,Win10 是 `windows9-64` | `vmcli VM Create -g windows10-64` 报错并列出合法值;`isoimages_manifest.txt` |
| **已验证**:`windows.iso` 在 Workstation 安装目录,根目录只有 `setup.exe`(没有 `setup64.exe`),Tools 12.5.3 | 挂载核对 |
| **已验证**:`vmware-vdiskmanager -a nvme` 被接受(用法文本里没列),写出的描述符 `ddb.adapterType` 仍是 `lsilogic` | 本机建过 1 GB 测试盘再删 |
| **已验证**:`vmrun` 用法表把 `fileExistsInGuest` 等全部归在 GUEST OS COMMANDS,`checkToolsState` 归在 GENERAL | `vmrun` 无参输出 |
| **已验证**:本机 `vmcli` 没有任何加密/TPM 模块 | `vmcli --help` |
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
| 已验证:四个 `.ps1` 语法、两个 `.xml` 格式(改后重新解析过)、`run-smoke-in-vm.ps1` 与 `new-vm.ps1` 的 `-WhatIf` 全流程 | 本机跑过 |
| 已验证:`new-vm.ps1` 的幂等拒绝与 `-Stage install` 的前置检查 | 本机跑过(用临时目录) |
| **已验证**:两台机的 `clean` 快照已回到出厂 `Restricted`(五个作用域全 `Undefined`) | 2026-08-30 逐台实测,§3.4d |
| **已验证**:加密机上 `deleteSnapshot` 报「Cannot read the virtual machine configuration file」并退 `-1`,而快照确实删掉了 | 同上;判据用 `listSnapshots` |
| §4.4 更新器:后继版本注入口 | **已有(U-30b)** —— `--update-feed <file-URL>`,只在带它启动的进程里生效,校验和与签名者检查不变;见 §4.4 前提 3。Windows 的 `OFFERS_ENABLED_WINDOWS` 已由 U-31 打开;macOS 的 `OFFERS_ENABLED_MACOS` 已由 U-32 打开;U-31、RC-046 在真虚机上用过 |
| §4.4 更新器:macOS 断电验收虚机 | **不存在** —— Mac mini 无可处置的 macOS 客户机(见 §Clean-machine coverage 的数字);Mac mini 本身不做断电。U-32 以 option B 在所有者账号里用 `SIGSTOP` + `kill -9` 跑了 M 表(§4.4「macOS 步骤」),这比断电弱:页缓存还在,缺 flush 看不出来 |
| §4.4 更新器:`updater/` 下的仓库版驱动 | **部分已验证** —— 2026-09-29 由 U-31 / U-32 的演练脚本搬入(旧的 `hard-reset-in-vm.ps1`、`in-guest-updater.ps1` 删除);`-WhatIf` 与 `scripts/release/cleanvm-tests.ps1` 跑通;仓库版尚未在真虚机和 Mac 上整轮跑过,下一次演练验 |

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
- Workstation Pro 17 加密虚机(快速加密 / 全盘加密,口令丢了找不回):
  <https://techdocs.broadcom.com/us/en/vmware-cis/desktop-hypervisors/workstation-pro/17-0/using-vmware-workstation-pro/configuring-and-managing-virtual-machines/encrypting-and-restricting-virtual-machines-ws-guide.html>
- 虚拟机硬件版本对照表:
  <https://knowledge.broadcom.com/external/article/315655/virtual-machine-hardware-versions.html>
- VMware Tools 静默安装(`setup.exe /S /v"/qn REBOOT=R"`):
  <https://knowledge.broadcom.com/external/article/376237/installing-vmware-tools-on-a-vmware-vsph.html>
- VMware Tools 静默安装的组件选择(同一条命令的官方形态与 `ADDLOCAL`/`REMOVE`):
  <https://techdocs.broadcom.com/us/en/vmware-cis/vsphere/tools/12-5-0/vmware-tools-administration-12-5-0/installing-vmware-tools/automatically-install-vmware-tools-on-multiple-windows-virtual-machines/specify-vmware-tools-components-for-silent-installations.html>
- 给 Windows 客户机加显示分辨率(`svga.autodetect` / `svga.maxWidth` / `svga.maxHeight` / `svga.vramSize`):
  <https://knowledge.broadcom.com/external/article/313896/adding-video-resolution-modes-to-windows.html>
- 实验性 vTPM 的代价(`managedvm.autoAddVTPM`、最低 16.2、加密了哪些文件、自动化坏在哪):
  <https://www.vimalin.com/blog/what-you-should-know-about-vmwares-experimental-vtpm/>
- `vmrun` 开加了实验性 vTPM 的机器(`The operation is not supported`):
  <https://communities.vmware.com/t5/VMware-Workstation-Pro/How-to-1-click-start-VMs-with-16-2-TPM/m-p/2927197>
- `vmrun -vp` 开正常加密机器可行:
  <https://communities.vmware.com/t5/VMware-Workstation-Pro/Issue-vmrun-Running-with-vp-encryptedVirtualMachinePassword/td-p/2893108>
