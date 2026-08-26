# Folio UI 只读审计：图标与动画

审计对象：`D:\Developer\BetterTerminal`，重点为 `crates/bt-app` 与 `crates/bt-render`。审计日期：2026-08-25。

## 范围、方法与证据边界

- 本报告是源码级只读审计；没有修改仓库、没有运行会写入 `target/` 的构建或测试。
- 优先检查了 `crates/bt-app/src/main.rs` 与 `crates/bt-app/src/seats.rs`，随后追踪图标定义、各表面调用和动画状态机。
- Folio 是原生终端窗口。当前自动化安全规则不允许控制终端类应用，因此本轮没有把运行时截图伪装成视觉实证。下文把“源码实证”和“光学推断”分开表述；光学问题应在后续人工截图复核。
- 下列已有工单不作为本报告病灶重复提交：Files 菜单 New-terminal 行图标风格、Git 面板 BRANCHES/COMMITS 标题块、Move to window 单窗行、浮窗脚、设置说明截断。
- `file:line` 采用本次工作树中的行号；未提交改动可能令后续行号漂移。

## 结论先行

图标系统“形状定义集中、语义分配分散”。大多数矢量形状已经集中在 `marks.rs`，最终由 `bt-render` 当作 RGBA 图集纹理绘制；但动作到图形的映射、实际像素尺寸、颜色和显隐规则散落在菜单、pane、tab、Git、预览、设置与浮窗代码中。主要问题不是缺少图标基础设施，而是没有统一的语义注册表和光学规格。

动画系统已经有较成熟的 reduced-motion 基础，但表面层不一致：float、toast、rail、FLIP 重排有过渡，菜单、设置、搜索、peek、若干 modal/card 和 tab 内容切换多为硬切；tooltip/key hint 只有进入没有退出。时长从 90 到 300 ms 各自生长，缺少“按语义角色命名”的全局节奏令牌。

优先级建议：

1. 先拆开被过度复用的 `External`、`PaneClose`、`Plus`、`Chevron` 与 `Code`，补齐缺形动作。
2. 再统一 16×16 action grid、1.25 stroke 和光学墨水框；品牌图标与 favicon 作为明确例外。
3. 为 popup/card 补一组 120 ms 进入、90 ms 退出的克制过渡；不让关闭动画持有已经销毁的 terminal/pane。
4. 把 motion 令牌与 reduced-motion 策略从 `main.rs` 抽成独立模块，并用静态可分辨性测试约束它。

# 一、图标审计

## 1. 图标管线与集中度

`ChromeMark` 是主要集中定义处：枚举在 `crates/bt-app/src/marks.rs:125-743`，稳定 ID 在 `marks.rs:745-819`，viewBox 在 `marks.rs:1835-1930`，SVG body 在 `marks.rs:1932-2226`。SVG 在应用侧栅格化（`marks.rs:1112-1368`），`bt-render` 接收 RGBA sprite 并作为 textured quad 上传/绘制（`crates/bt-render/src/lib.rs:3247-3298`、`bt-render/src/lib.rs:6553-6609`）。因此即使源形状是 stroke，GPU 最终看到的也是预乘透明纹理，不是实时矢量描边。

颜色规则也相对集中：除 profile/brand marks 外，图标使用 `currentColor`，例外列表见 `marks.rs:849-870`；调用点再从各表面的 palette 传色。favicon 可以替换 `Globe` 的矢量图，保留网站自带像素与颜色（`marks.rs:1230-1247`）。

集中度判断：

- **集中得好**：基础几何、viewBox、SVG、缓存 ID、currentColor/固定色的边界。
- **仍然散落**：动作→mark 映射主要在 `profiles.rs`、`seats.rs`、`git_panel.rs`、`git_graph.rs`、`settings.rs`、`float.rs`；实际尺寸与颜色由各 surface 自己决定。
- **绕开中心**：设置、Git 计数、preview dirty/breadcrumb 使用字体字符；它们没有固定网格、stroke 或 join/cap，随字体栅格化改变。
- **结构性缺口**：`ChromeMark` 是“形状表”，还不是“语义图标系统”。没有类似 `ActionIcon::MoveToWindow -> ChromeMark::WindowArrowIn` 的唯一映射层。

## 2. `ChromeMark` 全量盘点

下表覆盖 `ChromeMark` 的全部语义图标。尺寸是 SVG viewBox；“实际槽位”仍由表面调用决定。颜色若写“currentColor”，即来自该表面的 palette。

