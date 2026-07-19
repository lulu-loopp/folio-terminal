# M1.9f 任务书:多行环境塌陷(阻断)+ 静默不渲染诊断 + 滚动闪回 + 行内公式

## 【续跑说明 2026-07-19,第三轮】

前两轮均被外部中断(第一轮是沙箱 `CreateProcessAsUserW 1312` 登录会话
失效,你正确地选择停下不盲写;第二轮跑到一半被 kill)。**工作树保留着
你的半成品**,协调者实测:

- 12 个文件已改(含 `baseline_subpixels`、`mode: MathMode` 字段的引入
  ——看得出行内公式的基线对齐已开工);
- `cargo check --workspace --tests` **通过**;
- `cargo test --workspace` **400 passed / 1 failed**,唯一失败:
  `session::tests::math_worker_intermediate_success_failure_and_layout_
  invalidation_are_projectable`(`session.rs:3776`)。

请**接着继续**,不要推倒重来:

1. 先修那条失败(很可能是新增字段后夹具未同步,与本任务的四条无关);
2. **逐条自查四条完成度**(多行塌陷 / 静默不渲染诊断 / 滚动闪回 /
   行内公式),在答复里明确"已完成 / 未完成",补齐未完成的;
3. 跑完整门禁,数字从实跑输出抄写并附原始片段;第一遍任何失败如实报告。

若判断已有半成品方向有误,停下说明理由再动。

档位 xhigh(第 2 条需诊断,第 4 条是新能力)。基线 = 提交 `d71d983`。
全部四条来自用户的系统性 LaTeX 测试(2026-07-19,三张实拍截图)。

---

## 阻断 1:多行环境全部塌成一行(矩阵 / cases / align)

**现象(用户实拍)**:
- `\begin{cases}` 分段函数 → `sgn(x) = {+1x > 0 0x = 0 −1x < 0}`,
  换行与对齐全失,大括号未随行数拉伸;
- 矩阵 → `A = (a11 a12 a13 a21 a22 a23 a31 a32 a33)`,九元素排成一行,
  括号未拉伸;`[1 0 0 1][x y] = [x y]` 同;
- `align` → `(a+b)^2 = a^2+2ab+b^2 (a-b)^2 = ... (a+b)(a-b) = ...`
  三式挤成一行。

**协调者定位的根因(请验证后修)**:
`crates/bt-math/src/lib.rs:24` 的 Typst 模板:

```typst
#let converted = eval("$" + sys.inputs.source + "$", scope: mitex-scope)
```

Typst 语义:`$x$`(定界符紧贴内容)= **inline 数学**;
`$ x $`(定界符与内容间有空白)= **block/display 数学**。
**inline 模式不换行**——MiTeX 把 LaTeX `\\` 转成的 Typst 换行在 inline
下被忽略,于是所有多行环境塌成一行,`\left(`/`\right)` 之类的可伸缩
定界符也因此不拉伸。

**要求**:
1. 按 `key.mode` 选择正确的 eval 形式:Display → `"$ " + source + " $"`;
   Inline → `"$" + source + "$"`。**先验证这个假设**(可用一个 cases /
   matrix 源做最小复现),验证结果写进答复;若根因不同,如实报告并按
   真因修;
2. 与 `#math.equation(block: ...)` 的关系一并厘清(两处都在控制 display
   语义,不要互相打架);
3. **回归**:cases / pmatrix / bmatrix / aligned / align 各一,断言
   **栅格高度显著大于单行高度**(证明确实多行)且**宽度不异常**;
   建议同时断言行数(可用栅格里非空白行的分段数近似)。
4. **310 语料门禁必须重跑**——本改动影响所有公式的排版模式,
   语料里若有多行样本,其基线尺寸会变;如实报告变化并确认是改善。

## 阻断 2:部分公式静默保持源码,用户不知为何

