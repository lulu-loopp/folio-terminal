# M1.9u 任务书:occlusion 边界打磨(源码尾泄漏 + Jump 芯片) + 顶裁几何

档位 high(动 M1.9t 的 identity/placement 投影 + viewport 清行 + 检测护栏,属 M1.9n
相邻精细区,静态门禁会假绿)。基线 = 当前工作树(HEAD `b9faf31` M1.9t + 本 session 给
bt-repaint-oracle 加的 `BT_PROBE_VERBOSE` 行级 dump)。**跑 cargo 一律带 `-BypassSandbox`**
(Codex Windows 沙箱 `CreateProcessAsUserW 1312` 会杀 cargo)。

**先完整读**:
- `docs/HANDOFF-formula-rendering.md`(当前状态、红线、验证素材位置)
- `docs/reviews/M1.9-large-block-flash-independent-audit.md`(M1.9t 的架构根因:CC 内部
  可滚动窗格 row0–17 + 固定 chrome row19+;跨边界 band 无单一 delta;这是 occlusion 的由来)
- `git show b9faf31`(M1.9t:`content_fingerprint`/`LiveOccurrencePlacement`/
  `occluded_source_rows`/`logical_band_start`/分段映射/occlusion)

---

## 三个问题(用户 2026-07-22 真机 image233/234/235/236,均为「渲染过后」小残缺)

已用真机录制 `.tmp-repaint-capture/cc-topbot.vt`(106×33)逐帧定位,frame 153 一帧同时
暴露 ①②:

```
block[1] display=Rendered band=14..=16 occluded=6 occ_id=1   (aligned Maxwell 块,逻辑 9 行 14..22,只渲染 14-16)
row[16] (被 band 覆盖,已清)
row[17] |\nabla \cdot \mathbf{B} &= 0 \       Jump to bottom (ctrl+End) ↓     ← ① 尾行露源码 + ② 芯片压同行
row[19] |──────  (分隔线 chrome)
row[20] |❯       (prompt chrome)
row[22] |[Opus 4.8 …]  (状态栏 chrome)
```

### ① occluded 尾行仍露源码(image233,主要问题,有真红门)
- **现象**:块判 occluded 的尾行落在 band 底(16)与 chrome(19)之间时**物理可见**,viewport
  只清 exact band 行(`bt-viewport/src/lib.rs:1303-1312`,`block_first..=block_last`),这些
  occluded 行不在清除范围 → 源码尾泄漏。
- **难点(架构缺口)**:viewport 只从 `MathBlockPlacement.occluded_source_rows`
  (`lib.rs:200`)拿到 occluded **计数**,不知道该清哪几个终端行。**不能简单清
  `band_end+1 .. band_end+occluded`**——那会擦掉 CC 重绘上去的 chrome(分隔线/prompt/状态栏)。
- **正确方向**:projection(`project_live_record`,`session.rs:4690-4796`)已经知道每个 occluded
  行的 target row 与它对应的 proven source(`record.identity.source_rows` 的 `band_offset`)。
  让它**产出「occluded 且仍是本块源码的终端行集合」**交给 viewport 清(blank cells)。判定
  = 该终端行 target 落在 `[0, terminal_end]` 且**其可见 grid 内容仍匹配本块该 offset 的
  proven source 前缀**(CC 尚未把它重绘成 chrome;若已是 chrome 则不清)。传给
  `MathBlockPlacement`(如 `occluded_visible_rows: Vec<u32>` 或 range),viewport 清这些行的
  cells,与现有 band 清行同源(`lift`/`clear`)。
- **芯片同源**:row17 上的 Jump 芯片随这些行一起被清(②a 自动解决)。
- **红线**:被 chrome 遮住(不匹配本块源码)的行**绝不清**;清行判定必须基于**本块 proven
  source 精确/前缀相等**,不用「看起来像公式就清」的启发式(那会误清邻块/散文)。

### ② Jump 芯片文字进公式(image234/235)
- **②a**(芯片落 occluded 尾行):随 ① 清行解决,无需单独处理。
- **②b**(芯片落**可见渲染行**并被烤进 artifact):frame 138 row17
  `$$\hat{f}(\xi) = \int_{-\infty}^{\in Jump to bottom (ctrl+End) ↓  dx$$` —— CC 的芯片
  overlay 覆写了傅里叶积分行中段,`detect_block_math`(`crates/bt-detect/src/lib.rs:462`)
  把整行当合法 `$$…$$` 检测 → 芯片文字烤进渲染。
- **方向**:识别 CC Jump-chip overlay 签名(`Jump to bottom (ctrl+End)` / 尾随 `↓`),
  **带此 overlay 的行不进检测/不烤进 artifact**(视为瞬态 chrome,不是公式内容)。这是
  CC 专属,和 `restore_stripped_environment_newlines` 同理挂 `MathLayoutOptions` 开关
  (默认 on,CC 修好后可关)。