| 名称 / 形 | 定义与绘制规格 | 颜色来源 | 出现表面（代表性 `file:line`） |
|---|---|---|---|
| `Gear`，实心齿轮 | `marks.rs:129`、`marks.rs:1935-1936`；24×24；fill，无 stroke | currentColor | 窗口设置按钮 `seats.rs:9032`；设置页品牌/标题 `settings.rs:9019` |
| `WindowMinimize`，短横 | `marks.rs:132`、`marks.rs:1937-1938`；10×10；stroke 1.0 | currentColor | caption controls `seats.rs:9039` |
| `WindowMaximize`，圆角方框 | `marks.rs:134`、`marks.rs:1939-1940`；10×10；stroke 1.0，`rx=1.8` | currentColor | caption controls `seats.rs:9044` |
| `WindowClose` / `TabClose` / `PaneClose`，X | 枚举别名 `marks.rs:136-149`；同一 10×10 body `marks.rs:1941-1942`；stroke 1.0 | currentColor | 窗口 `seats.rs:9048`；tab `seats.rs:9742`；pane 头 `seats.rs:7735`；toast `toast.rs:1052`；notice `notice.rs:310`；search `search.rs:1334`；也被复用于删除/丢弃/离开比较，见病灶 P3 |
| `Plus`，十字 | `marks.rs:150`、`marks.rs:1943-1947`；10×10；stroke 1.2，round cap | currentColor | 新 tab `seats.rs:9800`；Git stage/load more `git_panel.rs:443-451`；create/stage/tag 菜单 `profiles.rs:6118-6134` |
| `Minus`，短横 | `marks.rs:610-625`、`marks.rs:2112-2113`；10×10；stroke 1.2 | currentColor | Git unstage `git_panel.rs:443-451`、`profiles.rs:6118-6134` |
| `Chevron`，V 形折线，可旋转 | `marks.rs:151-172`、`marks.rs:1966-1967`；10×10；stroke 1.2，round cap/join | currentColor | pane 头下拉 `seats.rs:7787`；通用下拉 `seats.rs:7576`；preview 后退/前进旋转 90°/270° `seats.rs:15450-15464`；search 上/下一个 `search.rs:1312-1320` |
| `TreeDisclosure`，实心三角，可旋转 | `marks.rs:180-191`、`marks.rs:2014-2017`；10×10；fill | currentColor | Files tree 展开 `seats.rs:14905`；菜单 submenu `profiles.rs:9002` |
| `ProfilePowerShell` | `marks.rs:192-195`、`marks.rs:1948-1953`；16×16；蓝底 fill，白线 stroke 1.35 | 固定 `#2C5C9E` + white | profile/session 菜单 `profiles.rs:841-992` |
| `ProfileUbuntu` | `marks.rs:196-212`、`marks.rs:1975-1987`；16×16；橙/白 fill，stroke 1.0/1.5 | 固定品牌色 | profile/session 菜单 `profiles.rs:841-992` |
| `ProfileGit` | `marks.rs:213-215`、`marks.rs:1988-1995`；16×16；橙 fill、白线 1.25、实心节点 | 固定品牌色 | profile/session 菜单 `profiles.rs:841-992` |
| `ProfileCmd` | `marks.rs:216-224`、`marks.rs:1996-2005`；16×16；深灰 fill，边 0.8，内线 1.35 | 固定 `#3A3A3A`/`#CCC` | profile/session 菜单 `profiles.rs:841-992` |
| `ProfileGeneric` | `marks.rs:225-249`；动态 SVG `marks.rs:1528-1547`；16×16；彩色 fill + 白线 1.35 | `MarkColour` + white | 未识别 profile `profiles.rs:1243`；设置 profile `settings.rs:3305` |
| `File`，折角纸张 | `marks.rs:250-251`、`marks.rs:1954-1958`；16×16；stroke 1.15 | currentColor | Files tree `seats.rs:14917-14954`；Git open diff `profiles.rs:6118-6134` |
| `Globe`，经纬线球体 | `marks.rs:252-286`、`marks.rs:2192-2197`；16×16；stroke 1.15 | currentColor，或 favicon 自带色 | web/session 项 `profiles.rs:4386-4411`；favicon 替换路径 `marks.rs:1230-1247` |
| `Folder`，闭合文件夹 | `marks.rs:287-288`、`marks.rs:1959-1960`；16×16；fill | currentColor | Files tree `seats.rs:14917-14954`；new in folder `profiles.rs:7954-7986`；tab/pane 上下文 `seats.rs:7481-7500` |
| `FolderOpen`，两层打开文件夹 | `marks.rs:173-179`、`marks.rs:2006-2013`；16×16；fill，背层 opacity .55 | currentColor | Reveal in folder / reveal in Explorer `profiles.rs:5422-5467`、`profiles.rs:6118-6134` |
| `Panel`，侧栏框 | `marks.rs:289-290`、`marks.rs:1961-1965`；16×16；stroke 1.1 | currentColor | recent/session window-like项目 `profiles.rs:4386-4411` |
| `Copy`，重叠圆角矩形 | `marks.rs:291-297`、`marks.rs:2047-2051`；16×16；stroke 1.3 | currentColor | file/terminal copy `profiles.rs:5422-5467`、`profiles.rs:7116-7125`；preview rail `seats.rs:15491`；也表示 duplicate pane `profiles.rs:7954-7986` |
| `Paste`，剪贴板 | `marks.rs:298-304`、`marks.rs:2052-2057`；16×16；外 stroke 1.3、内线 1.1、顶片 fill | currentColor | file/terminal paste `profiles.rs:5422-5467`、`profiles.rs:7116-7125` |
| `Split`，框内分隔 | `marks.rs:305-321`、`marks.rs:2119-2128`；16×16；stroke 1.1 | currentColor | pane split `profiles.rs:7954-7986`；Git compare `profiles.rs:6118-6134` |
| `SplitRight` / `SplitDown`，左右/上下双 pane | `marks.rs:322-336`、`marks.rs:2129-2142`；16×16；stroke 1.1 | currentColor | 设置 split direction `settings.rs:1501-1503` |
| `Float`，框 + 外发箭头 | `marks.rs:337-343`、`marks.rs:2018-2025`；16×16；stroke 1.2 | currentColor | pane float / move to new tab `profiles.rs:7954-7986`；preview float `seats.rs:15239-15309`；浮窗自身 `float.rs:1970-2101` |
| `External`，裸右上箭头 | `marks.rs:344-357`、`marks.rs:2187-2191`；16×16；stroke 1.2 | currentColor | Open with `profiles.rs:5428`；Move pane to new window `profiles.rs:7978`；Move to window `profiles.rs:7984`；preview open in browser `seats.rs:15505` |
| `DockLeft` / `DockRight`，窗框 + 实心侧板 | `marks.rs:358-374`、`marks.rs:2026-2031`、`marks.rs:2071-2076`；16×16；框 stroke 1.2，侧板 fill opacity .7 | currentColor | dock/float 布局动作，主要映射在 `float.rs:1970-2101` |
| `Check`，勾 | `marks.rs:375-381`、`marks.rs:2045-2046`；16×16；stroke 1.6，round | currentColor | 菜单选择/radio/check `profiles.rs:9447-9457` |
| `ResizeGrip`，角部弧线 | `marks.rs:382-392`、`marks.rs:2032-2044`；8×8；stroke 1.5，opacity .55 | currentColor | 浮窗/可缩放表面手柄 `float.rs:1970-2101` |
| `PinOff` / `PinOn`，图钉 | `marks.rs:393-408`、`marks.rs:1968-1974`；16×16；off stroke 1.25，on fill，整体旋转 45° | currentColor | tab pin `seats.rs:9598`；pin 宽度/透明度过渡由 `main.rs:23767` 控制 |
| `LockOff` / `LockOn`，开/闭锁 | `marks.rs:409-434`、`marks.rs:2199-2217`；16×16；shackle stroke 1.25，on body fill | currentColor | preview lock `seats.rs:15239-15309` |
| `PaneZoomIn` / `PaneZoomOut`，角括号收放 | `marks.rs:435-453`、`marks.rs:2218-2225`；16×16；stroke 1.25 | currentColor | pane zoom menu `profiles.rs:7954-7986` |
| `Save`，软盘 | `marks.rs:454-459`、`marks.rs:2058-2063`；16×16；外 stroke 1.3，内 stroke 1.15 | currentColor | preview head `seats.rs:15239-15309` |
| `Eye`，眼睛 | `marks.rs:460-468`、`marks.rs:2064-2068`；16×16；stroke 1.3 | currentColor | preview toggle `seats.rs:15239-15309` |
| `Code`，尖括号 | `marks.rs:469-471`、`marks.rs:2069-2070`；16×16；stroke 1.4 | currentColor | preview source/devtools `seats.rs:15239-15309`；Git Copy hash `git_graph.rs:4675` |
| `GitBranch`，节点与分支曲线 | `marks.rs:591-609`、`marks.rs:2077-2095`；16×16；stroke 1.15 | currentColor | checkout/branch actions `profiles.rs:6118-6134` |
| `GitGraph`，多轨提交图 | `marks.rs:696-707`、`marks.rs:2097-2111`；16×16；stroke 1.15 | currentColor | Git panel open graph `git_panel.rs:443-451` |
| `Tag`，标签 | `marks.rs:626-638`、`marks.rs:2143-2151`；16×16；outline 1.15 + 实心孔 | currentColor | Git create/tag `profiles.rs:6118-6134`、`git_graph.rs:4978` |
| `Refresh`，环形箭头 | `marks.rs:639-651`、`marks.rs:2152-2161`；16×16；弧 stroke 1.3 + fill 箭头头部 | currentColor | terminal restart/refresh `profiles.rs:7116-7125`；Git `git_panel.rs:443-451`；preview loading `seats.rs:15474-15480`；graph `git_graph.rs:4302` |
| `SelectAll`，虚线/角框选择 | `marks.rs:652-660`、`marks.rs:2162-2168`；16×16；stroke 1.3 | currentColor | terminal select all `profiles.rs:7116-7125` |
| `Broom`，扫帚 | `marks.rs:661-668`、`marks.rs:2169-2174`；16×16；stroke 1.3/1.2 | currentColor | terminal clear `profiles.rs:7116-7125` |
| `Eraser`，橡皮 | `marks.rs:669-674`、`marks.rs:2175-2180`；16×16；stroke 1.3 | currentColor | terminal erase/reset 类动作 `profiles.rs:7116-7125` |
| `Pencil`，铅笔 | `marks.rs:675-686`、`marks.rs:2181-2186`；16×16；stroke 1.3 | currentColor | file rename `profiles.rs:5422-5467`；**Rename branch 未映射** `profiles.rs:6123` |
| `GitMergeCurve`，长形合并曲线 | `marks.rs:687-695`、`marks.rs:2114-2118`；14×27；stroke 1.5 | currentColor | Git graph lane/merge 装饰 `git_graph.rs` |
| `GraphCurve`，动态二次曲线 | `marks.rs:708-742`，动态 SVG `marks.rs:1496-1519`；网格和物理 box、stroke 均动态，round cap | currentColor | Git graph 连线 `git_graph.rs` |

