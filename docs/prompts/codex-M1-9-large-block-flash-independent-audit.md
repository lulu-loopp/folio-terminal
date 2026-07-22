# 独立审因:多行大块在 CC 重绘下反复 flash 的真根因(不修、只找因、可翻架构)

档位 high。**read-only 审因:禁止修改任何源码/测试/配置**,只读代码 + 跑 oracle 分析 +
写审因报告。基线 = 当前工作树(含未提交的 M1.9q/M1.9r/M1.9s)。

## 为什么是独立审因(而非又一个修复单)

协调者在"已渲染公式跨 CC 重绘保持"这条线上**连续 3 轮打偏**:
- M1.9n:发布前 reconcile 重装 band → 真机损坏(静态门禁全绿);
- M1.9q:顶部出屏保持,对多行大块只做一半;
- M1.9s:针对"band 中间态匹配失败"假设修,**红门 flash 12→9 微降、大块仍反复 flash**。

协调者的"band 中间态某行不匹配 → 丢弃"假设,连两轮修都没消除 flash,**很可能没抓到真
根**。故切独立审因:**不预设任何协调者的假设为真,允许推翻现有架构**(record 锚定方式、
band 匹配策略、M1.9o 保留三分法),给根因,不给补丁。

## 铁证现象(协调者已用真实录制 + oracle 逐帧确认)

- 真实录制 `.tmp-repaint-capture/cc-topbot.vt`(真实 Claude Code:`$$\begin{aligned}
  …\end{aligned}$$` 麦克斯韦块 + `$$…\begin{pmatrix}…\end{pmatrix}…$$`)。
- oracle 逐帧(`BT_PROBE_COLUMNS=106 BT_PROBE_ROWS=33`,bin `bt-repaint-oracle`)追踪:
  **多行大块 aligned/pmatrix 反复 RENDERED↔SOURCE 震荡 ~12 次**(aligned: frame 131 R→
  153 S→…→整段 flash 十几次)。
- **对照**:`cc-scrollout.vt` 的 9 个**矮块**(1–2 行 band)在同样 M1.9o/M1.9q 下
  **保留住、EXIT=0、不 flash**。
- 用户亲证:这些块**都渲染过**(不存在从没渲染),纯保持问题。

## 审因要回答的核心问题(给 file:line + 帧字节佐证)

1. **矮块 vs 大块的本质差异**:为什么同样的保留逻辑,1–2 行 band 保得住、6 行 band 保不住?
   是 band 行数本身,还是大块跨越了某个结构边界(如 CC 把大块分多个 2026 事务重绘、
   或大块跨越 scroll region / 屏幕边界)?
2. **挑一个具体 flash 点深挖**(如 aligned 首次 R→S 的那一帧):对照 `cc-topbot.vt` 在那个
   chunk 的原始重绘字节,说明 CC 到底做了什么(是整块 clear+重画?分行增量?插入/删除行?
   scroll region 内滚动?),以及现有 M1.9o `alternate_record_semantics_match` /
   M1.9q offscreen/平移 在这一帧的哪一步、因什么条件判 record 无效。
3. **架构层判断**:现有"record 锚定在 live 网格行 + band 每行字节精确匹配 + 三分法
   (原地保留/平移/兜底 reconcile)"的架构,**是否根本上不足以**稳定保持一个在 CC 反复
   2026 重绘中的多行大块?若是,缺的是什么(如:按内容/源码指纹锚定而非网格行?重绘事务
   边界感知?band 作为整体单元而非逐行匹配?)——给方向,不写实现。
4. M1.9s 半成品(session.rs 未提交改动)**具体改了什么、为什么没消除 flash**——它的思路
   哪里不对或不足。

## 红线(判断改法可行性时必须守)

- 不得为保持而猜前缀/新检测 → M1.9p 首次检测歧义、M1.9k 错配散文红线不可破;
- 保持命中必须能证明是同一块(源码精确相等)→ 绝不显示错误公式;
- 任何架构建议不得回退 M1.9o 三条 repaint 回归、M1.9m 几何、坐标不变量。

## 交付(写进 output 文件,不改代码)

矮块/大块差异的本质结论、一个具体 flash 帧的字节级根因链(file:line + chunk 字节)、
现有架构是否足够的判断 + 若不足给出架构级方向、M1.9s 为何没生效的诊断。
**只审不修,不提交。**

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
