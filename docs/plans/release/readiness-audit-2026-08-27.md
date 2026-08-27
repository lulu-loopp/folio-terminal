# Folio v0.1 preview 公开发布就绪审计

> 日期 2026-08-27 ・ 范围 `D:\Developer\BetterTerminal` 全仓 + 依赖图 + 产物
> 性质:**只读审计**。除本文件外未改动仓库任何内容。
> 目标形态:GitHub 公开仓(源码)+ Releases 便携 zip;README 即首页,GitHub Pages 后延。
>
> **取证口径**:每条带 `file:line`、命令输出或实测数据。没验证到的一律标 `[未盘]` / `[未验证]`,不推测、不补编。
> **覆盖度声明见文末第 6 节** —— 原单八个面里有一个面(签名的外部行情)因子代理未返回而未盘,已如实标注。

---

## 0. 三十秒版

**今天的仓库不能直接公开**,不是因为质量,是因为**三样东西根本不存在**:

1. **没有 LICENSE 文件**,而 `Cargo.toml:22` 写着 `license = "MIT OR Apache-2.0"` —— 声明与仓库自相矛盾。
2. **根目录没有 README** —— GitHub 首页会是空的。
3. **没有第三方许可声明**,而 exe 里嵌着 OFL 字体、GUST 字体、PDFium BSD 字体、一枚逐字的 Google Material 图标、
   BSD-2 的 PSReadLine 二进制,链接图里还有一个 **MPL-2.0** 的包。

**另有三个"装上就炸/悄悄变差"级问题**:exe 静态导入 `VCRUNTIME140.dll`(干净 Windows 上双击直接报缺 DLL);
ConPTY sidecar 不进 zip 会**静默**退回 inbox 实现(resize 稳定性回归,不报错);
版本号是 `0.0.0` 且 `--version` 会被当未知参数以退出码 2 拒绝(收到 bug 报告认不出是哪个 build)。

**好消息,而且都是硬证据**:

- **全树没有任何网络客户端**。`Cargo.lock` 568 个包里没有 reqwest / ureq / hyper / h2 / curl / tokio / rustls / native-tls / openssl;
  `grep -rnE '\b(TcpStream|TcpListener|UdpSocket|WinHttp*|InternetOpen*|URLDownload*|WSAStartup)\b' crates/ vendor/` **零命中**;
  `telemetry|analytics|sentry|update_check|phone_home|beacon` **零命中**。全工作区唯一的 `std::net` 引用是
  `crates/bt-transcript/src/lib.rs:212` 的 `host.parse::<std::net::Ipv6Addr>()`,是超链接识别里的字符串校验,不建连接。
  **"无遥测"这句话可以写得理直气壮。**
- **两条迁移链逐级连续、零缺口**:`settings.json` v1→v24(`crates/bt-persist/src/migrate.rs:38-62`,23 步)、
  `session.json` v1→v11(`:547`,10 步),`keybindings`/`pins`/`profiles` 都还在 v1。
- **注意力管道的安全描述是对的且是 fail-closed**:`crates/bt-platform/src/attention_pipe.rs:257-258`
  `D:P(A;;GA;;;<logon sid>)` —— **protected** DACL、**唯一一条** ACE、principal 是登录会话;
  拿不到 logon SID 就**根本不开端点**(`:390-395`),而不是退回一个宽松的默认值。
- **WebView2 缺席不致命**:`crates/bt-app/src/webhost.rs:730` 的 `RuntimeMissing` 是一张带
  "Download the runtime" 动词的卡(`:865`),终端本体照常工作,只有网页预览降级。
- **`unsafe` 纪律真的在执行**:`Cargo.toml:42-43` `unsafe_code = "deny"`;
  13 个一方 crate 里 **12 个 `unsafe` 计数为 0**,唯一允许 `unsafe` 的是 `bt-platform`(307 处,Win32 绑定层,
  其 `crates/bt-platform/Cargo.toml:105-108` 只有 `[lints.clippy]`、没有 `[lints.rust]`);
  全树只有**三处**具名逃生口(`bt-render/src/lib.rs:2785`、`bt-term/tests/lifecycle_matrix.rs:696-702`、
  `bt-viewport/tests/horizontal_budget.rs:53-59`),没有一处是偷偷的。

---

## 1. 必须做(不做就不能公开)

### M1 · 仓库根没有 LICENSE,而清单已经声明了许可

| | |
|---|---|
| **证据** | `git ls-files \| grep -i license` 只返回 `assets/fonts/NotoColorEmoji-LICENSE`、`assets/psreadline/2.4.6/License.txt`、`spikes/03-math-engine/assets/{KATEX,MITEX}-LICENSE`、`vendor/alacritty_terminal/LICENSE-APACHE`、`vendor/conpty/portable-pty/LICENSE.md`。**根目录零个。** 而 `Cargo.toml:22` 是 `license = "MIT OR Apache-2.0"` |
| **建议** | 补 `LICENSE-MIT` + `LICENSE-APACHE` 全文,README 末尾一句 dual-license。**版权行署名要当场定**(见 M9) |
| **工作量** | 小 |

### M2 · 根目录没有 README —— 公开首页是空的

| | |
|---|---|
| **证据** | `git ls-files \| grep -iE "^readme"` 只有 `assets/schemes/README.md`、`corpus/README.md`、`docs/spikes/README.md`、`vendor/*/README.md`。根目录没有。全仓 931 个跟踪文件,`docs/` 295 份里 **235 份含中文**(`git grep -lP "[\x{4e00}-\x{9fff}]" -- docs CONVENTIONS.md \| wc -l` = 235,分母 296) |
| **必须写进去的八件** | ① 这是什么 + 截图;② **系统要求**(Windows 10 1809+/11、WebView2、VC++ 运行库或 crt-static);③ 安装 = 解压运行 `folio.exe`;④ **未签名 exe 的 SmartScreen 怎么过**;⑤ **数据落在哪 + 无遥测声明**(见 M8);⑥ 从源码构建;⑦ 已知问题;⑧ 许可 |
| **建议** | README 用**英文**。中文内部文档的公开范围另裁(见 S11) |
| **工作量** | 中 |

### M3 · 没有第三方许可声明,而七类义务已经触发

许可工具 **本机三个都没装**(`which cargo-license cargo-deny cargo-about` 全 not found),
所以下表是**直接读 `~/.cargo/registry/src/index.crates.io-*/<crate>-<ver>/Cargo.toml` 的 `license` 字段**统计的:
`Cargo.lock` 568 个包中 **554 个外部包全部命中**,只有 14 个 `bt-*` 本地包不在注册表缓存里。

**许可全景(554 个外部 crate,实测):**