### 结构/状态绘制原语（不是独立动作图标）

| 原语 | 规格与用途 |
|---|---|
| `ActiveTab`、`TabBody` | fill 几何；tab 背景/选中态，`marks.rs:472-590`、动态生成 `marks.rs:1402-1562` |
| `TabBodyRing` | 动态 stroke 的 tab focus/active ring，同上 |
| `ControlPill` / `ControlPillRing` | 控件 hover/press 胶囊底与边；fill 或动态 stroke，同上 |
| `Foot` | 小型连接/脚部 fill 几何，同上；已知“浮窗脚”工单不在本报告重复展开 |
| `CardCorner`、`Fill` | card corner 与矩形填充，同上 |
| `ProgressRing` | 动态环段，通常约 2 px 物理 stroke；状态/等待动画由 `main.rs:16620-16630` 驱动 |
| `ChromeQuad` / `OverlayQuad` | `bt-render/src/lib.rs:3040-3114`、`bt-render/src/lib.rs:3300-3318`；承载栅格化图标/overlay 的位置、颜色与 opacity |
| `OverlayLayer` | `bt-render/src/lib.rs:3375-3502`；各层有 opacity，但多数 popup 目前没有把它接到 tween |

## 3. 绕过 `ChromeMark` 的字符图标

| 字符 / 语义 | `file:line` | 绘制、尺寸与颜色 | 风险 |
|---|---|---|---|
| `↺` restore | `settings.rs:476`，绘制 `settings.rs:10925-10939` | `ChromeLabel`/glyph，约 13 px，palette 文本色 | 与矢量 `Refresh` 同属环箭头但形不同、字重受字体影响 |
| `↑` / `↓` profile reorder | `settings.rs:483-484`，绘制 `settings.rs:9981-10047` | 字符 glyph，约 12.5 px，palette 文本色 | 无统一网格/stroke；与 Git ahead/behind 箭头形成另一套视觉语言 |
| `⋯` row more | `settings.rs:638`，绘制 `settings.rs:10521-10557` | 字符 glyph，约 12.5 px | 可以保留语义，但应成为受控的 vector mark |
| `✕` env remove | `settings.rs:647`，绘制 `settings.rs:10276-10291` | 字符 glyph，约 12.5 px | 与 `PaneClose` 的 10×10/1.0 stroke X 不同形、不同粗细 |
| `‹` / `›` breadcrumb | `settings.rs:631-634`；preview `seats.rs:12936`、`seats.rs:15637` | 字符 glyph | 与 `Chevron`、`TreeDisclosure` 再形成第三套折线/箭头语言 |
| Git ahead/behind `↑` / `↓` | `git_panel.rs:1741-1752` | 计数 pill 中的文本字符 | 状态文本可接受，但不要复用作按钮图标 |
| preview dirty `●` | `seats.rs:12396`、`seats.rs:15162-15170` | 文本 glyph，accent 色 | 作为状态点合理；应明确列为 status-only 例外 |

