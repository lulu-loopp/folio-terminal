# M1.9l 任务书:公式块按 raster 真实高度呈现(顶裁 + 空白,同源)

档位 high(渲染几何 + free-height 投影,踩过同族坑 m1.9g)。基线 = 提交 `bac8d5a`。
来源:用户实测(2026-07-20,image 215)+ 协调者复现与用户补充。

## 一个根因,两个症状

公式块占 **N 个源码逻辑行**(原始 LaTeX 文本),但渲染后的 raster 像素高度与
`N × cell_height` **无必然关系**。当前 band 的**像素高**绑在**源码行数**上
(`size_live_task_band_for_artifact` 从 `source_rows` 起,只向下借空行/向上撑到
`ceil(raster/cell)`,**从不缩到 source_rows 以下**),于是:

- **症状 A(raster 比源码行数高)**:借不到足够空行 → band 比 raster 矮 →
  **第一行顶部被裁几 px**(用户 image 215:bare `\begin{align}` + 分数,`\rho`
  分子顶被切);
- **症状 B(raster 比源码行数矮)**:band 停在 `source_rows` 不收拢 →
  **raster 上下多出大片空白**(用户:"源代码高度很高,渲染后上方下方很多空白")。

**正确模型(free-height)**:块**覆盖** N 个源码逻辑行(藏住原始 LaTeX),但**按
raster 的真实像素高呈现**——高则撑开、矮则收拢,**后续内容按投影随之上/下移**,
两个方向都不该有裁切或空白。这正是 DESIGN 的"投影布局不变量"(逻辑行→像素可变高)。

## 复现(alt 屏,`BT_PROBE_INPUT`)

症状 A(高):
```
\begin{align}
\nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_0} \\
\nabla \times \mathbf{B} &= \mu_0 \mathbf{J} + \mu_0 \varepsilon_0 \frac{\partial \mathbf{E}}{\partial t} \\
\nabla \times \mathbf{E} &= -\frac{\partial \mathbf{B}}{\partial t}
\end{align}
```
症状 B(矮):源码多行但渲染短,例:
```
$$


x = 1


$$
```
(`$$` 与内容分行、中间多空行 → 源码 ~7 行,渲染 `x=1` ~1.5 cell → 上下大片空白)

**这是既有渲染几何 bug,非 M1.9k 引入**(M1.9k diff 触及 band/clip 几何 0 行,已核)。

## 根因方向(协调者观察,先独立复核可推翻)

- `size_live_task_band_for_artifact`(`crates/bt-term/src/session.rs:3434`):
  `band_start/end = source span`,`needed = ceil(raster/cell) - source_rows`
  只增不减 → band 像素高 `>= source_rows × cell`,**从不等于 raster 高**;
- `live_presentation_height_subpixels`(`session.rs:3383`):tight raster 上加呼吸留白;
- 竖向定位/裁剪 `crates/bt-render/src/lib.rs:2167-2195`:`band_top`、`content_offset`、
  `visible_top = top.max(pane_top)`、`clip_height_subpixels`;
- 投影层 free-height:`crates/bt-viewport/src/lib.rs`(逻辑行→像素高映射、块占用 band)。

m1.9g 修过同族(居中偏移把 raster 顶推出 scissor → `clip_top = top.max(pane_top)`);
本次要判定被裁的几 px 与多出的空白**精确从哪来**,给 subpixel 级算式与 file:line。

## 要求

1. **定性**:给出块呈现高度、raster 高、band 覆盖行数、offset、clip 上下沿的
   subpixel 级算式,分别解释症状 A 的裁切来源与症状 B 的空白来源(证实是同一处);
2. **统一修复**:块的**呈现像素高 = raster 高(+ 既有呼吸留白)**,与它覆盖的源码
   逻辑行数解耦——
   - raster 高于源码占位:向下(必要时向上)扩展呈现高度,**完整 raster 不被 clip
     上/下沿裁切**;后续内容下移;
   - raster 矮于源码占位:**band 收拢到 raster 高**,不在 raster 上下留源码行数造成
     的空白;被覆盖的源码逻辑行按 free-height 折叠,后续内容上移;
   - frozen 侧与 live 侧行为一致(两条呈现路径都改)。
3. **不回退**:m1.9g 的 `d` 上升部不裁、左缩进(m1.9h)、水平居中/竖直居中、
   借空行的**非重叠证明**(不得占用邻块已借的行/不得覆盖邻行内容)、`N rows above`
   顶部溢出指示、光标/输入/状态行始终可见、帧级铁律 B(clip 覆盖 block)——全部保持;
4. 横向无关,不动横滚/wrap。

## 回归

- 症状 A(align):断言 raster **顶部 subpixel ≥ clip 上沿**(不裁);变异:临时把
  呈现高少算一行 → 变红;
- 症状 B(多行短公式):断言块呈现高 **≈ raster 高(±1 cell 内)**、raster 上/下**无
  源码行数造成的空白 band**;变异:临时恢复"band 不缩到 source_rows 以下" → 变红并
  显示多余空白;
- raster 高度 = 源码占位(整格)时行为不变;
- m1.9g 单块 `d` 不裁、邻块不重叠、`N rows above` 溢出回归仍绿。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。门禁数字从实跑输出抄写并附原始片段;第一遍
任何失败如实报告。

## 交付(写进 output 文件)

竖向几何因果链 file:line(证实 A/B 同源)、统一修复方案与取舍、新增回归(含两向
变异)、门禁实跑数字。停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