- **红线(M1.9k)**:护栏只针对**确切的 Jump-chip 签名字符串**,不得放宽成「含控制符/方括
  号就不检测」这类会误伤真公式或把散文当公式的宽判定。加变异测试:去掉芯片 → 该行仍正常
  检测渲染;含芯片 → 不烤芯片。

### ③ 渲染公式顶部被裁(image236,主屏+备用屏,几何,定位较浅需先隔离)
- **现象**:渲染公式**顶部被切**(如 `∇·E = ρ/ε₀` 的 ρ 上缘)。
- **已排除**:不是 `centered_content_offset`(`lib.rs:556`,被 `.max(0)` 夹住,居中永不把顶
  推到 band 上方;artifact 比 band 高时只溢出底部)。
- **待隔离**(两个候选,先做隔离实验再改):
  (a) 渲染器(bt-app/GPU)对 artifact 的 clip 窗口 `[top_subpixels, top+clip_height]`
      与 `content_offset_subpixels`(`lib.rs:1260-1265`/history 路径 `1690-1720`)的配合,
      顶部被外层 pane clip 或 band clip 切掉;
  (b) bt-math 光栅 bbox 对高字形(ρ/积分号上缘)顶部 crop 太紧(`height_subpixels`/
      `baseline_subpixels` 不含上缘 ascent)。
- **主屏也有** → 与 alt occlusion 无关,是几何/光栅层。**先加几何/光栅单测隔离到 (a) 或
  (b)**,再针对性修。**headless oracle 看不到像素**,最终以用户真机截图确认。

---

## 验证(逐源轨迹为准,别信 EXIT 假绿——handoff 铁律)

### ① 真红门(必须从红变绿)
```
BT_PROBE_VERBOSE=1 BT_PROBE_INPUT=.tmp-repaint-capture/cc-topbot.vt \
  BT_PROBE_CHUNKS=.tmp-repaint-capture/cc-topbot.vt.chunks \
  BT_PROBE_COLUMNS=106 BT_PROBE_ROWS=33 \
  cargo run --locked --offline -p bt-term --bin bt-repaint-oracle 1>trace.txt 2>verbose.txt
python scripts/dev/check-occlusion-leak.py verbose.txt
```
- **当前(M1.9t)= RED:28 帧 / 30 泄漏行**(全是 `band_end+1` 的 occluded 尾行露
  `\end{aligned}`/`\nabla…&=0\`/`$$`/`A=\begin{pmatrix}` + 芯片)。
- **① 修好后此门 = 0**。给 before/after 计数。

### 不许退化(全绿保持)
- **flash 门**:`bt-repaint-oracle` 对 `cc-topbot.vt` 与 `cc-scrollout.vt` 的 EXIT 仍 = 0
  (R→S flips 恒 0;矮块基线不动)。
- M1.9o 三条 repaint 回归、M1.9k 两条红线、M1.9m 几何、M1.9r 还原、M1.9t 分段映射/occlusion
  测试、坐标不变量、N-rows-above 全保持。
- `bt-term` + `bt-viewport` + `bt-detect` 全测 `--locked --offline`;clippy `-D warnings`;fmt。
  (`bt-pty` 的 `real_conpty_child_receives_color_environment…` 宿主 ConPTY 不稳时 flaky,
  与本任务无关。)

### 新增回归
- **①**:headless 断言——一个 Rendered 块的 occluded 尾行仍在可见区(band 底与 chrome 间)
  时,那些行的 cells 被清(无源码残留);变异(退回只清 exact band 行)→ 红。
- **②b**:含 Jump-chip 的 `$$…$$` 行不烤芯片(单测);去掉芯片正常渲染。
- **③**:几何/光栅单测锁定顶裁根因(a 或 b),修后顶部不裁;主屏+alt 各一。

## 红线(不可破,违反即 NO-GO)
- **M1.9p**:回看态首次检测多行对称 `$$…$$` = NO-GO(第0行前缀不可信)。**本任务不新增任何
  首次检测**;只处理**已证明块**的 occluded 行清除与已渲染 artifact 的芯片污染。frame 153
  的 rows1-6(`\end{pmatrix}`/`$$`,opener 已滚出顶部的 pmatrix)是**合法 source,不许碰**。
- **M1.9k**:错配散文红线、CJK 守卫、九类误报守卫、CommonMark 上下文。② 护栏只匹配确切
  Jump-chip 签名。
- **保持/清行须源码精确相等**:① 清行判定必须本块 proven source 精确/前缀相等,不用启发式。
- **不 reconcile 重装、不另算 band 几何**(M1.9n 栽过):复用 M1.9t 的 identity/placement
  与分段映射,只在其上补 occluded 可见行的清除信息。

## 实施序
红门确认(跑 check-occlusion-leak.py 见 30)→ ① projection 产出 occluded 可见行 + viewport
清行(红门归 0)→ ②b 检测芯片护栏 → ③ 隔离+几何修 → 全门禁 + clippy + fmt。**每步先在
真红门/单测看红,再实现看绿。** Codex 常被后台超时杀,核心写完即可,我(Claude)接手跑
编译 + 逐源轨迹 + 门禁验收。