## 4. 图标病灶

### P1 — pane 头 `⌄` 与 `✕` 线粗不一致（高）

源码实证：同一 pane-head 控件组内，关闭使用 `PaneClose`（`seats.rs:7735`），下拉触发使用 `Chevron`（`seats.rs:7787`）。X 的 SVG stroke 是 1.0（`marks.rs:1941-1942`），chevron 是 1.2 且 round cap/join（`marks.rs:1966-1967`）；pane 主题槽位又分别约为 8 px 与 13 px（`theme.rs:2712`、尺寸组装 `seats.rs:5414`）。它们不仅名义 stroke 不同，缩放后的物理线宽也不会自然相等。

光学判断：大槽位的 `⌄` 会比小槽位的 `✕` 更显粗、更占面积。应按同一 header-control family 以最终物理像素校准，而不是照搬 viewBox stroke。

### P2 — `External` 一形多动词（高）

同一枚裸右上箭头同时表示 Open with（`profiles.rs:5428`）、Move pane to new window（`profiles.rs:7978`）、Move to window（`profiles.rs:7984`）和 preview Open in browser（`seats.rs:15505`）。其中“打开外部资源”和“移动 pane 到容器”是不同对象、不同结果；用户给出的两条 Move 菜单实证确实共用 `External`。

建议：裸箭头只保留 `OpenExternal`；移动动作必须带容器轮廓：`TabArrowOut`、`WindowArrowOut`、`WindowArrowIn`。这条诊断针对语义复用，不重复报告已在修的“单窗行”显示问题。

### P3 — `PaneClose` 的 X 兼任关闭、删除、丢弃和退出比较（高）

X 本来出现在窗口/tab/pane/toast/notice/search 的关闭动作（例如 `seats.rs:9048`、`seats.rs:9742`、`toast.rs:1052`），但 Git 菜单把 delete/discard 映射到 `PaneClose`（`profiles.rs:6124`），Git graph 的 Leave compare 也用 `PaneClose`（`git_graph.rs:4677`）。关闭 transient surface、删除对象、丢弃修改、退出模式具有不同风险与可逆性，不应只靠菜单文字消歧。

建议新增 `Trash`、`RevertFile`、`CompareExit`，X 只做关闭。

### P4 — `Plus`、`Copy`、`Split`、`Code` 也发生语义过载（高）

- `Plus` 同时是 new tab（`seats.rs:9800`）、Git stage、create 与 load more（`git_panel.rs:443-451`、`profiles.rs:6118-6134`）。load more 尤其不应是“新增”。
- `Copy` 同时表示复制内容和 duplicate pane（`profiles.rs:5422-5467`、`profiles.rs:7954-7986`）。
- `Split` 同时表示拆分 pane 和比较版本（`profiles.rs:6133`、`profiles.rs:7964`）。
- `Code` 同时表示查看源码/开发工具与 Copy hash（`seats.rs:15239-15309`、`git_graph.rs:4675`）。

应分别补 `Stage`/`Unstage`、`MoreDown`、`DuplicatePane`、`Compare`、`Hash`、`DevTools`，让基本图形不再承担冲突动词。

### P5 — 同一“展开/导航”动作存在三套形（中高）

Files tree 与菜单 submenu 使用实心 `TreeDisclosure`（`seats.rs:14905`、`profiles.rs:9002`），设置 Advanced 使用 `Chevron` 的端点/旋转状态（`settings.rs:9808`），设置与 preview breadcrumb 又用字符 `‹/›`（`settings.rs:631-634`、`seats.rs:15637`）。preview 前进/后退甚至复用下拉 `Chevron` 并旋转（`seats.rs:15450-15464`）。

建议只保留一套 outline chevron family 用于 disclosure，另做有杆的 `ArrowLeft/Right` 用于历史导航；breadcrumb 使用同一 chevron 的小尺寸导出，不用字体字符。tree 可以通过缩进和旋转表达层级，不必另用实心三角。

### P6 — 同动词异形：remove/close 与 restore/refresh（中）

设置环境变量 remove 用字体 `✕`（`settings.rs:647`、`settings.rs:10276-10291`），其他关闭用矢量 X（`marks.rs:1941-1942`）。设置 restore 用字体 `↺`（`settings.rs:476`、`settings.rs:10925-10939`），terminal/Git/preview 的 restart/refresh 用矢量 `Refresh`（`marks.rs:2152-2161`）。

建议把设置字符迁回 `ChromeMark`；restore 若语义为“恢复默认”，应是带时钟/回退含义的 `Restore`，不要与网络/数据刷新合并。

### P7 — 实心彩色、实心单色与细线单色混在同一栏（中）

Profile/brand marks 固定彩色且 filled（例外规则 `marks.rs:849-870`），Folder/FolderOpen 也是 filled（`marks.rs:1959-1960`、`marks.rs:2006-2013`），而 File、Copy、Split、External 等 action/object 多为 1.1–1.3 的细线。在同一菜单或树行内，filled folder 的墨量会明显大于线性 action icon。

判断：品牌与 favicon 固定彩色是合理例外；问题在于普通 utility/action 列没有 fill policy。建议 action 一律单色 outline；fill 仅用于 selected/on、状态点和品牌。Folder 若作为对象类型可保留 fill，但进入 action 菜单时提供 outline action 版本。

### P8 — 光学尺寸不齐，10/16/24/8 与调用槽位交叉（中高）

源网格横跨 8、10、14×27、16、24 和动态 graph grid。菜单对 10-grid 边缘图标与 16-grid 图标采用不同槽位（`profiles.rs:85-136`）；同一 X 在 tab 约 8 px（`theme.rs:2271`）、pane 约 8 px（`theme.rs:2712`）、float close 约 9 px（`float.rs:437`）、window 约 10 px（`theme.rs:2147`）。同一 `External` 在普通菜单与 preview rail 又使用不同实际尺寸（`profiles.rs:85-136`、`seats.rs:12403`）。

