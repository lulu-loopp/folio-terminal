# Folio UI 只读审计 — 图标统一块 / 动画补齐块 盘点清单

日期:2026-08-25 · 只读,未改动任何仓库文件

## ⚠️ 关于版本的重要说明(先读)

**审计过程中仓库前进了 5 个提交**,`3cc2316` → `027c1d0`(Aug 25 21:50–22:00 落地),改动了 33 个文件 7298 行,其中恰好包含本报告引用最密的四个:`main.rs`(+1383)、`profiles.rs`(+514)、`seats.rs`(+331)、`marks.rs`(+244)。

处理方式:
- **本报告的所有 file:line 已全部重新锚定到 `027c1d0`(HEAD)**,并逐条复验了结论本身是否仍然成立 —— **全部成立**,变的只是行号。
- **截图是 `3cc2316` 的**(`target/release/folio.exe`,Aug 25 19:16 构建,早于这五个提交 2.5 小时)。截图佐证的是几何与配色事实,这些在 HEAD 未变;但**截图不反映 HEAD 新增的 `ProfileLine` 线稿族**(见 §0.5)。
- 未改动的引用源(`bt-render/theme.rs`、`toast.rs`、`tooltip.rs`、`keyhint.rs`、`notice.rs`、`peek_strip.rs`、`restore.rs`、`search.rs`、`git_graph.rs`)行号原样有效。

实机取证:隔离 APPDATA 探针窗,`scripts/dev/ui-probe.ps1`,DPI scale = 2.0,截图 `01-base.png` … `06-termmenu.png` 与 `crop-*.png`。探针进程已自行关闭。仓库 `git status` 全程干净。

已知在修、本报告**不重复报**:files 菜单 New-terminal 行图标风格、git 面板 BRANCHES/COMMITS 标题块、Move to window 单窗行、浮窗脚、设置说明截断。

---

## 0. 先看这里 —— 需要你裁的五条,和不用裁的其余

**不用裁的**(是缺陷,不是取舍,直接进施工单):病灶①线粗、④风格混杂、⑤光学尺寸、③a Rename 无图标、动画②硬切与节奏。

**需要你裁的五条**(现有代码里都写着一条相反方向的书面理由,推翻它们要你点头):

| # | 现状与其书面理由 | 我的意见 |
|---|---|---|
| 裁1 | **`Move pane to new window` 与 `Move to window ▸` 共用 `External`**。`profiles.rs:8018-8022` 自辩:「同一句话,差别只在落到哪里,这正是子菜单的作用」。实测两行光栅**逐像素相同**。 | **拆**。区分符 `▸` 在行尾,离图标 400px,读者的眼睛不会跑那么远 |
| 裁2 | **「去新 tab」拿带框 ↗、「去新窗口」拿裸 ↗**(`profiles.rs:8010,8017`)。而 `marks.rs:405-409` 定的语义是「框=离开树,裸=离开窗口」。 | 要么**互换**,要么改 `marks.rs` 的语义注释。现在两处互相打架 |
| 裁3 | **peek strip / glance 卡故意无淡入**。`peek_strip.rs:19-25` 明写「任何 motion 模式下都不淡入,本来就没有淡入可去掉」。 | **不动**。除非你要「全局一致」压过这条 |
| 裁4 | **进度环读数过渡故意不遵守 reduced-motion**。`main.rs:17556` `SweepTween` 明写 deliberately。 | **接受**,但它是全库唯一豁免,值得在新的 motion 模块里登记成显式白名单 |
| 裁5 | **`i-gear` 保留 Material 24 格实心** vs 重画成 16 格描边。它是唯一的外来件,但也是全世界最认得的设置图标。 | **重画**。它现在挨着三根 1.0 发丝线,截图 `crop-caption.png` 一眼可见违和 |

---

## 0.5 审计期间刚落地的变更 —— 「统一 pen」已经开工了

HEAD 的 `marks.rs` 新增了一个 **`ProfileLine(ProfileGlyph)`** 族(`marks.rs:131-139`),把三枚 profile 品牌标(Console / Ubuntu / Git)做成**单色线稿**版本,并且给了它们**一个共同的笔**:

```
crates/bt-app/src/marks.rs:156
const PROFILE_LINE_STROKE_UNITS: &str = "1.2";
```

`marks.rs:141-155` 的理由值得整段引用,因为它和本报告的结论完全同向:

> *"**The one number, and it is the column's rather than the mark's.** The file row's menu draws its glyphs between `1.15` (`#i-file`) and `1.3` (`#i-copy`, `#i-paste`, `#i-pencil`); `#i-external`, the row directly above `New terminal here`, is `1.2`, and so is `#i-float`. A line rendition exists to stop one row standing out, so it takes the weight its neighbours have rather than inventing a ninth."*

**三点影响:**

1. **house pen 已经被这条实际裁成 1.2,不是我原先建议的 1.15。** 本报告的规格草案(§1.4 A)已据此改为 **1.2**,不再推 1.15 —— 一条刚落地的裁决比我的偏好优先。
2. **它承认了本报告病灶①的存在**(「1.15 到 1.3」「不要发明第九种」),但只在**一枚新形**上执行,没有回头统一既有的 46 枚。
3. **它单独解决不了问题。** 统一 pen 只管「名义 stroke」;实际粗细还要乘 `绘制框 ÷ viewBox`,而绘制框仍由 40 多个各自为政的常量决定。同一支 1.2 的笔:15px 框 → 1.125、14px → 1.050、13px → 0.975、11px → 0.825、9px → 0.675。**光是框的差异就能让一支统一的笔出来 1.67× 的粗细差。** 所以 §1.3「尺寸没有单一来源」这条结论不但没被这次改动推翻,反而被它反证。

---

# 审计一:图标

## 1.0 一句话结论

**几何有单一来源,尺寸和语义没有。** 46 枚符号的路径全部集中在 `marks.rs` 的 `SYMBOL_BODY`/`SYMBOL_VIEW_BOX` 两张表里(这一层做得很干净);但**每个绘制点自己决定把它画多大**,而**「哪个动词配哪枚形」被摊在 14 个各自为政的 `fn mark()` 里**。所有病灶都从这两个缺口里长出来。

---

## 1.1 图标清单(几何总表)

来源:`crates/bt-app/src/marks.rs`(HEAD 行号)
- 枚举 `ChromeMark`:`marks.rs:161`(46 个 symbol slot + 若干程序生成形)
- 视窗格:`SYMBOL_VIEW_BOX`:`marks.rs:1990`
- 路径本体:`SYMBOL_BODY`:`marks.rs:2087`
- 栅格化:`svg_document`:`marks.rs:1489`(SVG → resvg 光栅,缓存键 `mark_key`:`marks.rs:1377`)
- 颜色注入:`currentColor` 替换;`takes_current_color`:`marks.rs:951`(profile 品牌标**不**跟主题)
- 符号索引:`symbol_index`:`marks.rs:1886`
- **新**:线稿 profile 族的统一笔 `PROFILE_LINE_STROKE_UNITS`:`marks.rs:156`

