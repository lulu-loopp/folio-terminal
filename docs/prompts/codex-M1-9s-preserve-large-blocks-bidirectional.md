# M1.9s 任务书:已渲染的多行大块跨重绘/双向滚动保持渲染(保留缺口补全)

档位 high(承接 M1.9o/M1.9q 保留逻辑,M1.9n 栽过的深改保持区——须真实红门+清醒)。
基线 = **当前工作树**(`efd2587` + 未提交的 M1.9q + M1.9r)。
**先读** `git show efd2587`(M1.9o alt 重绘事务:begin/finish_alternate_repaint、
alternate_record_semantics_match、shift_live_record)、session.rs 现有 M1.9q 的
`alternate_offscreen_decorations`/`restore_alternate_offscreen_decorations`(:1145-1229)、
以及 `docs/reviews/M1.9p-scrollback-symmetric-ambiguity-nogo.md`。

## 现象(协调者用真实录制 + oracle 逐帧证实)

用户明确:**截图里变源码的块都是"公式全渲染之后往上/往下滚动一些"才出现的,不存在
"从没渲染"**。协调者用 `cc-topbot.vt`(真实 CC,含 `$$\begin{aligned}…\end{aligned}$$`
麦克斯韦块 + `$$…\begin{pmatrix}…\end{pmatrix}…$$`)经 repaint oracle 逐帧追踪,证实
这些**多行大块反复 RENDERED↔SOURCE 震荡十几次**(aligned: frame 131 R→142 S→143 R→
152 S→…→314 R→316 S;pmatrix 同样反复),每次 CC 2026 重绘就丢一次。

**关键对比**:`cc-scrollout.vt` 的 9 个**矮块**(1–2 行 band)在 M1.9o/M1.9q 下保留住了
(该红门 EXIT=0、不 flash);但**多行大块**(aligned 约 6 行 band)保不住。

## 根因方向(独立复核,给 file:line)

- M1.9o 的原地保留 `alternate_record_semantics_match`(`session.rs:~4179`)要求公式 band
  的**每一行**在新旧网格字节精确相等;多行大块 band 有 6 行,CC 2026 重绘的**中间态**
  只要任一 band 行尚未画完/被 `\x1b[K` 临时清空,整块匹配失败 → record 丢弃 → 重检测
  → 撞对称 `$$` 歧义 → 变 source(下一帧画完又渲染回来 → 反复 flash);
- M1.9q 的 offscreen 队列/平移同样以"band 每行可匹配"为前提,对大块不稳;
- **请复核**:多行大块反复 flash 的确切触发是不是"band 中间态部分行不匹配"?还是平移
  判定对大块失败?给出具体 frame 的重绘字节佐证。

## 要做的(纯保持,不碰首次检测)

**用户已证实这些块都有 render record(渲染过)**,所以本片是**纯保持问题**,
**不新检测、不猜前缀、不碰 M1.9p 首次检测歧义、不碰 M1.9k 错配红线**。目标:

1. **多行大块的保留对"重绘中间态"稳健**:不因 band 某几行在一次重绘的中间态暂不匹配
   就丢弃整个 record;只要能证明是同一块(源码精确相等的可见部分)就保持,等重绘完成
   再校正几何。渲染过的大块在 CC 反复 2026 重绘下**不得反复 flash**。
2. **双向出屏保持**:opener 出屏顶(M1.9q 部分做)+ **closer 出屏底**(image228,未做)
   都保持渲染可见部分(顶部/底部 clip)。
3. 覆盖 aligned/pmatrix/cases 等多行环境块。

## 安全(与 M1.9n 失败的分界)

- **保留/平移,不 reconcile 重装**:M1.9n 栽在"对所有情形重算 band"→ 不一致损坏。本片
  必须"能保留就保留、能平移就平移",绝不重算已证明块的 band;
- 保持命中仍需**源码精确相等**(可见部分),防显示错误公式(承接无碰撞身份);
- 被 CC 的 UI 覆盖(如 "Jump to bottom" 芯片文字真进了 grid 那行,源码确实变了)导致的
  source_rows 变化**不在本片范围**(那是源码真被覆盖,另处理)——聚焦纯重绘/滚动的 flash。

## 硬约束(不可回退)

- M1.9o 三条 repaint 回归、M1.9k 两条红线、M1.9m 几何、M1.9r 还原、九类误报守卫、
  CJK 守卫、坐标不变量①、帧铁律 B、N-rows-above 全部保持;
- `cc-scrollout.vt` 红门仍 **EXIT=0**;主屏路径不变;vendor/glyphon 零改。

## 回归(各配变异)

1. **真实红门**:`.tmp-repaint-capture/cc-topbot.vt`(需先 `cp` 到位)经 oracle 回放,
   **当前 aligned/pmatrix 反复 RENDERED↔SOURCE flash;修复后:一旦 Rendered 即保持、
   不再因纯重绘/滚动回 source**(Jump 芯片污染帧豁免)。给 before/after 的 state 轨迹;
2. 合成 headless 回归(进 `tests/repaint_flash_oracle.rs`):多行(≥5 行 band)公式渲染后,
   模拟 CC 分多次 `\x1b[K`+重画的中间态重绘 → 断言无 source 帧;变异(退回"band 任一行
   不匹配即弃")→ 红;
3. closer 出屏底保持回归;变异 → 红;
4. `cc-scrollout.vt` 仍 EXIT=0;M1.9o 三条 repaint 仍绿;
5. M1.9p 首次检测负例(从没渲染的对称 `$$` 保持源码)不变。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;数字实跑抄写、附原始片段;第一遍失败如实报。
(`bt-pty` 的 `real_conpty_child_receives_color_environment…` 今日宿主 ConPTY flaky,
与本片无关,单独标注勿改归因。)

## 交付(写进 output 文件)

大块反复 flash 的根因复核 file:line + 具体重绘字节佐证、保留稳健化的改法、双向出屏接入点、
真实红门 before/after 轨迹、新增回归(含变异)、门禁数字。停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