| 数量 | 许可 |
|---:|---|
| 246 | MIT OR Apache-2.0 |
| 101 | MIT |
| 50 | Apache-2.0 OR MIT |
| 31 | MIT/Apache-2.0 |
| 28 | Apache-2.0 |
| **25** | **Unicode-3.0** —— ICU4X 全家:`icu_collator`/`icu_collections`/`icu_locale*`/`icu_normalizer*`/`icu_properties*`/`icu_provider*`/`icu_segmenter*`、`yoke`、`zerovec`、`zerofrom`、`zerotrie`、`tinystr`、`litemap`、`writeable`、`potential_utf` |
| 19 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT(`rustix` ×2、`linux-raw-sys` ×2、`wasi`/`wasip2`/`wasip3`、`wit-*`、`wasmparser` ×2、`wasm-encoder`、`wasm-metadata`、`wit-component`、`wit-parser`) |
| 7 | MIT OR Apache-2.0 OR Zlib(`glyphon`、`raw-window-handle`、`cursor-icon`、`xkeysym`、`tinyvec_macros`、`zune-core`、`zune-jpeg`) |
| 9 | Unlicense OR MIT / Unlicense/MIT(`memchr`、`aho-corasick`、`byteorder`、`byteorder-lite`、`winapi-util`、`csv`、`csv-core`、`same-file`、`walkdir`) |
| 4 | **Zlib 单许可**(`foldhash` ×2、`slotmap`、`zlib-rs`) |
| 3 | Zlib OR Apache-2.0 OR MIT(`bytemuck`、`bytemuck_derive`、`tinyvec`) |
| 3 | **BSD-2-Clause 单许可**(`arrayref`、`kamadak-exif`、`mutate_once`) |
| 3 | BSD-3-Clause OR Apache-2.0(`moxcms`、`pic-scale`、`pxfm`) |
| **2** | **BSD-3-Clause 单许可**(`tiny-skia`、`tiny-skia-path`) |
| 2 | BSD-3-Clause OR MIT OR Apache-2.0(`num_enum`、`num_enum_derive`) |
| 2 | BSD-2-Clause OR Apache-2.0 OR MIT(`zerocopy`、`zerocopy-derive`) |
| 2 | MIT OR Apache-2.0 OR LGPL-2.1-or-later(`r-efi` ×2 —— **取 MIT/Apache 一侧**) |
| **1** | **MPL-2.0 —— `option-ext 0.2.0`**(经 `dirs-sys` → `dirs` 进来) |
| 1 | **ISC 单许可**(`libloading 0.8.9`) |
| 1 | Apache-2.0 WITH LLVM-exception 单许可(`ar_archive_writer 0.5.2`) |
| 1 | Apache-2.0 OR GPL-2.0-only(`self_cell 1.3.0` —— **必须显式选 Apache-2.0 一侧**) |
| 1 | Apache-2.0 OR BSL-1.0(`ryu`) |
| 1 | Apache-2.0 AND MIT(`dpi 0.1.2` —— 是 **AND**,两份都要给) |
| 1 | (MIT OR Apache-2.0) AND Unicode-3.0(`unicode-ident`) |
| 1 | 0BSD OR CC0-1.0(`roman-numerals-rs`) |
| 1 | 0BSD OR MIT OR Apache-2.0(`adler2`) |
| 1 | MIT OR Apache-2.0 OR CC0-1.0(`fast-srgb8`) |
| 1 | BSD-2-Clause OR Apache-2.0(`serial2`) |
| 1 | Apache-2.0 / MIT(`fnv`) |
| 1 | MIT OR Zlib OR Apache-2.0(`miniz_oxide`) |

**没有 GPL / AGPL / LGPL 强制项。** 唯一需要单独处理的是 **`option-ext` 的 MPL-2.0**:
文件级 copyleft,链进二进制不传染,但**必须提供该包被覆盖文件的源码获取途径**(指向 crates.io / 上游仓库即可)。

**七类已触发的义务:**

| # | 义务 | 证据 |
|---|---|---|
| ① | **`vendor/alacritty_terminal` 是 Apache-2.0 且被改过** —— §4(b) 要求在被改文件里显著声明改动 | `vendor/alacritty_terminal/Cargo.toml:29` `license = "Apache-2.0"`。`git log --oneline -- vendor/alacritty_terminal` 至少 16 个 commit,含 grapheme clustering、OSC 10/11/12/4 回读、DEC 2031、resize 三大块。目录里**只有 `LICENSE-APACHE`,没有 NOTICE,也没有任何改动说明** |
| ② | **嵌入字体 —— `assets/fonts/NotoColorEmoji_WindowsCompatible.ttf`,10.7 MB,OFL-1.1** | `crates/bt-render/src/lib.rs:350` `include_bytes!`。**实测字体 name 表**:family=`Noto Color Emoji`、copyright=`Copyright 2022 Google Inc.`、version=`2.051;GOOG;noto-emoji:20250818:e92753bfa55fd449e427d4d325f9c8c40408c74e`、license=OFL-1.1、CBDT/CBLC 位图。`assets/fonts/NotoColorEmoji-LICENSE:1` = `Copyright 2013 Google LLC`,**未声明 Reserved Font Name** → 磁盘上改名不受 OFL §3 约束;但 §1 要求版权与许可随字体分发 —— **字体嵌进了二进制,zip 里就得有 NOTICE** |
| ③ | **hayro 的 14 款标准 PDF 字体 —— PDFium/Foxit,BSD-3-Clause** | `Cargo.toml` 的 `hayro = { version = "=0.7.1", …, features = ["embed-fonts"] }`;实物 `~/.cargo/registry/src/…/hayro-interpret-0.7.0/assets/Foxit*.pfb` 共 14 个,许可原文 `assets/LICENSE_FOXIT` 首行 `Copyright 2014 PDFium Authors`,**含"Neither the name of Google Inc. … endorse or promote"第三条** |
| ④ | **typst 内嵌字体 —— OFL + GUST + Bitstream Vera + PDFium + CC0 五种** | `Cargo.toml` 的 `typst-as-lib = { …, features = ["typst-kit-fonts", "typst-kit-embed-fonts"] }`。`typst-assets-0.15.1/NOTICE` 分节:`:4` OFL-1.1 → Libertinus Serif(**有 Reserved Font Name**:"Linux Libertine"/"Biolinum"/"STIX Fonts");`:99` **GUST Font License v1.0** → NewComputerModern;`:136` **Bitstream Vera** → DejaVu Sans Mono;`:329` CC0 → ICC profiles;`:864` PDFium BSD → Foxit;`:899` NewCM10-Regular 的 Distribution Exception。包本身 `Cargo.toml:28` `license = "Apache-2.0"` |
| ⑤ | **一枚逐字复制的 Google Material 图标** | `crates/bt-app/src/marks.rs:2842` 是 `SYMBOL_VIEW_BOX`(63 个条目)里**唯一**的 `"0 0 24 24"`,其余是 `0 0 16 16` / `0 0 10 10`;对应 `SYMBOL_BODY[0]`(`marks.rs:2977-2978`)`// #i-gear`,path 数据 `M19.14 12.94c.04-.3.06-.61.06-.94…` 与 `design/ui-mockup.html:4505` 逐字相同 —— 这是 Material Design `settings` 24dp 的规范路径。**全仓 `viewBox="0 0 24 24"` 只此一处**(`grep -oE '<symbol id="[^"]+" viewBox="0 0 24 24"' design/ui-mockup.html` 单条),其余图标是本项目按 Fluent 2 惯例自绘(`marks.rs:1-3` 自述来源为 `design/ui-mockup.html`)。Material Icons 是 **Apache-2.0**,要许可副本 + 变更声明 |
| ⑥ | **十份配色方案的上游 MIT** | `assets/schemes/README.md:36-68` **已把出处与许可写得很干净**(microsoft/terminal MIT、mbadolato/iTerm2-Color-Schemes MIT,并逐一点名 Son A. Pham / Ethan Schoonover / Pavel Pertsev / Zeno Rocha / Sven Greb)。**缺的只是许可全文** —— MIT 要求副本随分发 |
| ⑦ | **PSReadLine 二进制被嵌入并安装到用户 Documents** | `crates/bt-app/src/psreadline.rs:131-160` 的 `BUNDLED_FILES` 九个文件含两个 `.dll` + 两个 Polyfiller;装到 `Documents\WindowsPowerShell\Modules\PSReadLine`(`psreadline.rs:103`)。**这一条已经做对了** —— `License.txt` 在数组里,注释(`:122-127`)明写"不是可选项,PSReadLine 是 BSD-2,二进制分发必须携带声明"。实测 `assets/psreadline/2.4.6/License.txt` = **BSD-2-Clause,Copyright (c) 2013, Jason Shirk**。只需在总 NOTICE 里再列一次 |

