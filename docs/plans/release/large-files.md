# 大文件签收表(门 0)

**为什么有这张表**:公开仓的每一个字节都会进入每一次 `git clone`,而且删不掉 ——
一旦推上去,再删也只是删了工作树,历史里还在。所以在建 remote 之前逐项签收一次:
留,还是现在删。

**口径**:`git ls-files -z` 的 1127 个跟踪文件,合计 120,261,538 字节(114.7 MiB),
按工作树字节数降序取前 30(第 30 名 514,221 字节)。测量于 0.2.3 发布准备工作树,
`git ls-files` + 文件大小,未含 `.git/` 本身。上一次口径是 967 个文件、
合计 104,925,500 字节;新进前三十的五项(两张 README 动图、两张三面拼板、`bt-platform/src/lib.rs`)
逐条在下面;退出前三十的五张 spike 截图仍在仓里,结论不变。

**没有文件接近 GitHub 的单文件 100 MiB 硬限**,最大一个 18.27 MiB。
`.gitattributes` 里没有 `filter=lfs`,也不建议为这几个引入 LFS ——
LFS 会给每个 clone 加一层必须配置的依赖,换来的只是三个文件的下载时机。

**本表不删任何文件**。三十项的结论全是 keep,理由逐条在下面;
若将来有一项要删,改这张表并在同一条 commit 里删。

| # | 字节 | 文件 | 结论 | 理由 |
|---:|---:|---|:--:|---|
| 1 | 19,157,581 | `vendor/alacritty_terminal/tests/ref/row_reset/grid.json` | **keep** | 上游 0.26.0 自带的 ref 测试夹具,与 crates.io 归档逐字节相同(`scripts/check-vendor-notices.ps1` 每次都在证明这一点)。`tests/ref.rs:57` 的 `ref_tests!` 名单点名 `row_reset`;删掉它 = 删掉上游那条回归线,而 CI 里「vendored alacritty is a workspace member」这道门存在的唯一理由就是让上游测试继续跑。而且删了它,vendor 目录就不再是上游的忠实副本,§4(b) 的「改动清单」也就多一条本可以不要的条目 |
| 2 | 10,682,364 | `assets/fonts/NotoColorEmoji_WindowsCompatible.ttf` | **keep** | `crates/bt-render/src/lib.rs` 用 `include_bytes!` 把它编进 exe:这是彩色 emoji 的唯一来源,Windows 自带的 `seguiemj.ttf` 是 COLR/CPAL,这一份是 CBDT/CBLC 位图,渲染路径不同。OFL-1.1,版权与许可随附在 `assets/fonts/NotoColorEmoji-LICENSE`,并进 `THIRD-PARTY-NOTICES.md` |
| 3 | 9,744,490 | `vendor/alacritty_terminal/tests/ref/history/grid.json` | **keep** | 同 1;`tests/ref.rs:49` 点名 `history` |
| 4 | 6,706,235 | `crates/bt-app/src/main.rs` | **keep** | 一方源码 |
| 5 | 2,475,739 | `docs/DESIGN.md` | **keep** | 设计正史。公开范围归门 7 裁;门 0 只保证它不带机器路径 |
| 6 | 2,099,195 | `crates/bt-app/src/seats.rs` | **keep** | 一方源码 |
| 7 | 1,852,026 | `docs/screenshots/tab-into-pane-dark.gif` | **keep** | README 两语共用的动图(把窗格拖成标签页的那两段手势),双主题各一份,1200×675。它是 README 里唯一动起来的东西,静图讲不了这两下;规格与哈希见 `docs/screenshots/README.md`。自制 |
| 8 | 1,719,158 | `vendor/conpty/Microsoft.Windows.Console.ConPTY.1.24.260512001.nupkg` | **keep** | 上一版官方 ConPTY sidecar,留作**可复现 A/B 归档**:`vendor/conpty/README.md` 的 pin 判决表把「1.24 FAIL / 1.25 PASS」当作 pin 成立的证据,而那张表只有在两份包都在仓里时才可重跑。`crates/bt-pty/build.rs:12` 只引用 1.25,1.24 不进任何构建。MIT(Microsoft),许可原文见 `vendor/conpty/LICENSE-MICROSOFT-TERMINAL` |
| 9 | 1,717,498 | `vendor/conpty/Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg` | **keep** | 在用的 sidecar 源包。`crates/bt-pty/build.rs` 校验包与条目 SHA-256 后解出 `conpty.dll` / `OpenConsole.exe` 到 `target/`;解出物是构建产物,**不签入**。留包而不留解出的二进制,是让「装进 zip 的那两个文件」有一条可复现、可校验的来路。MIT(Microsoft),同上 |
| 10 | 1,672,534 | `vendor/alacritty_terminal/tests/ref/vim_24bitcolors_bce/grid.json` | **keep** | 同 1;`tests/ref.rs:67` |
| 11 | 1,426,030 | `docs/screenshots/tab-into-pane-light.gif` | **keep** | 同 7,浅色那一份 |
| 12 | 1,311,010 | `crates/bt-term/src/session.rs` | **keep** | 一方源码 |
| 13 | 1,221,626 | `vendor/alacritty_terminal/tests/ref/grid_reset/grid.json` | **keep** | 同 1;`tests/ref.rs:48` |
| 14 | 1,199,026 | `crates/bt-app/src/settings.rs` | **keep** | 一方源码 |
| 15 | 1,073,282 | `crates/bt-render/src/lib.rs` | **keep** | 一方源码 |
| 16 | 980,443 | `crates/bt-app/src/profiles.rs` | **keep** | 一方源码 |
| 17 | 959,220 | `design/assets/file-icons-r2/options.png` | **keep** | 图标 r2 轮的比选大图,`design/assets/file-icons-r2/options.html` 的渲染结果 —— 决策记录的证据面。自制,来源见 `design/PROVENANCE.md` |
| 18 | 905,414 | `design/ui-mockup.html` | **keep** | 交互式设计母本;`crates/bt-app/src/marks.rs` 的符号表以它为来源。自制 |
| 19 | 853,061 | `docs/spikes/artifacts/win-landing/shot-02-actioncenter.png` | **keep** | spike 证据截图 |
| 20 | 792,190 | `vendor/alacritty_terminal/tests/ref/vim_large_window_scroll/grid.json` | **keep** | 同 1 |
| 21 | 773,961 | `vendor/alacritty_terminal/tests/ref/underline/grid.json` | **keep** | 同 1 |
| 22 | 658,859 | `vendor/alacritty_terminal/tests/ref/tab_rendering/grid.json` | **keep** | 同 1 |
| 23 | 631,882 | `assets/readme/surfaces-light.png` | **keep** | 三面拼板,口径与理由见下面「README 图片这一批」;按 0.2.3 的口径它单张已进前三十,所以两处都点名 |
| 24 | 608,886 | `vendor/alacritty_terminal/tests/ref/colored_reset/grid.json` | **keep** | 同 1 |
| 25 | 569,559 | `vendor/alacritty_terminal/tests/ref/colored_underline/grid.json` | **keep** | 同 1 |
| 26 | 553,845 | `assets/readme/surfaces-dark.png` | **keep** | 同 23,深色那一份 |
| 27 | 545,401 | `docs/spikes/artifacts/webview2/child-04-resize-anim-1.png` | **keep** | spike 证据截图 |
| 28 | 532,567 | `crates/bt-platform/src/lib.rs` | **keep** | 一方源码 |
| 29 | 526,841 | `THIRD-PARTY-NOTICES.md` | **keep** | 门 1 的产物,也是发布 zip 必带件。大是因为它把每一份许可**全文**带上而不是点名 —— 五百多个包里有一百九十多份互不相同的 MIT 版权行,少一份就少一次署名。由 `scripts/generate-notices.ps1` 生成,`scripts/check-notices.ps1` 守漂移 |
| 30 | 514,221 | `crates/bt-viewport/src/lib.rs` | **keep** | 一方源码 |

