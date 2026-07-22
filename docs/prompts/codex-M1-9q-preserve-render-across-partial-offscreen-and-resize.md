# M1.9q 任务书:已渲染公式跨「部分出屏 + resize」保持渲染(保留,不重检测)

档位 high(承接 M1.9o 平移逻辑 + 踩渲染几何)。基线 = 提交 `efd2587`(M1.9o)。
**先读** `git show efd2587`(M1.9o 的 alt 重绘事务:begin/finish_alternate_repaint、
alternate_record_semantics_match、shift_live_record)、审因 `docs/reviews/
M1.9-formula-pipeline-audit.md`、以及 `docs/reviews/M1.9p-scrollback-symmetric-
ambiguity-nogo.md`(理解为何"首次检测"对称 `$$` 不可做,而"保留已证明公式"可做)。

## 用户拍板的方向(协调者已用真实录制证实)

用户洞察:「我们让滚动不闪靠的是保留现有渲染装饰;那只要让 `$$` 出屏的公式仍保持
原样渲染即可」。协调者用 `BT_PTY_DUMP` 录了真实 Claude Code「公式渲染→上滚→变源码」
(`.tmp-repaint-capture/cc-scrollout.vt`),用 repaint oracle 回放证实:**9 个公式
先到过 Rendered、随后 flash 回 source**——即它们是**渲染后丢失**,不是从没渲染。

**根因(M1.9o 的缺口)**:
- M1.9o 修了「公式在可见网格内移动」→ 平移保留(不闪);
- 但当公式移到**部分出屏**(opener 行移到可见网格 row 0 之上),
  `alternate_record_semantics_match`(`session.rs:3924-3948`)对 band 每行调
  `shift_live_row`,出屏行返回 `None` → **整个匹配失败 → 装饰进 unresolved →
  被丢弃/重检测 → 撞对称歧义 → 变源码(flash)**;
- **resize** 同理:它是另一次重排,公式行位置变、band 几何变,现有保留/平移路径未覆盖
  resize 触发,公式装饰失效变源码。

## 要做的(两个触发,同一个「保留已证明公式」原则)

### ① 部分出屏仍保留 + 渲染
- 扩展平移匹配:公式 band 的**可见部分**行字节仍匹配已知公式时,**保留 artifact**,
  不因 opener/若干行移出可见网格顶就整个丢弃;把移出的部分按「顶部溢出」处理。
- 渲染部分出屏的 live 公式:顶部裁掉、显示可见部分(复用 m1.9e/m 的 N-rows-above /
  顶部溢出几何;live 装饰需能表达 band 上边界在可见 row 0 之上并 clip)。

### ② resize 时保留
- resize(网格列/行数或 DPI 变)时,已渲染公式的装饰应**跨 resize 保留其已证明身份**:
  源码内容不变则复用(layout 变则按 M1.9j/M1.9o 的渲染键重渲染像素,但**不退回源码**——
  即保持"这是公式"的状态,只更新像素,不闪源码)。给出 resize 路径的接入点 file:line。

## 安全性(这是本片能做、而 M1.9p 不能做的分界)

- **保留一个已证明的公式 ≠ 首次检测一个歧义块**。公式身份在它**完整可见时已被证明**
  (渲染过、有 record);本片只保留/延续该身份,**不新检测**、**不猜前缀**。
- 可见部分字节匹配已知公式 → 高置信同一公式;**万一**错配,显示的是「另一个真公式的
  像素」而非「散文排成公式」——**不触碰 M1.9k 的错配红线**(那条只在"新把散文当公式"
  时触发)。请在交付里写明这条分界论证。
- **不放松首次检测**:M1.9p 裁决的「回看态首次遇到多行对称 `$$` 保持源码」不变;本片
  只作用于**已有 render record**的公式。

## 硬约束(不可回退)

- M1.9o 三条 repaint 回归、M1.9k 两条红线、M1.9m 几何、九类误报守卫、CJK 散文守卫、
  坐标不变量①、帧铁律 B、`N rows above`、m1.9g 不裁 / m1.9h 左缩进 全部保持;
- 主屏路径不变(本片是 alt live 装饰 + resize);
- 绝不显示错误公式:复用/保留命中仍需源码精确相等(承接无碰撞身份)。

## 回归(各配变异)

1. **真实红门**:`.tmp-repaint-capture/cc-scrollout.vt(+.chunks)` 用 repaint oracle
   回放,**当前 EXIT=1(9 公式 flash),修复后必须 EXIT=0**:
   ```
   BT_PROBE_INPUT=.tmp-repaint-capture/cc-scrollout.vt \
   BT_PROBE_CHUNKS=.tmp-repaint-capture/cc-scrollout.vt.chunks \
   BT_PROBE_COLUMNS=106 BT_PROBE_ROWS=33 \
   cargo run --locked --offline -p bt-term --bin bt-repaint-oracle
   ```
2. 合成 headless 回归(进 `tests/repaint_flash_oracle.rs`):公式渲染→重绘使其 opener
   移出可见顶→断言无 source 帧(仍 Rendered/部分渲染);变异(退回"出屏即弃")→ 红;
3. resize 回归:公式渲染→resize→断言不退回源码;变异 → 红;
4. **不错配负例**:一个**从没渲染过**的多行对称 `$$` 在回看态仍保持源码(M1.9p 不变);
5. M1.9o 三条 repaint 回归仍绿。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。数字从实跑抄写、附原始片段;第一遍失败如实报。

## 交付(写进 output 文件)

部分出屏保留+渲染的接入点 file:line、resize 保留的接入点、安全性分界论证(保留已证明
公式 vs 首次检测歧义)、真实红门 EXIT 1→0 证据、新增回归(含变异)、门禁数字。
若判断"部分出屏渲染"与 alt live 装饰模型有不可调和冲突,停下说明。停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