**建议做法**:仓库根一份 `THIRD-PARTY-NOTICES.md`,zip 里带同一份。
**用 `cargo about` 或 `cargo deny check licenses` 生成而不是手抄**,并做成 CI 的一道门 ——
上面这张表的准确性来自 554 个包的实测,手抄会在下一次 `cargo update` 后失真。
`vendor/alacritty_terminal/` 里另加 `CHANGES-FOLIO.md` 列改动块(Apache-2.0 §4(b))。
`option-ext` / Material gear / alacritty 改动声明这三条需要手写。

**工作量**:中。

### M4 · exe 静态导入 `VCRUNTIME140.dll` —— 干净机器上双击直接报缺 DLL

| | |
|---|---|
| **证据** | 解析 `dist/folio-next11.exe`(77.3 MB)的 PE 导入表:导入的 DLL 含 **`VCRUNTIME140.dll`、`VCRUNTIME140_1.dll`**;其余是 `api-ms-win-crt-*`(UCRT,Windows 10+ 系统自带)、`d3d11`/`dxgi`/`dcomp`/`dwrite`/`uxtheme`/`imm32`/`combase` 等系统件。**延迟导入表为空。** 全仓 `grep -rn "profile.release" Cargo.toml crates/*/Cargo.toml` **无输出** → 用的是 Rust MSVC 默认的动态 CRT |
| **后果** | `VCRUNTIME140.dll` 属于 **VC++ Redistributable**,不是 Windows 组件。没装 VS / 没装 redist 的机器上双击 = "无法启动此程序,因为计算机中丢失 VCRUNTIME140.dll"。这是**便携 zip 最典型的首次体验事故** |
| **建议** | 首选 `-C target-feature=+crt-static`(仓库级 `.cargo/config.toml` 给 `x86_64-pc-windows-msvc` 加 `rustflags`)。本树**没有 C/C++ 依赖**(syntect 走 `regex-fancy` 不走 onig、`zlib-rs` 纯 Rust、hayro `#![forbid(unsafe_code)]`),静态 CRT 不会撞 C 库单例问题。次选:README 写清 + zip 带 redist 链接。**必须在一台没装 redist 的干净 VM 上实测** |
| **工作量** | 小(改一行 + 一次干净机验证) |

### M5 · 便携 zip 的内容没有定,而少一个文件就是静默的功能回归

| | |
|---|---|
| **证据** | `vendor/conpty/portable-pty/src/win/psuedocon.rs:165-172`:加载器**要求 `conpty.dll` 与 `OpenConsole.exe` 成对出现在同一目录**,注释写明"支持的选择只有两种:打包的这一对,或操作系统的实现"。`vendor/conpty/README.md` 记了 pin 的理由(上游 `microsoft/terminal#18725`)、包 SHA-256、以及 x64 两个条目的 SHA-256,并明确写着"提取出的二进制是 `target/` 下的构建产物,**不得签入**"。`crates/bt-pty/build.rs:24-54` 把它们释放到 profile 输出目录与 `deps/`,并按 native `.targets` 布局在每份输出下再镜像一份 `x64/OpenConsole.exe` |
| **后果** | zip 里只放 `folio.exe` → 加载器找不到成对文件 → **静默退回 inbox ConPTY** → `docs/M1.8-resize-visual-stability.md` 记录的整套 resize 稳定性回归。**不报错,只变差** |
| **zip 装什么(逐项结论)** | ✅ `folio.exe` ・ ✅ `conpty.dll` + `OpenConsole.exe` + `x64\OpenConsole.exe`(MIT / Microsoft;`vendor/conpty/README.md` 已记 Authenticode 签名与哈希)・ ✅ `README.txt`、`LICENSE-MIT`、`LICENSE-APACHE`、`THIRD-PARTY-NOTICES.md` ・ ❌ **`folio.ps1` / `folio.bash` 不放** —— 它们 `include_str!` 进了 exe(`crates/bt-app/src/shell_integration.rs:48`、`:569`),按下 Add 时写到 `%APPDATA%\Folio\shell-integration\`(`shell_integration.rs:139`)・ ❌ pdfium 已否决(`docs/DESIGN.md:2768`,走 hayro 编译进 exe)・ ❌ WebView2 Loader 不用带(导入表里**没有** `WebView2Loader.dll`,是静态链接)・ ❌ MF / WebView2 运行时不随包 |
| **建议** | 把 zip 清单写进 release 脚本并加一道"清单对得上"的检查;产物同时发 `SHA256SUMS.txt` |
| **工作量** | 小 |

### M6 · 版本号是 `0.0.0`,`--version` 会被拒绝,产物无法被指名

| | |
|---|---|
| **证据** | `Cargo.toml:21` `version = "0.0.0"`;`crates/bt-app/Cargo.toml:3` `version.workspace = true`。`CARGO_PKG_VERSION` 全仓**唯一生产消费者**是 `crates/bt-pty/src/lib.rs:98` `const TERM_PROGRAM_VERSION` → **每个被 Folio 拉起的 shell 都看到 `TERM_PROGRAM_VERSION=0.0.0`**。`crates/bt-app/src/cli.rs` 的参数表里**没有 `--version` / `-V` 分支**(`grep -n '"--version"\|"-V"' crates/bt-app/src/cli.rs` 无命中),`--version` 会落到 `cli.rs:247` 的 `CliFault::UnknownFlag` 被拒绝。`main.rs:174` 的 `WINDOW_TITLE = "Folio M0-beta"` 是**测试专用常量**,真窗口标题是焦点 tab 名。**没有 About 界面**;`crates/bt-app/` 下**没有 `build.rs`**;全仓**没有 `.ico`、没有 `.rc`、没有 `winres`/`winresource`/`embed-resource` 依赖**(`git ls-files \| grep -iE "\.ico$\|\.rc$"` 无输出)→ 资源管理器属性页的产品/版本/公司全空,任务栏与 Alt-Tab 是 Windows 默认图标 |
| **后果** | 用户报 bug 说不清是哪个 build;而**三份诊断文件也都不打版本戳**(panic 日志 `main.rs:82181-82197`、`diagnostics.log`、hang 报告),等于收到的每一份现场都缺最关键的一行 |
| **建议** | ① `version = "0.1.0"`;② 加 `--version` 分支,打 `folio 0.1.0 (<短 sha>)`;③ **三份诊断文件头行加同一个版本戳**(这一条比 ① 更值钱);④ 加 `.ico` + VERSIONINFO(一个 `build.rs` + `winresource`)。④ 可降到"该做" |
| **工作量** | 中 |

### M7 · CI 从来没跑过,而且那道"禁止 ignored 测试"的门是空的

| | |
|---|---|
| **证据 A(从没跑过)** | `git remote -v` **无输出**;`gh run list` 返回 `failed to determine base repo: no git remotes found`。`.github/workflows/ci.yml` 存在且写得很细(adapter 边界门 + 自证、lint 门 + 自证、vendor 成员检查、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`),**但从未在 GitHub Actions 上执行过一次** |
| **证据 B(门是空的)** | `.github/workflows/ci.yml:122-123`:`$out = cargo test --workspace --locked -- --ignored --list 2>&1 \| Out-String` / `if ($out -notmatch '0 tests, 0 benchmarks') { $out; throw "ignored tests are not allowed" }`。`--workspace` 会跑十几个测试二进制,`Out-String` 把它们的输出拼成**一个**字符串;只要**任何一个** crate 没有 ignored 测试(它就打印 `0 tests, 0 benchmarks`),整串就 match,门**永远不 throw**。而树里**真有 23 个 `#[ignore]`**:`crates/bt-pty/src/lib.rs` 19 个、`crates/bt-app/src/marks.rs` 1、`crates/bt-term/src/session.rs` 1、`crates/bt-transcript/src/paths.rs` 2 —— 全在普通 `#[cfg(test)] mod tests` 里(`crates/bt-pty/src/lib.rs:1284-1285`),**没有 feature gate**(`crates/bt-pty/Cargo.toml` 无 `[features]`)。**这道门恰好违反了它上面第 90-101 行自己立的规矩**:"一道不会失败的门等于一条不会失败的断言 —— 先证明门会响,再信绿灯" |
| **本机专属项已核实不在仓里** | `~/.cargo/config.toml` 的 `jobs = 19` 在**用户主目录**,仓库里**没有 `.cargo/` 目录** → 不影响 CI。`rust-toolchain.toml` 钉 `1.94.1-x86_64-pc-windows-msvc`(含 host triple),CI 用 `toolchain: none` 让它作主 —— 这一段设计是对的 |
| **runner 上跑得起来吗** | **`[未验证]`** —— 需要实测,不要靠推断。三个具体疑点:① `crates/bt-render/src/lib.rs` 有 `request_adapter` 的测试,GitHub 托管 runner 无独立 GPU,能否落到 WARP 软件适配器要实测;② `crates/bt-app/src/{main,webhost}.rs` 与 `crates/bt-platform/src/webview.rs` 走 WebView2,runner 上是否预装 Runtime 要实测;③ ConPTY sidecar 由 `build.rs` 从签入的 nupkg 解出,vendor 目录在仓里,这一条应当没问题 |
| **建议** | ① 建 remote,**先让 CI 真跑一次并变绿** —— 这是其他一切判断的地基;② 修那道门(按二进制逐行解析,或 `--format json`,或改成白名单断言"恰好 23 个且名单对得上");③ 先加一个最小 smoke job 实测 WARP 与 WebView2,**再**决定哪些测试在 CI 上跑、哪些标成本地探针 |
| **工作量** | 中 |