| 符号 id | 变体 | 网格 | 画法 | 名义 stroke | 主要表面 |
|---|---|---|---|---|---|
| `i-gear` | `Gear` | **24** | **纯填充**(Material 原样) | — | 标题栏设置钮 |
| `i-min` | `WindowMinimize` | 10 | 描边 | **1.0** | 标题栏 |
| `i-max` | `WindowMaximize` | 10 | 描边 | **1.0** | 标题栏 / 聚焦模式行 |
| `i-close` | `WindowClose`/`TabClose`/`PaneClose` | 10 | 描边 | **1.0** | 标题栏 / tab / pane 头 / 菜单删除行 / 设置删除行 / 预览 rail 停止 |
| `i-plus` | `Plus` | 10 | 描边 · round cap | **1.2** | 新建 tab / git Stage / 新建分支 |
| `i-minus` | `Minus` | 10 | 描边 · round cap | **1.2** | git Unstage |
| `i-chev` | `Chevron{deg}` | **10×6** | 描边 · round | **1.2** | profile 选择器 / pane 头 / 独 pane ghost / files 根 / **预览 Back-Forward(转 90°/270°)** / 搜索钮 / 设置分组 |
| `i-tri` | `TreeDisclosure{deg}` | 10 | **填充** | — | files 行展开 / 子菜单 ▸ / 设置侧栏树 |
| grip | `ResizeGrip` | **8** | 描边 · opacity .55 | **1.5** | 浮窗右下角 |
| `i-file` | `File` | 16 | 描边 | **1.15** | 预览头 / peek / restore / toast / Open diff |
| `i-globe` | `Globe{favicon}` | 16 | 描边 | **1.15** | 网页页签(可被 favicon 顶替) |
| `i-git-branch` | `GitBranch` | 16 | 描边 | **1.15** | git masthead / checkout 行 |
| `i-git-graph` | `GitGraph` | 16 | 描边 | **1.15** | git 面板「打开 Git 图」按钮(`git_panel.rs:459`) |
| `i-tag` | `Tag` | 16 | 描边 + 填充点 | **1.15** | git 图 ref 标签 / Create tag |
| `i-folder` | `Folder` | 16 | **纯填充** | — | pane 头 files / files 行 / 面包屑 / 菜单多处 |
| `i-folder-open`| `FolderOpen` | 16 | **纯填充** + opacity .55 | — | files 展开行 / Reveal in Explorer |
| `i-panel` | `Panel` | 16 | 描边 | **1.1** | 未知 pane / 面板开关 |
| `i-split` | `Split` | 16 | 描边 | **1.1** | Split with / Compare with… / 设置切分方向 |
| `i-split-right`/`down` | `SplitRight`/`SplitDown` | 16 | 描边 | **1.1** | Split 子菜单 / 设置切分方向 |
| `i-float` | `Float` | 16 | 描边 | **1.2** | pane 头浮出 / Move pane to new **tab** / 预览 pop out |
| `i-external` | `External` | 16 | 描边 | **1.2** | Open with / Move to new **window** / Move to window ▸ / 预览外开 |
| `i-dock-left`/`right` | `DockLeft`/`DockRight` | 16 | 描边 + 填充块 opacity .7 | **1.2** | 浮窗 DOCK |
| `i-pin`/`i-pinned` | `Pin{filled}` | 16 | 描边 / 填充 | **1.25** | tab pin / 行 pin |
| `i-lock`/`i-locked` | `Lock{engaged}` | 16 | 描边 / 填充 | **1.25** | 预览 pane 锁(`seats.rs:15375`) |
| `i-zoom-in`/`out` | `PaneZoom{zoomed}` | 16 | 描边 | **1.25** | pane 头 / Zoom pane 行 |
| `i-copy` | `Copy` | 16 | 描边 | **1.3** | 复制路径/哈希/主题/名 / Duplicate pane / 复制地址 |
| `i-paste` | `Paste` | 16 | 描边 **1.3 + 1.1** + 填充块 | 混合 | 终端菜单 / Insert path |
| `i-save` | `Save` | 16 | 描边 **1.3 + 1.15** | 混合 | 预览头 |
| `i-eye` | `Eye` | 16 | 描边 | **1.3** | markdown 渲染视图 |
| `i-code` | `Code` | 16 | 描边 | **1.4** | markdown 源码视图 / **开发者工具** / git CopyHash / Parent |
| `i-refresh` | `Refresh` | 16 | 描边 **1.3** + 填充箭头 | 混合 | git 重读 / Restart shell / 网页重载 |
| `i-select` | `SelectAll` | 16 | 描边 | **1.3** | 终端菜单 |
| `i-broom` | `Broom` | 16 | 描边 **1.3 + 1.2** | 混合 | Clear screen |
| `i-clear` | `Eraser` | 16 | 描边 | **1.3** | Clear scrollback… |
| `i-pencil` | `Pencil` | 16 | 描边 | **1.3** | files Rename / 设置编辑行(`settings.rs:9951`) |
| `i-check` | `Check` | 16 | 描边 | **1.6** | Reveal 成功回执(1300ms)/ git 过滤勾选 |
| `p-pwsh`/`p-ubuntu`/`p-git`/`p-cmd`/`p-shell` | Profile* | 16 | **实心彩色品牌标** | 1.35(内白线) | tab / pane 头 / 菜单 |
| **`p-*-line`** | **`ProfileLine`(新)** | 16 | **单色描边** | **1.2(统一)** | 文件菜单 New-terminal 行 |
| `.ggr path.side` | `GitMergeCurve` | **14×27** | 描边 | **1.5** | 迷你 git 图 |
| — | `GraphCurve` | 生成 | 描边 | 调用方给 | 完整 git 图 |
| — | `ActiveTab`/`TabBody(Ring)`/`ControlPill(Foot/Ring)`/`CardCorner`/`Fill`/`ProgressRing` | 生成 | 填充/描边 | — | 底板,不是图标 |

**名义 stroke 一共 10 个值:1.0 / 1.1 / 1.15 / 1.2 / 1.25 / 1.3 / 1.35 / 1.4 / 1.5 / 1.6。** 光是 16 格族里就有 7 档。

---

## 1.2 病灶

### ① 线粗不一致 —— 用户实证成立,且比目测更糟

「名义 stroke」不是屏幕上的粗细。真正的粗细 = 名义 stroke × (绘制框 ÷ viewBox),而**绘制框由每个调用点自己填**。我把全仓库的绘制框常量代进去算了一遍(脚本 `stroke.py`,输出 `stroke-table.txt`):

**pane 头那一行(用户报的那处),三枚并排:**

| 控件 | 符号 | 绘制框 | **实际描边(逻辑 px)** | 常量位置(HEAD) |
|---|---|---|---|---|
| `⌄` | `i-chev` 10×6 | 13×13 | **1.56** | `seats.rs:5453` = 13.0,绘制 `seats.rs:7827` |
| `🗀` | `i-folder` 16 | 13×13 | 填充 | `seats.rs:5435` = 13.0 |
| `✕` | `i-close` 10 | **8×8** | **0.80** | `bt-render/theme.rs:2712` = 8.0,绘制 `seats.rs:7770-7774` |

**1.56 : 0.80 = 1.95×。** 实机截图 `crop-panehead.png`(6× 放大)一眼可见:`⌄` 粗、`✕` 细、中间夹一块实心 `🗀`。像素测量(`m2.py`,2× DPI):chevron 列向游程中位数 5.0px,close 3.0px。

根因:`⌄` 的 viewBox 是 **10×6**,塞进 13×13 的方框时按 `xMidYMid meet` 取 min(13/10, 13/6) = 1.3 倍;`✕` 的 viewBox 是 10×10,塞进 8×8 只有 0.8 倍。两个缩放系数差 1.63 倍,再乘上名义 stroke 1.2 : 1.0,就是 1.95。

**同一枚 `i-chev` 在六个表面有六种粗细/朝向:**

| 表面 | 框 | 实际描边 | 位置(HEAD) |
|---|---|---|---|
| files 根路径 | 8×5 | **0.96** | `seats.rs:12270-12271` |
| 标题栏 profile 选择器 | 9×6 | **1.08** | `bt-render/theme.rs:2312-2313` |
| 搜索条按钮 | 10×10 | **1.20** | `search.rs:133` |
| 独 pane ghost | 11×7 | **1.32** | `seats.rs:5514,5518` |
| **预览头 Back / Forward** | 11×11 | **1.32** | `seats.rs:15521`(转 90°)/ `15533`(转 270°) |
| pane 头 | 13×13 | **1.56** | `seats.rs:5453` |

**同一枚 `i-close` 三种:** 0.80(tab / pane / toast / focus card)、0.90(浮窗 `float.rs:501`)、1.00(标题栏 / 菜单行 / notice `notice.rs:57` / 设置 `settings.rs:9430,9957`)。

**同一枚 `i-file` 1.66× 跨度:** 0.65(peek strip leaf,`peek_strip.rs:140`)→ 1.08(file peek,`file_peek.rs:114`)。

代码里已经有人发现过这个问题并**局部**修过两次:`profiles.rs:87-125` 的 `ITEM_MARK_EDGE_TO_EDGE_LOGICAL_PX = 10.0`(`profiles.rs:125`),以及 HEAD 刚落地的 `PROFILE_LINE_STROKE_UNITS`(`marks.rs:156`)。两次都是**局部**的:前者按枚举列名单(漏了四枚,见病灶⑤),后者只管一族新形。