多尺寸本身不是错；缺的是统一的 optical ink box、目标物理 stroke 与逐尺寸导出规则。目前“viewBox stroke × 任意调用缩放”会放大差异。

### P9 — 明确缺形或已有形未映射（中高）

- Find 菜单明确没有 mark：`profiles.rs:7121`。
- Rename branch 没有 mark：`profiles.rs:6123`，但 `Pencil` 已存在于 `marks.rs:675-686`，属于映射缺口。
- Move-to-existing/new-window/new-tab、Duplicate pane、Compare、Stage/Unstage、Load more、Delete、Discard/Revert、Copy hash、DevTools、Restore defaults 均缺唯一语义形；现在依赖复用。

## 5. 统一图标规格草案

### 网格与光学盒

- Utility/action/object 主网格统一为 **16×16**；目标墨水框 **12×12**，允许圆弧或斜线最多越界 0.5。
- Window caption 的 10×10 可作为平台特例保留，但从同一 master geometry 导出；不可直接把 10-grid mark 任意放大混入 16-grid action family。
- 默认展示槽位：菜单 14 px、toolbar 14 px、header compact 12 px、window caption 10 px。每个槽位都有固定光学校准，不再由调用点自由指定。

### 描边、端点与圆角

- 16-grid 基准 stroke：**1.25**；密集内部结构可降至 **1.125**，高强调 check 可升到 **1.5**。
- 10-grid caption stroke：**1.0**；若与 16-grid action 并排，按最终物理像素校准到同一视觉重量。
- action 的斜线、箭头、chevron 统一 round cap / round join；框体统一 round join。
- 16-grid 外框半径 2，内 pane/小框半径 1.5；图标内部圆角不借用按钮背景圆角。

### 颜色与填充

- Utility/action 默认 `currentColor` 单色 outline。
- filled 只用于 selected/on 状态、状态点、品牌与 favicon；普通危险动作不靠红色 fill 单独编码，必须同时有独特形。
- 品牌 marks、favicon 保留自有颜色，但用统一 16 px chassis 与 optical inset。
- disabled/secondary 通过 palette token 改色；不要在 SVG body 内写任意 opacity，`FolderOpen` 背板、`ResizeGrip` 等现有例外应转成命名状态层。

### 每动词一形：建议分配

| 动词 | 唯一图形 | 处置 |
|---|---|---|
| Close surface | `X` | 合并 `WindowClose/TabClose/PaneClose` 几何，保留语义别名与尺寸导出 |
| Delete | `Trash` | 新增，不再用 X |
| Discard/Revert | `FileRevert` | 新增，不再用 X |
| Add/New | `Plus` | 保留，只用于真正创建 |
| Stage / Unstage | `FilePlus` / `FileMinus` 或受控 Git 节点版 | 新增 |
| Load more | `ChevronDownBar` 或 `MoreDown` | 新增，不再用 Plus |
| Disclosure | 单一 `Chevron`，按方向旋转 | 合并 TreeDisclosure、Advanced、submenu |
| Back / Forward | `ArrowLeft` / `ArrowRight`（有杆） | 新增，不复用下拉 chevron |
| Open external | 裸 `ArrowUpRight` | `External` 改名/收窄语义 |
| Move to new tab | `TabArrowOut` | 新增 |
| Move to new window | `WindowArrowOut` | 新增 |
| Move to existing window | `WindowArrowIn` | 新增 |
| Float pane | `PanelArrowOut` | 由现有 `Float` 专用，不和 move-to-tab 混用 |
| Copy | 双纸张 `Copy` | 保留 |
| Duplicate pane | `OverlappingPanelsPlus` | 新增 |
| Split | pane 框内分隔 `Split` | 保留 |
| Compare | 双向箭头/双文档 `Compare` | 新增 |
| Find | `Magnifier` | 新增 |
| Rename file/branch | `Pencil` | 复用已有形并补 branch 映射 |
| Refresh | 环形双箭头 | 保留 `Refresh` |
| Restore defaults | 回退时钟 `HistoryRestore` | 新增，替代字符 `↺` |
| Source | `CodeBrackets` | `Code` 收窄语义 |
| DevTools | `TerminalPrompt` 或 wrench/braces | 新增 |
| Copy hash | `Hash` | 新增 |
| Checkout branch | `BranchSwitch` | 新增；`GitBranch` 保留作对象/状态 |
| Create branch | `BranchPlus` | 新增 |
| Create tag | `TagPlus` | 新增；`Tag` 保留作对象 |

# 二、动画审计

## 1. 既有运动基础与 reduced-motion 规矩

系统 motion 模式读取 Windows client-area animation 设置，区分 Full/Reduced（`main.rs:16556-16600`）。已有两条曲线：`EASE_IN_OUT=[.42,0,.58,1]` 与 `EASE=[.25,.1,.25,1]`（`main.rs:16483-16486`），另有更利落的 `GRAB_EASE=[.2,0,0,1]`（`main.rs:17481-17542`）。

现有 reduced-motion 规矩值得保留：

- working/breathe 在 Reduced 下不是消失，而是固定 opacity .6（`main.rs:16488-16523`；周期常量 `theme.rs:2245-2252`）。
- `wait_halo_opacity` 在 Reduced 下为 0，但 warning border 仍提供静态可分辨状态（`main.rs:16526-16553`）。
- indeterminate progress 在 Reduced 下固定为顶部弧段，而不是持续旋转（`main.rs:16620-16630`；周期 `theme.rs:2253-2254`）。
- 通用 `RevealTween` 在 Reduced 下直接采样终态且不继续请求 frame（`main.rs:17343-17464`）。

这套原则应提升成全局契约：**减少运动不等于移除状态；每个只靠运动表达的状态都必须有静态形/色/文案替代；Reduced 下 decorative tween 不留 frame deadline。**

## 2. 出现、消失、状态切换全量盘点

总 overlay 顺序集中在 `main.rs:31883-32139`：preview/terminal bars、command rail、rail/card flight、pane veil/dock preview、search、notice、web sheet、modal、layout peek、float、各菜单、toast、key hint、tooltip、file peek、drag ghost、window ring。下表按用户可见表面合并同机制项。

