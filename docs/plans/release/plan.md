# v0.1 preview 发布计划(三份审计合成,2026-08-27)

来源:`readiness-audit-2026-08-27.md`(Claude 盘点)× `readiness-gaps-2026-08-27.md`(签名行情/首次体验补盘)× `readiness-review-codex-2026-08-27.md`(Codex 对审:14 错 13 漏)。Codex 的更正全部采纳;下面是合成后的**门**,不再引用三份里被更正的旧结论。

## 总判断
今天不能公开。阻塞项按类分七道门;每道门是一片施工单,可并行,全部绿再定日期。时机裁决(用户):打磨期跑一轮后发;发布前一天零在飞线时做历史重写(②:filter-repo 去 `Claude-Session` trailer、邮箱 `weiyishi@umich.edu`→`weiyishi197@gmail.com`(用户裁 2026-08-28;`git config user.email` 已就地改为此,发布前 filter-repo 把历史全部改写为它)、`Co-Authored-By` 留)。

## 门 0 清洁(必须)
- `corpus/claude-code-session.btcr`、`cargo-build-flood.btcr` 含真实姓名/邮箱/组织/用户名/安装路径(Codex 错1):**重录或脱敏重写**,并让 `corpus/README.md` 的「已做环境替换」成真;加一条红门:`rg -a` 扫全部跟踪文件不得命中邮箱/`C:\Users\`/`D:\Developer\`。
- `scripts/dev/width-acceptance.ps1:2`、`ui-probe.ps1:672` 绝对路径→相对仓根;`folio.bash:109-111` 示例用户名换通用。
- `.gitignore` 加 `.env*`、`*.pfx/*.p12/*.pem/*.key`、`.vs/`、`.vscode/`,并忽略整个 `.claude/`(现只忽略 worktrees)。
- 大文件签收表(Codex 漏1):`vendor/alacritty_terminal/tests/ref/*/grid.json`(19MB+9.7MB)、`assets/fonts/NotoColorEmoji_WindowsCompatible.ttf`(10.7MB)、两份 ConPTY nupkg(1.7MB×2)——逐项 keep/remove 写理由(倾向:测试 JSON 留(vendor 原样)、字体留、nupkg 留并补微软 MIT 文本)。
- `design/`、`test-assets/`、`corpus/` 各补 `PROVENANCE.md`(自制/上游/生成物 + 许可)。
- 最终历史跑 gitleaks/trufflehog 全历史扫描,结果入库。

## 门 1 法律(必须)
- 根目录 `LICENSE-MIT` + `LICENSE-APACHE`(与 `Cargo.toml` 的 `MIT OR Apache-2.0` 一致)。
- `THIRD-PARTY-NOTICES.md` 由 `cargo-about` 生成并**签入 + CI 漂移检查**,覆盖全部 554 外部包(含漏的 fxhash/pollster/rustc-hash/shared_library,25 个 Unicode-3.0 的全文声明);`option-ext` MPL-2.0 固定精确源码归档 + 全文 + 取得方式。
- 上游完整 NOTICE 原样传播:`typst-assets`(含 CC BY 4.0/BSD-3/W3C/NewCM10 GPL3+FE+DE——不许手摘五类)、`hayro`(14 Foxit 字体 + LAB/CGATS ICC)、Noto OFL、Material 齿轮署名、十套 scheme、PSReadLine。
- `vendor/alacritty_terminal`:23 个差异文件逐文件核 §4(b) 显著修改声明(上游无 NOTICE,不是缺件);`CHANGES-FOLIO.md` 作索引。
- `vendor/conpty/` 两份 nupkg 与 zip 均随附 Microsoft Terminal MIT 版权/许可原文。
- `OpenConsole.exe` 静态含 fmt/WIL/jsoncpp 等组件,署名只在 `microsoft/terminal` 的 `NOTICE.md`(NuGet 包不带):按 tag 取原文入 `licenses/`,记 URL/tag/SHA-256,整段纳入 `THIRD-PARTY-NOTICES.md`。
- 版权行署名到人:`LICENSE-MIT`、`LICENSE-APACHE` 附录、PE `LegalCopyright` 三处一致。
- 根目录 `TRADEMARK.md`(双许可不授商标,改版必须换名换标识;图标文件的**版权**照常随双许可给)。
- `CONTRIBUTING.md` 的 `## Licensing contributions`(Apache-2.0 §5 口径的入站=出站,不做 CLA/DCO)。
- 一方 crate 全部 `publish = false`,防误发 crates.io。

