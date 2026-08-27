# Folio v0.1 preview 发布就绪审计复核

复核对象：docs/plans/release/readiness-audit-2026-08-27.md；最终复核快照 HEAD=a1c239b6e891cd2c4b46a8e39552d604ee993d7d。口径：只读检查当前工作树、索引、Cargo 元数据和本机 Cargo 注册表缓存；未运行会写入 target/ 的构建或测试命令。审阅期间出现的其他并行工作树修改不作为原审计的反证。以下只列可由文件、行号或命令输出证明的问题，不补签名外部行情，也不补原审计第 6 节明确留出的三条首次体验调查。

## 审计中的错误

1. **“跟踪文件里没有任何邮箱地址”和“仓库清洁完整盘完”是错误结论，且这是公开前阻塞项。**

   - 原审计 docs/plans/release/readiness-audit-2026-08-27.md:199-201 使用 git grep -I；-I 会跳过被 Git 判为 binary 的 BTCR，因此给出了假阴性。
   - rg -a -n -m 4 'weiyishi024@gmail\.com|Welcome back Weiyi|C:\\Users\\Weiyi|D:\\Developer\\BetterTerminal' corpus 的输出包括：
     - corpus/claude-code-session.btcr:29：Welcome back Weiyi、weiyishi024@gmail.com's Organization、D:\Developer\BetterTerminal。
     - corpus/claude-code-session.btcr:1：C:\Users\Weiyi\.local\bin\claude.EXE。
     - corpus/cargo-build-flood.btcr:1：C:\Users\Weiyi\.cargo\bin\cargo.exe；同文件 :57、:62-63 等含 D:\Developer\BetterTerminal。
   - docs/spikes/01-corpus.md:30、:34 明说 claude-code-session.btcr 是 Claude Code 2.1.210 的真实交互录制，不是合成夹具；corpus/README.md:9-10 又声称每份夹具的来源与环境替换已记录。这里不是“可能泄露”，而是文件中实际存在姓名、邮箱、组织名、用户名、安装路径和真实会话 UI。

2. **554 个外部包的许可证表少了 4 个条目。**

   - cargo metadata --format-version 1 --locked 的当前输出是 external=554、registry=552、nosource=16（14 个 bt-* 加 2 个本地 vendor）。
   - 按 license 分组有一组 4 × Apache-2.0/MIT：fxhash 0.2.1、pollster 0.4.0、rustc-hash 1.1.0、shared_library 0.1.9；原表 docs/plans/release/readiness-audit-2026-08-27.md:74-105 没有这一行，表内数量相加仅 550。
   - 因而 :70 的“554 个外部包全部命中”和 :427 的“许可完整盘完”不能由所列全景表支持。

3. **typst 内嵌资产不是审计所称的“OFL + GUST + Bitstream Vera + PDFium + CC0 五种”。**

   - %USERPROFILE%\.cargo\registry\src\...\typst-assets-0.15.1\NOTICE:452-465 还列出 WHATWG 派生数据的 CC BY 4.0 与 BSD-3-Clause。
   - 同文件 :820-853 列出 W3C Document License，且 :840-843 明确要求所有副本或其部分带完整 NOTICE。
   - 同文件 :898-910 明确说 NewCM10-Regular 是 GPL-3.0-or-later + Font Exception + Distribution Exception，不是 GUST。
   - 原审计 :117、:383 把资产义务压成五类，并把 NewCM10-Regular 放进 GUST 描述，结论不完整。这里不推导 GPL 对 Folio 的传播效果；可证事实是发布判据不能只生成那五类摘要，必须保留并核对上游完整 NOTICE。