### M8 · 隐私与信任面没有对外说法,而这个产品会写用户级配置文件

这是**公开后最容易被质疑的一面**,而代码本身其实比大多数同类干净 —— 问题在于**没有一句对外的话**。
下面每条都验证过,建议原样变成 README 的 "Privacy & what Folio writes" 一节。

| 项 | 实测事实 | 要说的话 |
|---|---|---|
| **配置目录** | `crates/bt-app/src/persist.rs:775-800` `storage_dir()` = `%APPDATA%\Folio`(`persist.rs:761` `STORAGE_NAME`),`APPDATA` 缺失时退到 `temp_dir()`。旧名 `BetterTerminal`(`persist.rs:758`)会被自动搬家,失败时**保守地继续用旧目录**而不是开一个空的新目录 | 列出目录与卸载方法 |
| **目录里到底有什么(实测本机)** | `settings.json`、`session.json`、`session.lock`、`pins.json`、`profiles.json`、`schemes/`、`shell-integration/`、`diagnostics.log`,**外加一整套 WebView2 用户数据**:`Cache/`、`Code Cache/`、`GPUCache/`、`Local Storage/`、`Session Storage/`、`Network/`、`blob_storage/`、`DIPS`、`Preferences`、`Local State`、`Shared Dictionary`、`DawnGraphiteCache/`、`DawnWebGPUCache/` | **这一条必须写**:网页预览的 cookie / localStorage / 缓存和你的设置在同一个文件夹。"清预览缓存"和"删设置"目前是同一个动作 |
| **session.json 明文 URL** | `crates/bt-persist/src/session.rs:26-34` **代码自己已经把这条写下来了**:"A page's URL is written verbatim — scheme, host, port, path, **query and fragment** — into this file and into `pins.json`, in the clear … **a URL carrying a session token is stored in the clear**"。实测本机 `session.json` schema 11、13 个 `cwd` 字段,cwd 也是明文 | 把代码里这段话翻成英文放进 README。用户已知情接受,但**公开后是新用户在读** |
| **diagnostics.log** | `docs/DESIGN.md:211`:`diagnostics::enter_resident_run` 在进程决定常驻的那一刻把 `stdout`/`stderr` 搬到 `%APPDATA%\Folio\diagnostics.log`。实测本机 56 行 / 8250 字节,内容是 `BT_DPI stage=… winit_scale=2 …` 一类,**本次抽样零条用户路径**。**没有看到轮转/上限** | 说明它存在、会长、可以随时删 |
| **hang 报告** | `docs/DESIGN.md:125`:看门狗把**站点、静默时长、裸栈、计数器**写到 `%APPDATA%\Folio\hang-reports\<UTC>.txt` | 用户会把它贴到 issue 里 —— **README 要提醒他们先看一眼** |
| **panic 日志** | `crates/bt-app/src/main.rs:82181-82197` `install_panic_log_hook`:写 `unix_ms`、线程名、panic 信息、`Backtrace::force_capture()`,**append** 到 `std::env::temp_dir().join(PANIC_LOG_FILENAME)`,而 `main.rs:259` `PANIC_LOG_FILENAME = "bt-app-panic.log"` —— **文件名还叫旧代号**。且 `main.rs:17` 是 `#![windows_subsystem = "windows"]`,`previous(info)` 的默认输出落进已被重定向的 stderr → **双击启动崩溃时用户看到的是窗口凭空消失,没有任何提示** | ① 改名 `folio-panic.log`;② 崩溃时至少弹一个说明路径的 message box(`bt_platform::message_box` 已有,`main.rs:report_at_the_front_door` 就在用) |
| **三个 hooks 安装器** | 三个模块的头部注释**已经把边界说得很清楚**:`attention_hooks.rs:1-15` 只写用户自己的 `~/.claude/settings.json`(`:50`/`:53`),**并明确拒绝写进仓库里的 `.claude/`**,理由引的是上游自己的安全提示;路径**问系统要而不是拼 `%USERPROFILE%`**(`:15` 注释 + `:81`)。`attention_codex.rs` 只碰 `~/.codex/config.toml` 的 `notify` 键(`:69`/`:72`/`:75`),**用户已有 notify 就拒绝动**(`:258` `"codex already runs a notify program of your own"`),且用 `toml_edit` 保留注释与排版。`attention_copilot.rs` 写 `%USERPROFILE%\.copilot\hooks\`(`:86`/`:89`),尊重 `COPILOT_HOME`(`:13-15`)。三者都只**写配置**,不执行任何东西 | 这是加分项,**要主动写出来**:写哪三个文件、什么时候写、怎么撤。别让人自己去读代码 |
| **注意力管道** | `crates/bt-platform/src/attention_pipe.rs:257-258` `D:P(A;;GA;;;<logon sid>)`;`:212-217` 端点名里含 logon SID 的**摘要**而非 SID 本身,加进程 ID 与不可猜位(`:234-243`);`:390-395` 拿不到 logon SID 就**不开端点**。`:7` 注释点名了它在防什么:默认 DACL 会让 Everyone(含匿名登录)连上 | 一句话:"只有当前登录会话能连,连不上就不开" |
| **无遥测** | 见第 0 节的三组零命中 | **明确写一句 "Folio makes no network requests of its own."** 并补一句例外:网页预览是 WebView2,由用户输入的地址决定;`crates/bt-app/src/webhost.rs:990-992` 的搜索引擎 URL 是地址栏输入时才组装的 |

**工作量**:中(全是写字,不改代码;panic 文件改名 + message box 是小改)。

### M9 · 名字、署名与提交历史 —— 公开前必须当场定的三件

| 项 | 证据 | 建议 |
|---|---|---|
| **商标注意项** | 产品定名记录(memory `product-name-folio`,2026-08-13 拍板):品牌 **Folio**、仓库/搜索名 **folio-terminal**、二进制 `folio.exe`。尽调结论:终端品类零重名;**crates.io 的 `folio` 名被一个已 yank 的静态站生成器占着** → 发布用 `folio-terminal`;**⚠ FOLIO Technologies, Inc. 2022 年有 "FOLIO" 可下载软件类美国商标申请,serial 97445151** —— 开源无碍,**商业化前必须请律师正式检索,或用 "Folio Terminal" 组合形态** | 仓库名 `folio-terminal`;README 首句用 **"Folio Terminal"** 全称,正文再简称 Folio;先不注册 crates.io |
| **提交历史里的两样东西** | ① `git log --format='%b' \| grep -c "claude.ai/code/session"` = **570** —— 570 个 commit 带 `Claude-Session: https://claude.ai/code/session_…` trailer;`Co-Authored-By:` 1044 条;61 个 commit 是 `codex:` 前缀。② `d8dad84` 的**主题行**把 `Co-Authored-By` 和 `Claude-Session` URL 直接拼进了标题(`git log --format='%h %s' \| grep "Claude-Session"` 单条命中)。③ 全部 1034 个 commit 的作者与提交者邮箱是 **`weiyishi@umich.edu`**(不是 `weiyishi024@gmail.com`)—— 公开后这个学校邮箱**会永久可见** | 三选一,**必须选**:(a) 原样公开(最省事,接受邮箱与 session 链接可见);(b) 保留历史但先 `git filter-repo` 换邮箱 / 删 `Claude-Session` trailer(**1034 个 SHA 全变**,只能在建 remote **之前**做);(c) 新仓从一个 squash 的初始 commit 开始,旧历史留私仓。**注意 (b)(c) 只有现在能做** —— 一旦 push 就晚了。`Claude-Session` URL 对外人不可解析,不是泄密,但会暴露工作流;`Co-Authored-By` 按既有裁决保留 |
| **`test.md` 是键盘乱码,而且被跟踪着** | `git ls-files test.md` 命中,内容一行 `sdsfjhgkjdbfvgmnsdbvfmnabsvasd` | 删 |

