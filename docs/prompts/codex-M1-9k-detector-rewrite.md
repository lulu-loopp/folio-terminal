# M1.9k 任务书:公式检测器统一重写(审因 R1-R3)

档位 high(踩两条红线:绝不错配散文、绝不把非数学文字渲染成公式)。
基线 = 提交 `30a7aea`(M1.9j 已 stash,不在树里)。
**先读审因报告** `docs/reviews/M1.9-formula-pipeline-audit.md`——它有完整 file:line
因果链与形态全集,本任务书是它 R1-R3 的执行版。

## 为什么重写(不是再补 if)

现检测器是**三套不一致语义**拼的:单行 `$$X$$` 特判、`$$` 独占整行的多行
scanner、全局 `context_start_trusted: bool`。后果(审因证实):
- **FM1**:`$$` 与内容同行且跨行(`$$\oint…` 换行 `…\mathbf{A}$$`)**不检测**——
  这是 Claude Code 实际输出格式;
- **FM4-A**:主屏一长(>1024 行或有 tombstone)→ trust-bool = false →
  检测器**整体熄火**,连单行 `$$x$$` 都不试;
- **FM4-K**:非真 CommonMark → 4 空格缩进代码里的 `$$x$$` 被**误渲染**(踩
  "忠实显示文字"红线)。

## 三阶段(有序)

### R1 · 先建会红的端到端门(改任何生产码之前)

加回归,当前基线上**必须红**,重写后**必须绿**:
1. FM1 三族:`$$…`/`…$$` 跨行、`\[…`/`…\]` 跨行、`\begin{align}…`/`…\end{align}`
   与内容同行且跨行;
2. 主屏 >1024 行 / 有 tombstone 后,视野内单行 `$$x$$` 与完整环境**仍检测**;
3. **负例**:4 空格缩进代码块里的 `$$x$$` **不渲染**(保持字面);
4. alt 屏明确 clear(`\x1b[2J\x1b[H`)后整屏重画,新 snapshot 内的块正确检测,
   且**不与被清掉的历史前缀错配**;
5. 同一 LaTeX 在两种终端宽度(触发/不触发物理折行)下检测结果一致。
第一遍把这些跑红并把红的输出抄进交付,证明门有效。

### R2 · 建 `MathOccurrence` 数据

一个 occurrence 显式分离(不再让下游猜):
- `original_source`(含定界符的原样文本,供复制/显示源码)、
- `render_source`(送 MiTeX 的 body/环境)、
- `delimiter_kind`(Dollars/Brackets/Environment(name))、`mode`(Display;Inline 仍禁)、
- **逐行 cell segment map**:块跨的每个逻辑行 → 该行 source 占的精确终端 cell 列
  区间(供 show-source 高亮/选择/复制,取代 M1.9j 的单包围盒 `min..max` 与
  `unicode-width` 回算——要用终端已定的 grapheme/cell 宽度)。

### R3 · 统一 scanner 替换三套特判 + trust-bool

一个扫描器,输入**逻辑行**(用 `CapturedRow.continues`/WRAPLINE 从物理行重建,
消除 FM4-B 物理折行依赖)+ **真实初始解析状态**(取代 bool),token 携带
行 id 与 byte 范围。要求:
1. **形态全集**(审因列的,逐条回归):
   - `$$body$$` 单行 + `$$body…`/`…body$$` 跨行;
   - `\[body\]` 单行 + 跨行同行-body;
   - MiTeX 已验证的 equation/align/gather/multline/cases/matrix 家族,允许
     begin/end 与内容同行;
   - outer dollars 内嵌方向性环境;
   - `$...$` 与 `\(...\)` inline **保持禁用**;
2. **红线 A 绝不错配散文**:对称定界符(`$$`)跨不可信前缀的配对,靠**真实
   解析状态**判定;无法证明的对称前缀返回 **Ambiguous → 保留源码**,绝不猜配对
   把中间散文排成公式。非对称(`\[`/`\]`、begin/end)天然可辨,照常;
3. **红线 B 绝不误渲染文字**:实现**真 CommonMark 块上下文**(fenced ``` 与
   ~~~ **和** 4 空格/tab 缩进代码块)——代码上下文内的 `$$…$$` 一律字面;保留
   既有九类误报守卫与 CJK 散文守卫(`block_body_looks_like_prose`);
4. **不可信起点不再整体返回空**(废掉 FM4-A 的熄火):trust 变成真实解析状态,
   未知前缀只对**受该前缀影响的对称配对**判 Ambiguous,其余照常检测;
5. escaped 定界符、空块、未闭合、超 `MAX_MATH_SOURCE_BYTES` → 字面;
6. 复杂度:检测限定在**视口可见逻辑行 + 必要上下文**,不扫全历史(审因/既有
   O(n²) 教训);给复杂度论证。

## 硬约束(不可回退)

- 现有全部回归(310 语料、G1/G2/G3、bt-detect delimiter/environment、bt-math
  多行环境、m1.8/M1.9 命名)**不回退**;
- 两条红线各有**变异验证**:去掉对称-Ambiguous → 错配散文回归变红;去掉
  CommonMark 代码上下文 → 缩进代码误渲染回归变红;恢复后绿;
- alt 屏 M1.9i-B 的 `alternate_detection_context`(被移除行推进解析状态)是这次
  "真实状态取代 bool"的雏形,应被**纳入/推广**,不是丢弃;
- 主屏 `frozen_detection_context` 既有机制复用,别造第二套。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。门禁数字从实跑输出抄写并附原始片段;
第一遍任何失败如实报告。

## 交付(写进 output 文件)

R1 红门证据 → R2 数据结构 → R3 scanner 设计 + 形态覆盖清单 + 两条红线的不错配/
不误渲染证明(含变异验证)→ 复杂度论证 → 新增回归清单 → 门禁实跑数字。
停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