4. **对 vendor/alacritty_terminal 的 Apache-2.0 判断有两处表述错误。**

   - 本机同版本上游缓存 alacritty_terminal-0.26.0 根目录只有 LICENSE-APACHE，没有 NOTICE；Apache-2.0 §4(d) 只要求传播“作品所含的 NOTICE”，所以“目录里没有 NOTICE”本身不是缺陷。
   - 文件哈希对比 vendor 与缓存上游得到 diff_count=23；rg -l -i 'BetterTerminal|Folio|modified|modification|changed from upstream' vendor/alacritty_terminal 只命中 5 个文件，其中 LICENSE-APACHE 不是被改源文件。也就是说，审计 :114 的“没有任何改动说明”太绝对，但多数差异文件又确实没有文件内改动声明。
   - 已有标记可见 vendor/alacritty_terminal/src/grid/mod.rs、src/grid/row.rs、src/term/mod.rs、src/tty/windows/mod.rs；其余差异需逐文件处理。审计 :125 单独增加 CHANGES-FOLIO.md 可以改善可读性，但不能替代 Apache-2.0 §4(b) 对“modified files”的显著声明。

5. **“本树没有 C/C++ 依赖，所以 +crt-static 安全”的前提为假。**

   - Cargo.lock 锁定 webview2-com-sys 0.39.1；其缓存 Cargo.toml:15-17 声明 build.rs。
   - %USERPROFILE%\.cargo\registry\src\...\webview2-com-sys-0.39.1\build.rs:39-45 复制 WebView2Loader.dll、WebView2Loader.dll.lib、WebView2LoaderStatic.lib；src/lib.rs:13-22 在 MSVC 下以 kind="static" 链接 WebView2LoaderStatic。
   - rustc --print cfg --target x86_64-pc-windows-msvc 当前输出没有 target_feature="crt-static"；现有 dist/folio-next11.exe 的 PE 导入又确有 VCRUNTIME140.dll 与 VCRUNTIME140_1.dll。原审计 :134 的现状证据成立，:136 的兼容性结论不成立。
   - 正确结论只能是：在独立输出目录做一次 +crt-static release 构建，检查链接是否成功及最终 PE 导入表，再做干净机启动验证；在这三个结果出来前，不能把静态 CRT 写成“一行安全修复”。

6. **便携包不需要 x64\OpenConsole.exe；运行时契约只要求根目录的一对文件。**

   - vendor/conpty/portable-pty/src/win/psuedocon.rs:169-178 只检查 current_exe 父目录下的 conpty.dll 与 OpenConsole.exe。
   - crates/bt-pty/extract-conpty-sidecar.ps1:68-74 把 OpenConsole.exe 额外镜像到 x64\，注释说明这是 NuGet native .targets 布局；它不是 Folio 加载器的读取路径。
   - 因而原审计 :145 的 zip 精确清单多列了 x64\OpenConsole.exe。带上不会破坏运行，但会无谓重复一个约 1.7 MiB 的二进制；真正硬门是 folio.exe 同目录下根对根的 conpty.dll + OpenConsole.exe。

7. **ignored 测试数量和分布已经写错；门失效的原因判断则是对的。**

   - rg -n '^\s*#\[ignore(?:\s*=.*)?\]' crates vendor -g '*.rs' 的当前输出是 ignored_attributes=20：crates/bt-pty/src/lib.rs 19 个，crates/bt-app/src/marks.rs:6073 1 个；bt-term/session.rs 与 bt-transcript/paths.rs 为 0。
   - 原审计 :163、:402 写成 23 个，并列出已经不存在的 3 个位置；“23 个精确白名单”会把当前正确树判红。
   - .github/workflows/ci.yml:122-123 的确永远可能被任意一个测试二进制的 0 tests, 0 benchmarks 短路，而且没有先检查 cargo 的退出码。若政策真是“零 ignored”，可直接写成：

     ~~~powershell
     $lines = @(cargo test --workspace --locked --color never -- --ignored --list 2>&1)
     if ($LASTEXITCODE -ne 0) {
       $lines | Write-Host
       throw "listing ignored tests failed"
     }
     $ignored = @($lines | Where-Object { "$_" -match ':\s+test$' })
     if ($ignored.Count -ne 0) {
       $ignored | Write-Host
       throw "$($ignored.Count) ignored tests are not allowed"
     }
     ~~~

   - 当前 20 个里 19 个的属性文字明确写 dev probe/upstream A/B/host-timing sensitive，另一个明确“writes a picture”；如果政策是保留本地探针，正确做法不是断言零，而是把 20 个完整测试名放进版本化 allowlist，用 Compare-Object 双向比较，任何新增、删除或改名都红。

