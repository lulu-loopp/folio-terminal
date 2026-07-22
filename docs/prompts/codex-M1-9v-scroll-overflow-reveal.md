# M1.9v 任务书:高多行公式顶裁 = 撑高向上溢出 + 可上滚量不足(大屏滚不到顶)

档位 high(动 alt 屏滚动溢出/呈现几何,M1.9m/M1.9n 相邻,静态门禁会假绿)。基线 = 当前工作树
(HEAD `5627e29` M1.9u + 本 session 给 oracle 加的 `BT_PROBE_GEOMETRY`/`BT_PROBE_SCROLLTOP`
+ bt-viewport 的 `debug_scroll_extent`)。**跑 cargo 一律带 `-BypassSandbox`**。

**先完整读**:`docs/HANDOFF-formula-rendering.md`、`git show 951c3fc`(M1.9u)、`git show 20846b1`
(M1.9m 呈现模型:alt expand-only、向上溢出、绝不下推输入框)。

## 现象(用户 2026-07-22 真机)

- **高多行公式(aligned/矩阵)顶部被窗格裁**(主屏+备用屏)。
- **大屏向上滚,滚到「11 rows above」就卡住、到不了会话顶(Claude Code banner)**;小屏能滚到顶。

## 根因(字节级实测坐实,用户判断正确——不是 CC 的锅)

CC 在 **alt 屏**(`?1049h`,无 scrollback)。真机录制 `.tmp-repaint-capture/cc-large.vt`
(约 212×48,用户最大化窗口)证:
1. **CC 已经滚到它自己的顶**——录制字节里 banner「Claude Code」在文件 0.949、prompt「请你输出」
   在 0.991 处**被 CC 重绘**(滚轮转发给 CC → CC 一路重绘到顶)。所以不是 CC 没滚到顶。
2. **我们把公式撑高了**(expand-only,`bt-viewport/lib.rs:1707-1748` 把 band 行高按 artifact 撑高;
   `1664-1668` 注释:alt 额外像素**向上溢出**固定网格、绝不下推输入框)。散布的公式累积撑高,把
   上方 banner/prompt 顶到**窗格之上**。
3. **可上滚量不足以把顶顶回来**。`last_live_overflow_rows`(`lib.rs:956`,= ceil(live_extra_height
   /cell))在 stuck 态实测 = **5**,但用户看到「11 rows above」→ 撑高顶出的量 > 允许上滚量 → 差的
   部分永远够不到。`scroll_by_rows`(`lib.rs:782`)上滚到 offset=5 就 capped。
4. **最顶块 content_off 变负、直接被裁**。stuck 态最终帧(`BT_PROBE_GEOMETRY` dump)aligned 块:
   `band=0..=7 top_sub=-84309 content_off=-17579(负) clip_h=177493 < art_h=199680`
   → artifact 顶 = top_sub+content_off = -101888,重度顶裁。content_off 负来自
   `centered_content_offset - hidden_height`(`lib.rs:1251-1265`):向上溢出时 hidden_height 吃掉了
   居中量还超出。

**小屏 vs 大屏**:小屏公式占比小、累积撑高 ≤ 允许上滚量,能露全;大屏公式多、撑高累积超过允许量
→ 露不全。行为符合。

## 要做(方向,非逐症补丁)

**核心 = 让 alt 屏「可上滚量」覆盖当前网格的全部向上撑高量,使滚到顶时能把被顶出窗格的最顶内容
(banner/prompt/高公式顶)完整拉回视野;且最顶可达块不得因 content_off 负而顶裁。**

1. **可上滚量 = 全部向上溢出**:核对 `last_live_overflow_rows`/`live_extra_height`
   (`lib.rs:949-965`)为何只 = 5 而非撑高实际总量。要点:溢出是**向上**的,可上滚量必须等于
   「当前网格总呈现高 − 窗格高」向上的部分(以 cell 向上取整),让 `scroll_by_rows` 能一路上滚到
   把 row0 的内容顶完整露出。别只算部分行/漏算跨块累积。
2. **最顶可达态不得顶裁**:当块已滚到「可达的最顶」(live overflow 用尽),其 artifact 顶必须 ≥
   窗格顶(content_off 不得为负导致 mid-glyph 裁)。审 `content_offset = centered - hidden_height`
   在 clipped_top 情形的符号,保证滚到顶时顶对齐、不切。
3. **守 M1.9m 红线**:**绝不下推输入框/状态栏**(expand-only 向上溢出正是为此);短公式仍在 band
   内居中;不改 primary 自由高度路径。
4. **不 reconcile 重装、不碰 M1.9u occlusion 清行/芯片护栏、不碰首次检测(M1.9p)**。

## 验证(逐帧几何为准,不信 EXIT 假绿)

**真红门** `.tmp-repaint-capture/cc-large.vt`(约 212×48;**维度是从满宽 ─ 分隔线=211、最大 CUP 行=47
推断的,Codex 应先核对/微调到内容连贯**):
```
BT_PROBE_SCROLLTOP=1 BT_PROBE_GEOMETRY=1 BT_PROBE_INPUT=.tmp-repaint-capture/cc-large.vt \
  BT_PROBE_CHUNKS=.tmp-repaint-capture/cc-large.vt.chunks BT_PROBE_COLUMNS=212 BT_PROBE_ROWS=48 \
  cargo run --locked --offline -p bt-term --bin bt-repaint-oracle 2>scroll.txt
```
- **修前**:`SCROLLUP` 上滚 capped 在 offset≈5、`RESTEXTENT live_allow≈5`、最顶 aligned 块
  content_off 负/artifact 顶 < 0(够不到顶、顶裁)。
- **修后判据**:滚到顶时**最顶 rendered 块 artifact 顶(top_sub+content_off)≥ 0**(不再顶裁),
  且可上滚量足够把 row0 内容露全(offset 能到覆盖全部向上撑高的值,不再卡 5)。给 before/after 的
  `SCROLLUP`/`RESTEXTENT`/`GEOM` 对比。
- **像素最终以用户真机截图确认**(headless 看不到最终屏幕像素;image236 顶裁 + 大屏能滚到 banner)。

**不许退化**:M1.9u ① 红门 `scripts/dev/check-occlusion-leak.py`(cc-topbot 仍 0)、flash 门
(cc-topbot/cc-scrollout EXIT 0)、cc-topbot(小屏)滚动仍能露全、全 workspace 测试 + clippy
`-D warnings` + fmt。**注意**:大屏录制上滚时若 oracle 报 flash,先判是维度不准的假信号还是真回归
(小屏 cc-topbot 不 flash 为基线)。

## 交付

改完跑门禁 + 真红门 before/after,把「改了什么、live_allow 修前修后、最顶块 artifact 顶修前修后、
门禁结果」写到 `CODEX-M1-9v-RESULT.md`。Codex 常被后台超时杀、核心写完即可,我(Claude)接手跑
编译 + 真红门几何轨迹 + 门禁验收,再交用户真机截图确认。
