# M1.9o 任务书:备用屏 expand-only 行高 + 消灭滚动/点击/双击闪回

档位 high(踩红线 + 交互态,前面 M1.9n 在此翻车)。基线 = 当前工作树
(M1.9m 提交 `20846b1` + **已就位的交互重绘 oracle 工具**,未提交)。
**先读**:审因 `docs/reviews/M1.9-formula-pipeline-audit.md`、
`docs/prompts/codex-M1-9-alt-presentation-audit2.md`(第二次审的裁决)。

## 你有一个能红转绿的 oracle(务必用它验)

协调者已修好并验证了交互重绘 oracle:
- `crates/bt-term/src/bin/bt-repaint-oracle.rs`:headless 逐帧回放,公式渲染后
  又露源码就非零退出;
- `crates/bt-term/src/diagnostics.rs`:`FormulaFlashOracle`(已修多行块退回的匹配 bug);
- **红门素材**:`.tmp-repaint-capture/scroll-repaint.vt(+.chunks)`——一段"公式渲染→
  alt 整屏重绘平移"的确定性序列。**在 M1.9m 上跑它当前 EXIT=1(判红/有闪)**:
  ```
  BT_PROBE_INPUT=.tmp-repaint-capture/scroll-repaint.vt \
  BT_PROBE_CHUNKS=.tmp-repaint-capture/scroll-repaint.vt.chunks \
  BT_PROBE_COLUMNS=80 BT_PROBE_ROWS=24 \
  cargo run --locked --offline -p bt-term --bin bt-repaint-oracle
  ```
  **修复后它必须 EXIT=0**。并把这段(及一段"原地重绘不平移"序列)固化成
  `tests/repaint_flash_oracle.rs` 的 headless 回归——**改前红、改后绿**(变异验证)。
- 不许削弱 oracle 来"过关"(passing-by-deletion 是纪律红线)。

## 要做的两件(同一片)

### ① 备用屏行高:expand-only(删收拢/底锚)
用户拍板:备用屏是 Claude Code 固定 N×M 栅格,free-height 只该用主屏。改为:
- **块呈现高 = `max(公式净高 + 上下对称 padding, 源码行数 × cell)`**:
  - 公式**比源码行高** → 撑高,顶部溢出(既有 `N rows above`,用户已接受);
  - 公式**比源码行矮** → **不收拢**,老实占满源码行 band、公式竖直居中(局部留白);
- **删掉 alt 的收拢 + 底部锚定**(M1.9m 的 Issue 4 来源):
  `bt-viewport/src/lib.rs` 的 alt free-height/收拢(约 1592-1671)、底锚(约 1048-1065)、
  以及 alt 的借空行/按 raster 撑 band(`bt-term/src/session.rs` 约 996-1010/1093-1125/
  3472-3562)——这些**只留给 Primary**;主屏净高/padding/居中/`show_source` 内容偏好、
  padding 可配全部**保留**。
- 论证:expand-only 消除顶部空白 + 输入框上移,与坐标不变量①一致;高公式不缩放不裁切。

### ② 消灭闪回:内容寻址保留 / 平移 / 兜底(不要 M1.9n 的 reconcile-重装)
根因:每个 PTY chunk 后立即删装饰(`session.rs` 约 900-960),真实 clear/重写跨多
chunk,首个中间态就删了→drain 发源码帧→200ms 后才重检测。**三分法(能保留就保留、
能平移就平移,band 绝不另算)**:
1. **原地重绘**:drain 边界比语义快照(源码 + 解析前缀);未变就**保留现有 record/
   artifact/正确 band,不删、不重检测**——直接消灭点击/双击闪;
2. **明确整屏滚动/平移**:复用现有 record **平移**(`session.rs` 约 2353-2395/2564-2601
   的既有平移机制),不重算 band;
3. **只有 clear/home 后不可证的任意搬移**:才用 bounded M1.9k reconcile(纯源码搜索
   守不住 code/prose/Ambiguous 红线);固定 alt band 后禁止再算 free-height/借行几何。
- **M1.9n 的教训**:它对所有情形都 reconcile-重装、band 另算一套 → 交互时 `$$` 露成
  源码、损坏。**本片必须让保留/平移路径的 band 与全新检测完全一致**(共享同一段几何
  代码,不复制)。
- 双击:确认它不切 `show_source`(只眼睛按钮切,`main.rs` 约 1170);双击落公式外→
  转发 Claude Code→重绘→闪,由①覆盖。

## 硬约束(不可回退)

- M1.9k 检测形态全集 + 两条红线(Ambiguous 不错配、CommonMark 代码不误渲染)、
  九类误报守卫、CJK 散文守卫;m1.9g 单块 `d` 不裁、m1.9h 左缩进、帧铁律 B、
  `N rows above`、光标/输入/状态可见、终端坐标不变量①;主屏几何行为不变;
- **绝不显示错误公式**:保留/平移/复用命中必须源码精确相等(承接无碰撞身份);
- 性能:发布前若有同步检测,只覆盖可见范围、无公式快速短路、Expose 零成本。

## 回归(各配变异)

1. **红门**:`scroll-repaint.vt` 从 EXIT=1 → EXIT=0;固化成 headless 回归,变异
   (退回删装饰)→ 红;
2. 原地重绘序列:公式全程无源码帧;
3. expand-only:高公式撑高不裁(顶 subpixel ≥ clip 上沿)、矮公式占满源码 band 不收拢、
   无输入框上移/顶部空白;各配变异;
4. M1.9k 形态、两条红线、主屏几何、`N rows above` 全绿。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。数字从实跑抄写、附原始片段;第一遍失败如实报。

## 交付(写进 output 文件)

expand-only 落地(撤哪些/留哪些 file:line)、三分法各路径 file:line + band 一致性论证、
红门 EXIT 从 1→0 的实跑证据、新增回归(含变异)、门禁数字。若某不变量冲突,停下说明。
停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