| 表面 / 状态 | 当前过渡、时长、缓动 | `file:line` | 结论 |
|---|---|---|---|
| Profile/root/preview/file/Git/terminal/pane/graph-filter 菜单 | 无 enter/exit tween；状态直接 `Some/open` 与 `take/close`。submenu 只有 dwell，视觉仍硬切 | 统一 popup close `main.rs:33093-33112`；profile open `main.rs:33045-33052`；root `main.rs:49229-49253`；preview `main.rs:50035-50061`；file `main.rs:50189-50217`、`main.rs:50369-50376`；Git `main.rs:50450-50457`；terminal `main.rs:51509-51529`、`main.rs:51591-51598`；pane `main.rs:52088-52106`、`main.rs:52301-52308`；filter `main.rs:53132-53156` | **硬切**；这是最大的一组节奏缺口 |
| 菜单 submenu | 250 ms hover dwell，300 ms safe hold；没有 opacity/位移 | `profiles.rs:7568`、`profiles.rs:7583`、`profiles.rs:8296`；状态切换 `main.rs:51690-51712`、`main.rs:52315` 附近 | 延迟控制完善，但出现本身突兀 |
| Settings modal 开/关 | scrim/dialog 直接构建与移除，无 tween | toggle `main.rs:33341-33370`；绘制 `settings.rs:8959-8982` | **硬切**；适合 120/90 ms fade，dialog 仅 4 px 微位移 |
| Settings Advanced 开合 | chevron 端点/方向与行内容直接切换，无高度/opacity tween | `settings.rs:9808`；状态更新 `main.rs:33555-33559` 附近 | **硬切**；Reduced 下仍须用方向与文案静态可分 |
| Quit/dirty gate/PSReadLine/restore/profile 等 modal/card | overlay 分支直接选择，无统一 per-layer tween | overlay 选择链 `main.rs:31950-32039` | **硬切**；确认类 modal 可统一 popup family |
| Search bar 出现/消失 | 直接打开/关闭，无 enter/exit | `main.rs:29971`、`main.rs:30016`；绘制 `search.rs:1307-1334` | **硬切**；应与 command rail 对齐 |
| Notice / web sheet | overlay 条件式加入，无统一 transition | `main.rs:31950-32039`；notice close `notice.rs:310` | **硬切**；notice 可用 toast-like fade，sheet 用 panel role |
| Layout peek strip | 350 ms 延迟；源码明确采用 hard display switch，无视觉 transition | `peek_strip.rs:15-25`、`peek_strip.rs:59-68` | **硬切（明示）**；延迟后仍跳现 |
| File peek | 350 ms 延迟后直接替换/移除 | `main.rs:48186-48220`，尤其 `main.rs:48216-48217` | **硬切**；可与 tooltip/card 共用 120/90 ms |
| Float 窗/卡 | enter/exit 120 ms，`EASE`；opacity + 5 px rise；Reduced 直接终态 | 常量 `float.rs:89-90`；tween `float.rs:974-1008`；主题尺度 `theme.rs:2825-2830` | 已有；位移略大于建议的 4 px，但节奏合理 |
| Float hover-intent / close grace | 180 ms intent，220/420 ms grace；这是等待规则，不是视觉动画 | `float.rs:67-90` | 不要误并入 motion duration token |
| Toast | enter 120 ms `EASE` + 8 px slide；exit 90 ms fade；life 6 s/4 s；Reduced 通过 still 模式 | `toast.rs:54-91`、`toast.rs:347-390`；调用 `main.rs:28877-28884` | 完整但 8 px 比 float/建议族更跳；可收至 4 px |
| Tooltip | delay 380 ms（command card 120 ms）；fade-in 90 ms `EASE`；关闭直接移除，无 fade-out | `tooltip.rs:31-70`、`tooltip.rs:742-751`、`tooltip.rs:839-848` | **进入/退出不对称** |
| Key hint | delay 800 ms；fade-in 90 ms；源码明确无 exit fade；Reduced 直接终态 | `keyhint.rs:41-63` | **进入/退出不对称**；长 delay 合理，不应当作节奏不一致 |
| Scrollbar | dwell 900 ms 后 fade 180 ms `EASE`；Reduced 在 dwell 后 snap | `termscroll.rs:146-168`、`termscroll.rs:412-420`、`termscroll.rs:438` | 合理的低优先级 chrome 退场 |
| Command rail tick/selection | width 100 ms、background 140 ms、opacity 120 ms `EASE`；jump flash 950 ms | `cmdrail.rs:197-215`、`cmdrail.rs:246-251` | 局部节奏过细分；950 ms flash 在 `cmdrail.rs:1709-1721` 没有明确 Reduced 分支，需补静态可分审查 |
| Rail 展开/标签 | rail 180 ms `EASE`；label 100 ms，打开延迟 60 ms | `main.rs:17466-17479`；主题常量 `theme.rs:2435-2446` | 已有且克制；可归入 panel/fast role |
| Disclosure chevron 转向 | 140 ms `GRAB_EASE`；Reduced 瞬时终态 | `main.rs:17481-17542` | 合理；但图标本身应先统一 |
| Files disclosure triangle | 120 ms `GRAB_EASE` | `files.rs:376-394`；调用尺度 `seats.rs:12239` | 与全局 chevron tween 重复实现，宜合并 |
| Tab 激活/内容切换 | active tab 状态直接赋值，terminal 内容立即切；无 crossfade | `main.rs:26693-26697` | **硬切**；内容立即切是对的，可只给 active chrome 90 ms 色/底过渡 |
| Tab reorder / landing | FLIP 160 ms + landing wash 200 ms，`GRAB_EASE` | `main.rs:17545-17678` | 已有；结构运动完整 |
| Tab pin reveal | width 160 ms、opacity 120 ms | `theme.rs:2258-2265`、`main.rs:23767` | 已有；与 header 其他控件显隐不一致 |
| Pane reorder/new/resize | pane FLIP 200 ms；new pane fade 180 ms `EASE`；resize card 100 ms | `main.rs:17681-17734` | 已有；结构动作合理 |
| Pane close | dead pane 立即移除；survivors 做 FLIP，没有 dead-pane exit | `main.rs:18068-18086` | 不应为了动画持有已销毁 terminal；保持此安全策略 |
| Pane-head hover controls | close/chevron/folder/float 一起条件加入，源码明确不做 transition | `seats.rs:7701-7711`、具体 marks `seats.rs:7735-7803` | **硬切**，且与 tab pin/preview tools 节奏不一致 |
| Preview head/rail hover tools | 主要依赖即时 palette/opacity 状态，工具直接出现 | `seats.rs:15181-15310`、`seats.rs:15391-15507` | **多为硬切**；可复用 fast opacity role |
| Dock preview | fade 100 ms | `main.rs:20014` | 已有；可归入 fast role |
| Drag ghost | 条件成立即绘制；drop/landing 时消失，没有 tween；位置实时跟 pointer | `main.rs:19969-19971`、`main.rs:32601-32640` | 出现/消失偏硬，但位置绝不能加缓动；只可在过阈值时 70–90 ms opacity |
| Git commit/detail 卡展开 | 状态 clone/替换直接发生，无 transition | `main.rs:46048-46055`、`main.rs:47008-47013`、`main.rs:54888-54889` | **硬切**；若高度变化大，用内容 fade，不建议慢高度动画 |
| Working/breathe、wait halo、progress ring | 1.7 s breathe；halo Reduced=0；spinner 1.1 s，Reduced 固定弧 | `main.rs:16488-16630`、`theme.rs:2245-2254` | reduced-motion 标杆，应保留 |
| Value/progress sweep | 300 ms `EASE`，现有设计在 Reduced 下仍保留数值到达过程 | `main.rs:17309-17340` | 这是“数值变化”而非装饰位移；可保留，但需在 motion 文档明确例外 |
| Window ring/hover/press 等即时状态 | overlay 最后绘制，颜色/存在主要随状态即时切 | `main.rs:32120-32139` | 即时 hover/press 可以不动画；focus/active ring 必须静态清楚 |

