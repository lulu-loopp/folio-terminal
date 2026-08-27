# Folio v0.1 preview 就绪审计 —— 两处缺口补齐

> 日期 2026-08-27 ・ 补的是 `docs/plans/release/readiness-audit-2026-08-27.md` 第 6 节标出的两个未盘面:
> **第 4 面「签名与分发的外部行情」** 与 **第 6 面「首次体验的三条 + 三条另核」**。
> 性质:**只读**。除本文件外未改动仓库任何内容。
>
> **取证口径**:缺口 A 每条带官方 URL 与抓取日期(全部为 **2026-08-27** 当日抓取);
> 缺口 B 每条带 `绝对路径:行号`。二手来源(经销商、第三方博客)会显式标出来源等级。
> 查不到的一律标 `[未查到]`,不推测。

---

## 缺口 A · 签名与分发的外部行情

### A0. 先说结论(一句话版)

**签名在 2026 年**不再**能买到「首次运行不弹窗」**。EV 证书的即时放行在 2024 年被取消,微软自己的官方对比表里
EV 一行写的是 "Same as OV since 2024 — no longer instant bypass"。
所以对 v0.1 preview 而言,**签与不签的差别不是「有没有 SmartScreen 提示」,而是「下一个版本要不要从零开始重新积累」**。
详见 A7 的路线推荐。

---

### A1. 未签名 exe 在 Windows 11 的 SmartScreen 确切行为

**来源**:

- Microsoft Learn《SmartScreen reputation for Windows app developers》
  https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
  (同文源码 https://github.com/MicrosoftDocs/windows-dev-docs/blob/docs/hub/apps/package-and-deploy/smartscreen-reputation.md ,抓取 2026-08-27)
- Microsoft Learn《Code signing options for Windows app developers》,页面 `ms.date: 2026-08-24`
  https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options (抓取 2026-08-27)

**① 触发条件不是「未签名」,是 Mark-of-the-Web。**
SmartScreen 的这条 App Reputation 检查由文件上的 `Zone.Identifier` 备用数据流(MOTW,`ZoneId=3`)触发。
对便携 zip 这意味着:

- 从浏览器下载的 zip 带 MOTW;
- **用 Windows 资源管理器解压 → MOTW 会传播到解压出来的 `folio.exe`** → 双击触发 SmartScreen;
- 用旧版 7-Zip(< 24.09,其 "Propagate Zone.Id stream" 默认为 No)解压 → 解出来的 exe **没有** MOTW → **不触发**。
  这个差异就是 CVE-2025-0411(7-Zip MOTW bypass,2025-02 进 CISA KEV)。
  来源(二手,安全媒体):https://cybersecuritynews.com/7-zip-mark-of-the-web-bypass/ (抓取 2026-08-27)

  **对 README 的意义**:提示是**大概率会出现**的(多数人用资源管理器解压),但**不是必然**,
  所以 README 的措辞应是 "you may see…";而且**不要**教用户"用 7-Zip 解压就没提示了" —— 那是在教用户绕过 MOTW。

**② 用户看到的路径**(Windows 10 / 11 一致):
弹出标题为 **"Windows protected your PC"**(中文版:"Windows 已保护你的电脑")的模态框,
正文 "Microsoft Defender SmartScreen prevented an unrecognized app from starting. Running this app might put your PC at risk."
默认只有一个 **"Don't run"** 按钮;必须点左下角的 **"More info"**(更多信息)展开,
才会显示 App / Publisher 两行与 **"Run anyway"**(仍要运行)按钮。
**未签名时 Publisher 一行显示 "Unknown publisher"**;签名后显示证书主体名。

**③ 声誉是怎么累积的 —— 两个独立的信号。**
官方原文:

> "When a file is not signed, SmartScreen reputation must build for each new version of your files, starting with zero reputation."
>
> "Reputation cannot transfer from previous versions unless both were signed using the same publisher identity."
>
> "As downloads accumulate: SmartScreen reputation builds up automatically."
>
> "It can take several weeks and hundreds of clean installs from a wide audience."

拆开就是:

| 信号 | 载体 | 跨版本继承? |
|---|---|---|
| **file hash reputation** | 每一个具体文件的哈希 | ❌ 每发一个新 build 就从零开始 |
| **publisher reputation** | 签名证书的 publisher identity | ✅ 同一身份签的后续版本可继承 |

**未签名 = 只有第一条。** 所以 Folio 每发一个 v0.1.1、v0.1.2,SmartScreen 计数都归零。
微软官方对比表里 "No signature" 一行的 SmartScreen 列写的是
**"❌ Strong SmartScreen block; enterprises may block entirely"**,并注明 "Not recommended for public distribution"。

**④ EV 不再即时放行(本次最重要的行情更正)。**
官方原文:

> "EV certificates previously bypassed SmartScreen entirely on first download… **That behavior was removed in 2024.**
> EV-signed files now go through the same reputation-building process as OV certificates."
>
> "Paying the EV premium ($400+/year) solely to avoid SmartScreen warnings is **no longer justified**."

---

### A2. SignPath.io / SignPath Foundation 开源计划

**来源**(均抓取 2026-08-27):

- https://signpath.org/terms.html —《SignPath Foundation conditions for Open Source projects》
- https://signpath.org/
- https://signpath.io/solutions/open-source-community
- 微软官方对该计划的背书段落见 code-signing-options 页尾 "Open source: SignPath Foundation" 一节

**资格条件**(`signpath.org/terms.html` 逐条,原文):

| # | 条件 | 原文 |
|---|---|---|
| 1 | 无恶意软件 | "The project must not contain malware or potentially unwanted programs." |
| 2 | **OSI 许可、且不得商业双许可** | "The project must use an OSI-approved Open Source license without commercial dual-licensing for all components." |
| 3 | 无专有组件 | 不得包含专有 / 非开源组件(系统库除外),尤其是维护者本人或其关联组织发布的闭源件 |
| 4 | 活跃维护 | "The project must be actively maintained." |
| 5 | **已经发布过** | "The project must already be released in the form that should be signed." |
| 6 | 功能有说明 | "The project's functionality must be described on its download page or in the app store entry." |
| 7 | **可验证构建** | 二进制必须以可验证的方式从源码构建(即 **CI 出包**,不是本机丢上去的 zip) |

**接受后的义务**(同页,很多人漏掉):

- 团队所有成员必须开 **MFA**;
- 必须有明确的 **Authors / Reviewers / Approvers 角色划分**;
- **必须在项目主页发布一份 code signing policy**;
- 签名产物必须强制带上 metadata 属性;
- **每一次 release 都要人工 approve** 才会签("Every release needs manual approval for signing")。

**签发的是什么证书**:

- 证书**签发给 SignPath Foundation 本身**,不是签给项目或维护者
  —— 原文 "The code signing certificate is issued to SignPath Foundation";
  `signpath.org/` 首页把这一点当卖点写:证书以 Foundation 名义签发,**维护者不必做个人身份验证**,私钥存在 SignPath 的 HSM 里。
- **等级 = OV。** 这一条 signpath.org 自己没写死,但**微软官方页明确写了**:
  "The program provides **OV-level** certificate signing through a managed pipeline."
  (code-signing-options,`ms.date: 2026-08-24`)

**对 SmartScreen 的效果**:与任何 OV 证书相同 —— **不即时**,靠 publisher reputation 跨版本累积。
Foundation 证书是**多个项目共用一张证书**,理论上共享 publisher reputation,
但**两边官方页都没有正面说明这一点**,标 `[未查到]` —— 不要当卖点写进 README。

**限制 / 每月次数**:`[未查到]`。
`signpath.org/terms.html`、`signpath.io/solutions/open-source-community`、`signpath.io/pricing`(该 URL 返回 404)
三处都没有列 OSS 计划的签名次数配额;只查到 SignPath 的**商业** Starter / Basic 订阅有 signing request 上限。
**申请前应直接问 info@signpath.io。**

**申请流程**:`signpath.org` 导航里的 **Apply** 链接(`https://signpath.org/apply`)。
该页抓取时只渲染出标题 "Apply for a free SignPath.io subscription",表单字段未取到 → 具体填哪些内容 `[未查到]`。
社区经验(**二手来源,等级低**):https://zenn.dev/shm_7ec/articles/signpath-oss-code-signing 记录了一次完整申请过程。

**对 Folio 的差距清单**(把上表逐条对到本仓现状):

| SignPath 条件 | Folio 现状 | 差距 |
|---|---|---|
| OSI 许可、无双许可 | `Cargo.toml:22` 声明 `MIT OR Apache-2.0` | ✅ 声明对;但**根目录没有 LICENSE 文件**(审计 M1)→ 先补 |
| 无专有组件 | 554 个外部 crate 全开源(审计 M3) | ✅ |
| 已发布过 | 尚未发布 | ❌ **必须先发一版 v0.1 才有资格申请** |
| 可验证构建 / CI 出包 | `.github/workflows/ci.yml` 存在但**从未跑过**(审计 M7) | ❌ 与 M7 是同一件事 |
| 功能有说明 | 根目录**没有 README**(审计 M2) | ❌ 与 M2 是同一件事 |
| 主页发布 code signing policy | 无 | ❌ 新增一份文档 |

**结论**:SignPath 的准入门槛与审计里 **M1 / M2 / M7 三条必做项完全重合**。
把那三条做完并发出 v0.1,SignPath 的资格就自然齐了 ——
**这是 v0.1 先不签名的另一个理由:现在也还不够格申请。**

---

### A3. Azure Trusted Signing(2026 年已改名 **Azure Artifact Signing**)

**来源**(均抓取 2026-08-27):

- https://learn.microsoft.com/en-us/azure/artifact-signing/overview — `ms.date: 2026-01-02`
- https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart — `ms.date: 2026-05-21`
- https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
- https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options — `ms.date: 2026-08-24`
- https://github.com/Azure/artifact-signing-action

**① 改名**:Trusted Signing → **Azure Artifact Signing**。
资源提供程序仍叫 `Microsoft.CodeSigning`,CLI 扩展是 `az extension add --name artifact-signing`,
旧的 `azure/trusted-signing-action` 仍在,新名是 `Azure/artifact-signing-action`。

**② 个人开发者资格**(quickstart Prerequisites 的 Note,原文):

> "Public Trust certificates are available to organizations in the United States, Canada, the European Union,
> the United Kingdom, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway, and Israel.
> **Individual developers must be located in the United States or Canada.**"

**关于「要不要注册公司 / 3 年历史」**:

- **个人开发者不需要注册公司** —— Individual identity validation 是一条独立通道,不走 Organization。
- **「3 年可验证经营历史」那条规则是针对「组织」的,而且已经不在当前文档里**:
  2025-04 前后公开预览期的文档与社区问答里明确写着组织需 "verifiable tax history of three or more years";
  但 **2026-05-21 版的 quickstart Prerequisites 只列地域限制,不再提 3 年**。
  旧规则的存在证据:https://learn.microsoft.com/en-us/answers/questions/2284182/trusted-signing-is-only-available-to-organizations (抓取 2026-08-27)
  → 对个人开发者这条规则本来就不适用,不构成障碍。

**③ 个人身份验证怎么做**(quickstart "Identity Validation - Individual Developer" 一节):

1. 必须先有 **Azure 订阅 + Microsoft Entra 租户**;
2. **billing account 的 Account Type 必须是 "Individual"**,且姓名地址必须与要上证书的信息**逐字一致**
   —— 表单是**从 billing account 自动填充且只读**的,要改只能去改账单信息;
3. 走 **Microsoft Entra Verified ID**,第三方 ID 验证商(文档举例 **AU10TIX**):
   邮箱 PIN → 手机号 → 手机扫码 → **政府签发的带照片证件**(护照 / 驾照 / 州 ID / 美国社保卡;
   **不收**学生证、图书证、会员卡)+ **自拍活体**;
   证件上没有地址的还要补**近三个月的水电费账单或银行对账单**;
4. 证书主体里**会出现**:姓名、城市、州/省、国家;**不会出现**:街道地址、邮箱(可选勾选才带街道/邮编);
5. 处理时长:文档给的公开身份验证口径是 **1–20 个工作日**;个人侧 Verified ID 通过后状态"几分钟"更新。

> ⚠ **对开源作者的实际含义**:个人路线会把**你的真名 + 所在城市/州**写进每一份签名,
> 任何下载者右键属性→数字签名都能看到。
> 这正是 SignPath Foundation 路线的核心差异 —— 那条路证书在 Foundation 名下,**不暴露个人身份**。

**④ 价格**:

- 微软官方 code-signing-options 表:**"~$9.99/month"**。
- Azure 定价页(抓取时金额栏渲染为 `$-`,未取到具体数字):
  **Basic 每月含 5,000 次签名、每种 certificate profile 类型 1 个;
  Premium 每月含 100,000 次签名、每种 10 个**;超出按次计费(单价未渲染)。
- **无硬件 token 费用**(私钥在微软 FIPS 140-3 Level 3 HSM 里)。

**⑤ 签名方式**:

- SignTool + Trusted Signing 的 **dlib**;
- **GitHub Action**:`Azure/artifact-signing-action`(旧名 `azure/trusted-signing-action`),
  需 Windows runner,配 endpoint(如 `https://eus.codesigning.azure.net/`)、account name、certificate profile name、
  要签的文件通配;认证走 OIDC(GitHub → Azure)或 service principal;
- Azure DevOps task;Azure CLI / PowerShell 管理资源。
- **区域**:签名端点必须落在支持区域(East US / West US / West US 2 / West US 3 / Central US / North Central US /
  South Central US / West Central US / North Europe / West Europe / Switzerland North / Poland Central /
  Japan East / Korea Central / Brazil South,共 15 个)。

**⑥ 对 SmartScreen 的效果 —— 官方原文(这条要原样记住)**:

> "New files can show a SmartScreen warning until they accumulate sufficient reputation.
> Azure Artifact Signing does **not** provide instant SmartScreen trust, but signing consecutive releases with a
> consistent publisher/signing identity lets publisher reputation build over time, so later releases can inherit trust."

**声誉不是即时的。** 花 $9.99/月买到的是「跨版本继承」,不是「今天就不弹窗」。

---

### A4. 其他路线

#### (a) 自签名 —— **对公开分发无用,而且更糟**

官方原文(code-signing-options):

> "A self-signed certificate is not trusted by Windows by default and **will trigger a strong SmartScreen block**
> for any user who hasn't manually installed the certificate as a trusted root.
> This makes self-signed certificates unsuitable for public distribution."

**为什么更糟**(三条):

1. 证书链验证失败 → Authenticode 状态是 **untrusted**,在某些策略下比「压根没签」评级更差;
2. 拿不到任何 publisher reputation —— 声誉只从**受信根**链下来的身份累积;
3. 会诱导用户去做「把一张来路不明的证书装进 Trusted Root」这个
   **比双击一个未签名 exe 危险得多**的操作。

官方给自签留的唯一用途是「本机开发测试」和「企业内网 + IT 用 Intune/GPO 下发根证书」。

#### (b) 买 OV 证书

官方口径(code-signing-options,`ms.date: 2026-08-24`):

| 项 | 内容 |
|---|---|
| 价格 | **$150–300/年**(DigiCert / Sectigo / GlobalSign 等) |
| 身份验证 | CA 验证**组织**法人身份;要几个工作日 |
| **硬件要求** | **2023-06 起 CA/Browser Forum 强制**私钥存 **HSM 或硬件 token**;多数 CA 提供 USB token 或云 HSM 选项 |
| SmartScreen | 与 Azure Artifact Signing **完全等价** —— 靠声誉积累,不即时 |
| EV | **$400+/年**,2024 起与 OV 无差别,**不再建议**为 SmartScreen 买 EV |

> **USB token 与 CI 的冲突**:实体 token 插在本机,GitHub Actions 摸不到。
> 要在 CI 里签就必须选**云 HSM 版**(DigiCert KeyLocker、SSL.com eSigner、Sectigo 云签名等),那会另外加钱。
> 这是「买 OV」对一个 CI 出包的项目最实际的成本项。

#### (c) Certum "Open Source Developer" 证书 —— 个人可买、全球可买

这是 **US/CA 之外的个人开源作者**最常见的正规路线,值得单列。

- 证书主体:**Common Name 带 "Open Source Developer" 字样,Organization 字段就是 "Open Source Developer"**,后面跟本人姓名;
- 在微软根信任链内,**能累积 SmartScreen 声誉**;
- 价格(**二手来源,非 Certum 官方页,均抓取 2026-08-27**):
  - 智能卡 + 读卡器套装 **85 €** + 证书 **30 €/年** —— https://github.com/oneclick/rubyinstaller2/wiki/CertumCodeSigning
  - 2025-10 的一份实测记录:**69 € + 运费**,续期 **29 €** —— https://piers.rocks/2025/10/30/certum-open-source-code-sign.html
- **同样是硬件卡** → 同样不能直接进 CI。Certum 另有 cloud 版,经销商报 **$116 起**
  (https://www.sslmentor.com/certum/certumcodecloud ,二手来源)。

> ⚠ 以上价格全部来自经销商 / 个人博客,**Certum 官方定价页未直接抓取核实**,
> 决策前必须以 certum.store / certum.eu 官方页为准。

#### (d) sigstore / cosign

**对 Windows Authenticode 无效。** sigstore 签的是 OCI 制品 / 独立签名文件,
Windows 的 SmartScreen 与 Authenticode 完全不认。
可以作为**补充的透明日志证明**(和 `SHA256SUMS.txt` 同一层作用),但不能代替代码签名。
(本次未抓取 sigstore 官方页,此为通识性判断,标 `[未验证]`。)

---

### A5. winget 与 scoop 的准入条件(作为后延项)

#### winget(Windows Package Manager 社区仓)

**来源**(均抓取 2026-08-27):

- https://learn.microsoft.com/en-us/windows/package-manager/package/repository —《Submit your manifest to the repository》
- https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md

**① manifest 形态**:
提交到 `microsoft/winget-pkgs`,目录结构必须是 `manifests / <首字母> / <Publisher> / <Application> / <Version>`,
且 manifest 里的 `PackageIdentifier`、`PackageVersion` 必须与目录逐段对得上。
一次 PR **只能一个包的一个版本**(违反会拿 `PullRequest-Error` 标签)。

**② 便携 zip 支持** —— 支持,但要多写两个字段:

- winget **1.5 起支持 zip 归档**;
- `InstallerType: zip` 时,**`NestedInstallerType` 是必填**(对 Folio 就是 `portable`);
- `NestedInstallerFiles` 也是必填,指出 zip 里哪个文件是真正要装的可执行文件(可多个);
- 对 Folio 的具体含义:zip 里除 `folio.exe` 之外还有 `conpty.dll` / `OpenConsole.exe` / `x64\OpenConsole.exe`
  (审计 M5)。portable 安装器会把 zip 解到一个包目录并给 `folio.exe` 建 symlink 进 PATH,
  同目录的 sidecar 会一起解开 → **ConPTY 成对文件的契约能满足**。这一条必须实测,标 `[需实测]`。

**③ 签名要求** —— **winget 不要求安装器有数字签名。**
提交期望里列的是这些(原文要点):

- manifest 合 schema;
- **所有 URL 指向安全站点**,且**必须 HTTPS**(否则 `Validation-HTTP-Error`);
- **InstallerUrl 必须直接来自发布者自己的发布位置**,**不许经过跳转 / 短链**
  (否则 `Validation-Indirect-URL` / `Validation-Domain` / `Validation-Unapproved-URL`)
  → GitHub Releases 的直链是合规的;
- **`InstallerSha256` 必须与 URL 内容匹配**(否则 `Error-Hash-Mismatch`)
  → 这条要求**发布后永不重传同名产物**;
- 安装器**必须支持非交互式安装**(否则 `Validation-Unattended-Failed`);
- 装完能被找到、**卸载要干净**(`Validation-Executable-Error` / `Validation-Uninstall-Error`);
- **要过多家杀软扫描**(`Binary-Validation-Error`,自动跑一批 AV;判 PUA 就直接拒);
- **有一个专门的 `Validation-VCRuntime-Dependency` 标签**:
  > "The package has a dependency on the C++ runtime that could not be resolved."
  → **这正好命中审计 M4(exe 静态导入 `VCRUNTIME140.dll`)。**
  即:M4 不修,**winget 上架会被这个标签直接卡住**。这是 M4 之外一个独立的、必须修的理由。
- 微软保留任意拒绝权:"Microsoft reserves the right to refuse a submission for any reason."

**④ 提交前自测**:`winget validate <path>` + 仓库自带的 `Tools\SandboxTest.ps1`(Windows Sandbox 里跑一遍)。

**⑤ Folio 的准入门槛小结**:
需要 ① 稳定的 GitHub Releases 直链 ② 每版新的 SHA256 ③ **M4 修掉 VCRUNTIME 依赖**
④ zip + `NestedInstallerType: portable` ⑤ 过 AV 扫描。
注意 ⑤ 是真实风险:终端类程序带 `BT_PTY_DUMP` 这种"能把全部 PTY 字节落盘"的功能,
文档里那句 "genuine tools used for debugging and low-level activities appear as PUA" 说的正是这类,标 `[需实测]`。
**全程不需要签名。**

#### scoop

**来源**:https://github.com/ScoopInstaller/Scoop/wiki/Criteria-for-including-apps-in-the-main-bucket (抓取 2026-08-27)

main bucket 五条(原文):

1. **"a reasonably well-known and widely used developer tool (e.g. if it's a GitHub project, it should have at least 500 stars and 150 forks)"**
2. "the latest stable version of the program"
3. "the full version i.e. not a trial version"
4. "a fairly standard install (e.g. uses a version-specific download URL, no elaborate pre/post install scripts)"
5. **"a non-GUI tool"**

**对 Folio 的判定**:

- 第 1 条(500 star / 150 fork)—— **v0.1 preview 显然不满足**;
- 第 5 条(非 GUI)—— **Folio 是 GUI 程序,永远不满足 main bucket**;
- → **正确的落点是 `extras` bucket,或者自建 `folio` bucket**
  (自建 bucket 零门槛:一个装 JSON manifest 的 GitHub 仓即可);
- scoop 同样**不要求签名**;需要**版本化的下载 URL + hash**,与 winget 一致;
- extras bucket 的具体准入条件该 wiki 页未列 → `[未查到]`。

---

### A6. GitHub Actions `windows-latest` runner 的三个疑点

**来源**(均抓取 2026-08-27):

- https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md
- https://github.com/actions/runner-images/issues/14017 、 https://github.com/actions/runner-images/issues/14016
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution

**① runner 是什么**:

- `windows-latest` 自 **2026-06-08 ~ 06-15** 起 = **Windows Server 2025 + Visual Studio 2026** 镜像;
- 镜像 README 头部:**`OS Version: 10.0.26100 Build 33296`**,`Image Version: 20260818.232.1`;
- 相关条目:`Windows Software Development Kit | 10.1.26100.7705`;
  **`Microsoft Visual C++ 2022 Minimum Runtime | x64 | 14.51.36247` 与 `… Additional Runtime | x64 | 14.51.36247` 已预装**
  → ⚠ **这意味着 CI 上跑 `folio.exe` 永远不会暴露审计 M4 的 VCRUNTIME 缺失问题。**
  **M4 只能在干净 VM 上验;CI 绿灯对这一条毫无证明力。** 这是本节最值钱的一条。

**② WebView2 运行时** —— **镜像清单里没有 WebView2**。

- 通读 `Windows2025-Readme.md` 的 installed-software 列表,**"WebView2" 一词不出现**;
- 列表里**有** `Microsoft Edge 151.0.4129.86` 与 `Microsoft Edge Driver 151.0.4129.93`;
- **Edge ≠ WebView2 Runtime**:官方 distribution 文档明确写
  "A production release of a WebView2 app can only use the WebView2 Runtime as the backing web platform, **not Microsoft Edge**",
  且 Evergreen Runtime "will be included as part of the **Windows 11** operating system"
  —— **Windows Server 2025 不在这个保证里**;
- **结论**:**不能假设 runner 上有 WebView2 Runtime。**
  这是「清单未列出」的缺席证据,比推断强、比实测弱,标 **`[需实测]`**。
  CI 上跑一次官方给的检测法即可一次性定论:
  `reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv`
  (`pv` 存在且 > `0.0.0.0` 才算装了);
- **如果没有**:CI 里加一步 `MicrosoftEdgeWebview2Setup.exe /silent /install`(bootstrapper 约 2 MB),
  或把 `crates/bt-app/src/webhost.rs` / `crates/bt-platform/src/webview.rs` 相关测试标为本地探针;
- **这反而是个好消息**:runner 天然就是「没装 WebView2 的干净机」,
  正好可以在 CI 里**自动化验证审计里 `RuntimeMissing` 那张降级卡**(`crates/bt-app/src/webhost.rs:730`)。

**③ ConPTY** —— **可用,而且本项目根本不依赖它**。

- `CreatePseudoConsole` 等 ConPTY API 自 **Windows 10 1809 / Windows Server 2019(build 17763)** 起在 `kernel32` 中提供;
  runner 是 **build 26100**(Server 2025)≫ 17763 → **inbox ConPTY 一定可用**。
  (此为版本推断,非镜像清单的直接证据,标 `[推断]`。)
- 但更关键的是:**Folio 自带 sidecar**。`crates/bt-pty/build.rs:24-54` 从签入的 nupkg 解出
  `conpty.dll` + `OpenConsole.exe` 到输出目录;`vendor/conpty/portable-pty/src/win/psuedocon.rs:165-172`
  的加载器只认「打包的这一对」或「操作系统的实现」两种。
  → **CI 上跑的是打包那一对,与 runner 的 inbox 版本无关。** 这一条不构成风险。
- 镜像清单里**没有** Windows Terminal / OpenConsole 条目(已核实),不影响上面的结论。

**④ DPI / GPU / 无头可行性**:

- **GPU**:镜像清单里**没有任何独立显卡驱动条目**(只有 `Microsoft.VisualStudio.Component.Graphics` 这类 VS 组件)。
  → `crates/bt-render/src/lib.rs` 的 `request_adapter` 只能落到 **WARP**(D3D12 软件光栅器,`d3d10warp.dll` 系统自带)。
  但 wgpu 默认的 `request_adapter` **不会**自动挑软件适配器,通常需要显式允许
  (`force_fallback_adapter: true`,或按适配器名筛选)。**必须实测**,标 `[需实测]`
  —— 与审计 M7 的疑点 ① 是同一件事。
- **DPI**:GitHub 托管 runner 的桌面分辨率与 DPI 缩放**在镜像 README 里没有任何条目** → `[未查到]`。
  可以确定的是它**只有一个虚拟显示器**,因此:
  - `scale = 1`(100%)这一路能测;
  - **跨显示器、跨 DPI、`PER_MONITOR_AWARE_V2` 的切换路径在 runner 上无法覆盖**;
  - memory 里 ui-probe 那条「每个探针进程必设 `PER_MONITOR_AWARE_V2`」的验收,**在 CI 上只能验单 DPI 一半**。
- **建议**:把 DPI 相关测试**显式标成本地探针**(和那 23 个 `#[ignore]` 一起,在审计 M7 的门修好后按白名单钉住)。
  **不要指望 CI 覆盖 DPI 面,也不要因为 CI 绿了就认为 DPI 被验过。**

---

### A7. 签名路线推荐(个人开源作者 / 便携 zip / 想过 SmartScreen)

#### 第一原则:2026 年没有「花钱当天就不弹窗」这个选项

EV 的即时放行 2024 年被取消,微软自己写的。所以决策不是「签不签得起」,而是
**「什么时候开始让 publisher reputation 起跑」**。

#### v0.1 preview:**不签**。三条理由,每条都有证据

1. **签了也一样弹。** 新证书的 publisher reputation 从零起步,官方口径 "does not provide instant SmartScreen trust"。
   preview 阶段下载量本来就小,**积累不起来** —— 这笔钱在 v0.1 买不到任何用户可感知的东西。
2. **现在还不够格申请免费的那条路。** SignPath Foundation 要求「已经发布过」+「CI 可验证构建」,
   而本仓 CI **一次都没跑过**(M7)、**还没有 README**(M2)、**根目录没有 LICENSE**(M1)。
3. **有更便宜的替代动作**:`SHA256SUMS.txt` + README 一节 SmartScreen 说明。这是 preview 阶段的正确对价。

**v0.1 必须做的两件小事**:

- Releases 里发 **`SHA256SUMS.txt`**,让人能自校验;
- README 一节 **"Windows SmartScreen"**(英文),写清:
  未签名 → 首次运行可能出现 "Windows protected your PC" → 点 **More info** → **Run anyway**;
  并给出 zip 的 SHA-256 与一行自校验命令(`Get-FileHash folio-0.1.0-windows-x64.zip -Algorithm SHA256`)。
  **不要**写「用 7-Zip 解压就不会弹」。

#### v0.2 起:**首选 SignPath Foundation**

| | |
|---|---|
| **为什么是它** | ① **免费**;② **OV 级**(微软官方页的说法);③ **证书在 Foundation 名下 → 不把作者真名+城市写进每一份签名**;④ 私钥在它的 HSM,不用买卡、不用管密钥;⑤ 它要求的「CI 可验证构建」本来就是审计 M7 要做的事 |
| **准入差距** | 发一版 v0.1 → 修好 CI 并跑绿 → 补 LICENSE / README → **另写一份 code signing policy 挂在项目主页** → 申请 |
| **代价** | 每次 release 要人工 approve;团队要开 MFA;要划 Authors / Reviewers / Approvers 角色 —— 对单人项目都是几分钟的事 |
| **未知** | 每月签名次数配额 `[未查到]`,申请前问 info@signpath.io |

#### 备选(按优先级)

| 序 | 路线 | 什么时候选 | 代价 |
|---|---|---|---|
| B | **Azure Artifact Signing** | 作者在**美国或加拿大**,且 SignPath 申请被拒或嫌流程重 | ~$9.99/月;**真名 + 城市/州会出现在每一份签名里**;要 Azure 订阅且 billing account 名址逐字一致;身份验证要交政府证件 + 自拍 + 可能的地址证明;1–20 个工作日 |
| C | **Certum "Open Source Developer"** | 作者**不在美加**,SignPath 也不成 | ~69–85 € 起 + 30 €/年(二手价,需官方核实);**硬件卡 → 不能直接进 CI**(除非上 cloud 版,约 $116 起) |
| D | **普通 OV(DigiCert / Sectigo)** | 有企业采购需求 | $150–300/年 + **强制 HSM / token**;要进 CI 得买云 HSM,再加钱 |
| ✗ | **EV** | **不选** | $400+/年,2024 起与 OV 无差别 |
| ✗ | **自签** | **不选** | 官方口径是 "strong SmartScreen block",比不签更糟,且诱导用户装根证书 |

#### 一条容易被忽略的战术

无论最终走哪条,**一旦开始签,就不要换身份**。
官方原文:"Reputation cannot transfer from previous versions unless both were signed using the same publisher identity."
从 SignPath 换到 Azure、或从 Azure 换到 Certum,**publisher reputation 会全部清零重来**。
所以 **v0.2 那次选择是一次性的**,值得在动手之前把 A2 / A3 的地域与身份约束核一遍。

#### 与 winget / scoop 的关系

两家**都不要求签名**。所以「上架」这条线**不被签名阻塞**,反而被两件别的事阻塞:

- **winget**:审计 **M4(VCRUNTIME140)必须先修** —— 有一个专门的 `Validation-VCRuntime-Dependency` 拒绝标签;
  外加稳定直链 + 每版新 SHA256 + zip 的 `NestedInstallerType: portable`;
- **scoop**:main bucket **永远进不去**(GUI + 500 star 门槛),目标应是 `extras` 或**自建 bucket**。

---

## 缺口 B · 首次体验(全部代码取证,`绝对路径:行号`)

### B0. 三十秒版

一台全新机器上第一次双击 `folio.exe`,发生的是:

1. 窗口 **960×600 逻辑像素**开在系统默认放置的那个角(**不居中、不按工作区算**);
2. 第一个 tab 是 **Windows PowerShell 5.1(`winps`)**,**哪怕机器上装了 PowerShell 7 也是**
   —— 这是 2026-08-11 的一条裁决的直接后果,不是 bug(B1 成因 A);
3. 如果用户手动切到 pwsh 而 PowerShell 7 是 **Microsoft Store 包**,那一行在 picker 里是**灰的**
   —— 这是一个**真误判**,`Path::is_file()` 打不开 AppExecLink(B1 成因 B);
4. 第一屏打完之后,一条 **PowerShell 整合提示条**浮出来,**并且每次启动、每开一个新 PowerShell pane 都会再来一次**,
   除非按「不再提示」(B2);
5. Agents 页三行 hooks 显示 Off —— 因为**三个用户文件都不存在**,不是因为有个默认值(B3);
6. 如果它崩了,**窗口凭空消失,零反馈**,日志静静躺在 `%TEMP%\bt-app-panic.log`(B5)。

---

### B1. 默认 profile 的探测顺序

**内置五条,顺序固定:`pwsh` → `winps` → `wsl` → `gitbash` → `cmd`**

- `crates/bt-app/src/profiles.rs:783` `pub fn shipped() -> Vec<Profile>`;
  五行 id 分别在 `:786` / `:830` / `:865` / `:904` / `:963`;
- 顺序被测试钉死:`crates/bt-app/src/profiles.rs:11644`
  `assert_eq!(listed, ["pwsh", "winps", "wsl", "gitbash", "cmd"]);`

**每条怎么探测(`ProgramSource`)**:

| profile | 探测方式 | 行号 |
|---|---|---|
| `pwsh` | `ProgramSource::PowerShellSeven` → `bt_pty::resolve_powershell_seven` | `crates/bt-app/src/profiles.rs:816` |
| `winps` | `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`,**单一固定路径** | `crates/bt-app/src/profiles.rs:848-851` |
| `wsl` | `%SystemRoot%\System32\wsl.exe` | `crates/bt-app/src/profiles.rs:886-889` |
| `gitbash` | ① PATH 上 `git.exe` 的祖先目录 + `bin\bash.exe` ② `%ProgramFiles%\Git\…` ③ `%ProgramFiles(x86)%\Git\…` ④ `%LocalAppData%\Programs\Git\…` | `crates/bt-app/src/profiles.rs:912-929` |
| `cmd` | `%SystemRoot%\System32\cmd.exe` | `crates/bt-app/src/profiles.rs:967-970` |

**没有注册表,没有 `where.exe`。** 全部落到同一个 trait 方法:

```rust
// crates/bt-pty/src/shell.rs:40-42
fn is_file(&self, path: &Path) -> bool {
    path.is_file()
}
```

pwsh 的三级链(`crates/bt-pty/src/shell.rs:129-149`):
`BT_SHELL` 优先(`:113-119`)→ ① PATH 搜索 `pwsh.exe`(`:157-163`,同样用 `is_file`)
→ ② `%ProgramFiles%\PowerShell\7\pwsh.exe`(`:133`)→ ③ `%LocalAppData%\Microsoft\WindowsApps\pwsh.exe`(`:142-147`)。

#### 为什么 Store 版 pwsh 没被选中 —— 两条独立成因

**成因 A(决定性,与 Store 无关):首次启动 `default_profile` 是空串,空串一律落 `winps`。**

- `crates/bt-persist/src/settings.rs:165` `pub const DEFAULT_PROFILE_UNSET: &str = "";`
- `crates/bt-persist/src/settings.rs:879` `default_profile: DEFAULT_PROFILE_UNSET.to_owned(),`(`SettingsV1::default()`)
- `crates/bt-app/src/profiles.rs:2690-2698`
  ```rust
  fn default_profile_in(table, stored, available) -> usize {
      table.position_of_id(stored)
          .filter(|index| available(*index))
          .unwrap_or_else(|| fallback_profile_in(table))
  }
  ```
- `crates/bt-app/src/profiles.rs:2651-2653`
  ```rust
  fn fallback_profile_in(table: &ProfileTable) -> usize {
      table.position_of_id(WINDOWS_POWERSHELL_ID).unwrap_or(0)
  }
  ```
  `WINDOWS_POWERSHELL_ID = "winps"`(`crates/bt-app/src/profiles.rs:767`);
  用户裁决 2026-08-11 明写在 `crates/bt-app/src/profiles.rs:2618`:「**It is `winps` and no longer `pwsh`**」。
- **测试自己承认这一点**:`crates/bt-app/src/profiles.rs:12100-12103`
  `default_profile(DEFAULT_PROFILE_UNSET, &all) == fallback_profile()`
  —— 注意左边第二个参数是 `&all`(**装齐了 pwsh 的机器**),**照样返回 `winps`**。
  注释写的是「nobody has ever opened the setting: the floor」。
- 启动路径:`crates/bt-app/src/main.rs:27366`
  `profiles::default_profile(&settings_store.loaded().default_profile, &profile_programs);`
- 迁移侧同样写空:`crates/bt-persist/src/migrate.rs:101-110`,
  注释明说「Writing `"pwsh"` here would … be pinning a decision its owner never made」。

> **结论:即使 pwsh 装在 `C:\Program Files\PowerShell\7`,首启第一个 tab 也是 Windows PowerShell 5.1。**
> 这不是 bug,是裁决;但对「首次体验」而言,**产品出厂即 5.1**。

**成因 B(Store 特有,导致 pwsh 那一行在 picker 里直接变灰):`Path::is_file()` 打不开 AppExecLink。**

子代理在本机做了只读验证(`%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe`):

- `GetFileAttributesEx` 说存在:`len=0 attrs=Archive, ReparsePoint`;
- 用 `CreateFileW`(即 Rust `fs::metadata` → `File::open` 走的那条)打开:
  **失败,`ERROR_CANT_ACCESS_FILE (1920)` "The file cannot be accessed by the system"**;
- Rust std 的 `metadata` 只在 `ERROR_SHARING_VIOLATION` 时回退 `FindFirstFile`,**1920 直接返回 `Err`** ⇒ `is_file() == false`;
- `%ProgramFiles%\PowerShell\7\pwsh.exe` 在本机**不存在**;
- **注册表持久 PATH**(`folio.exe` 从 Explorer 启动时继承的那一份)里带 `pwsh.exe` 的目录**只有**
  `C:\Users\<user>\AppData\Local\Microsoft\WindowsApps` —— 就是那个 reparse point。
  真 exe 所在的 `C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe`
  **只出现在当前 pwsh 会话的 PATH 里,不在 HKLM/HKCU 的 `Path` 值里**。

所以 `find_pwsh` 三级全 miss → `resolve_powershell_seven` 返回 `None`
→ `ProfilePrograms::probe`(`crates/bt-app/src/profiles.rs:3294`)给 pwsh 存 `None`
→ `is_available(pwsh) == false` → picker 画灰(`crates/bt-app/src/profiles.rs:3358`)。

**代码里那句自我辩护是错的**:

```rust
// crates/bt-pty/src/shell.rs:27
/// dangling reparse point answers `false`, exactly as a spawn attempt against it would fail.
```

对 **AppExecLink 不成立** —— `CreateProcess` **能**跟随 AppExecLink 正常启动,只有 `CreateFileW` 不能。
**探测比 spawn 更严格,于是误判。**

**全都找不到时的兜底**:`fallback_profile()` = `winps`(`crates/bt-app/src/profiles.rs:2638`),
它指名 `%SystemRoot%` 下的 5.1,**永不为空**;测试
`crates/bt-app/src/profiles.rs:11855-11880` `the_fallback_profile_can_always_be_started`。
再往下还有 bt-pty 自己的最后一跳:spawn 失败时换 `powershell.exe`,
pane 的 profile 也跟着改成 `fallback_profile()`(`crates/bt-app/src/main.rs:25030-25037`)。

**默认 profile 存哪**:`settings.json` 的 `"default_profile"`,
类型是 **id 字符串而不是索引**(`crates/bt-persist/src/settings.rs:391`,理由在 `:380-392`);
schema 见 `crates/bt-persist/src/settings.rs:290`。**首次启动其值 = `""`。**

---

### B2. PowerShell 整合提示条的首次触发时机与频率

**持久化位**:`powershell_integration_offer: bool`

- 定义 `crates/bt-persist/src/settings.rs:702-703`,`#[serde(default = "default_powershell_integration_offer")]`;
- **默认 `true`**:`crates/bt-persist/src/settings.rs:827-829`;`SettingsV1::default()` 在 `:907`;
- 迁移引入 = **schema v17→v18**,写 `true`:`crates/bt-persist/src/migrate.rs:394-398`;
- app 侧镜像 `crates/bt-app/src/settings.rs:4157`,`Default` 里 `:4329` = `true`。

> ⚠ **这个 bool 不是「已提示过」的记录,是「允不允许提示」的总闸。**
> 全仓**没有任何「已问过一次」的持久位**。

**触发时机** —— 不是启动、不是开 pane 的瞬间,而是**每一帧 turn 里 `drain_pty` 之后轮询**:

- `crates/bt-app/src/main.rs:74290` `self.settle_pane_notices()?;`
  (在 `Runtime::turn` 内,`:74285` 注释「Directly after the drain」);
- 判定体 `crates/bt-app/src/main.rs:32324-32380`。

**五个条件同时满足才画**:

1. 设置为 on(`crates/bt-app/src/main.rs:32326-32329`);
2. `shell_integration::is_powershell(program)` —— 文件名 stem 是 `pwsh` 或 `powershell`
   (`crates/bt-app/src/shell_integration.rs:585-595`);
3. `$PROFILE` 路径已问到 —— **是真去 spawn 一个 PowerShell 问的**,异步,每个 program 一辈子一次:
   ```rust
   // crates/bt-app/src/shell_integration.rs:713-725
   let output = std::process::Command::new(program)
       .args(["-NoProfile", "-NonInteractive", "-Command", PROFILE_COMMAND])
       .creation_flags(CREATE_NO_WINDOW)
   ```
   `PROFILE_COMMAND = "$PROFILE.CurrentUserCurrentHost"`(`crates/bt-app/src/shell_integration.rs:610`)。
   首次调用返回 `None` 并起后台线程(`:679`),答案落地后 `wake()` → `AppEvent::PowerShellProfileProbed`
   → 全窗口重跑 `settle_pane_notices`(`crates/bt-app/src/main.rs:78786`);
4. `offer_for` 读那个文件,**没有未注释的、含 `folio.ps1` 的行** → `Offer::Owed`
   (`crates/bt-app/src/shell_integration.rs:997-1005`;判据 `profile_declares_integration` 在 `:761-770`,`#` 开头不算);
5. `Offer::showing` 再加两道:shell 必须已经**输出过东西**(`leaf.output_revision > 0`),
   且**没收到过 OSC 133**(`crates/bt-app/src/shell_integration.rs:961-973`)。

> **确切时机 = 第一个 PowerShell pane 打出第一屏之后、后台探测进程返回之后的那一帧。**
> 没有固定延时,取决于 `pwsh -NoProfile` 冷启动耗时(几百 ms 量级)。

**频率** —— **每次启动都弹,每开一个新 PowerShell pane 都弹**:

- 条带是 **per-leaf** 状态(`crates/bt-app/src/main.rs:5851` `integration_offer: Option<shell_integration::Offer>`),**不持久化**;
- `×` / Esc 关掉 = `Offer::Closed`(`crates/bt-app/src/main.rs:32499`),
  注释明写(`:32489-32492`):「**Not** the same as `Silent`: nothing was decided, so the next PowerShell is asked again」;
- 停下来只有三条路:装了整合、收到 OSC 133、或按了**「不再提示」**
  = `NoticeVerb::Never` → `apply_powershell_integration_offer(false)`
  (`crates/bt-app/src/main.rs:32469`,写盘在 `:40232-40234`)。

**条带文案**(`crates/bt-app/src/i18n.rs:3137-3143`):

```
PowerShell integration is not installed. Folio uses it for command marks and status.
```

按钮 `Add to $PROFILE` / `Don't show again`;装完变 `Added` 态 + `Restart shell` 按钮
(`crates/bt-app/src/notice.rs:113-117`)。

> **对发布的意义**:这是新用户**第一分钟就会看到**的东西,而且**关掉还会回来**。
> README 的 "First run" 一节应当直接把它写出来,并说明「Add to $PROFILE 会往你的 `$PROFILE` 里写一行」
> —— 呼应审计 M8 的「写哪三个文件」原则。
> 另外这条也是审计 §S5 已知问题里「未装 PowerShell 整合时鼠标残留修复不生效」的同一入口。

---

### B3. Agents 页三个 hooks 安装行的默认值

**关键结论:这三行不在 `settings.json` 里,也没有 schema 迁移。**
在 `crates/bt-persist/src/settings.rs` 与 `crates/bt-persist/src/migrate.rs` 全文 grep
`claude_hooks|codex_notify|copilot_hooks` —— **零命中**。

三行的字段只在**内存**结构 `SettingsValues` 上(`crates/bt-app/src/settings.rs`):

- `pub claude_hooks: bool` — `crates/bt-app/src/settings.rs:4174`
- `pub codex_notify: bool` — `crates/bt-app/src/settings.rs:4178`(**Codex 那行叫 `CodexNotify`,不是 hooks,写的是 `notify` 键**)
- `pub copilot_hooks: bool` — `crates/bt-app/src/settings.rs:4182`

`Default` impl 里三个都是 `false`(`crates/bt-app/src/settings.rs:4335-4337`):

```rust
context_menu: false,
claude_hooks: false,
codex_notify: false,
copilot_hooks: false,
```

但注释 `:4332-4333` 说明这只是「a machine that never installed the verb」,**真值每次从用户自己的文件读**:

```rust
// crates/bt-app/src/main.rs:27733-27741
claude_hooks_installed: attention_hooks::state() == attention_hooks::State::Installed,
codex_notify_installed: attention_codex::state() == attention_codex::State::Installed,
copilot_hooks_installed: attention_copilot::state()
    == attention_copilot::State::Installed,
```

读的三处文件:

| 行 | 文件 | 行号 |
|---|---|---|
| Claude | `$CLAUDE_CONFIG_DIR` 或 `~/.claude/settings.json` | `crates/bt-app/src/attention_hooks.rs:50` `:53` `:102`;`state()` `:107-119` |
| Codex | `$CODEX_HOME` 或 `~/.codex/config.toml` 的 `notify` 键 | `crates/bt-app/src/attention_codex.rs:65` `:69` `:72`;`state()` `:130` |
| Copilot | `$COPILOT_HOME` 或 `~/.copilot/hooks/folio.json` | `crates/bt-app/src/attention_copilot.rs:86` `:89` `:92` |

行的勾选态直接映射文件事实:`crates/bt-app/src/settings.rs:3557-3567`;
选项集是 `FORMULA_OPTIONS: [bool; 2] = [true, false]`(`crates/bt-app/src/settings.rs:876`),两态开关;
插入顺序 `crates/bt-app/src/settings.rs:3809-3811`。

**开关打开是否立即写用户文件:是,按下即写,没有确认按钮。**

`crates/bt-app/src/main.rs:35627-35635` → `apply_claude_hooks` / `apply_codex_notify` / `apply_copilot_hooks`:

```rust
// crates/bt-app/src/main.rs:39912-39921
fn apply_claude_hooks(&mut self, install: bool) -> Result<bool> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("folio.exe"));
    let outcome = attention_hooks::apply(install, &exe);
    self.app.claude_hooks_installed =
        attention_hooks::state() == attention_hooks::State::Installed;
```

写完弹 toast;写失败则开关**回弹到文件的真实状态**(注释 `crates/bt-app/src/main.rs:39899-39902`)。

> **所以「三个默认值是不是 Off」的准确答案:不是一个默认值,是一次探测。**
> 全新机器上三个文件都不存在 → 三行都显示 Off,**且 Folio 从不主动装**。
> 这一条比「默认 Off」更强,**README / SECURITY.md 应当原样这么写**
> —— 它同时回答了审计 M8 里「什么时候写、怎么撤」两问:**只有用户按下那一行才写,再按一次就撤**。

---

### B4. 第一次打开的窗口尺寸与位置

**硬编码常量,不按工作区算,不居中。**

```rust
// crates/bt-app/src/main.rs:157-158
const INITIAL_WIDTH: f64 = 960.0;
const INITIAL_HEIGHT: f64 = 600.0;
```

(逻辑像素)

**首启路径**:`session.json` 无 tab 时 `restore_window_placement` 直接返回 `None`
(`crates/bt-app/src/main.rs:82051-82055`,注释「An empty tab list is a window that is not a window:
on a first run there are no entries at all」),于是:

1. 尺寸 `crates/bt-app/src/main.rs:27377`
   `.unwrap_or(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT))`
   传给 `opening_window_attributes`(`:80601-80614`);
2. 然后 `crates/bt-app/src/main.rs:27456-27461` 用 Win32 精确重置外框:
   ```rust
   let opened_at = dpi_snapshot(&window)?;
   bt_platform::set_window_outer_rect(
       hwnd,
       startup_window_rect(restored, opened_at.rect, opened_at.authoritative_scale),
   )
   ```
3. `startup_window_rect`(`crates/bt-app/src/main.rs:82094-82118`):`placement == None` 时
   **尺寸取 960×600 × 该窗口的权威 scale,位置取 `opened_at.rect` 的 left/top**
   —— 也就是 **winit / `CW_USEDEFAULT` 已经把窗口放到的那个角,原样保留**
   (`:82111` `.unwrap_or((opened_at.left, opened_at.top))`)。

**高 DPI**:scale **不用 winit 的**,用 `GetDpiForWindow` 的权威值
—— `crates/bt-app/src/main.rs:80430-80443`,
`authoritative_scale: f64::from(win32_dpi) / WIN32_DEFAULT_DPI`。
所以 192 DPI(200%)机器上首启客户区 = **1920×1200 物理像素**。

**多显示器**:首启**不参与判断**(没有保存矩形可判)。落哪块屏由 `.with_inner_size(size)` 之后系统的默认放置决定;
`opening_window_attributes` 的注释(`crates/bt-app/src/main.rs:80612-80613`)明说这个开局尺寸
「is for landing the window on the right monitor at the right DPI」。
**恢复**时才有可见性判定(`:82063-82078`,判据是「some of the window is reachable」而非全包含),
矩形无屏可见就**只丢位置、保尺寸**。

**尺寸下限保护**:`sane_restored_size`(`crates/bt-app/src/main.rs:82019-82026`)
—— 存档尺寸小于一个最小 pane 就退回 960×600。

---

### B5. 崩溃时用户看到什么

**结论:什么都看不到。没有 message box,没有路径提示。**

**panic hook**:`crates/bt-app/src/main.rs:82179-82197`,装在 `main()` 的第三行(`:82243`)。

写入路径:

```rust
// crates/bt-app/src/main.rs:82225-82227
fn panic_log_path() -> PathBuf {
    std::env::temp_dir().join(PANIC_LOG_FILENAME)
}
```

文件名字面量:

```rust
// crates/bt-app/src/main.rs:259
const PANIC_LOG_FILENAME: &str = "bt-app-panic.log";
```

即 **`%TEMP%\bt-app-panic.log`**,append 模式(`:82229-82232`),
内容含 `unix_ms`、线程名、panic info、`Backtrace::force_capture()`。
(**文件名仍是旧代号** —— 与审计 M8 / §3 的裁决一致,应改为 `folio-panic.log`。)

**`message_box` 全仓只有一个调用点**:

```rust
// crates/bt-app/src/main.rs:82221
bt_platform::message_box(APP_NAME, &text);
```

它在 `report_at_the_front_door`(`crates/bt-app/src/main.rs:82215-82222`)里,
**只服务 CLI 参数错误**,且只在 `write_to_console` 返回 false(双击启动、没有控制台)时才弹。
`bt_platform::message_box` 定义在 `crates/bt-platform/src/lib.rs:7319`,
其文档 `:7307-7309` 明写「**The single message box this product is allowed to raise**」。
**panic 路径上没有任何 message box 调用。**

**`#![windows_subsystem = "windows"]`** 在 `crates/bt-app/src/main.rs:17`。
默认 panic 输出走 `previous(info)`(`:82195`)打 stderr,而 stderr 的去向:

- **从 shell 启动** → `bt_platform::adopt_parent_console()`(`crates/bt-app/src/main.rs:82236`,
  实现 `crates/bt-platform/src/lib.rs:7074`)把 null 句柄接到父控制台,**能看到**;
- **但紧接着** `crates/bt-app/src/main.rs:82283` `diagnostics::enter_resident_run(&storage)`
  (`crates/bt-app/src/diagnostics.rs:191-214`)会 `redirect_std_streams_to_file` 到
  `%APPDATA%\Folio\diagnostics.log` 并 `detach_console()`
  —— 除非设了 `BT_*TRACE*` 一类诊断变量(`console_was_asked_for`,`crates/bt-app/src/diagnostics.rs:192`);
- **双击启动** → `AttachConsole` 失败,全程 no-op
  ⇒ **窗口消失,零反馈,用户不知道 `%TEMP%\bt-app-panic.log` 存在**。

**`diagnostics.log` 有轮转**(这一条更正审计 M8 里「没有看到轮转/上限」的说法):

- `crates/bt-app/src/diagnostics.rs:58` `LOG_FILENAME = "diagnostics.log"`;
- 轮转副本 `crates/bt-app/src/diagnostics.rs:61` `"diagnostics.prev.log"`;
- **阈值 4 MiB**(`crates/bt-app/src/diagnostics.rs:76`)。
- `%APPDATA%\Folio` 由 `crates/bt-app/src/persist.rs:775-783` 决定(`APPDATA` 缺失时退 `temp_dir()`)。

**hang watchdog 报告**:写 `%APPDATA%\Folio\hang-reports\hang-<紧凑UTC时间戳>.txt`

- 目录常量 `crates/bt-app/src/hang_watch.rs:165` `pub const REPORTS_DIRECTORY: &str = "hang-reports";`,
  启动挂接 `crates/bt-app/src/main.rs:82294`
  `hang_watch::start(storage.join(hang_watch::REPORTS_DIRECTORY));`
- 文件名 `crates/bt-app/src/hang_watch.rs:856-862` `format!("hang-{compact}.txt")`;
- **只保留最新 16 份**(`crates/bt-app/src/hang_watch.rs:159` `REPORTS_KEPT`),**阈值 5 秒**(`:140`);
- 写完只 `eprintln!` 一行(`crates/bt-app/src/hang_watch.rs:1035-1039`),同样进 `diagnostics.log`,
  **用户界面上没有任何提示**。

> **对发布的意义**:审计 M8 的「崩溃时至少弹一个说明路径的 message box」这一条,
> 代码层面**门是现成的** —— `bt_platform::message_box` 已存在、已在 `report_at_the_front_door` 里用着,
> panic hook 只差一次调用。但注意 `lib.rs:7307-7309` 那句「唯一被允许的 message box」是一条**成文约束**,
> 加第二个调用点属于改规矩,应当同时改那句注释,而不是绕过它。

---

### B6. `--help` 输出

**有。`--help` / `-h` / `/?` 三种拼法,退出码 0。**

```rust
// crates/bt-app/src/cli.rs:219
Some("--help" | "-h" | "/?") => return Err(CliFault::HelpAsked),
```

`crates/bt-app/src/cli.rs:126-130` `exit_code()`:`HelpAsked => 0`,**其余一律 `2`**。

> ⚠ 这一条与审计 M6 的记载**不冲突但需精确化**:
> `--help` **是有的且退出码 0**;**没有的是 `--version` / `-V`**,后者会落到 `UnknownFlag` 被以退出码 2 拒绝。
> M6 的结论不变。

**帮助文本原文(英文,`crates/bt-app/src/i18n.rs:4730-4736`)** ——
`{profile_ids}` 在首启时展开为 `pwsh, winps, wsl, gitbash, cmd`
(`crates/bt-app/src/cli.rs:155-166`,用 `profiles::count()` / `profiles::id`;行首两个空格来自 `\x20`):

```
folio [--cwd <folder>] [--profile <id>] [<path>]

  --cwd <folder>    the first pane opens in that folder
  --profile <id>    the first pane's shell: pwsh, winps, wsl, gitbash, cmd
  <path>            a folder opens a pane there; a file opens a preview
  -h, --help        this text
```

中文版(`crates/bt-app/src/i18n.rs:4737-4743`):

```
folio [--cwd <文件夹>] [--profile <id>] [<路径>]

  --cwd <文件夹>    第一个窗格在这个文件夹里打开
  --profile <id>    第一个窗格用哪种 shell:pwsh, winps, wsl, gitbash, cmd
  <路径>            文件夹等同 --cwd,文件则打开预览
  -h, --help        显示这段说明
```

**全部参数表**(`crates/bt-app/src/cli.rs:207-252`):

| 参数 | 说明 |
|---|---|
| `--cwd <folder>` / `--cwd=<folder>` | 第一个 pane 的目录,`CWD_FLAG` `crates/bt-app/src/cli.rs:176` |
| `--profile <id>` / `--profile=<id>` | 第一个 pane 的 profile id,`crates/bt-app/src/cli.rs:178` |
| `<path>` 位置参数 | 只能一个;目录 = 开 pane,文件 = 开预览 |
| `--` | 结束 flag 解析,之后全当位置参数(`crates/bt-app/src/cli.rs:218`) |
| `-h` / `--help` / `/?` | 帮助,退出码 0 |
| `-Embedding` / `/Embedding` | **未在 help 里列出**,COM 激活路径用;大小写不敏感、两种 sigil 都接(`crates/bt-app/src/cli.rs:222-229`) |
| `folio attention <event> [--json]` | **未在 help 里列出的子命令**,在 `cli::parse` 之前就被截走(`crates/bt-app/src/main.rs:82248-82250`),agent hook 用;它自己的 `--help` 走 `AttentionFault::NothingAsked`(`crates/bt-app/src/cli.rs:425`) |

**未知参数处理**:

```rust
// crates/bt-app/src/cli.rs:246-248
Some(flag) if flag.starts_with('-') => {
    return Err(CliFault::UnknownFlag(flag.to_owned()));
}
```

另有 `MissingValue`(带值 flag 没给值,或值以 `-` 开头)、
`Repeated`(同一 flag 给两次,**不做 last-one-wins,直接拒**,理由 `crates/bt-app/src/cli.rs:114-118`)、
`ExtraPath`(第二个裸路径)。**全部退出码 2。**

**用户看不看得到**:

```rust
// crates/bt-app/src/main.rs:82215-82222
fn report_at_the_front_door(fault: &cli::CliFault) {
    i18n::install(resolved_language(
        persist::SettingsStore::open().loaded().language,
    ));
    let text = cli::refusal_text(fault);
    if !bt_platform::write_to_console(&format!("{text}\n")) {
        bt_platform::message_box(APP_NAME, &text);
    }
}
```

`write_to_console`(`crates/bt-platform/src/lib.rs:7270-7300`)自己
`AttachConsole(ATTACH_PARENT_PROCESS)` 再开 `CONOUT$` 用 `WriteConsoleW` 写
(为了中文不 mojibake,`crates/bt-platform/src/lib.rs:7290-7297` 还手动把 LF 换成 CRLF)。

> **所以在 `#![windows_subsystem = "windows"]` 下:**
> 从 shell 敲 `folio --help` **能在那个 shell 里看到**;
> 双击 / Explorer 启动则 `AttachConsole` 失败,**退化成一个 MessageBox**。
> **两条路都不会静默** —— 这一点比 panic 路径做得好,也正说明 panic 路径缺的只是一次同样的调用。

错误时输出 = **一行说明 + 空行 + 完整 usage**(`crates/bt-app/src/cli.rs:167-170`);
错误句子如 `There is no --nope option.` / `--cwd takes a value.`(`crates/bt-app/src/i18n.rs:4745-4759`)。

---

## 缺口 B 的三条待裁决(不在原单里,是查出来的)

| # | 事 | 证据 | 性质 |
|---|---|---|---|
| **B-a** | **首启默认落 `winps` 而非 `pwsh`** | `crates/bt-app/src/profiles.rs:2618` 的裁决注释 + `:12100-12103` 的测试(装齐 pwsh 也返回 winps) | **产品决策**。2026-08-11 的裁决把「unset 时选谁」和「floor 是谁」写成了同一件事。preview 公开前要不要拆开(floor 仍是永不缺席的 5.1,unset 时的首选另判)?**这是新规则,需要拍板,不是 bug 修复** |
| **B-b** | **`ShellEnvironment::is_file` 对 Store 版 pwsh(AppExecLink)误判为不存在** | `crates/bt-pty/src/shell.rs:40-42` 用 `Path::is_file()`;`:27` 的注释「exactly as a spawn attempt against it would fail」**写反了** —— `CreateProcess` 能跟随 AppExecLink,`CreateFileW` 不能(实测 `ERROR_CANT_ACCESS_FILE 1920`) | **真 bug**。修法是通用的:reparse point 上改用 `GetFileAttributesExW` 语义(存在且非目录即可)。**Microsoft Store 是 PowerShell 7 在 Windows 11 上的主流安装方式之一**,这条会影响相当一部分新用户 |
| **B-c** | **`diagnostics.log` 其实有 4 MiB 轮转** | `crates/bt-app/src/diagnostics.rs:61` `:76` | **更正审计 M8**:那一格写的「没有看到轮转/上限」不成立。README 的说法应改成「会轮转,保留一份 `.prev`」 |

> **⚠ B-b 已于 2026-08-27 复核翻案 —— 不是 bug,不要照上表去修。** 上表(及 §B0 第 3 条、§B1「成因 B」)那条链的最后一环是**推断而非实测**:实测在同一个进程里,对着本机那个 AppExecLink,用 std 给 `metadata` 用的参数直接 `CreateFileW` 得 `Err(1920)`,而 `Path::is_file()` 得 **`true`** —— `rust-toolchain.toml` 钉住的 Rust **1.94.1** 的 `fs::metadata` 把这次被拒的 open 当作再问一次的理由,退到不开文件的那条路上读目录项(handle 一个没开出来而属性 `0x420` 与长度 `0` 却回来了),AppExecLink 的 reparse tag 既不是 symlink 也不是 junction,于是读作一个文件。「只在 `ERROR_SHARING_VIOLATION` 时回退」说的是更早的 std。**所以 Store 版 pwsh 那一行不灰、探测顺序落 pwsh**;§B0 第 3 条与 §B1「成因 B」整段应按此读。真正为真的只有那半句:`:27` 的注释确实写反了,而且它是一句**邀请** —— 照它去把探测收紧成一次 open,就会当场造出这里以为已经存在的 bug。注释已改成事实,三条测试(含一次真红)把这条契约钉住,`GetFileAttributesExW` 语义**没有**引入(它比 std 现在做的更窄)。取证与理由见 `docs/DESIGN.md` §7.22。**B-a(成因 A)不受影响,仍待拍板。**

---