**顺带核实过、结论是"干净"的**(不用动):`git ls-files dist` **零输出**、`git ls-files .claude` **零输出**、
`git ls-files .agents` 零输出、`target*/` 与 `local-images/` 与 `.playwright-mcp/` 都在 `.gitignore` 里、
`git status --porcelain` **全空**;
`git log -S` 搜 `sk-ant` / `AKIA` / `api_key` / `BEGIN RSA PRIVATE KEY` / `password=` **全零命中**
(`ghp_` 的唯一命中是 `vendor/conpty/*.nupkg` 这个二进制包的误报);
跟踪文件里**没有任何邮箱地址**(`git grep -nIE "[a-z0-9._%+-]+@[a-z0-9.-]+\.(com|org|net|io|dev)"` 排除 example.com 后零命中)。

**工作量**:小(决策成本 > 执行成本;若选 (b)/(c) 则为中)。

---

## 2. 该做(发布质量,不做也能上,但会立刻被问)

### S1 · 未签名 exe 的 SmartScreen 说明与签名路线

- **本地事实(已验证)**:`dist/` 下所有 exe **均未签名**(仓库里没有任何签名步骤、`.github/workflows/ci.yml` 无签名 job、没有 `signtool` 调用)。
- **外部行情(SignPath OSS 计划、Azure Trusted Signing 的资格与价格、Certum、SmartScreen 的确切文案与 EV 是否仍即时放行)**:**`[未盘]`** —— 见第 6 节。
- **建议**:v0.1 preview 可以**不签**,但 README 必须写清 SmartScreen 的过法(更多信息 → 仍要运行),并在 Releases 里给 `SHA256SUMS.txt` 让人自行校验。签名路线单独立一单,先把上面那几家的官方要求核实清楚再决策。
- **工作量**:小(说明);签名本身另计。

### S2 · 没有 CHANGELOG、没有 release 命名约定、没有社区文件

`git ls-files .github` **只有 `.github/workflows/ci.yml` 一个文件** —— 没有 issue 模板、没有 `SECURITY.md`、
没有 `CONTRIBUTING.md`、没有 `CODE_OF_CONDUCT.md`、没有 dependabot 配置。根目录也没有 `CHANGELOG.md`。
**建议**:`CHANGELOG.md`(v0.1.0 一节即可)、产物命名 `folio-0.1.0-windows-x64.zip`、
`SECURITY.md`(写清 hooks 与管道的边界,把 M8 的内容指过去)、一个 bug 报告模板(**要求贴版本号和诊断文件路径**)。
**工作量**:小。

### S3 · `[profile.release]` 缺失,exe 77.3 MB

`grep -rn "profile.release" Cargo.toml crates/*/Cargo.toml` 无输出 → 默认 release(opt-level 3、无 LTO、codegen-units 16、不 strip)。
体积大头已知:`assets/fonts/NotoColorEmoji_WindowsCompatible.ttf` **10.7 MB**、typst 内嵌字体、hayro 的 300 KB Foxit 字体、
syntect + two-face 语法集、PSReadLine 437 KB。
**建议**:加 `lto = "thin"`、`codegen-units = 1`、`strip = "symbols"`。
**不要加 `panic = "abort"`** —— `main.rs:82181` 的 panic 钩子与栈回溯是现有诊断手段。
**工作量**:小(改几行 + 一次构建对比)。

### S4 · 快捷键表可以自动生成,现在只有 UI 里有