---

### ② 一枚图标多个动词共用

实机取证 `crop-panemenu.png`(pane 菜单整张)。

**a) `External`(裸 ↗)—— 用户报的那处,实锤。**
`Move pane to new window`(`profiles.rs:8017`)与 `Move to window ▸`(`profiles.rs:8023`)相邻两行,**光栅逐像素完全相同**:

```
identical rasters: True   maxdiff 0
```
(测量 `02-panemenu.png` 中两行的 52×56 像素块)

代码里有一段自辩(`profiles.rs:8018-8022`):「同一句话,差别只在落到哪里,这正是子菜单的作用」。但唯一的区分符 `▸` 在行的**最右端**,离图标 ~400 逻辑 px。这条要用户裁。

`External` 还同时是:`Open with`(`profiles.rs:5436`)、预览头「交给系统浏览器」(`seats.rs:15576`)。**一形四义。**

**b) `i-close`(✕)—— 五种语义,横跨两个变体名。**
- 关窗 / 关 tab / 关 pane(`seats.rs:7774`、标题栏、`float.rs`)
- **删除分支 / 删除标签 / 丢弃改动**(`profiles.rs:6141`,用 `PaneClose`)
- **设置里删除一行**(`settings.rs:9957`,用 `WindowClose`)—— 同一个「删除」动词,在两个表面选了**两个不同的变体名**
- **停止导航**(`seats.rs:15548`:载入中把 reload 换成 `PaneClose`)
- 关闭设置对话框(`settings.rs:9430`)

「关闭」「删除」「停止」是三种不同量级的动作,共用一张画。

**c) `Refresh`(↻)—— 三种语义。**
重读仓库(`git_panel.rs:460`、`git_graph.rs:4302`)、**重启 shell**(`profiles.rs:7141`,截图 `crop-termmenu2.png`)、网页重载(`seats.rs:15550`)。

**d) `Code`(`</>`)—— 四种语义,其中一处代码里明写「刻意复用」。**
markdown 源码视图(`seats.rs:15323`、`15831`、`float.rs`)、**开发者工具**(`seats.rs:15346`,注释:*"the same glyph markdown's `Edit source` wears, and the same sentence"*)、git `Copy hash`(`git_graph.rs:4675`)、**跳到父提交**(`git_graph.rs:4679`)。后两者和「源码」毫无关系。

**e) `Split`(田)—— 三种语义。**
`Split with`(`profiles.rs`)、**`Compare with selected` / `Compare with working tree`**(`profiles.rs:6150`)、设置「默认切分方向」示意图(`settings.rs:1518`)。

**f) `Copy` —— 五种动词。**
`Copy path`(`profiles.rs:5474` 附近)、git 的 `Copy path/hash/subject/name` 四个文案(`profiles.rs:6146-6149` 一组)、终端选区 `Copy`、**`Duplicate pane`**、预览头复制地址。「复制文本」和「复制一个 pane」是两件事。

**g) `Chevron` —— 下拉箭头兼任导航箭头。**
它既是「我底下有一张折叠的表」(profile 选择器、pane 头、files 根、设置分组),又转 90°/270° 当**网页的后退/前进**用(`seats.rs:15521,15533`)。浏览器的后退键从来不是下拉箭头。

**h) `Folder`(实心夹)** 在 profile 选择器里连着两行:`Files pane` 与 `New terminal in folder…`(截图 `crop-profilemenu.png`)。**注:此行归属已有单**,这里只记录「相邻重复」这一面。

---

### ③ 同一动词在不同表面长得不一样

**a) `Rename` —— 一处有形,一处压根没有形,而且理由是过期的。**
- files 菜单:`profiles.rs:5474` `Self::Rename => ChromeMark::Pencil` ✅
- 设置编辑行:`settings.rs:9951` 也用 `Pencil` ✅
- git 菜单:`profiles.rs:6140` `Self::RenameBranch => None`,上面挂着注释(`profiles.rs:6130`):

  > *"The house's mark set is cut from geometry and has no pencil in it"*

  **这句话已经不成立了。** `#i-pencil` 在 `marks.rs:732` / `SYMBOL_BODY[39]`(`marks.rs:2334`),是 2026-08-19 加进来的,并且**同一个文件里 700 行开外的 `Rename` 已经在用它**。这是一条僵尸注释挡住的空槽。

**b) `Close` / `Delete` —— 三个变体名,同一张图,分配无规律。**
`WindowClose` / `TabClose` / `PaneClose` 是 `symbol_index` 里的同一个 slot,但:pane 菜单的 `Close pane` 用 `TabClose`,git 菜单的删除行用 `PaneClose`(`profiles.rs:6141`),设置的删除行用 `WindowClose`(`settings.rs:9957`)。读代码的人无从判断该用哪个。

**c) `Move to new tab` 与 `Move to new window` 的框/裸之分是反的(存疑,请裁)。**
`MoveToNewTab => Float`(带框 ↗,`profiles.rs:8010`)、`MoveToNewWindow => External`(裸 ↗,`profiles.rs:8017`)。而 `marks.rs:405-409` 给这对定的语义是:**框 = 这个 pane 离开树,裸 = 这份内容离开窗口**。「去新 tab」还在同一个窗口里,却拿了「离开」意味更重的框;「去新窗口」真的离开了窗口,反而拿了轻的裸箭头。

**d) `Find…` 无图标。** 终端右键菜单里唯一一个空图标列的行(`profiles.rs:7138` `Self::Find => None`,截图 `crop-termmenu.png`),上下都有图标,读起来像掉了一枚。

---

### ④ 风格混杂(实心 vs 细线,同一 run 内)

**a) 标题栏右端:实心齿轮 + 三根 1.0 发丝线。**(截图 `crop-caption.png`,3× 放大)
`i-gear` 是 Material Design 的 **24 格纯填充**路径(`marks.rs:2088` 起),整张表里唯一一枚 24 格、唯一一枚从别人的图标集整段抄来的。它右边 `—` `▭` `✕` 是 10 格 1.0 描边。一个实心团挨着三根线。

**b) pane 头:`⌄`(线)+ `🗀`(实心)+ `✕`(线)。**(截图 `crop-panehead.png`)
中间那块实心 folder 的墨量是两边的数倍。像素测量(`m2.py`):folder 墨点 342,chevron 116,close 164。

**c) 独 pane 右上角 ghost:实心 `🗀` + 发丝 `⌄`。**(截图 `crop-ghost.png`,6× 放大)只有两枚控件,一实一虚。

**d) pane 菜单一列里 6 枚描边 + 1 枚实心。**(截图 `crop-menucol-a.png`)`New terminal in folder…` 的实心 folder 夹在 zoom / split / duplicate / float / external 五枚描边之间。像素墨量:folder **453**,其余 144–324。
（**注**:HEAD 刚落地的 `ProfileLine` 正是在解决这一类问题的**文件菜单版本**,已有单,不重复报;但 **pane 菜单这一列的实心 folder 不在那张单里**。）

**e) profile 选择器整菜单是彩色品牌标 + 蓝实心夹。**(截图 `crop-profilemenu.png`)这一菜单内部自洽(全实心),但它和 pane 菜单(全描边为主)是两套语言,而两者都从同一枚 `⌄` 打开。

**f) 「未保存」圆点是字体字符,「状态/注意力」圆点是几何 —— 两枚圆点两种物质。**

- **状态点**走 `marks::status_dot_sprite`(`marks.rs:1113`),四个表面共用一个函数(strip / rail 行 / focus card / peek 示意),实心=`ControlPill`、空心=`ControlPillRing`。这一枚做得完全正确,而且是注意力块「关动画后必须静态可分」那条规矩的载体。
- **未保存点**是 `PREVIEW_DIRTY_DOT`(`seats.rs:12414`,`"\u{25cf}"` 即 `●`)当**文本标签**画的,三处:预览头 `seats.rs:15233`(13px 字号,`seats.rs:12412`)、浮窗头 `float.rs:2029-2031`(13px,`float.rs:528`)、file peek 头 `file_peek.rs:905`(**9px**,`file_peek.rs:116`)。