8. **“CI 从来没跑过”超出了证据。**

   - git remote -v 为空和 gh run list 无法确定仓库，只能证明当前 clone 无法查询远端；它们不能证明这个工作流在任何远端历史上从未运行。
   - 原审计 :162 应改为“本地无可核验的 GitHub Actions 运行记录，发布前必须在目标公开仓取得一次绿灯”。这是未验证，不是已证明的历史事实。

9. **CI 对 WebView2 的疑点放错了位置；真正未闭合的是 GPU 和真实 ConPTY 测试。**

   - crates/bt-platform/src/webview.rs:750 是生产创建入口；该文件 :2119-2252 的 8 个单元测试只测矩形和 handoff 状态，不调用 CreateCoreWebView2EnvironmentWithOptions。WebView2 Runtime 缺失不会阻止这些单元测试。
   - crates/bt-render/src/lib.rs:16633-16646 与 :17364-17377 是非 ignored 的真 adapter 测试；GpuContext::headless 在 :3850-3861 设置 force_fallback_adapter=false。仓库 CI 没有安装 WARP。
   - wgpu 自己的 Windows 测试工作流明确执行 .github/actions/install-warp，见 https://github.com/gfx-rs/wgpu/blob/trunk/.github/workflows/ci.yml 。这不是 Folio 必然失败的证明，但足以证明原审计不能把“标准 runner 会自动落 WARP”当既定条件。
   - crates/bt-pty/src/lib.rs:2275-2305、:2531-2577 是非 ignored 的真实 ConPTY/cmd/PowerShell 测试；它们会在 cargo test --workspace 中实际启动子进程。
   - DPI 命名测试如 crates/bt-platform/src/lib.rs:8801、crates/bt-render/src/lib.rs:12305、:14300、:14537 和 crates/bt-app/src/main.rs:95706、:96075 使用标量/fixture；生产的 GetDpiForWindow 在 crates/bt-platform/src/lib.rs:5007-5013。当前证据没有显示这些单元测试需要交互桌面。
   - .github/workflows/ci.yml:15、:48 使用会迁移的 windows-latest；截至复核日官方 runner-images 将其映射为 Windows Server 2025，见 https://github.com/actions/runner-images 。可行性结论应是：WebView2 单元测试不是门；GPU adapter 和真实 ConPTY 必须在目标 runner 实跑，且 GPU job 应显式安装/选择软件 adapter。

10. **WebView2 数据目录与设置目录并不相同。**

    - crates/bt-app/src/persist.rs:763-780 将设置/会话放到 %APPDATA%\Folio。
    - crates/bt-app/src/webhost.rs:440-455 明写 “%LOCALAPPDATA% and never %APPDATA%”，路径为 %LOCALAPPDATA%\Folio\WebView2。
    - 原审计 :177 和发布判据 :391 写成同一个 %APPDATA%\Folio 目录，并说“清预览缓存”和“删设置”是同一动作，均错误。README 和卸载说明必须分别列出 roaming 配置与 local WebView profile。

11. **diagnostics.log 不是“没有轮转/上限”，hang 报告也不把 128 KiB 原始栈字节写入文件。**

    - crates/bt-app/src/diagnostics.rs:60-76 定义 diagnostics.prev.log 与 4 MiB 启动轮转；:161-172 执行替换；:203 调用。准确说法是“每次启动检查，保留一代；当前运行期间不逐写限流，因此单次运行仍可超过 4 MiB”。
    - crates/bt-app/src/hang_watch.rs:768-795 的报告写 RIP/RSP、扫描字节数、模块解析出的地址；原始栈读取发生在 crates/bt-platform/src/hang.rs:49-55、:316、:501-516，文件只写模块名/偏移/地址，不写原始 128 KiB。
    - 因而原审计 :179 的“没有轮转/上限”和 :180 的“裸栈写到报告”都不准确。报告仍含地址和运行状态，应提醒用户审阅，但风险描述要按实际格式写。