## 3. 动画病灶

### M1 — popup/card family 两极分化（高）

Float 与 toast 已有 120 ms enter（`float.rs:974-1008`、`toast.rs:347-390`），但所有菜单由 `Some/open` 与 `take/close` 直接增删（`main.rs:33093-33112`、各菜单引用见上表），settings、search、peek 和 modal/card 也硬切。相似体量与层级的表面采用不同节奏，用户会感到有的“贴上来”，有的“闪出来”。

### M2 — tooltip/key hint 只有淡入，没有淡出（中高）

Tooltip fade-in 90 ms 后直接移除（`tooltip.rs:742-751`、`tooltip.rs:839-848`）；key hint 更在源码中明确无 exit fade（`keyhint.rs:41-63`）。这会让出现显得精致、消失却像逻辑分支跳变。建议退出统一 90 ms opacity；pointer 命中区域可以立即关闭，视觉残影放在非交互 overlay 中。

### M3 — pane/tab/preview chrome 的显隐节奏不一致（中高）

Tab pin 有 160/120 ms reveal（`theme.rs:2258-2265`、`main.rs:23767`），pane-head 控件明确无过渡（`seats.rs:7701-7711`），preview tools 多为即时 opacity/加入（`seats.rs:15181-15310`）。它们都属于 hover-dependent secondary chrome，却没有共同的 fast role。

### M4 — tab 内容硬切与结构动画没有分层说明（中）

Tab 激活直接切换状态（`main.rs:26693-26697`），tab reorder 却有完整 160/200 ms FLIP/landing（`main.rs:17545-17678`）。不建议对 terminal 像素内容做交叉淡化，那会制造输入延迟和双影；但 active tab 的背景、ring、label 色可以 90 ms 过渡。把“内容立即、chrome 微过渡”写成规则即可消除突兀而不拖慢终端。

### M5 — 时间值过多，按实现而非语义命名（中）

目前存在 90、100、120、140、160、180、200、300 ms，以及多个 120/180/250/350/380/800/900 ms 延迟。很多数值单独看都合理，但散在 `main.rs`、`theme.rs`、`float.rs`、`toast.rs`、`tooltip.rs`、`keyhint.rs`、`cmdrail.rs`、`files.rs`、`termscroll.rs`。同类表面无法自动保持节奏，也难以统一 Reduced 行为。

### M6 — Reduced 契约尚未覆盖所有局部动画（中高）

核心 `RevealTween`、wait halo、progress ring 已做到静态可分（`main.rs:16488-16630`、`main.rs:17440-17453`），但局部 `cmdrail` jump flash 950 ms 的实现没有明显 Reduced 分支（`cmdrail.rs:1709-1721`）。Files disclosure、tooltip/keyhint 等虽各自处理了一部分，策略仍是分散的。新增统一过渡前必须先把“终态采样、不留 deadline、状态仍可辨”作为测试契约，而不是简单把 duration 设为 0。

## 4. 克制过渡族建议

### 全局角色

| 角色 | 时长 / 曲线 | 用途 |
|---|---|---|
| `Fast` | 90 ms，`EASE` | opacity、颜色、ring、hover secondary chrome；tooltip/menu exit |
| `Popup` | enter 120 ms `GRAB_EASE`；exit 90 ms `EASE` | menu、tooltip、file peek、轻量 card；进入 `opacity 0→1 + y:4→0 px`，退出只 fade |
| `Panel` | 180 ms，`EASE` | settings dialog、rail、notice/web sheet、new pane；最多 4 px 位移 |
| `Structural` | 160–200 ms，`GRAB_EASE` | tab/pane FLIP、布局重排；几何保持现有实测值 |
| `Value` | 300 ms，`EASE` | progress/value sweep；不用于 popup |

延迟不并入节奏角色：command preview 120 ms、float intent 180 ms、submenu 250 ms、layout/file peek 350 ms、tooltip 380 ms、key hint 800 ms、scrollbar dwell 900 ms 都是意图/可发现性规则，应保留独立命名。

### 各表面落地