这违反 `marks.rs` 自己的开篇立法(`marks.rs:654`,R4 把 masthead 的 `⎇` 判掉时写的):

> *"A codepoint is not a drawing: whether `⎇` exists at all is a fact about the font the machine happens to have, its width is another..."*

`●` U+25CF 在不同字体里的**光学直径能差 40%**,而它就画在一枚几何圆点旁边两百像素处。R4 已经为 `⎇` 立过一次这个规矩,`●` 是同一条规矩下漏网的那枚。

**g) 图标自身内部混描边粗细** —— `i-paste`(1.3 外框 + 1.1 内线 + 填充块)、`i-save`(1.3 + 1.15)、`i-broom`(1.3 柄 + 1.2 头)、`i-refresh`(1.3 弧 + 填充箭头)、`i-tag`(1.15 + 填充点)。这些是从 mock-up 逐字抄来的,但意味着「一枚图标一个 stroke」这条规矩现在连**枚内**都不成立。

---

### ⑤ 光学尺寸不齐(同菜单内)

pane 菜单一列 8 行,实测墨迹外接框(`measure.py`,`02-panemenu.png`,2× DPI → 折算逻辑 px):

| 行 | 符号 | 墨迹 (物理 px) | 折算逻辑 px | 墨量 |
|---|---|---|---|---|
| Zoom pane | `i-zoom-in` | 24×24 | 12.0 | 220 |
| Split with | `i-split` | **28**×24 | **14.0** | 324 |
| New terminal in folder… | `i-folder` | 24×21 | 12.0 | **453** |
| Duplicate pane | `i-copy` | 24×24 | 12.0 | 268 |
| Move pane to new tab | `i-float` | 21×22 | 10.5 | 235 |
| Move pane to new window | `i-external` | **20**×20 | **10.0** | 144 |
| Move to window | `i-external` | 20×20 | 10.0 | 144 |
| Close pane | `i-close` | 20×20 | 10.0 | 172 |

**同一列内 10.0 → 14.0,40% 跨度;墨量 144 → 453,3.1 倍。** `crop-menucol-a.png` 里 split 的内十字明显比 zoom 的角括号淡而细(1.1 vs 1.25 stroke),folder 是一块实心。

**已修一半但漏了四枚。** `item_mark_logical_px`(`profiles.rs:128-138`)按**枚举名单**把 6 枚「10 格族」降到 10px 框,`_ => 15.0` 兜底。名单里有 `Plus`,**没有 `Minus`、没有 `Chevron`、没有 `ResizeGrip`、没有 `TreeDisclosure`** —— 后四枚同样是 10 格(或 10×6 / 8 格)、同样边到边无气口的形,规则却没覆盖到。

后果最重的是 `Minus`。git 行菜单的 STAGED 组(`profiles.rs:6243` 起)五行,实际描边/墨宽:

| 行 | 形 | 框 | 实际描边 | 墨宽 |
|---|---|---|---|---|
| **Unstage** | `Minus` | **15** | **1.80** | **13.5** |
| Discard | `PaneClose` | 10 | 1.00 | 10.0 |
| Open diff | `File` | 15 | 1.08 | 8.9 |
| Reveal in Explorer | `FolderOpen` | 15 | 填充 | 13.1 |
| Copy path | `Copy` | 15 | 1.22 | 10.5 |

**`Minus` 是全菜单系统里最粗最长的一枚,比同菜单每一个邻居重 1.5–1.8×**,而它本来是这张表里最简单的一根横线。

而 `marks.rs:677` 写死了它和 `Plus` 的契约:

> *"Cut from `#i-plus`'s own path so the pair are one drawing minus a stroke: same ten-unit box, same 1.2 weight, same round cap. A minus struck independently would be a different length or a different weight from the plus sitting a row above it, which on two buttons that mean opposite things is exactly where it would show."*

菜单的尺寸规则把这个契约当场毁掉:`Plus`(CHANGES 组的 `Stage` 行,`profiles.rs:6138`)得到 1.20 / 9.0,`Minus`(STAGED 组的 `Unstage` 行,`profiles.rs:6142`)得到 1.80 / 13.5。

**准确性说明**:这两行**不会同时出现**在一个菜单里 —— `profiles.rs:6243` 起按 `GitGroup` 二选一。所以这不是「相邻两行不一致」,而是两条:①同一枚 `Minus` 在自己那张菜单里比每个邻居重 1.5–1.8×;②同一个动作对(暂存/取消暂存)在两次打开之间形变了 1.5×。
路径:`profiles.rs:6138`/`6142` → `6723` `mark: item.row.mark()` → `4249` `px(item_mark_logical_px(glyph))`。
(注:git 面板**行内**的 `+`/`−` 按钮走的是另一条路 —— `GIT_ACT_GLYPH_LOGICAL_PX = 11.0`,`git_panel.rs:168`,`455`/`454` —— 两者都是 11px 框,那里没有这个问题。)

这是本次审计里**唯一一条有明确书面契约被违反**的,建议列为 P0。

---

## 1.3 来源结构评估

### 已经归一的两层(**保持,别动**)

1. **几何**:46 枚 symbol 的路径全在 `marks.rs` 两张 const 表里(`marks.rs:1990`、`2087`),mock-up 逐字抄录,注释里写清了每个决定的出处。绘制统一走 `svg_document`(`marks.rs:1489`)→ resvg → `mark_key`(`marks.rs:1377`)缓存。**没有任何一处绘制点在自己画路径。**
2. **颜色**:统一走 `bt_render::ChromePalette` 的具名字段(`palette.accent`、`palette.git_act_glyph`、`palette.menu_item_hint_text` …),`currentColor` 在 `svg_document` 内一处替换。品牌标由 `takes_current_color()`(`marks.rs:951`)显式排除。

**顺带的好消息:「用字符冒充图标」这条基本已经扫干净了。** 全仓库搜 `▸ ✕ ⌄ › ✓ ● → ⎇ − ×` 等字面量,UI 里只剩三处,两处正当:
- `↑↓←→`(`shortcuts.rs`,`named_label`)—— 键名,本来就是文字
- `…`(`tooltip.rs:132` `PEEK_ELLIPSIS`)—— 截断省略号,本来就是文字
- **`●`(未保存点,三处)—— 不正当**,见病灶④f

子菜单的 `▸` 也已经是几何(`ChromeMark::TreeDisclosure`,`profiles.rs:7504`、`9096`),不是字符。R4 那次清扫是有效的。

### 没有归一的两层(**病灶全在这里**)

3. **尺寸:一个绘制点一个常量,共 40+ 个。**
   每个 `ChromeSprite::new(mark, rect, color)` 的 `rect` 由调用方自己算,大小来自各文件自己的 `*_GLYPH_LOGICAL_PX` / `*_MARK_LOGICAL_PX`。清单(HEAD 行号,部分):
   `bt-render/theme.rs:2147,2150,2177,2271,2302,2308-2313,2539,2712,2714,2716,2718`;
   `seats.rs:5431,5435,5440,5453,5514,5518,5596,12251,12253,12270,12271,12351,12412,12421,12428`;
   `profiles.rs:85,86,125`;`float.rs:487,501,505,521,528`;`git_panel.rs:76,168,339`;`git_graph.rs:345,1486,1488`;`peek_strip.rs:140,145`;`file_peek.rs:114,116`;`restore.rs:126`;`notice.rs:57`;`search.rs:133`;`toast.rs:142,162`。
   **没有任何一处知道「这枚形的 viewBox 是几」,所以没有任何一处能把实际描边算出来。** 这就是病灶①⑤的机制性根因,也是 §0.5 里那支刚统一的 1.2 笔单独救不了场的原因。

4. **语义(动词 → 形):14 个各自为政的 dispatcher。**
   `profiles.rs` 里的 `FileMenuRow::mark`、`GitMenuRow::mark`、`TermMenuRow::mark`、`PaneMenuRow::mark`、`git_filter_mark`、`recent_mark`、`mark(index)`;
   `git_panel.rs:453`、`git_graph.rs` 的 `detail_part_mark`;
   `main.rs` 的 `tab_mark`;`peek_strip.rs` 的 `leaf_mark`;`seats.rs` 的 `pane_mark`;`marks.rs:1980` 的 `preview_row_mark`;`settings.rs` 的行按钮。
   **没有反向索引**(一枚形被哪些动词用),所以「一形多义」只能靠人肉 grep 发现 —— 这正是 `External` 那对能活到用户报上来的原因。