## 门 2 构建与分发(必须)
- 版本 `0.1.0` 一处定、四处一致:`Cargo.toml`、`--version`(新增;`--help` 已有)、PE `VERSIONINFO`+图标(`build.rs` 资源)、三类诊断文件头。
- `[profile.release]`(现只有 profile.test):LTO/codegen-units/strip 量体积与启动;保留 unwind 直到 panic 策略另定。
- CRT:**先做实验再定**——独立目录 `+crt-static` release 构建(webview2-com-sys 静态链 `WebView2LoaderStatic.lib`)→ PE 导入表无 `VCRUNTIME140*` → 干净 VM 启动;三步过才采用,否则 README 写明需 VC++ Redistributable(winget 有独立拒收标签 `Validation-VCRuntime-Dependency`,runner 预装 VC++ 故 CI 绿灯对此零证明力)。
- zip 精确清单**七个文件**:`folio.exe`、根目录 `conpty.dll` + `OpenConsole.exe`(加载器只读根目录;`x64\` 副本不带)、`LICENSE-MIT`、`LICENSE-APACHE`、`THIRD-PARTY-NOTICES.md`、`TRADEMARK.md`。**README 不进 zip**(用户裁 2026-08-27:相对链接与截图在 zip 里全死)。zip 之外的 release 附件:`SHA256SUMS.txt`、SBOM、`option-ext-0.2.0.crate`(MPL-2.0 源码可得性)。由 tag 触发的 release workflow 自动产出并校验清单/尺寸/哈希,打包前自己跑 `check-notices.ps1` + `check-vendor-notices.ps1`,**禁止从 dist/ 手挑**。
- panic:双击用户要看到可见提示 + 日志路径(`%TEMP%\bt-app-panic.log` 现在无提示);三类诊断文件含版本与 commit。
- 签名:v0.1 **不签**(EV 即时放行 2024 已取消;SignPath 要求「已发布过」),README 写 SmartScreen 说明(MOTW 触发,不教绕);v0.2 首选 SignPath Foundation(免费、OV、基金会名下);**一旦签就不能换身份**。

## 门 3 CI(必须)
- ignored 门重写:保留 20 个探针→版本化 allowlist + `Compare-Object` 双向比较(Codex 给了脚本);`cargo` 退出码先查。
- `windows-latest`→`windows-2025` 明确;第三方 action 钉 commit SHA;`permissions: contents: read` 保持。
- 拆 job:纯逻辑(fmt/clippy/单测)/ 真实 ConPTY(bt-pty 非 ignored 真子进程测试)/ GPU(显式装 WARP,`force_fallback_adapter=false` 不能指望自动找到)/ 产品 smoke(建窗、WebView2 缺席降级卡——runner 无 WebView2 正好、100%/200% DPI、ConPTY `source=sidecar`)。
- 目标公开仓留一次完整绿灯 URL(「本 clone 无 remote」既不证明跑过也不证明没跑过)。

## 门 4 隐私与安全(必须)
- README 隐私节分列两目录:`%APPDATA%\Folio`(settings/session/pins/profiles/schemes/shell integration/diagnostics)与 `%LOCALAPPDATA%\Folio\WebView2`(cookie/storage/cache/autofill),各给清理命令。
- `session.json` 明文披露全字段:URL(含 query/fragment/token)、cwd、文件/目录路径、pane 手动名、页面标题、recent 时间戳、git 分支、monitor_id、窗口几何。
- WebView2:显式 `SetIsGeneralAutofillEnabled(false)` 与密码自动保存显式设定(现依赖默认 true);iframe/subresource 策略给明确结论;已有拒绝面(新窗/下载/权限/外部 URI fail-closed)写进 SECURITY.md。
- hooks 安装器:改**同目录临时文件 + flush/sync + 原子替换**;文档写明接受 `CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`COPILOT_HOME` 重定向、备份与卸载行为、reparse/symlink 处置。
- 管道:SECURITY.md 写 protected logon-SID DACL + 远程拒绝 + fail-closed,**并明写「同登录会话内不做生产者身份认证」**。
- `BT_*` 表:release 仍生效的读/写/截断/内容捕获行为逐条(含 `BT_PROBE_INPUT`、`BT_FOCUS_THUMB_DUMP`、通用 trace 任意路径)。
- diagnostics.log 准确措辞:启动时 4 MiB 轮转保留一代,运行期不限流;hang 报告写模块/偏移不写原始栈。

## 门 5 干净机(必须,发布前最后一道)
Win10 1809+ 与 Win11 各一台无 VS/VC redist/WebView2 的 VM,从 zip 开始:`--version`、PE 图标/VERSIONINFO、shell 输入、resize、ConPTY 来源、WebView2 缺席卡、受控 panic 的可见提示;有 WebView2 的 VM 验顶层 gate 与四个拒绝面。截图入库。

**2026-08-28 首次真跑:未过。** 两台机建好、冒烟七条全绿,红的是产品三条(有预览座的窗口关不掉、无 WebView2 的机器上预览座整块不画、同一批窗口的外壳文字溢出版面)。逐条结果、验收项覆盖表与挂账见 `clean-vm-results-2026-08-28.md`;手册与工具的修补见 `clean-vm.md` §3.4b/§3.4c。

## 门 6 首次体验(该做)
- ~~Store 版 pwsh 探测(AppExecLink)修复~~——**翻案(2026-08-27)**:std 1.85–1.94 的 `is_file` 对 AppExecLink 本就答 true,picker 从未灰过;审计缺口 B「成因 B」是推断错误,只有注释写反(已改+钉)。
- **待裁**:`default_profile` 未设时 floor=winps 改为「探测顺序第一个在场的」(推荐改);PowerShell 整合提示条「一次运行只问一次」(推荐改)。
- 首窗 960×600 不居中、无 `--version`——随门 2。
- 建仓后补 `repository` 并 `repository.workspace = true`(现在 GitHub 仓库还没建,字段先空着;`[workspace.package]` 里 `publish = false` 旁已留位置)。

## 门 7 文档(该做)
README(中英:是什么、截图/GIF、下载、SmartScreen 说明、隐私节、快捷键表自 `BINDINGS` 生成、已知问题含跨 DPI 未复现)、SECURITY.md、CHANGELOG、CONTRIBUTING(三门纪律);内部中文规划文档保留公开(透明)但门 0 已清路径。

## 可后延
winget(需 CRT 门过)、scoop(main bucket 进不去,自建 bucket)、GitHub Pages 单页、v0.2 签名、SBOM 深化。**Windows 11 顶层右键菜单**(「Open Folio here」现只在「显示更多选项」经典层;顶层需带包身份的稀疏 MSIX + `IExplorerCommand` COM 服务器 + 签名——排在 v0.2 签名之后)。

## 发布判据(条件,非日期)
门 0–5 全绿 + 门 6 两裁决落地 + 目标仓一次绿灯 + 历史重写完成 + 干净机截图入库。