`crates/bt-app/src/i18n.rs:6573` 已经在 `for binding in crate::shortcuts::BINDINGS` 遍历这张表给 Shortcuts 页供词,
`main.rs:82554` 也在遍历它。**表是现成的,生成器是缺的。**
**建议**:加一个 `#[test]` 把 `BINDINGS` 渲染成 markdown 写进 `docs/shortcuts.md`,并断言与签入的文件一致(和 CI 现有的"门要能响"风格一致)。
i18n 是双语的(`i18n.rs:107-121` `Language::{English, Chinese}`,`:154-157` `LanguageMode::{System, English, Chinese}`),
所以生成两份也几乎不多花。
**工作量**:小。

### S5 · 已知问题列表要从内部文档里摘出来

`docs/HANDOFF-2026-08-21.md:504` 的 §6「如实挂着的缺口」已经是一份**很诚实**的清单,但它是中文内部文档且混着裁决过程。
里面对外有意义的至少包括:未装 PowerShell 整合时鼠标残留修复不生效;X10 抬起残留;
"等你回答"只有 BELL 那一半、没有命令面板、没有标题栏 waiting 药丸;
横滚没有 Line wrapping 设置、CSI 14t 被丢弃;
聚焦卡片列数与字体字面相关(默认 Consolas 59/28 列,换 Cascadia Mono 同宽只有 26 列);
公式线 `\hbar` 一类 live 渲染失败。
**另外**:任务里提到的"跨 DPI 上半黑未复现" —— `HANDOFF §6` 开头那条**已结案并撤销了 DPI 嫌疑**
(`BT_MOUSE_TRACE` 日志全程 `scale=2`,真因是 alt 屏闸门 ③,已推翻)。**别把已撤销的嫌疑写进已知问题。**
**建议**:README 里一节英文 Known issues,只留用户能撞见的。
**工作量**:小。

### S6 · 14 个 crate 都没有 `publish = false`

`grep -rn "publish" Cargo.toml crates/*/Cargo.toml` **无输出**,`[workspace.package]`(`Cargo.toml:20-24`)
也**没有 `description` / `repository` / `authors` / `homepage` / `readme` / `keywords`。
**建议**:给所有 `bt-*` 加 `publish = false`(它们不是给别人用的库),`Cargo.toml` 补 `description` 与 `repository`。
**工作量**:小。

### S7 · 本地有 11 个 `worktree-agent-*` 分支,建 remote 后一推就全出去

`git branch` 共 **13 个**:`main`、`probe/ime-trace`,加 **11 个 `worktree-agent-<hash>`**;
`git worktree list` 显示 4 个仍锁在 `.claude/worktrees/` 下,另有 `D:/Developer/bt-wt-build`、`D:/Developer/bt-wt-ime` 两个外部 worktree。
**建议**:第一次 push 用 `git push origin main`(**不要 `--all` / `--mirror`**);清理或改名后再推别的。
`.gitignore:24` 只忽略了 `.claude/worktrees/`,**没忽略 `.claude/` 本身** —— 建议改成整个 `.claude/`,
免得日后一个 `settings.local.json` 冒出来。
**工作量**:小。

### S8 · WebView2 硬化:三项关死了,两项开着

| 设置 | 现状 | 位置 |
|---|---|---|
| `IsWebMessageEnabled` | **false** ✅ | `crates/bt-platform/src/webview.rs:1078` |
| `AreHostObjectsAllowed` | **false** ✅ | `webview.rs:1089` |
| `AreDefaultContextMenusEnabled` | **false** ✅ | `webview.rs:1105` |
| `IsStatusBarEnabled` | false ✅ | `webview.rs:1092` |
| **`AreDevToolsEnabled`** | **true** ⚠ | `webview.rs:1102`,注释(`:1094-1100`)说明是**故意的**:预览头有 Developer tools 动词、`F12` 是快捷键表一行,关掉这两个就都没用了;窗口通过 `webhost::claimable_chords` 占住 `F12`,引擎自己的加速键到不了页面 |
| **`IsScriptEnabled`** | **未设置 → 默认 true** ⚠ | 全仓 `grep -rn "IsScriptEnabled"` **零命中** |
| 导航拦截 | **顶层有**:`webview.rs:1147-1180` `add_NavigationStarting`,`:676` 同步问 `gate`、`:619` 说明 `SetCancel` 不能事后决定 | —— |
| **子框架 / 子资源拦截** | **没有** ⚠ | `grep -rn "FrameNavigationStarting\|WebResourceRequested"` **零命中** |

**判断**:这三条**不是必须**,因为 host object 与 web message 两扇门都关死了,页面回不到宿主进程;
但**要写进 README**,让人知道预览的是一个**能跑脚本的真浏览器**,而不是一个静态渲染器。
**建议**:发布说明里写清"HTML/网页预览由 WebView2 承载,脚本会执行;host 对象与消息通道已关闭"。
**工作量**:小(写字);真加 `FrameNavigationStarting` 是中。

### S9 · `BT_*` 调试开关:只有一个在 release 里被编译掉

来自安全面审计(该子代理已自行更正过前一版的数字,以下是更正后的):

- **`cfg!(debug_assertions)`(运行期判断、代码仍编进去)全工作区零使用。**
- **`BT_HANG_SELFTEST` 是唯一被 `#[cfg(debug_assertions)]` 切开的**:release 侧在
  `crates/bt-app/src/hang_watch.rs:1112` 是一个空 stub。**其余 `BT_*` 全部在 release 里活着。**
- **四个"能拿到内容"的开关活在 release 里**:
  **`BT_PTY_DUMP`**(`crates/bt-pty/src/lib.rs:81,162` —— **每个 pane 的完整原始 PTY 字节流,等同键盘记录器级别**)、
  `BT_IME_TRACE`(`main.rs:71704`)、`BT_CHROME_DUMP`(`main.rs:20024,20139`)、
  `BT_SEMANTIC_TRACE`(`bt-term/src/session.rs:1241`);
  另有 `BT_WEB_DEV`(`webhost.rs:429`)与两个安装器重定向沙箱
  `BT_POWERSHELL_PROFILE`(`shell_integration.rs:665`)、`BT_PSREADLINE_DOCUMENTS`(`psreadline.rs:834`)。
- **一条形状匹配**:`crates/bt-app/src/diagnostics.rs:148-153`(判定在 `:151`)遍历**整个环境**,
  按 `name.starts_with("BT_") && name.contains("TRACE")` 的**形状**决定 `folio.exe` 是否保留继承来的控制台 ——
  任何叫 `BT_*TRACE*` 的名字都满足,包括不在任何表里的。`BT_PTY_DUMP` 与 `BT_HANG_SELFTEST` 因不含 `TRACE` 而被排除,
  这一点由 `:271-297` 的测试钉住。
- **`BT_CONPTY_FORCE_SYSTEM` 的生产读点在 vendor 补丁里**:`vendor/conpty/portable-pty/src/win/psuedocon.rs:168`(注释 `:165`)。

**判断**:这些都**不是提权路径**(每一个都需要先控制进程环境),所以不阻塞发布。
但 **`BT_PTY_DUMP` 必须进公开文档**:一个能把终端全部输入输出落盘的环境变量,用户有权知道它存在。
**建议**:README 或 `docs/diagnostics.md` 一节列出这几个开关及其代价。
**工作量**:小。

### S10 · 降级(读到未来版本)行为要写进文档