### 评估结论

归一的**难点不在几何**(已经归一了),在于要新增一层:
- **一张 `viewBox → 光学基准` 的表**,让尺寸能被算出来而不是被填出来;
- **一张 `动词 → 形` 的总表**,让「一形多义」在编译期或测试期可查。

两者都可以在**不动 `SYMBOL_BODY` 一个字节**的前提下加上去。

---

## 1.4 建议规格草案

### A. 统一 stroke —— 以 **16 格 1.2** 为准(跟随 HEAD 刚落地的裁决)

**原先我打算推 1.15**(因为 `marks.rs` 里三处把 `#i-file` 的 1.15 当基准援引)。但 §0.5 那条刚落地的 `PROFILE_LINE_STROKE_UNITS = "1.2"` 已经把 house pen 裁成 **1.2**,理由写得很正当(「取邻居的重量,不要发明第九种」),**一条落地的裁决优先于我的偏好**。

16 格 1.2 折算到各常见框:15px → **1.125**、14px → **1.050**、13px → **0.975**、11px → 0.825、9px → 0.675。

**规格:任何图标在任何表面,最终描边落在 1.05 ± 0.10 逻辑 px。** 这意味着**光统一笔不够,还得约束框**:小于 12px 的绘制框要么放大,要么该形单独加粗补偿。

要执行它需要一个函数(`marks.rs` 新增,不动路径表):

```
ChromeMark::design_stroke_units(self) -> Option<f32>   // 名义 stroke,查表
ChromeMark::view_box_units(self)     -> [f32; 2]       // SYMBOL_VIEW_BOX 已有
// 由此可导出:
optical_stroke_px(mark, box_w, box_h) = stroke_units * min(box_w/vw, box_h/vh)
```
再加一条**红门测试**:遍历全仓库所有绘制点的 (mark, box) 组合,断言 `optical_stroke_px ∈ [0.95, 1.15]`。这条测试一旦立起来,病灶①⑤永久关闭,而且以后任何人加新绘制点都躲不过。

**必须重画的形(名义 stroke 离 1.2 太远,靠缩放救不回来):**

| 形 | 现 stroke | 目标 | 理由 |
|---|---|---|---|
| `i-check` | **1.6** | 1.2 | 全表最粗,15px 菜单框里 1.50 |
| `i-code` | **1.4** | 1.2 | 和它成对的 `i-eye` 是 1.3,一对控件两个重量 |
| `i-copy`/`paste`/`save`/`eye`/`refresh`/`select`/`broom`/`clear`/`pencil` | 1.3(含枚内 1.1/1.15/1.2 杂音) | 1.2 | 「终端菜单 + 预览头」整族降 |
| `i-panel`/`split`/`split-right`/`split-down` | 1.1 | 1.2 | 最细的一族,整族升 |
| `i-pin`/`lock`/`zoom-*` | 1.25 | 1.2 | |
| `i-file`/`globe`/`git-branch`/`git-graph`/`tag` | 1.15 | 1.2 | 与新 pen 对齐 |
| `i-float`/`external`/`dock-*` | 1.2 | **已对** | 它们正是新 pen 的取值来源 |
| `i-gear` | 填充 24 格 | **重画为 16 格 1.2 描边** | 唯一的外来实心件 |
| grip | 1.5 @ 8 格 | 1.2 @ 16 格 | |
| `i-merge` | 1.5 @ 14×27 | 保留(它是图不是标) | |

**10 格族(`min`/`max`/`close`/`plus`/`minus`/`chev`)的处理**:它们在 10 格里 1.0/1.2,折算后已在带上;但 `i-chev` 的 **10×6** 非方形 viewBox 是病灶①的机制性根因(缩放取 min,短边一进方框就爆)。建议把 `i-chev` 和 `i-minus` **重切到 16 格方形**;`min`/`max`/`close`/`plus` 是真正的窗口控件族,可保留 10 格,但要在总表里显式登记为「edge-to-edge 族」,并把 `item_mark_logical_px` 改成**查这个属性**而不是列枚举名。

### B. 网格与圆角

- **网格:16 单位**,唯一。现存例外三处 —— `i-gear`(24,建议重画)、`i-tri`(10,建议保留:纯填充三角不吃 stroke,且 `marks.rs` 已有书面理由)、`i-merge`(14×27,是图不是标,保留)。`i-chev` / `i-minus` / grip 的例外建议撤销。
- **墨迹范围:1.6 – 14.4**(四边各留 1.6 单位气口)。现在越界的:`i-eye`(1.4–14.6)、`i-panel`/`i-split`(1.5–14.5)、`i-tag`(2.2–13.8 偏小)、`i-external`(3.6–12.4 明显偏小)、`i-file`(3.0–12.5 偏小)。
- **圆角**:现存 rx 值 0.9 / 1.2 / 1.5 / 1.6 / 1.7 / 1.8 / 2.0 / 2.2。建议收成两档:**外框 rx=2.0**、**内件 rx=1.2**(注:新落地的 `ProfileLine` 外框已经用 1.2,与此有冲突,施工时统一)。
- **端点**:全部 `stroke-linecap="round"` + `stroke-linejoin="round"`。现在**至少 12 枚**没写 linecap(`i-min`/`i-max`/`i-close`/`i-file`/`i-panel`/`i-eye`/`i-tag`/`i-split`/`i-split-right`/`i-split-down`/`i-dock-left`/`i-dock-right`)。更糟的是**枚内自相矛盾**:`i-copy` 外框无 cap 而内路径有、`i-globe` 圆与椭圆无 cap 而纬线有、`i-lock` 锁梁有 cap 而锁体无、`i-save` 外框有 linejoin 内线两者皆无。

### C. 每动词一形 —— 分配表

**保持现状(一形一义,健康):**
Gear=设置 · Plus=新增 · Minus=移除 · Folder=文件夹 · FolderOpen=已展开/已揭示 · File=文件 · Globe=网页 · Paste=粘贴 · Save=保存 · Eye=渲染视图 · Pin=固定 · Lock=锁定 · PaneZoom=缩放 · SelectAll=全选 · Broom=清屏 · Eraser=清回滚 · Tag=标签 · GitBranch=分支 · GitGraph=Git 图 · SplitRight/Down=方向切分 · DockLeft/Right=停靠 · Check=完成回执 · TreeDisclosure=展开指示 · Pencil=编辑/重命名

**需要拆分(一形多义 → 多形):**

| 现在共用 | 拆成 | 备注 |
|---|---|---|
| `External` = Open with / MoveToNewWindow / MoveToWindow▸ / 预览外开 | **`External`= 交给外部程序**(Open with、预览外开)<br>**新增 `WindowNew`= 去新窗口**<br>**新增 `WindowPick`= 去某个窗口** | 用户报的那处 |
| `i-close` = 关闭 / 删除 / 停止 | **`Close`= 关闭**(保留 ✕)<br>**新增 `Delete`/`Trash`**= 删除分支/标签/丢弃/删设置行<br>**新增 `Stop`**= 停止导航(实心方块) | ✕ 兼职三件事、横跨两个变体名 |
| `Refresh` = 重读仓库 / 重启 shell / 网页重载 | **`Refresh`= 重读**(保留 ↻)<br>**新增 `Restart`**= 重启进程 | |
| `Code` = 源码视图 / 开发者工具 / CopyHash / Parent | **`Code`= 源码视图**(保留 `</>`)<br>**新增 `DevTools`**<br>**新增 `Hash`**= `#` 或指纹形<br>Parent 改用图形类的形 | 四义,代码自陈其一是刻意 |
| `Split` = 切分 / 对照 | **`Split`= 切分**(保留 田)<br>**新增 `Compare`**= 两块 + `⇄` | `SplitRight` 已是「两块并排」,需可区分 |
| `Copy` = 复制文本 / Duplicate pane | **`Copy`= 复制文本**<br>**新增 `Duplicate`**= 两个叠放的 pane 框 | |
| `Chevron` = 下拉 / 网页前进后退 | **`Chevron`= 下拉**<br>**新增 `NavBack`/`NavForward`**= 真正的导航箭头 | |
| `Float` / `External` 的框/裸分配 | **待用户裁**(见病灶③c) | |

