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