`crates/bt-persist/src/migrate.rs:6-9` 与 `:990-993` 说明得很清楚:
"missing → silent default;unparseable / **future-version** → default + explicit reason";
`read_with_fallback` 在**尝试解析版本信封以外的任何东西之前**就检查版本,整份退回默认值,**从不 panic**。
`:14-18` 另说明未识别字段靠 serde 默认语义静默丢弃(**没有任何类型设 `deny_unknown_fields`**)。
**风险**:装了 v0.2 又退回 v0.1 的用户,配置会**整份**变默认;而"未识别字段静默丢弃"意味着退回后再存盘,
v0.2 写的新键就没了。
**建议**:README 的升级一节写一句"降级会重置配置,请先备份 `%APPDATA%\Folio`";
或让 future-version 的那次回退**不覆盖原文件**(需读代码确认现状,本次未查到写回时机 —— **`[未验证]`**)。
**工作量**:小(文档)/ 中(改行为)。

### S11 · 中文内部文档的公开范围

`docs/` 295 份、`spikes/` 74 份、`design/` 28 份全部签入,其中 **235 份含中文**。
内容包括:`docs/PROBLEM-LIST.md`(含逐家竞品评估与产品策略)、`docs/HANDOFF-*.md`(交接与待裁决清单)、
`docs/prompts/` **52 份 `codex-*.md` 任务单**、`docs/reviews/` 23 份、`docs/plans/` 97 份(含大量证据 jsonl / 截图)。
**扫出来的机器痕迹**:`git grep -c "Weiyi"` 全仓 **341 处 / 39 个文件**(其中 `crates/` 里 8 个文件是**测试夹具字符串**,
如 `crates/bt-app/src/profiles.rs:12228` 的 `(r"C:\Users\Weiyi", "/mnt/c/Users/Weiyi")` —— 那是 WSL 路径映射测试,
**换成 `C:\Users\dev` 只是改夹具,不影响正确性**);
`git grep -l AppData` **30 个文件**,其中 `crates/bt-transcript/src/paths.rs:1740-1746` 与
`crates/bt-viewport/src/lib.rs:7161-7232` 用的是 **Claude scratchpad 的完整路径**做长路径截断夹具。
**建议**:
- **公开 `docs/DESIGN.md`、`CONVENTIONS.md`、`docs/shell-integration.md`、`assets/schemes/README.md`、`vendor/conpty/README.md`** —— 这几份是真正的技术资产,透明度收益大于噪音。
- **`docs/prompts/` 建议不公开**(52 份 AI 任务单,与产品无关且会主导读者印象);`docs/plans/*/evidence*` 那些 jsonl / 截图可以留在私仓。
- **`crates/` 里的 `Weiyi` 夹具建议统一换成 `dev`** —— 这是**唯一一处会随源码永久流传的个人痕迹**,而且改起来零风险。
- 中文文档本身**不必翻译**,但 README 要用英文,并说明"设计文档为中文"。
**工作量**:中。

### S12 · `spikes/` 里有一份第三方 JS

`spikes/03-math-engine/assets/katex.min.js` 签入,`KATEX-LICENSE`(MIT)与 `MITEX-LICENSE` 同在 —— **义务已尽**。
且 KaTeX **没有进产品**:`grep -i katex crates/` 只在 `preview.rs:1910`、`main.rs:3286-3287` 的**注释**里出现,
公式线走的是 typst + mitex。六个 spike 各自声明 `[workspace]`,不参与主构建。
**建议**:保留即可,或整个 `spikes/` 移出公开仓(它们是研究记录,对使用者无价值)。
**工作量**:小。

---

## 3. 可后延

| 项 | 理由 / 证据 | 工作量 |
|---|---|---|
| **winget / scoop 上架** | 需要稳定的下载 URL 与哈希,v0.1 preview 先让人手动下 zip;要求细节 **`[未盘]`** | 中 |
| **代码签名** | 见 S1,外部行情未盘;preview 阶段用 SHA256SUMS + README 说明顶得住 | 中 |
| **GitHub Pages / 官网** | 已裁 README 即首页 | — |
| **双语文档** | i18n 在**产品里**已经是完整双语(`crates/bt-app/src/i18n.rs` 6587 行,`Language::{English, Chinese}` + `LanguageMode::System` 三态);**文档**双语可后延 | 中 |
| **统一 `bt-*` / `BT_*` 命名** | 定名记录明确裁过"**不改** `bt-*` crate 前缀与 `BT_*` 环境变量(代号血统 + 内部兼容)";用户可见面已全是 Folio(`main.rs:171` `APP_NAME = "Folio"`、`crates/bt-platform/src/lib.rs:1230` `NOTIFICATION_AUMID = "Folio.Terminal"`、`crates/bt-app/Cargo.toml:15-17` `[[bin]] name = "folio"`)。**唯一该现在改的是 `PANIC_LOG_FILENAME = "bt-app-panic.log"`**(见 M8),那是用户会看到的文件名 | 小(仅那一处) |
| **`scripts/shell-integration/betterterminal.{ps1,bash}` 旧名垫片** | 仍签入,`docs/shell-integration.md:461` 说明是改名前的旧名。留着无害 | 小 |
| **减小 77 MB** | S3 的 lto+strip 先做;真要小得砍 NotoColorEmoji(10.7 MB)或 typst 字体,那是产品决策不是发布决策 | 大 |
| **`bt-platform` 补 `[lints.rust]`** | 它是唯一允许 `unsafe` 的一方 crate(307 处),现状是 `Cargo.toml:105-108` 只有 `[lints.clippy]`。可以改成显式 `unsafe_code = "allow"` 把"故意的"写下来,但不影响发布 | 小 |
| **`FrameNavigationStarting` / `WebResourceRequested` 拦截** | 见 S8,顶层导航已拦,host 通道已关 | 中 |

---

## 4. 建议发布日的判据(是条件,不是日期)

**只有下面六条同时为真,才定日子。** 前四条是硬门,后两条是"发出去之后不会立刻后悔"。

### 硬门(缺一条就不发)

1. **一台没装 VS、没装 VC++ Redistributable、没装 WebView2 的干净 Windows 10 与 Windows 11 虚机上,
   解压 zip 双击 `folio.exe`,窗口起来、能开 shell、能敲字、能 resize。**
   —— 这一条同时验掉 M4(VCRUNTIME)、M5(ConPTY 成对文件)、和 WebView2 缺席时的降级路径。
   **不是"应该能",是要有截图。**
2. **`folio.exe --version` 打出一个真版本号,且 panic 日志 / `diagnostics.log` / hang 报告三份文件的头行都带同一个版本戳。**
   —— 收得到能用的 bug 报告,是"preview"这个词的最低对价(M6)。
3. **法律面闭合**:根目录有 `LICENSE-MIT` + `LICENSE-APACHE`;
   有一份**生成的**(不是手抄的)`THIRD-PARTY-NOTICES.md`,里面点名了
   `option-ext` 的 MPL-2.0、Material `#i-gear` 的 Apache-2.0、alacritty 的 Apache-2.0 §4(b) 改动声明、
   四组嵌入字体(OFL / GUST / Bitstream Vera / PDFium)、十份配色方案的 MIT、PSReadLine 的 BSD-2;
   `vendor/alacritty_terminal/` 里有改动清单。(M1、M3)