**缺形的动词:**

| 动词 | 位置 | 建议 |
|---|---|---|
| `RenameBranch` | `profiles.rs:6140` = `None` | **直接接 `Pencil`**,删掉 `profiles.rs:6130` 那句过期注释 |
| `Find…` | `profiles.rs:7138` = `None` | **新增 `Search`**(放大镜) |
| `Close pane` 用 `TabClose` / 删除行用 `PaneClose` / 设置删除用 `WindowClose` | 三处 | 三个别名合成一个,尺寸靠 A 的光学规则解决 |

---

# 审计二:动画

## 2.0 帧调度地基(决定新增动画的成本)

**没有 60fps 轮询。** 每个会动的子系统实现 `deadline(now) -> Option<Instant>`,`Runtime::turn` 每帧依次 advance,再用 `earliest_deadline`(`main.rs:10136`)折叠成一个最早唤醒时刻,`FolioApp::about_to_wait_inner`(`main.rs:75438`)据此 `set_control_flow(WaitUntil | Wait)`。静止时进程真挂起。

**新增一处动画的成本 = 一个 `sample(now, motion) -> (value, still_moving)` + 一个 `deadline()` + 在 `earliest_deadline` 数组里加一行。** 地基是现成的,补齐块基本不需要新基础设施。

## 2.1 现有动画清单(HEAD 行号)

| 表面 | 触发 | 过渡 | 时长 | 缓动 | 位置 |
|---|---|---|---|---|---|
| profile 箭头转向 | 打开/关闭 profile 菜单 | 旋转 0→180° | **140ms** | `GRAB_EASE`(`main.rs:17785`) | `ChevronTurn` `main.rs:17740-17775`;`start_chevron_turn` `main.rs:33720`;量化 `marks.rs:907` |
| files 行展开三角 | 展开/收起目录 | 旋转 0→90° | **120ms** | — | `seats.rs:12257` `FILES_ROW_TRI_TURN_MS`;`marks.rs:1026,1033` |
| tab 进度环读数 | 百分比变化 | sweep 插值 | **300ms** | `EASE`(`main.rs:16721`) | `SweepTween` `main.rs:17556` |
| tab 进度环 spinner | 不确定态 | 匀速旋转 | **1100ms/圈** | linear | `main.rs:28078` 附近;`theme.rs` |
| 「工作中」呼吸 | 命令运行中 | opacity 1.0↔0.28 | **1700ms/周期** | `EASE_IN_OUT`(`main.rs:16719`) | `breathe_opacity` `main.rs:16735` |
| 「等待」光晕 | 注意力块 wait | opacity 0→0.24→0 | 1700ms 同钟 | `EASE_IN_OUT` | `wait_halo_opacity` `main.rs:16778` |
| 光标闪烁 | 常驻 | **硬切换**(二值) | **550ms** | 无 | `CURSOR_BLINK_PHASE` `main.rs:176`;`main.rs:21680,21691,21710` |
| toast 进场 | 新通知 | 淡入 + 8px 上移 | **120ms** | `EASE` | `toast.rs:61,357` |
| toast 退场 | 到期/被挤 | 淡出(从当前值起) | **90ms** | `EASE` | `toast.rs:65,324-330,430` |
| tab 被挤开位移 | 拖拽 | 位移 | **160ms** | `GRAB_EASE` | `FlipTween` `main.rs:17803-17837` |
| tab 落地闪光 | 拖拽松手 | inset ring 淡出 | **200ms** | `GRAB_EASE` | `LandTween` `main.rs:17893-17912`(注释 `17919` 明写 200 vs 160 的差) |
| pane 重排位移 | 分屏/关闭 | FLIP | **200ms** | `GRAB_EASE` | `main.rs:18150` 附近 |
| 新 pane 出现 | 分屏 | 淡入 | **180ms** | `EASE` | `RevealTween` `main.rs:17597` |
| divider 拖拽卡片内缩 | 拖 divider | inset 过渡 | **100ms** | `EASE` | `RESIZING_CARD_TRANSITION` `main.rs:17969` |
| rail 展开/收起 | 切换 | 宽度 | **180ms** | `EASE` | `RAIL_TRANSITION` `main.rs:17703` → `theme.rs` |
| rail 文字 | 同上 | 淡入淡出 | **100ms**(**开时 +60ms 延迟**) | `EASE` | `theme.rs:2443,2446` |
| 浮窗开 | 弹出 | 淡入 + 5px 位移 | **180ms** | — | `float.rs:69` 附近 |
| 浮窗关 | 关闭 | 淡出 | **220ms**(向左飞 **420ms**) | — | `float.rs:71,79` |
| 浮窗通用 | — | — | **120ms** | — | `float.rs:90`;`theme.rs:2827` |
| tooltip | 悬停 | 延迟 + 淡入 | 延迟 + **90ms** | `EASE` | `tooltip.rs:44,70` |
| keyhint 卡 | 按住修饰键 | 延迟 + 淡入 | 延迟 + **90ms** | `EASE` | `keyhint.rs:53,59` |
| foot「已显示/已保存」回执 | 按下 | 停留后复位 | **1300ms** | — | `FOOT_REVEAL_FEEDBACK` `main.rs:11455` |
| 终端滚动条 thumb | 滚动 | 停留后淡出 | — | — | `termscroll.rs` |

## 2.2 病灶

### ① 硬切 —— 用户主诉「出现都太生硬」的具体清单

| 表面 | 打开路径(HEAD) | 有无过渡 | 判定 |
|---|---|---|---|
| **8 个 popup 面板全部** | `open_pane_menu` `main.rs:52669`;`open_term_menu_at` `main.rs:52090`;关闭统一 `close_popup` `main.rs:33423` / `close_popups_except` `main.rs:33401`(全是 `= Some(...)` / `= None`) | **无。一帧从无到有。** | ⚠️ **P0** |
| profile / root / preview 菜单 | `ProfileMenu{open:bool}` `profiles.rs:3431-3432`;`RootMenu{open:Option<SeatId>}` `profiles.rs:4775-4776`;`PreviewMenu{open:Option<LeafId>}` `profiles.rs:9725-9734` —— **字段里没有任何 `Instant` / tween** | **无**(只有箭头图标单独有 140ms) | ⚠️ **P0** |
| **设置对话框开合** | `SettingsPanel{open:bool}` | **无**。`settings.rs` 全文件 **`Duration::from_millis` 零命中、`Tween` 零命中**;21 处 `opacity` 全是静态置灰(`UNAVAILABLE_MARK_OPACITY`、`PROFILE_HIDDEN_MARK_OPACITY`)或「背景透明度」设置项的数值,**没有一处是时间驱动的** | ⚠️ **P0** |
| **设置 Advanced 折叠组** | 测试断言「折叠时不占任何像素,展开时立刻多出行」 | **无高度过渡**,整块瞬间插入 | ⚠️ P1 |
| **notice 条**(PowerShell 提示) | `notice.rs` `build` 纯几何,全文件动画关键词零命中 | **无** | P1 |
| **tab 切换内容** | `activate_tab` `main.rs:27007` | **无跨淡** | P2(可能是对的) |
| **git pending 变暗** | `GIT_PENDING_FADE = 0.45`,`git_panel.rs:213` | 名字叫 fade,**实为瞬间跳变** | P1 |
| **keyhint 退场** | `keyhint.rs:59` 注释直言 *"It has no exit fade at all"* | 有入场无退场 | P1 |
| **foot「已显示」回执** | `FOOT_REVEAL_FEEDBACK` `main.rs:11455` | 到期**直接换回**文案 | P2 |
| peek strip / glance 卡 | `peek_strip.rs:19-25`、`725-726`;`file_peek.rs` | **无 —— 但是有书面辩护的故意设计** | **不改**,除非用户翻案 |

### ② 节奏不一致

同一类动作,时长各异,没有统一的档位表:

- 淡入类:**90**(tooltip / keyhint)· **100**(rail 文字、停靠预览)· **120**(toast、float 通用)· **180**(rail 宽度、新 pane、浮窗开)· **200**(pane FLIP、tab 落地)· **300**(进度环)
- 淡出类:**90**(toast)· **220**(浮窗)· **420**(浮窗向左)
- 旋转类:**120**(files 三角)· **140**(chevron)
- 曲线只有两条(`EASE` `main.rs:16721`、`GRAB_EASE` `main.rs:17785`),这一层是齐的;**乱的是时长**。
- **常量分散**:`main.rs:16719-17969` 一段、`bt-render/theme.rs` 一段、`seats.rs:12257`、`float.rs:69-90`、`toast.rs:61-82`、`tooltip.rs:44-70`、`keyhint.rs:53-59`。没有一处能一眼看全。

### ③ 与 reduced-motion 的关系

开关:`enum Motion{Full,Reduced}` `main.rs:16797`,读 Win32 `SPI_GETCLIENTAREAANIMATION`。

**遵守的**(退化为终值且不再请求帧):`ChevronTurn::sample`(`main.rs:17740` 起)· `RevealTween::sample`(`17597`)· `FlipTween`(`17803`)· `LandTween`(`17893`)· `breathe_opacity`(`16735`,钉在 0.6)· `wait_halo_opacity`(`16778`,→ 0)· spinner(→ 12 点)· `float.rs` · `tooltip.rs` / `keyhint.rs` 的 opacity。回归测试齐全。

**注意力块「关动画后必须静态可分」的规矩,现有实现是合规的:**
`wait_halo_opacity` 在 Reduced 下返回 0,但**橙色警示边框照画** —— 去的是动效,留的是静态可分的形。`breathe_opacity` 在 Reduced 下钉在固定 0.6 而不是消失。spinner 钉在 12 点位置而不是不见。**新动画方案必须照抄这个模式:动画只能是「已经能静态分辨的状态」之上的一层增益,不能是唯一的信号载体。**

**三个例外,需要裁:**
1. `SweepTween`(进度环读数,`main.rs:17556`)**明确写了故意不遵守** —— 理由是「这是数字读数的抵达,不是东西在屏幕上飞」。可接受,但是全库唯一一处书面豁免。
2. `CursorBlink`(`main.rs:21680` 起)**签名里根本没有 `Motion`**,完全不受开关影响。光标闪烁通常被视为无障碍范畴,建议至少接上开关。
3. `toast.rs::opacity`(`toast.rs:357`)**签名里没有 `Motion`**,注释暗示「由调用方决定」但**没有找到任何调用方屏蔽它**。这是个真的漏接。

**最大的一条活雷:`motion` 只在启动时读一次。** 代码注释自己承认:*"read once at start-up... until this window listens for WM_SETTINGCHANGE"*。用户在 Folio 运行期间去系统设置里关动画,**要重启才生效**。动画补齐块把动画量提上去之后,这条会从「已知限制」变成「用户投诉」。**建议本块内一并接上 `WM_SETTINGCHANGE`。**

## 2.3 建议

### A. 一套克制的过渡族 —— 三档,两条曲线,一处常量

| 档 | 时长 | 位移 | 曲线 | 配给 |
|---|---|---|---|---|
| **快** | **90ms** | 无 | `EASE` | 悬停反馈、tooltip、keyhint、rail 文字、状态色变(含 git pending) |
| **标准** | **140ms** | **4px** | `GRAB_EASE` | **所有 popup / 菜单 / 弹层 / 对话框的出现与消失**、toast、notice、浮窗 |
| **慢** | **200ms** | 布局位移 | `GRAB_EASE` | 需要搬动版面的:rail 宽度、pane FLIP、设置折叠组高度、tab 落地 |

选 140 作标准档,理由:它已经是这个产品的「一次交互」节拍 —— chevron 转向 140ms 是从 mock-up CSS 逐字抄来的,而 chevron 转向和它下面的菜单**本来就是同一个动作的两半**。菜单现在硬切、箭头转 140ms,等于一个动作的两半一半有节奏一半没有。统一到 140 之后这条自动修好。

现有值往这三档收:90→快(已对)· 100→90 · 120→140(files 三角 120 可并入 140)· 160→140 · 180→200 · 220→200 · 300→保留(读数不是出现)· 420→200。

**位移方向规矩(一条):弹层从它的触发点方向来。** 菜单从锚点滑出 4px、toast 从屏幕边缘方向进来 4px(现在 8px,收到 4)、浮窗从被点的位置来(现在 5px,收到 4)。

### B. 全局节奏参数放哪

**新建 `crates/bt-render/src/motion.rs`**(或 `theme.rs` 里一个 `pub mod motion`):

```
pub const MOTION_FAST_MS: u32 = 90;
pub const MOTION_BASE_MS: u32 = 140;
pub const MOTION_SLOW_MS: u32 = 200;
pub const MOTION_TRAVEL_LOGICAL_PX: f32 = 4.0;
pub const EASE: [f32;4] = [0.25,0.1,0.25,1.0];
pub const GRAB_EASE: [f32;4] = [0.2,0.0,0.0,1.0];
```

放 `bt-render` 而不是 `bt-app`,与 `theme.rs` 的其余设计常量同居 —— `theme.rs` 里已经有 `RAIL_TRANSITION_MS`、`WINDOW_TAB_RING_SWEEP_TRANSITION_MS`、`FLOAT_WINDOW_ANIMATION_MS`,而 `main.rs:17703` 那一堆是它们的本地别名。收进去之后 `main.rs` 的常量退化成 re-export,`float.rs` / `toast.rs` / `tooltip.rs` / `keyhint.rs` / `seats.rs:12257` 各自的那份删掉。

**再加一条红门测试**:遍历所有 `*_MS` 常量,断言 ∈ {90,140,200} ∪ {显式豁免名单}。豁免名单里写清每一条的理由(550 光标、1100 spinner、1700 呼吸、300 读数、1300 回执)。这样第四档永远加不进来。

### C. 各表面配档(施工单)

| 表面 | 现状 | 配档 | 说明 |
|---|---|---|---|
| 8 个 popup 面板 | 硬切 | **标准 140 + 4px** | 从锚点方向来;关闭同档反向 |
| profile / root / preview 菜单 | 硬切 | **标准** | 与已有的箭头 140ms 对齐 |
| 子菜单展开 | 硬切(只有悬停延迟) | **标准** | 延迟不变,出现补过渡 |
| 设置对话框 | 硬切 | **标准 140 + 4px** | |
| 设置 Advanced 折叠 | 硬切 | **慢 200**(高度) | 版面搬动 |
| notice 条 | 硬切 | **标准** | |
| keyhint 退场 | 无 | **快 90** | 补上退场,与入场对称 |
| git pending 变暗 | 瞬变 | **快 90** | 名字已经叫 fade 了 |
| foot 回执文案 | 瞬变 | **快 90** 交叉淡 | 1300ms 停留不变 |
| toast | 120/90 | **140/90** | 位移 8→4 |
| 浮窗 | 180/220/420 | **200/200** | 撤销向左飞的特例 |
| rail | 180 + 文字 100/延迟 60 | **200 + 快 90/延迟 60** | 延迟规矩保留 |
| files 行三角 | 120 | **标准 140** | 与 chevron 同族 |
| tooltip | 90 | **快 90** | 已对 |
| pane FLIP / 新 pane / tab 落地 | 200/180/200 | **慢 200** | |
| divider 卡片内缩 | 100 | **快 90** | |
| peek strip / glance 卡 | 故意硬切 | **不改** | 有书面辩护 |
| tab 切换内容 | 硬切 | **不改** | 内容跨淡会让终端文字发糊 |
| 光标闪烁 | 550 硬切换 | **不改时长,补接 `Motion`** | |

### D. 与 reduced-motion 的红线(不许违背)

1. **每一处新过渡都必须收 `Motion` 参数**,`Reduced` 下 `sample` 首帧即终值且 `still_moving = false`(照抄 `RevealTween::sample`,`main.rs:17597`)。
2. **每一处新过渡都必须有一条「Reduced 下不请求任何帧」的测试**,照抄现有的 rail / chevron 回归测试。
3. **动画不得是唯一的状态载体。** 关掉动画后,菜单还是那个菜单(位置、边框、阴影全在),toast 还是那张卡,pending 还是那个 0.45 的暗度。这正是 `wait_halo_opacity` 现在做对的事(去光晕、留橙边)。
4. 顺手补上 `CursorBlink`(`main.rs:21680`)和 `toast.rs::opacity`(`toast.rs:357`)的 `Motion` 参数。
5. **接上 `WM_SETTINGCHANGE`**,让开关不用重启就生效。