- 菜单、tooltip、file peek：`Popup`；enter 淡入 + 4 px 微位移，exit 90 ms 只淡出。submenu 从父项方向横移 3 px，不做缩放。
- Settings/modal：scrim 120 ms opacity；dialog 180 ms opacity + 4 px。关闭时交互立即禁用，视觉层完成 fade 后丢弃。
- Search、pane-head、preview tools、active-tab chrome：`Fast` opacity/color；不移动 terminal 内容。
- Toast：保留 120/90 ms，把 8 px 降为 4 px，与 float/popup 共用位移尺度。
- Drag ghost：位置逐帧跟 pointer，不缓动；越过 drag threshold 时只做 70–90 ms opacity，drop 时立即交给现有 landing 动画。
- Pane close：不新增 dead-pane exit；terminal 销毁后只动画 surviving panes，维持 `main.rs:18068-18086` 的安全边界。
- 大高度内容（Git commit、Settings Advanced）：优先 90 ms 内容 crossfade/clip reveal；不做 300 ms 高度弹簧，避免终端周边布局漂浮。

### Reduced-motion 行为

- `Fast/Popup/Panel/Structural` 的位移、旋转、FLIP 在 Reduced 下直接采样终态，不登记下一帧 deadline。
- 状态不可只靠旋转、脉冲或位移表达：disclosure 仍有最终方向；working 仍有固定 `.6` opacity；wait warning 仍有 border；progress 仍有固定弧。
- 短 opacity 若只是避免闪烁，也默认静态终态；不要把“淡入很轻”当作绕过 reduced-motion 的理由。
- `Value` 是否保留由语义决定：现有 300 ms sweep 可作为“数值到达”例外，但必须有静态最终数字/条形，且不循环。
- 新增测试建议：Reduced 模式下所有 decorative tween 的 `frame_deadline == None`；每个 working/warning/loading state 对比静态截图仍有至少一个非运动差异。

## 5. 全局参数应放在哪里

建议新增单一 `crates/bt-app/src/motion.rs`（本报告不执行修改），集中：

- `Motion::{Full, Reduced}` 与系统设置读取后的策略接口；
- `MotionRole::{Fast, Popup, Panel, Structural, Value}`；
- `EASE`、`EASE_IN_OUT`、`GRAB_EASE` 与 cubic-bezier 求值；
- `RevealTween`、FLIP/chevron 等通用采样器；
- Reduced 下“终态采样 + 无 frame deadline”的测试辅助。

`bt-render::theme` 只保留视觉尺寸、颜色、圆角等 theme tokens，不再成为动画时长的第二来源。`float.rs`、`toast.rs`、`tooltip.rs`、`keyhint.rs`、`files.rs`、`cmdrail.rs`、`termscroll.rs` 通过 motion role 请求时长与曲线；意图延迟仍留在所属功能模块，但用明确名称区分 `HOVER_INTENT_DELAY` 与 `POPUP_ENTER_DURATION`。

# 三张实施清单初稿

## 重画

- [ ] pane-head `Chevron` / `PaneClose`：按最终物理像素统一 stroke 与 optical ink box（`seats.rs:7735`、`seats.rs:7787`）。
- [ ] `External` 拆为 OpenExternal、TabArrowOut、WindowArrowOut、WindowArrowIn。
- [ ] `PaneClose` 派生用法：delete、discard/revert、leave compare 改为专形。
- [ ] `Plus` 派生用法：stage/unstage、load more、create branch/tag 改为专形。
- [ ] `Copy`/`Split`/`Code` 派生用法：duplicate pane、compare、copy hash、devtools 改为专形。
- [ ] disclosure/navigation：实心 triangle、V-chevron、字符 `‹/›` 统一成 disclosure chevron + navigation arrow 两族。
- [ ] Folder/FolderOpen 的 action 版本改为 outline；品牌/profile/favicon 保持彩色例外。
- [ ] 字符 `↺ ↑ ↓ ⋯ ✕ ‹ ›` 中所有可点击动作改成 vector mark；`●` 仅保留 status-only。
- [ ] 10/16/24/8 grid 的显示导出做 optical normalization，不再任意缩放 master SVG。

## 合并

- [ ] `WindowClose` / `TabClose` / `PaneClose` 合并为一个 X master geometry，保留语义别名和尺寸导出。
- [ ] `TreeDisclosure`、Settings Advanced、submenu disclosure 合并为单一可旋转 chevron family。
- [ ] Settings profile up/down 与其他可点击排序箭头合并为 vector `ArrowUp/Down` family。
- [ ] ProfilePowerShell/Ubuntu/Git/Cmd/Generic 使用共同 16 px chassis、inset 与圆角政策，品牌内部图案保留。
- [ ] 动作→图形映射集中为 `ActionIcon` 注册表；菜单、pane、tab、Git、preview 不再各自选择 `ChromeMark`。
- [ ] 实际 icon slot、颜色角色、hover/disabled/selected policy 集中，调用点只传 semantic state。
- [ ] `RevealTween`、Files disclosure、float/toast/tooltip 等 duration/easing 收敛到 `motion.rs` 的角色令牌。

## 新增

- [ ] `Magnifier`（Find）。
- [ ] `Trash`（Delete）。
- [ ] `FileRevert` / `HistoryRestore`（Discard、Restore defaults）。
- [ ] `TabArrowOut`、`WindowArrowOut`、`WindowArrowIn`（pane move family）。
- [ ] `DuplicatePane`、`Compare`。
- [ ] `Stage`、`Unstage`、`MoreDown`。
- [ ] `Hash`（Copy hash）、`DevTools`、`ArrowLeft/Right`（历史导航）。
- [ ] `BranchSwitch`、`BranchPlus`、`TagPlus`。
- [ ] Rename branch 到现有 `Pencil` 的语义映射。
- [ ] `MotionRole::{Fast, Popup, Panel, Structural, Value}` 与统一 Reduced 策略测试。
- [ ] menu/settings/search/file-peek/modal 的 popup transition state；tooltip/key hint 的 exit state。
- [ ] 人工视觉复核截图集：100%/125%/150% DPI、浅/深色、Full/Reduced，重点量 pane-head、菜单 icon column、品牌/文件夹混排与 popup 首末帧。