12. **hooks 安装器的“只写用户目录、路径问系统要”描述过强。**

    - crates/bt-app/src/attention_hooks.rs:92-96、attention_codex.rs:115-119、attention_copilot.rs:215-219 都优先无条件接受 CLAUDE_CONFIG_DIR、CODEX_HOME、COPILOT_HOME，并把值直接变成 PathBuf；设置这些环境变量的人可把写入重定向到任意目录。
    - 三者分别在 attention_hooks.rs:355-370、attention_codex.rs:268-279、attention_copilot.rs:511-532 忽略备份写失败，再用 std::fs::write 直接覆盖目标；没有临时文件 + flush/sync + rename 的原子提交，也没有 canonicalize/reparse/symlink 边界检查。
    - 它们拒绝不可解析配置、保留非本产品条目，这些优点成立；但原审计 :182、:414 不能据此宣称文件系统边界已经闭合。需要明确“接受上游同名环境变量重定向”和“写入不是原子的”。

13. **release 中生效的 BT_* 内容/文件开关不止审计列的四个。**

    - BT_PTY_DUMP 在 crates/bt-pty/src/lib.rs:81、:182-184 对任意给定路径使用 File::create，截断并写完整 PTY 数据及 .chunks。
    - BT_PROBE_INPUT 在 crates/bt-app/src/main.rs:83172、:83175-83190 读取任意给定文件并替代 PTY 输入。
    - BT_FOCUS_THUMB_DUMP 在 main.rs:20272-20299，BT_CHROME_DUMP 在 :20202-20248/:20317-20360，BT_IME_TRACE 在 :72696-72703，通用 append 路径在 crates/bt-app/src/trace.rs:41-60。
    - BT_WEB_DEV 在 crates/bt-app/src/webhost.rs:417-433 可于启动时指定 URL；BT_POWERSHELL_PROFILE 与 BT_PSREADLINE_DOCUMENTS 分别在 shell_integration.rs:659-665、psreadline.rs:825-834 重定向用户文件操作。
    - 原审计 :296-301 漏了 BT_PROBE_INPUT、BT_FOCUS_THUMB_DUMP 以及通用文件 trace 的任意路径副作用。它们都要求先控制进程环境，不构成提权；应记录的是读取、创建、追加或截断文件以及可能捕获终端内容的事实。

14. **部分 file:line 与统计已不再对应当前树，不能直接拿来做机械验收。**

    - 当前 Cargo.toml:25 是 version = "0.0.0"，:27 是 license；原审计多处写 :21/:22。
    - 当前工作区 Cargo.toml:7-20 有 14 个 bt-* crate；原审计 :40 写“13 个一方 crate”，而它自己的 :70 又按 14 个本地 bt-* 计数。
    - 当前 .gitignore 的 .claude/worktrees/ 在 :28，不是原审计 :267 所称 :24。
    - 这些不改变“版本、根许可证、忽略规则”的大结论，但说明报告应记录审计 commit SHA，后续以该 SHA 的永久链接取证；否则并行开发一移动行号，证据就失去可复现性。

## 审计遗漏

