# 图标统一块 + 动画补齐块 — 定稿方案(v1 · **用户 2026-08-26 全部裁定**)

2026-08-25。双引擎审计合成:`audit-claude-2026-08-25.md`(实机逐像素测量)× `audit-codex-2026-08-25.md`(源码全量盘点)。两份独立得出同一结构结论,分歧处本文裁定。原始证据(截图/量测脚本/stroke-table)在 scratchpad `ui-audit/`,截图已随报告引用。

## 0. 两份审计的交叉校验

**双方独立同报**(可信度最高):External 一形四义、✕ 兼关闭/删除/丢弃/停止、Plus/Copy/Split/Code 过载、Chevron 兼下拉与导航、Find/RenameBranch 缺形、「动词→形」无总表、尺寸由 40+ 调用点各自决定、8 个菜单族全部硬切、tooltip/keyhint 有进无退、时长散档无令牌。

**互补发现**:
- Codex 独有:settings 页 7 处字体字符冒充图标(`↺ ↑ ↓ ⋯ ✕ ‹ ›`,settings.rs:476/483/631/638/647)——**Claude 报告「字符已扫干净」的结论被证伪**,以 Codex 为准;`Plus` 还兼 load-more;`PaneClose` 还兼退出比较;三套「展开」语言并存(实心三角/chevron/字符 ‹›);拖拽 ghost 位置绝不缓动的红线;pane 关闭不为动画持尸的安全边界。
- Claude 独有:实测 pane 头 ⌄ 1.56px vs ✕ 0.80px = **1.95×**,机制根因=`i-chev` 的 **10×6 非方 viewBox**;`Minus` 1.80/13.5 全菜单最重、**违反 marks.rs:677 的 Plus/Minus 同重书面契约**(P0);「未保存」`●` 是字体字符(违反 R4 为 ⎇ 立的法);gear 是唯一 Material 24 格实心外来件;12 枚缺 linecap+4 枚枚内矛盾;`motion` 只在启动读一次、不听 `WM_SETTINGCHANGE`;toast/光标闪烁漏接 `Motion`。

**分歧裁定**:
| 题 | Claude | Codex | 裁定 |
|---|---|---|---|
| 基准 stroke | 1.2(跟随已落地的 `PROFILE_LINE_STROKE_UNITS` 裁决) | 1.25 | **1.2** — 落地裁决优先 |
| 菜单进场时长 | 140(chevron 转向已是 140,一个动作两半) | 120(float/toast 现值) | **进 140 / 出 90**;float 归 Panel 档 |
| 尺寸治理 | `design_stroke_units()` + 光学红门测试 | 槽位表(菜单 14/工具栏 14/紧凑 12/标题 10) | **两者都要**:槽位表定框、红门测试守 1.05±0.10 |
| 语义层 | 动词→形总表+反向索引测试 | `ActionIcon` 注册表 | 同一件事,**做注册表 + 测试** |

## 1. 图标块施工序(P0 先行)

**P0(第一片,机制性,做完病灶永不复发):**
1. `marks.rs` 加 `design_stroke_units()`;**光学描边红门测试**:遍历所有 (mark, 槽位框),断言实际描边 ∈ [0.95, 1.15](显式豁免表:纯填充形/品牌标)。
2. **槽位表**:菜单 14 / 工具栏 14 / 紧凑头 12 / 标题栏 10,调用点只报槽位不报像素;`item_mark_logical_px` 的枚举名单改为查几何属性(edge-to-edge 族标注在表上)——顺手治 `Minus`(P0 契约违反)、Chevron/ResizeGrip/TreeDisclosure 漏网。
3. **`ActionIcon` 注册表**(动词→形唯一映射)+ 反向索引测试(一形多义编译期可查);14 个 `fn mark()` dispatcher 改查表。
4. 零成本三件:`RenameBranch`→`Pencil`(删过期注释)、`GitGraph` 的过期 `#[allow(dead_code)]`、`●`→几何 `DirtyDot`(复用 ControlPill,与 status_dot_sprite 同路)。

**P1(第二片,重画+拆分):**
5. 重画:`i-gear`(24 实心→16 格 1.2 描边)、`i-chev`(10×6→16 方格,1.95× 差的根因)、`i-minus`(随 plus 进 16)、全表 stroke 归 1.2(1.6/1.4/1.3/1.25/1.15/1.1 各族)、12 枚补 round linecap、圆角收外 2.0/内 1.2、五枚墨迹越界收进 1.6–14.4 气口。
6. 拆分+新形:External 拆三(`External` 交外部 / `WindowNew` / `WindowPick`);✕ 拆三(Close / `Trash` / `Stop`,统一三个变体名);`Refresh`/`Restart`;`Code`/`DevTools`/`Hash`;`Split`/`Compare`;`Copy`/`Duplicate`;`Chevron`/`NavBack`/`NavForward`;新 `Search`(Find);`Plus` 收窄(load-more 改 `MoreDown`,stage/unstage 用 Plus/Minus 本形留在 git 行内按钮)。
7. settings 页 7 处字符图标迁入 ChromeMark(`↺`→`HistoryRestore`、`↑↓`→矢量箭头族、`⋯`→受控形、`✕`→Close、`‹›`→chevron 小尺寸导出);面包屑 `‹›` 同迁。