## README 图片这一批(2026-08-27 起为 2x)

两张三面拼板已进上面的前三十(631,882 与 553,845 字节),其余单张都进不了;
它们是一起下的,所以在这里按批再签收一次。

| 批 | 张数 | 总字节 | 结论 | 理由 |
|---|---:|---:|:--:|---|
| `docs/screenshots/*.png` | 21 | 3,637,187 (3.47 MiB) | **keep** | README 两语共用的实拍图。2026-08-27 由 100%(1600×1000)改拍 200%(3200×2000),字节数约翻四倍——**这是明知的代价**:README 主要在 1.25×–2× 的屏幕上读,1x 图在那里是放大过的图。每张都过 `oxipng -o 4 --strip safe`(无损,省下 19.7%:3,192,930 → 2,564,435)。0.2.0 新增四张(快捷终端、聚焦搜索各双主题),其中 `quake-*` 取 1920×1200——同为 200%,只是取景框小,理由写在 `docs/screenshots/README.md` 里。0.2.1 新增一张 `context-menu.png`(786×593,55,470 字节),它是唯一一张不在本机拍的:一级菜单那一行只在登记了 `folio.msix` 的机器上存在,而本仓库任何东西都不在打包机上登记包,所以它出自验证用的干净虚拟机,按那台机器自己的 100% 取景,不放大。0.2.2 新增两张 `first-run-*.png`(3200×2000,当时合计 178,565 字节,后来重跑一次 oxipng 收到 167,213):初次设置卡每台机器只出现一次,所以它们是唯一一对靠 `BT_FIRST_RUN_CARD` 那扇门拍的,机器状态全部隔离,取景与其余 200% 那批一致。0.2.3 未新增截图。规格与逐张哈希见 `docs/screenshots/README.md` |
| `assets/readme/surfaces-*.png` | 2 | 1,185,727 (1.13 MiB) | **keep** | 三面拼板,由 `assets/readme/src/surfaces-*.svg` 裁上面那批渲染而成,随它们一同改为 3200×3240。同样过 `oxipng -o 4 --strip safe`(1,444,629 → 1,185,727)。SVG 一个字没改:版面用的是画布自己的单位 |

两批合计 **4,822,914 字节(4.60 MiB)**,占一次 clone 的约 4.3%。
`assets/readme/hero-*.svg` 是纯 SVG,两个加起来 5,952 字节,不入此列。

## 顺带核实过的两件

- `git ls-files dist` 零输出;`dist/` 在 `.gitignore` 里。开发机 `dist/` 下现有的
  77 MB exe **不会**随普通提交公开。
- `git ls-files -ci --exclude-standard` 零输出:没有「已被跟踪又被忽略」的冲突文件,
  门 0 新加的 `.env*` / `*.pfx` / `*.p12` / `*.pem` / `*.key` / `*.pdb` / `.vs/` /
  `.vscode/` / `.claude/` 规则没有和任何现存跟踪文件打架。