---

# 三张清单(初稿)

## 清单甲:重画(改 `SYMBOL_BODY` 里已有的形)

| # | 形 | 改什么 | 优先级 |
|---|---|---|---|
| 甲1 | `i-gear` | **24 格纯填充 → 16 格 1.2 描边**。唯一的外来实心件,挨着三根发丝线(`crop-caption.png`) | **P0** |
| 甲2 | `i-chev` | **10×6 → 16 格方形**。非方 viewBox 是 pane 头 1.95× 差的机制性根因 | **P0** |
| 甲3 | `i-check` | 1.6 → 1.2 | P1 |
| 甲4 | `i-code` | 1.4 → 1.2(与配对的 `i-eye` 对齐) | P1 |
| 甲5 | `i-copy` `i-paste` `i-save` `i-eye` `i-refresh` `i-select` `i-broom` `i-clear` `i-pencil` | 整族 1.3(含枚内 1.1/1.15/1.2 杂音)→ 1.2 | P1 |
| 甲6 | `i-panel` `i-split` `i-split-right` `i-split-down` | 整族 1.1 → 1.2 | P1 |
| 甲7 | `i-pin` `i-pinned` `i-lock` `i-locked` `i-zoom-in` `i-zoom-out` | 1.25 → 1.2 | P1 |
| 甲8 | `i-file` `i-globe` `i-git-branch` `i-git-graph` `i-tag` | 1.15 → 1.2 | P1 |
| 甲9 | `i-minus` | 10 格 → 16 格(与 `i-plus` 一起处理) | P1 |
| 甲10 | grip | 8 格 1.5 → 16 格 1.2 | P2 |
| 甲11 | 至少 12 枚缺 `stroke-linecap` 的 + 4 枚枚内自相矛盾的 | 统一补 `round` | P2 |
| 甲12 | 圆角 rx 八个值 | 收成外框 2.0 / 内件 1.2(注意与新 `ProfileLine` 的 1.2 外框对齐) | P2 |
| 甲13 | 墨迹越界的五枚(`i-eye` `i-panel` `i-split` `i-external` `i-file`) | 收进 1.6–14.4 气口 | P2 |

## 清单乙:合并 / 拆分(改 `fn mark()` 的分配)

| # | 动作 | 位置(HEAD) | 优先级 |
|---|---|---|---|
| 乙1 | **`Minus` 补进 `item_mark_logical_px` 的 edge-to-edge 名单**(更好:改成查几何属性) —— `Minus` 现 1.80 描边 / 13.5 墨宽,全菜单系统最重,违反 `marks.rs:677` 的 Plus/Minus 同重契约 | `profiles.rs:128-138` vs `marks.rs:677` | **P0** |
| 乙2 | `Chevron` `ResizeGrip` `TreeDisclosure` 同上 | 同上 | P1 |
| 乙3 | **`External` 拆三**:交外部 / 去新窗口 / 去某窗口 | `profiles.rs:5436,8017,8023`;`seats.rs:15576` | **P0**(待裁1) |
| 乙4 | **`i-close` 拆三**:关闭 / 删除 / 停止;并统一三个变体名 | `profiles.rs:6141`;`settings.rs:9430,9957`;`seats.rs:15548` | P1 |
| 乙5 | **`Refresh` 拆二**:重读 / 重启 | `profiles.rs:7141` vs `git_panel.rs:460` | P1 |
| 乙6 | **`Code` 拆三**:源码视图 / 开发者工具 / 哈希 | `seats.rs:15323,15346`;`git_graph.rs:4675,4679` | P1 |
| 乙7 | **`Split` 拆二**:切分 / 对照 | `profiles.rs:6150`;`settings.rs:1518` | P2 |
| 乙8 | **`Copy` 拆二**:复制文本 / Duplicate pane | `profiles.rs` PaneMenuRow::Duplicate | P2 |
| 乙9 | **`Chevron` 拆二**:下拉 / 网页前进后退 | `seats.rs:15521,15533` | P2 |
| 乙10 | **裁定 `Float`/`External` 的框/裸是否互换** | `profiles.rs:8010,8017` vs `marks.rs:405-409` | P1 待裁 |

## 清单丙:新增

| # | 新形 / 新设施 | 给谁 | 优先级 |
|---|---|---|---|
| 丙1 | **接线 `Pencil` 到 `RenameBranch`**(形已存在,被一句过期注释挡住) | `profiles.rs:6140` + 删 `6130` 的注释 | **P0**(零成本) |
| 丙2 | **`DirtyDot`(几何实心圆)取代字面量 `●`** —— 现为字体字符,违反 `marks.rs:654` 为 `⎇` 立的法;可直接复用 `ControlPill`,与 `status_dot_sprite`(`marks.rs:1113`)同路 | `seats.rs:12414,15233`;`float.rs:2029`;`file_peek.rs:905` | **P0**(近乎零成本) |
| 丙3 | `Search`(放大镜) | `profiles.rs:7138` `Find…` | P1 |
| 丙4 | `WindowNew` / `WindowPick` | `MoveToNewWindow` / `MoveToWindow ▸` | P1 |
| 丙5 | `Delete`/`Trash` | 删除分支 / 标签 / 丢弃 / 设置删行 | P1 |
| 丙6 | `Stop`(实心方块) | 预览 rail 载入中 | P2 |
| 丙7 | `Restart` | `Restart shell…` | P2 |
| 丙8 | `Compare`(两块 + ⇄) | `Compare with…` | P2 |
| 丙9 | `Hash` / `DevTools` / `Duplicate` / `NavBack` / `NavForward` | 见清单乙 | P2 |
| 丙10 | **`ChromeMark::design_stroke_units()` + 光学描边红门测试** —— 让病灶①⑤永久关闭 | `marks.rs` | **P0** |
| 丙11 | **动词 → 形 总表 + 反向索引测试** —— 让「一形多义」编译期可查 | 新文件或 `marks.rs` | P1 |
| 丙12 | **`bt-render/src/motion.rs`** 三档节奏常量 + 红门测试 | 新文件 | **P0**(动画块前置) |
| 丙13 | **接 `WM_SETTINGCHANGE`** 让 reduced-motion 不用重启 | `main.rs` | P1 |
| 丙14 | 清掉 `GitGraph` 上过期的 `#[allow(dead_code)]`(`marks.rs:763`)—— 它**已经被用了**(`git_panel.rs:459`) | `marks.rs:763` | P2 |

---

## 附:本目录文件

| 文件 | 内容 |
|---|---|
| `01-base.png` | 探针窗基线 |
| `02-panemenu.png` / `crop-panemenu.png` / `crop-menucol-a.png` | pane 菜单 —— 一形多义 + 光学不齐 + 风格混杂 |
| `03-split.png` / `crop-split.png` | 分屏后两个 pane 头 |
| `04-panehead.png` / `crop-panehead.png` | **pane 头 ⌄/🗀/✕ 三枚并排,用户主诉的实证** |
| `05-profilemenu.png` / `crop-profilemenu.png` | profile 选择器 |
| `06-termmenu.png` / `crop-termmenu.png` / `crop-termmenu2.png` | 终端右键菜单 —— `Find…` 空图标列、Restart 用 Refresh |
| `crop-caption.png` | 标题栏 —— 实心齿轮挨着三根发丝线 |
| `crop-strip.png` | 标签栏 ✕ / + / ⌄ 三种粗细 |
| `crop-ghost.png` | 独 pane ghost —— 实心夹 + 发丝箭头 |
| `stroke.py` / `stroke-table.txt` | 全表面实际描边计算 |
| `measure.py` / `m2.py` / `m3.py` | 截图墨迹像素测量 |
| `crop.py` | 截图裁切放大 |

**截图均摄于 `3cc2316`,早于 HEAD 五个提交;所引几何在 HEAD 未变,但截图不含 HEAD 新增的 `ProfileLine` 线稿族。**