**P2(第三片,收尾,2026-08-26 已落地 → `docs/DESIGN.md` §7.18 P2 续写):**三套展开语言合一(留可旋转 chevron;TreeDisclosure 保留与否见留裁)、品牌标统一 16px chassis、profile 菜单与 pane 菜单的实心/描边二语言统一政策(action 一律描边,fill 只给 selected/状态点/品牌)。

## 2. 动画块施工序

**前置(丙12):`bt-render/src/motion.rs`** — 三档 90/140/200 + `MOTION_TRAVEL=4px` + 两条既有曲线;红门测试:所有 `*_MS` ∈ {90,140,200} ∪ 显式豁免表(550 光标/1100 spinner/1700 呼吸/300 读数/1300 回执/意图延迟族按 Codex 裁定独立命名不入档)。

**第一片:** 8 个 popup 族 + profile/root/preview 菜单 + 子菜单 + 设置对话框 → 进 140+4px(从锚点方向)、出 90 纯淡;tooltip/keyhint 补 90 退场;toast 位移 8→4;notice 入族。**Reduced 红线照抄现行范式**:每处收 `Motion`、Reduced 首帧终值零 deadline、动画不得是唯一状态载体;顺手接 `WM_SETTINGCHANGE`(不重启生效)、补 toast/光标闪烁的 `Motion` 参数、查 cmdrail 950ms flash 的 Reduced 分支。
**第二片:** git pending 变暗 90、foot 回执交叉淡 90、设置 Advanced 高度 200、rail 归档、files 三角 120→140、浮窗 180/220/420→200/200、divider 卡片 100→90。
**不动(有书面辩护或安全边界):** peek/glance 故意无淡入、tab 内容硬切(chrome 可 90ms 色变)、拖拽 ghost 位置不缓动、pane 关闭不持尸、进度环读数 300 豁免(入豁免表)。

## 3. 用户裁决(2026-08-26):1/2/3/4/7 按推荐;**5 齿轮不改**(维持 Material 实心);6 按推荐(三角留树与子菜单,Advanced/面包屑归 chevron,字符退役)

原七条备案:

1. **裁1 External 拆分**——审计双方都判拆,推荐**拆**(默认执行,列此备案)。
2. **裁2 框/裸箭头语义**:MoveToNewTab 戴框↗、MoveToNewWindow 裸↗,与 marks.rs:405 的语义注释相反。推荐**互换**(拆分后此题随 `WindowNew` 新形自然消解,仅 Float 的用法需对齐注释)。
3. **裁3 peek 无淡入**:推荐**维持**(有书面辩护)。
4. **裁4 进度环读数豁免 reduced-motion**:推荐**接受并入豁免表**。
5. **裁5 gear 重画**:推荐**重画**成 16 格描边(唯一外来实心件)。
6. **裁6 TreeDisclosure 实心三角去留**:Codex 主张并入 chevron 一族;marks.rs 有书面理由保留(纯填充不吃 stroke)。推荐**保留**在 files 树/子菜单 ▸,但设置 Advanced 与面包屑归 chevron。
7. **裁7 菜单进场 140 vs 120**:已按 140 裁定,列此备案。

## 4. 工序与依赖

图标 P0(注册表+红门+槽位表)→ 图标 P1(重画+拆分,需新形设计)→ motion.rs 前置 → 动画一片 → 动画二片 → 图标 P2。图标 P0 与 motion.rs 可并行;重画的新形出生即受红门约束。每片照本仓纪律:红测先行、三门、实机截图、DESIGN 就地更新。

## 5. next11 验收回修(用户 2026-08-26 晚)

1. **✕ 视觉比 ⌄ 大**——红门只守线粗未守视觉面积;加「视觉面积带」,✕ 缩至墨盒约 10/16。
2. **文件夹回实心**(用户:原来的好看)——改判 P2 政策:文件夹归对象类,动词列里也实心;line 版撤。
3. **面包屑 `›` 回字符**(用户:原来的好看)——改判 P1:面包屑是路径句子,`›` 是标点非图标;白名单加「句中标点」。
4. **菜单开着时头整排保持显形**(派单方裁,用户问)——弹层=owner 表面延伸,同侧栏规矩。
5. **HTML 加 `</>` 源码翻转**(用户提议,采纳)——与 md 同款;DevTools 保留;远程页不给。
6. **设置文案全面重写**(用户:现在是注释不是文案)——先立 `copy-guide.md`,再全表重写中英,对照表 `copy-rewrite-2026-08-26.md` 供逐行审。
7. **PDF 预览不滚**——缺陷,查因中。
8. **文件夹:动作列描边、对象位实心**(用户 08-27 两次澄清的终裁:「菜单里全描边,突然一个实心怪」)——菜单行(动词)用描边版;files 树行/脚栏根路径/tab 与 pane 头 files 标/面包屑/restore 行用实心。回修线 `dc4df3d` 的「菜单也实心」被覆盖,P2 的分界回归。
9. **pane 头标题用满整行,悬停控件覆盖而非预留空白**(用户 08-27)——控件运垫渐变底,离开即让出;预览/浮窗头同族。