4. **CI 在 GitHub Actions 上真跑过一次并且是绿的**,而且那道 ignored-test 的门**被证明会响**
   ——按仓库自己第 90-101 行立的规矩办:种一个违规、要求红、再放回去。(M7)

### 软门(可以带着上,但发之前要有明确答案)

5. **README 存在且英文,并且已经回答了这五个问题**:装什么前置、SmartScreen 怎么过、
   数据写在哪(含 `%APPDATA%\Folio` 里那套 WebView2 缓存)、`session.json` 里的 URL 与 cwd 是明文、
   三个 hooks 安装器各写哪个用户文件以及怎么撤。加一句 "no network requests of its own"。(M2、M8)
6. **提交历史那三选一已经做出选择并执行完**(原样公开 / filter-repo / squash 新仓),
   `test.md` 已删,第一次 push 用 `git push origin main` 而不是 `--all`。
   —— **这一条只有在建 remote 之前才有得选。**(M9、S7)

### 不构成阻塞的(先说清楚,免得临门被它们绊住)

未签名 → SmartScreen 提示,README 说清即可;
exe 77 MB → 说清即可;
`AreDevToolsEnabled(true)` 与脚本可执行 → 是设计选择,写进说明即可;
23 个 `#[ignore]` 测试 → 它们要真 shell / 真 GPU,承认它们是本地探针即可,**但那道门必须先修好**;
中文文档 → 保留是加分不是减分,只要 README 是英文。

---

## 5. 附:本次实测到的一组"已经做对了"的事(建议写进 README,别浪费)

1. **无网络、无遥测、无更新检查** —— 三组 grep 零命中,`Cargo.lock` 无任何 HTTP 客户端。
2. **`unsafe_code = "deny"` 且真的在执行** —— 12/13 个一方 crate 零 `unsafe`,三处逃生口全部具名。
3. **配色方案的出处与作者逐一列明**(`assets/schemes/README.md:36-68`),连 iTerm2 仓库 LICENSE 里"每个主题版权归其作者"那句都逐字引了。
4. **PSReadLine 的 BSD-2 声明随二进制一起安装**,并且注释写明"这不是可选项"(`psreadline.rs:122-127`)。
5. **ConPTY sidecar 有包哈希、条目哈希与 Authenticode 记录**(`vendor/conpty/README.md`),`build.rs` 会校验后才解包。
6. **hooks 安装器拒绝写进仓库里的 `.claude/`**,理由引的是上游自己的安全提示(`attention_hooks.rs:5-8`)。
7. **管道是 fail-closed 的**(`attention_pipe.rs:390-395`)。
8. **两条迁移链零缺口**,且降级时整份退默认而非部分解析(`migrate.rs:6-9`)。

---

## 6. 覆盖度声明(诚实标注)

原单八个面的实际盘点情况:

| # | 面 | 状态 |
|---|---|---|
| 1 | **仓库清洁** | ✅ **完整盘完**。`git ls-files dist` / `.claude` / `.agents` 三个零输出;`.gitignore` 逐行读过;`Weiyi` 341 处 / `AppData` 30 文件全扫;历史用 `git log -S` 搜过五类密钥;`test-assets` / `corpus` / `design` / `spikes` 逐目录列过;`test.md` 与 11 个 worktree 分支是新发现 |
| 2 | **许可与第三方声明** | ✅ **完整盘完,且比原单要求更细**。554 个外部 crate 的 license 字段**逐包实测**(工具没装,改读注册表缓存);嵌入字体逐个定位并读了 NOTICE;Material `#i-gear` 定位到唯一一处;WebView2 是静态链接这点由导入表反证 |
| 3 | **安全与信任** | ✅ **盘完**,但过程有波折:安全面子代理**第一版报告有编造**(它自己承认并更正了 §5/§7/§8 的数字),更正后的部分我采信;而它的 §1–§4、§6 原始报告**未送达我这里**,所以**hooks 安装器、管道 DACL、`session.json` 字段、WebView2 六项设置我全部自己重新 grep 验证过一遍**,本文里的行号都是我亲自跑出来的。**一处更正了子代理的说法**:它称"无 `NavigationStarting` 拦截",实际 `webview.rs:1147-1180` **有**顶层拦截,缺的是 `FrameNavigationStarting` 与 `WebResourceRequested` |
| 4 | **签名与分发** | ⚠ **一半未盘**。**已盘**:zip 该装什么(由 ConPTY 加载器契约与导入表推出)、exe 未签名、WebView2 静态链接不用带 loader、`folio.ps1` 不进包(因为 `include_str!`)。**未盘**:SmartScreen 的确切文案与 EV 是否仍即时放行、**SignPath.io OSS 计划的资格与流程**、**Azure Trusted Signing 的资格与价格**、Certum / sigstore、winget 与 scoop 的提交要求、GitHub runner 是否预装 WebView2 —— 这些需要联网核实官方页面,**负责该项检索的子代理未返回结果,我拒绝凭记忆填空**。**建议交给 Codex 或另开一单专门补** |
| 5 | **CI / Release 流水线** | ✅ **盘完**。`ci.yml` 逐行读过;"从未跑过"与"ignored 门是空的"两条是本次最硬的发现;`~/.cargo/config.toml` 不在仓里已核实;版本号/CHANGELOG/产物命名都查了。**runner 上能不能跑标为 `[未验证]`** —— 我拒绝用推断代替实测 |
| 6 | **首次体验** | ⚠ **部分**。**已盘**:配置目录与实测文件清单、`storage_dir()` 的搬家逻辑、panic 钩子的路径与"用户看不到"的结论、`#![windows_subsystem = "windows"]`、五个 schema 版本号、两条迁移链的完整步进、降级行为、WebView2 缺席的降级卡。**未盘**:默认 profile 的**探测顺序**(pwsh/powershell/cmd/WSL 谁先谁后、全找不到时怎么办)、PowerShell 整合提示**首次出现的确切时机**、hooks 三行**默认是否为 Off 的代码级证据** —— 负责这一面的子代理未返回,这三条我没有亲自验到,**不写等于没查**。第 4 面的 "future-version 是否回写覆盖原文件" 同样标为 `[未验证]` |
| 7 | **名字** | ✅ **盘完**。商标注意项(serial 97445151)来自定名记录;`APP_NAME`、`NOTIFICATION_AUMID`、`[[bin]]` 三处已核实一致;唯一漏网是 `PANIC_LOG_FILENAME` |
| 8 | **文档** | ✅ **盘完**。README/CHANGELOG/社区文件的缺失、235/296 的中文比例、`BINDINGS` 可自动生成的证据、已知问题的出处、以及"跨 DPI 上半黑"其实已被日志撤销这一条 |

**一句话**:八个面里 **六个盘完、两个部分**(第 4 面的外部行情、第 6 面的三条首次体验细节)。
两处缺口都已在正文里就地标了 `[未盘]` / `[未验证]`,没有一条是用推测补上的。
建议把这两块作为独立一单交给 Codex 对审时一并补齐。

## 用户裁决(2026-08-27)

- **提交历史:选 ②**——建 remote 前一次 `git filter-repo`:去掉全部 `Claude-Session:` trailer、作者邮箱 `weiyishi@umich.edu` → `weiyishi024@gmail.com`;**时机=打磨期末、发布前一天、零在飞线**(hash 全变,所有 worktree/分支需先收完)。`Co-Authored-By` trailer 留(既有署名裁决)。