**现象(用户实拍,三条都没渲染)**:
- 留数定理:`$$ f(z) = \frac{1}{2\pi i} \oint_{\gamma} \frac{f(\zeta)}{\zeta - z}\,\mathrm{d}\zeta, \quad \left| \frac{\partial^2 u}{\partial x^2} + \frac{\partial^2 u}{\partial y^2} \right| \leq \epsilon $$`
- 麦克斯韦:`$$ \begin{aligned} \nabla \cdot \mathbf{E} &= ... \end{aligned} $$`
- 符号密度:`$$ \alpha \beta \gamma \delta ; \Gamma \Delta \Theta \Lambda ; \aleph_0 \in \mathbb{R} \subseteq \mathbb{C}, \quad A \cup B, ; A \cap B, ; \varnothing $$`

**要求**:
1. **诊断每一条的失败点**:是 `validate_source` 拒绝?`mitex::convert_math`
   报错(哪个命令不支持)?Typst 编译失败?还是根本没被检测到
   (九类拒绝里的哪一条)?**逐条给出结论与 file:line**;
2. 能修的修(如 MiTeX spec 缺项、我们的检测规则过严);
3. **不能修的必须可见**:静默保持源码在"我们主动不渲染"时是对的,
   但在"渲染失败"时用户无从知晓。给失败的块一个**克制的可见提示**
   (例如块右侧一个淡色标记,hover/工具条可看原因),不要弹窗、不要
   改动源码文本。设计上与 §7.2「一个区域只用一种语言说」一致;
4. `BT_PERF_TRACE` 增加失败原因计数(convert/compile/validate 各一)。

## 阻断 3:滚动时先闪回源码再重新渲染,观感割裂

**现象**:滚动过程中已渲染的公式**先变回源码**,停下后才重新渲染。

**根因方向**:滚动当前被当作强失效条件,而它**不改变内容**。
M1.9e 已确立"失效判据看内容不看事件"的原则,滚动是同一原则的遗漏。

**要求**:
1. 滚动**不得**使 live 装饰失效——内容未变则 artifact 存活;
2. 明确哪些仍必须失效:内容真变、清屏、alt 切换、resize、
   generation/layout 变化;
3. 回归:滚动 N 次(含 live↔转录边界来回)断言 artifact 未被重建
   (worker 任务数不增、Arc 指针不变);
4. 若滚动涉及 live→frozen 交接,复用 M1.9e 已有的 artifact 交接路径,
   同样不重排。

## 4. 行内公式 `$...$` 支持(新能力,用户期待)

**现象**:用户测试的第一类就是行内公式(`$E = mc^2$` 混在中文里),
当前**完全不检测**,全部显示源码。

**要求**:
1. 检测器支持 `$...$` 行内定界符,**九类误报纪律同等适用**并额外注意
   行内的高误报风险:货币符号(`$5 和 $10`)、shell 变量(`$PATH`、
   `$1`)、代码中的 `$`;**必须给出你的消歧规则与依据**,宁可漏检
   不可误检;
2. 排版模式按定界符决定(`$...$` → `MathMode::Inline`,
   `$$...$$` → Display)——**这是已确立的原则,不许由生命周期决定**;
3. 行内公式的几何:高度贴合行高(inline 排版天然如此),**不撑高行**;
   若某个行内公式排版后仍超出行高,保持源码;
4. 同一行内可有多个行内公式;行内公式与周围文本的基线对齐要正确
   (用户明确会检查"行内跟中文混排时基线/高度对不对");
5. 回归:多个行内公式同行、中英混排基线、误报集(货币/shell 变量/
   代码)全部保持源码。

---

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、m1.8 系与 M1.9
全部回归、clippy `-D warnings`、fmt;vendor/glyphon 零改。
门禁数字从实跑输出抄写并附原始片段;**第一遍出现的任何失败如实报告**。
结果写在最终答复:四条各自的根因/修复 file:line、阻断 2 的逐条诊断表、
行内消歧规则论证、新增回归清单、门禁数字。停下等审,不提交。
