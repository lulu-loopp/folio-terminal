# M1.8e 任务书:内容寻址行合成缓存 + O(1) 字形缓存淘汰(P2-17 主修)

## 档位

high。机理已由 m1.8d 审因钉死(docs/prompts/codex-M1-8d-render-perf.md 的产出,
提交 d3da0f0),方案骨架经审,照单实现;实现中发现方案与现实冲突时停下报告,
不自行改道。

## 背景(定量事实,全部可用 bt-replay 复现)

- 宽字形整形缓存 256 项(crates/bt-render/src/lib.rs:36),屏上唯一 CJK 超过
  256 即整体抖动:512 唯一字 2.297→42.512 ms/帧(18.5×),每帧 0 hit /
  19,600 miss / 19,600 eviction;淘汰是 O(capacity) 线性扫描(lib.rs:677-684,
  窄缓存同病 :528-535)。
- 行合成缓存按屏幕行位置记账(Vec<TextRow>,lib.rs:408,1530-1584):同一内容
  原位重写 3.625 ms/帧,整屏位移 47.260 ms/帧(13×)。大区块收起=整屏位移。
- 真实 Claude Code 字节流(私有 corpus,本地 scratchpad cc-collapse.btcr):
  零 CSI S/T/IL/DL,全部 CUP+EL 重写 + DEC 2026 包裹;真实会话屏已让宽缓存
  命中率掉到 55%(2422 evictions/175 帧)。
- 容量调参已证伪:256→4096 稳态 -87~-95%,但连续新内容帧 +58.6%(线性淘汰
  被放大),已撤回。结构修复不是调参。

## 交付四件

### 1. 内容寻址行合成缓存

把位置绑定的 `Vec<TextRow>` 扩成**受内存预算约束**的内容寻址缓存:

- key 至少含:完整 `CapturedCell` 行内容(text、wide spacer、bold/italic、
  前景/背景色、装饰)、font metrics/scale/font revision、状态栏覆盖。
- value:可复用 `Arc<ComposedRow>`。屏幕行位移只重映射 Arc,不重整形。
- 内存预算按**驻留字节**近似,不许用 entry 数冒充(cosmic_text::Buffer 的
  heap 实占未测——实现前先做 resident-byte 近似或跑一次 heap profiler,
  把近似公式写进注释与 trace)。
- 语义不变量:缓存命中的行与重新整形的行**渲染输出逐像素等价**(key 覆盖
  一切影响整形/布局的输入;拿不准的输入宁可进 key)。

### 2. 字形缓存 O(1) 淘汰

- 宽/窄单字形缓存改 O(1) LRU(intrusive list 或 clock),消灭每 miss 扫全表。
- 容量改字节预算;BT_PERF_TRACE 记录 wide/narrow resident bytes、
  hit/miss/eviction(计数器基建 m1.8d 已铺,沿用)。
- shape key 语义不变({text, bold, italic},颜色仍不进 key——颜色不影响整形)。

### 3. atlas 可观测性(不重构)

- 给 atlas 路径补 hit/miss/grow/evict/upload-bytes 计数(BT_PERF_TRACE 下)。
- glyphon 0.12 未公开这些——**优先方案**:本地 wrapper 层测量(prepare 前后
  diff 可得的量就 diff,得不到的如实标"不可测",不许估数)。**不许** patch
  glyphon registry 源;若确需上游 API,单独写一段"建议提给 glyphon 的 API 面"
  留档,不动手。
- 无数据支撑前不做图集重构、不做 CJK 预热(审因裁决)。

### 4. 回归矩阵(固定进门禁)

bt-replay --synthetic 扩为固定矩阵并断言量级:

- unique CJK ∈ {64, 256, 512, 1024, 2450} × {原位重写, 上移, 下移, 每帧新字,
  两集合交替} × 分块 {1B, 4KiB, 整帧}。
- 断言(release 下):512+ 唯一 CJK 的位移场景 render_per_frame 不超过原位
  场景的 3×(修前是 13×);任何场景 wide eviction 不再随帧数线性增长
  (稳态工作集装得下时为 0)。阈值若因机器差异不稳,改为相对断言并注明。
- 真实 corpus 冒烟:BT_REPLAY 私有 corpus 路径由环境变量传入
  (BT_REPLAY_PRIVATE_CORPUS),存在才跑,不存在跳过——corpus 含用户会话
  内容,永不入仓库。

## 预期(审因给出的下界,不许虚报)

- 512-CJK 位移:42.512 → ~2.2 ms/帧量级(-95% 是容量实验下界,行缓存应达同级)
- 2450-CJK 位移:47.595 → ~6 ms/帧量级
- 冷首见新内容(atlas 169 ms 那段)**不在本片承诺内**,如实保留并计量。

## 边界与纪律

- vendor/alacritty_terminal 零改动;glyphon registry 零改动。
- DEC 2026 批帧与无变化帧跳过(m1.8d)语义不动;若行缓存与
  presentation_equivalent 键有交互,以呈现等价键为准并补测试。
- 门禁:cargo test --workspace --locked、clippy --all-targets -D warnings、
  fmt --check;vendor 182 上游断言零改;新回归矩阵纳入。
- 工作树 design/*、docs/UI-UX.md 脏项不属于你,保持原样。
- 结果写 C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\ff11be2c-05c5-4a23-840f-a94405b44c35\scratchpad\m1-8e-result.md
  (绝对路径),含:修前/修后矩阵对照表、缓存驻留字节数、门禁数字、
  file:line 审阅入口。停下等审,不提交。
