# M1.9t 任务书:identity/placement 分层 + 事务级分段区域映射(大块跨 CC 内部窗格保持)

档位 high(架构级重构,M1.9n/q/s 在此连续翻车——这次有独立审因的铁证根因)。
基线 = 当前工作树。**先完整读** `docs/reviews/M1.9-large-block-flash-independent-audit.md`
(独立审因报告,frame153/chunk150 字节级根因链 + 6 条架构方向,全部照它做)。
再读 `git show efd2587`(M1.9o 保留三分法基线)。

## 真根因(审因已锁定,不要再用协调者旧假设)

**不是**"band 中间态某行没画完"(协调者旧假设,已被字节证据推翻)。**真因**:Claude Code
的输出**不占满终端**——它有内部「可滚动输出窗格(约 row 0–17)+ 固定 chrome(分隔线 row19/
prompt row20/状态栏 row22+)」。多行大块从 row10–17 平移到 row14–21 时,CC 只在内部窗格
row0–17 绘制、越过 row17 的尾部被窗格裁掉,**固定 UI 不随内容平移**(内容区 delta=+4、
chrome delta=0)→ **跨边界 band 没有单一全局 delta**。现有"record 锚 live 网格行 + 整
band 单一平移 + band 每行匹配"要求整块用一个 delta,band 尾部落到固定 UI 行(分隔线/prompt)
不匹配 → record 移入 offscreen 队列 → 恢复要求完整源码重现 → 部分可见大块永远恢复失败
→ 露源码。矮块不跨界(要么全可见要么全不可见)所以保得住。**行数只是触发几何条件,不是
语义;M1.9s 的 `>=5` 阈值 + transient 猜测是错的方向,须废弃。**

## 要做(照审因 6 条方向,不是逐症补丁)

1. **身份独立于 live row**:保存不可变 proven occurrence(完整 `original_source` 字节 +
   render source/artifact + 逻辑 source-row/cell offsets + 原始 band 几何 + occurrence
   continuity)。相同源码可重复出现 → 不能只靠 source hash 定位;候选 placement 必须唯一,
   歧义时不复用(不显示错误公式)。
2. **2026 提交后先求整屏分段 row mapping**:用旧/新网格的**精确 row/cell 内容指纹**,找出
   "上方一组行统一平移 + 下方一组行固定"的映射与边界;只接受由多个精确、唯一行对应证明的
   区域。**不猜 LaTeX 前缀、不做新公式检测。** frame153 应得内容区 `delta=+4, clip_end=17`
   与 chrome `delta=0`。
3. **band 是逻辑整体,projection 是可见交集**:record 保留完整逻辑 band;每 source segment
   保留相对 band 的逻辑偏移。只给 `band ∩ moving_content_region ∩ terminal` 建 exact
   placement/clip;**绝不把多个 offscreen segment clamp 到 row 0/last row**;被应用 chrome
   遮住的尾部标 **occluded**,不拿去与 prompt/分隔线行比较。
4. **只让精确字节参与身份延续**:可见 source segment 必须与 proven record 对应 segment 的
   `text/continues/cell_boundaries` 精确相等;隐藏 segment 不参与比较,重新出现前必须再次
   精确相等。**禁止 `before.starts_with(after)`、空行、delimiter 等中间态猜测**(审因判定
   它们既不表示结构边界、又弱于"源码精确相等"安全要求)。
5. **offscreen/dormant record 是 identity,不是唯一恢复机制**:full-source exact search 只
   作"完全重现"的安全路径;部分可见 prefix/suffix 通过 proven identity + 唯一 transaction
   mapping 恢复可见投影,**不经 M1.9p 首次检测、不重算 band**。
6. **失败守 M1.9n 红线**:不全局 reconcile 重装、不另算几何、不把 partial `$$` 当新块;
   所有已证明块继续复用原 band/artifact;首次检测仍由 M1.9k/p 的 Ambiguous/CommonMark 决定。

**真缺口 = 以 exact content fingerprint 建立的分段区域映射 + identity/placement 分层的
整体 band。** 事务边界(2026 ESU)基础已有(session.rs snapshot/finish),复用它。

## 必须同时修:oracle 诊断假绿(否则验证不可信)

审因发现 M1.9s 的诊断改动(`diagnostics.rs:113-122` 整帧含 "Jump to bottom" 就不记 flash
+ `bt-repaint-oracle.rs:65-68,100` 给整个 chunk 标 `source_mutated`)制造**假 EXIT=0**,
真实逐帧仍闪。**修正为 occlusion 感知**:某公式块的源码行被应用 chrome 遮住(occluded)
→ 不算 flash;某块纯粹因重绘/映射失败丢 record 露源码 → **算 flash**。去掉粗暴的整帧豁免。
让 EXIT 码重新可信。

## 废弃

M1.9s 的 `proven_band_rows >= 5` 宽松 matcher、`alternate_repaint_row_is_transient` 的
空行/前后缀/delimiter 猜测——用第 2/3/4 条的分段精确映射取代。

## 验证(以逐源状态轨迹为准,不只看 EXIT)

- **真实红门** `.tmp-repaint-capture/cc-topbot.vt`(106×33):修复后 aligned/pmatrix
  **首次 Rendered 后不再因纯重绘/滚动回 SOURCE**(唯一允许的 R→S 是源码那行真被 Jump chip
  覆盖、且标为 occluded 的情形)。给 before/after 的逐源轨迹(可用 scratchpad 的
  `trace_blocks.py` 思路:解析每帧 rendered/source_rows 追踪每个块 R↔S)。
- `.tmp-repaint-capture/cc-scrollout.vt`:矮块仍不闪(轨迹无新增 R→S)。
- M1.9o 三条 repaint 回归、M1.9k 两条红线、M1.9m 几何、M1.9r 还原、坐标不变量①、
  帧铁律 B、N-rows-above 全部保持。
- 新增 headless 回归:多行大块跨"内部窗格底"(非终端底)的分段重绘 → 断言无纯 flash;
  变异(退回整 band 单一 delta)→ 红。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。数字实跑抄写、附原始片段;第一遍失败如实报。
(`bt-pty` 的 `real_conpty_child_receives_color_environment…` 今日宿主 ConPTY flaky,
与本片无关,单独标注勿改归因。)

## 交付(写进 output 文件)

identity/placement 分层的数据结构与 file:line、分段 row mapping 的实现要点、occlusion
标记与 projection 可见交集的接入点、oracle 诊断修正、真实红门 before/after 逐源轨迹、
新增回归(含变异)、门禁数字。若某条方向与现有架构有不可调和冲突,停下说明。停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