1. **大文件/二进制盘点没有进入“公开仓清洁”结论。**

   - 只读等价命令 git ls-files → Get-Item.Length → Sort-Object Bytes -Descending 的前几名是：
     - vendor/alacritty_terminal/tests/ref/row_reset/grid.json：19,157,581 bytes；
     - assets/fonts/NotoColorEmoji_WindowsCompatible.ttf：10,682,364 bytes；
     - vendor/alacritty_terminal/tests/ref/history/grid.json：9,744,490 bytes；
     - crates/bt-app/src/main.rs：5,432,996 bytes；
     - 两个 vendor/conpty/*.nupkg：1,719,158 与 1,717,498 bytes。
   - .gitattributes:1-7 只有换行与 recording binary 规则；Select-String .gitattributes 'filter=lfs' 输出 0。没有文件超过 GitHub 单文件 100 MiB 限制，但两个近 10/20 MiB JSON fixture 和两份 nupkg 会永久进入 clone；审计 :426 宣称“完整盘完”却没有给 top-30 或保留理由。
   - git ls-files dist 输出 0，git check-ignore -v dist/folio-next11.exe 命中 .gitignore:26；这一点原审计是对的，dist 中现有 exe 不会随普通提交公开。

2. **.gitignore 对公开仓常见本机/凭据文件没有防线。**

   - .gitignore:1-30 没有 .env*、*.pfx、*.p12、*.pem、*.key、*.pdb、*.ilk、.vs/、.vscode/；只在 :28 忽略 .claude/worktrees/，没有忽略整个 .claude/。
   - git ls-files -ci --exclude-standard 当前输出 0，说明没有“已跟踪且被忽略”的现存冲突；遗漏是将来误加文件没有预防规则，不是声称仓里已经有这些秘密。

3. **scripts/ 中仍有本机路径和用户名。**

   - scripts/dev/width-acceptance.ps1:2 固定 D:\Developer\BetterTerminal。
   - scripts/dev/ui-probe.ps1:672 固定 D:\Developer\BetterTerminal\target\release\folio.exe。
   - scripts/shell-integration/folio.bash:109-111 的说明示例使用 /home/weiyi/src。
   - 这些至少会让公开脚本在别人的 checkout 上不可复现或继续传播个人用户名；原审计的 Weiyi 扫描只讨论 crates/ 夹具，漏了 scripts/。

4. **design/ 与 test-assets/ 没有仓内来源/许可说明，不能据“列过目录”宣告来源已核。**

   - git ls-files 统计 design 下有 21 个 PNG/JPG/SVG/ICO 类资产，design 内 README/LICENSE/NOTICE 数为 0；.gitignore:7-10 又专门把这些 PNG 重新纳入版本控制。
   - test-assets 有 22 个跟踪文件，README/LICENSE/NOTICE 数为 0；folio-pdf-test.html 与 folio-pdf-test.pdf 同在，可证明 PDF 至少有仓内源文档，但其余夹具没有统一的生成/来源清单。
   - 这不是断言资产来自第三方；可证结论是审计 :426 没有留下足以复核“自制/上游/生成物及许可”的证据。公开前应为这两个目录补一份逐类 provenance 清单。

5. **两份 Microsoft ConPTY NuGet 包的 MIT 文本没有随源码仓分发。**

   - git ls-files vendor/conpty 显示两个 nupkg、README.md 和 portable-pty/LICENSE.md；没有 Microsoft Terminal LICENSE 文件。
   - 读取两个 nupkg 的 ZIP entry，只命中 Microsoft.Windows.Console.ConPTY.nuspec，没有 LICENSE/NOTICE entry。
   - vendor/conpty/README.md:12 只给上游 LICENSE 链接；portable-pty/LICENSE.md:1-13 是 Wez Furlong 的 portable-pty MIT，不是 Microsoft Terminal 的版权声明。
   - 因此不只 release zip，公开源码仓本身也在再分发两个第三方二进制包；THIRD-PARTY-NOTICES 或 vendor/conpty/ 下应携带对应 Microsoft MIT 版权与许可原文。

6. **Unicode-3.0 的随附义务完全没有进入“七类义务”和发布硬门。**

   - 元数据统计有 25 个 Unicode-3.0 包，原审计 :81 已看到它们。
   - %USERPROFILE%\.cargo\registry\src\...\icu_collections-2.2.0\LICENSE:5、:13-22 要求版权与许可声明出现在所有副本或相关文档。
   - 原审计 :110-120 的七类义务和 :380-384 的法律硬门均没再出现 Unicode-3.0。许可证生成配置必须保留该组全文/声明，而不只是统计计数。

7. **option-ext 的处理不能只写“指向 crates.io / 上游仓库即可”。**

   - %USERPROFILE%\.cargo\registry\src\...\option-ext-0.2.0\LICENSE.txt:162-176 要求被覆盖源码以 MPL 条款可得，并告知可执行文件接收者如何取得；:358-360 还要求随文件提供 MPL 副本或取得地址。
   - 对当前未修改的 registry 版本，发布材料应固定到 option-ext 0.2.0 的精确源码归档/源码仓 commit，并附 MPL-2.0 文本和取得说明；泛指 crates.io 首页无法固定“对应这份 executable”的 Source Code Form。

8. **hayro 还嵌入两份 ICC profile，审计只列了 14 款 Foxit 字体。**

   - %USERPROFILE%\.cargo\registry\src\...\hayro-interpret-0.7.0\src\color.rs:669 include_bytes LAB.icc，:1067 include_bytes CGATS001Compat-v2-micro.icc。
   - 同包 assets/README.md:1 说 CMYK profile 来自 Compact-ICC-Profiles、CC0-1.0；:15 说 LAB.icc 由 LCMS2 生成；assets/CGATS_LICENSE.txt:1 是 CC0 文本。
   - 法律清单应同时纳入这些 ICC 资产的来源与适用声明，不能只覆盖 Cargo.toml 的 crate license 和 Foxit 字体。

9. **session.json 明文范围远大于 URL 与 cwd。**

   - crates/bt-persist/src/layout.rs:108-111 存 profile_id、cwd、manual_name；:136-145 存 files root/open/sel。
   - crates/bt-persist/src/session.rs:214-230 存窗口位置、DPI、monitor_id；:336-375 存 git branch 过滤；:395-425 存预览 path/title/source 和 recent timestamp；:511-534 存 recent seed 的 profile/cwd/manual_name/root/path。
   - 原审计 :178 和发布判据 :391 只要求披露 URL/query/fragment/token 与 cwd，漏了文件/目录路径、手动 pane 名、页面标题、最近历史时间、git 分支、显示器标识和窗口几何。

10. **WebView2 一项默认开启的隐私设置没有盘：general autofill。**

    - rg 'IsGeneralAutofillEnabled|IsPasswordAutosaveEnabled' crates/bt-platform/src/webview.rs 输出 0；现有设置只在 webview.rs:1071-1106 关闭 web message、host objects、status bar、context menu并开启 devtools。
    - Microsoft 的 CoreWebView2Settings 文档明确 IsGeneralAutofillEnabled 默认 true，并会保存/自动填充一般表单信息：https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2settings.isgeneralautofillenabled 。
    - 由于 Folio 使用持久化的 %LOCALAPPDATA%\Folio\WebView2 profile（webhost.rs:440-455），发布前应显式 SetIsGeneralAutofillEnabled(false)，或在隐私说明中准确披露 profile 会保存这类数据。密码自动保存也应显式设定，而不是依赖 WebView2 版本默认值。
    - 原审计关于顶层 NavigationStarting（webview.rs:1147-1185）以及缺 FrameNavigationStarting/WebResourceRequested 的判断正确；它还漏写已有的拒绝面：新窗口 :1294-1303、下载 :1315-1331、权限 :1333-1342、外部 URI :1379-1391 均 fail closed。

11. **注意力命名管道的 DACL 结论正确，但威胁边界没有进入公开判据。**

    - crates/bt-platform/src/attention_pipe.rs:243-259 构造 D:P(A;;GA;;;<logon SID>)，:390-399 取不到 SID 时 fail closed，:622-625 使用 PIPE_REJECT_REMOTE_CLIENTS；这些支持原审计的正面结论。
    - 同文件模块说明 :18-24 明确不把恶意同一用户进程纳入威胁模型。DACL 做到的是“同登录会话”，不是“只允许 Folio 安装的三个 hook 生产者”。
    - SECURITY/README 应逐字说明这个边界，避免把“只有当前会话能连”误读成同会话内的生产者身份认证。

12. **没有可复现的 release 流水线和归档清单门。**

    - git ls-files .github 只有 .github/workflows/ci.yml；该文件只有 adapter-boundary 与 check 两个 job（:13-123），没有 cargo build --release、打包、清单比对、THIRD-PARTY-NOTICES 校验、SHA256、SBOM 或 artifact upload。
    - 现有工作流还使用 actions/checkout@v4（:18、:51）、dtolnay/rust-toolchain@master（:54）、Swatinem/rust-cache@v2（:57）等可移动标签；permissions: contents: read（:7-8）是正确的最小权限，但第三方 action 未钉 commit SHA。
    - audit :146 只提出“写 release 脚本”，发布判据六条没有要求从干净 tag 可重复地产出精确归档。

13. **panic 的用户可见性虽在正文发现，却没有进入发布硬门。**

    - crates/bt-app/src/main.rs:17 使用 windows_subsystem="windows"；panic hook 只追加 temp 目录日志并调用旧 hook，现有行为不会给双击用户一个可见路径。
    - 原审计 :181 已正确建议 message box，但发布判据第 2 条 :378-379 只要求日志带版本号，没有要求崩溃时告诉用户日志在哪里。两者必须分开验收：诊断内容可归因，以及用户能发现诊断文件。

## 对发布判据的补充

1. **增加“公开源码快照清洁门”，并放到现有六条之前。**

   - 对拟打 tag 的 commit 运行二进制感知扫描，而不是 git grep -I：至少 rg -a 搜邮箱、C:\Users、/home/、D:\Developer、常见 token/private-key 形状；检查 git ls-files -ci --exclude-standard、git ls-files dist、git status --porcelain。
   - 必须先替换或删除 corpus/claude-code-session.btcr 与 cargo-build-flood.btcr 中本次已证实的个人/机器数据；scripts/dev 两个绝对路径和 folio.bash 的个人用户名也应清理。
   - 生成并人工签收 tracked top-30 大文件表；对两个大型 alacritty JSON、Noto 字体、两份 ConPTY nupkg逐项写 keep/remove 理由。对 design/、test-assets/、corpus/ 给出仓内 provenance 清单。
   - 当前更宽 secret regex 对工作树扫描除审计文档自述外未发现 AKIA/AIza/GitHub token/OpenAI key/private key 形状；这不替代带规则库的全历史扫描。发布门应对最终历史选择执行 gitleaks/trufflehog 等扫描并保存结果。

2. **把构建/分发门改成“由 tag 自动产出的可审计归档”。**

   - 从 clean checkout 执行 cargo build --release --locked；版本为 0.1.0，folio --version、PE VERSIONINFO、文件名和诊断头版本一致。
   - 明确 [profile.release]（当前 Cargo.toml 只有 :151-152 的 profile.test），记录 LTO/codegen-units/strip 的体积与启动对比；保留 unwind 直到 panic 诊断策略另有替代。
   - 静态 CRT 只能在 webview2-com-sys 的 WebView2LoaderStatic.lib 实际链接成功、PE 不再导入 VCRUNTIME140*.dll、干净机启动通过后采用；否则归档必须声明并验证 VC++ Redistributable 前置条件。
   - zip 精确清单应是 folio.exe、根目录 conpty.dll、根目录 OpenConsole.exe、README、双许可证、完整第三方声明；x64\OpenConsole.exe 不应在没有运行时消费者的情况下重复携带。
   - 自动校验 zip 内文件名、尺寸与 SHA-256，生成 SHA256SUMS.txt、依赖/SBOM 清单并上传同一次 workflow 的 artifact；禁止从开发机 dist/ 手工挑文件。

3. **把法律门改为“完整文本 + 精确源码 + 修改文件声明”，不能只点名七类。**

   - cargo-about/cargo-deny 配置和输出必须签入，CI 用 Cargo.lock 做漂移检查；生成物覆盖全部 554 个外部包，包括漏掉的 4 × Apache-2.0/MIT 和 25 × Unicode-3.0。
   - option-ext 0.2.0 提供精确对应源码、MPL-2.0 全文与取得方式。
   - typst-assets 直接保留/传播上游完整 NOTICE，不能手摘成五类；纳入 CC BY 4.0/BSD-3、W3C Document License、NewCM10 的 GPL3+FE+DE。
   - hayro 纳入 Foxit 字体及 LAB/CGATS ICC；Noto OFL、Material gear、十套 scheme、PSReadLine 保持原审计已有要求。
   - vendor/alacritty_terminal 对 23 个实际差异逐文件核对 §4(b) 标记；不要把不存在的 upstream NOTICE 当缺件。
   - 对仓内两份 Microsoft ConPTY nupkg 随附 Microsoft Terminal MIT 版权/许可原文；release zip 同样包含。

4. **把 CI 门拆成确定性的纯逻辑门、真实 Windows 门和 GPU 门。**

   - 修 ignored 门；若保留 20 个探针，版本化完整 allowlist 并双向比较，不再写“禁止 ignored”；若政策为零，就用本报告给出的逐行解析脚本。
   - 将 windows-latest 改为明确的 windows-2025（或另一个明确支持矩阵），第三方 actions 钉完整 commit SHA。
   - 普通 job 跑 fmt、clippy、纯逻辑/WebView 状态机/DPI 算术测试；真实 ConPTY 测试在 Windows job 明确跑并保存失败输出。
   - GPU job 显式安装并选择 WARP/软件 adapter，或使用有已知 adapter 的 runner；不能依赖 force_fallback_adapter=false 自动找到设备。
   - 另设产品 smoke：实际创建窗口/WebView2、WebView2 缺席降级、100%/200% DPI、ConPTY sidecar source=sidecar。WebView2 Runtime 不应被误列为当前单元测试 job 的前置。
   - 目标 GitHub 仓必须留下至少一次完整绿灯 URL；“本 clone 无 remote”不能作为没跑过或跑过的证据。

5. **扩大干净机门：不仅“窗口起来”，还要验归档身份和故障可见性。**

   - Windows 10 1809+ 与 Windows 11、无 VS/VC redist/WebView2 的 VM 各从 release zip 开始，验证 folio --version、PE 图标/VERSIONINFO、shell 输入、resize、实际 ConPTY 来源、WebView2 缺席卡。
   - 人工触发一条受控 panic，确认用户看到可见提示、提示给出 folio-panic.log 路径，且 panic/diagnostics/hang 三类文件均含版本与 commit。
   - 在有 WebView2 的 VM 实际访问测试页，验证顶层 gate、新窗口/下载/权限/外部 URI 拒绝；对 iframe/subresource 策略作出明确结论；显式设置或披露 general autofill/password autosave。

6. **把隐私与卸载说明设为硬门，而不是 README 的软答案。**

   - 分开列 %APPDATA%\Folio（settings/session/pins/profiles/schemes/shell integration/diagnostics）与 %LOCALAPPDATA%\Folio\WebView2（cookie、storage、cache、autofill profile），分别给清理命令。
   - 披露 session/pins/recent 中的 URL/query/fragment/token、cwd、文件/目录路径、pane 手动名、页面标题、时间戳、git 分支、monitor_id 与窗口几何。
   - 准确写 diagnostics.log 的 4 MiB 启动轮转/一代保留、hang 报告的模块地址而非原始栈、temp panic log，以及各 BT_* 开关的读/写/截断/内容捕获行为。
   - hooks 文档列出三个默认目标文件、上游环境变量可重定向路径、备份与卸载行为；代码发布前至少改为同目录临时文件 + flush/sync + atomic replace，并对 reparse/symlink 选择作明确处理。
   - 管道文档说明 protected logon-SID DACL、远程拒绝与 fail-closed，同时明确“同一登录会话内不做生产者身份认证”的威胁边界。

7. **增加发布供应链与来源门。**

   - 发布 commit/tag 必须固定；报告记录该 SHA，所有 file:line 使用该 SHA 的永久链接。
   - 锁文件、vendored patch、ConPTY nupkg 哈希、生成的 notices/SBOM 与 release archive 哈希由同一流水线关联；对两份 nupkg 的保留和 A/B 用途设测试或书面理由。
   - release workflow 使用最小 permissions、environment 审批和不可移动 action SHA；构建日志保留 rustc/cargo/toolchain 与 runner image 版本。

总评：原审计正确识别了“当前不能公开”的主结论，但 corpus 真实隐私数据、许可证漏项、静态 CRT 错误前提和不可执行的 CI 判据使其“六面完整盘完”的自评不成立，v0.1 preview 仍应视为未达到发布门槛。
