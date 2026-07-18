# Spike 03 修正：no-go 判过头了 —— 改判 A-with-caveats

> 前置：`8d399f2 spike: validate math latency and backpressure risks`
> 只做这一件事：**修正 `docs/spikes/03-math-engine.md` 的判决。不碰 04/05/06，不进 M0，不重跑 benchmark。**
> 这是文档修正 + 语料/证据核对，不是重做 spike。工程做对了，判决读错了。

## 零、先说清楚：哪些是对的，别动

- **310/310 返回 SVG、视觉结构正确、进程隔离（Job Object 32 MiB 硬上限 + 私有字节看门狗 + `TerminateJobObject` 真抢占）** —— 全部属实，已复核。**「超时即抢占」是真的**：`hang` worker 死睡不返回，父进程轮询到超时后外部 kill，不是任务返回后看表。这些是本 spike 最有价值的产出，保留。
- **C（QuickJS + KaTeX）的 no-go 是对的**：KaTeX 只出 HTML/MathML，无 SVG 输出模式，`foreignObject` 不是 wgpu 可直接消费的独立 SVG，且没有 KaTeX→SVG 路径（MathJax-SVG 是另一个引擎，不在任务书指定的路径 C 内）。这条判得干净，保留。

## 一、【核心】A 的"一票否决"是误判 —— 那是 adapter 的 bug，不是引擎的能力边界

**报告第 10 行**说 A 的 9 个自定义宏缺 h/d 是"按任务定义一票否决"。**这个读法错了，而报告自己的数据就是反证。**

**去核语料**：`corpus/math-expressions.jsonl` 的 30 条 `custom-macro` **结构完全相同**——都是 `\newcommand{\vect}[1]{\mathbf{#1}} \vect{v_N} \cdot \vect{w_{N+1}}`，只差一个整数下标。引擎每次都渲染"粗体向量·粗体向量"。**而 `docs/spikes/artifacts/03-math-engine.json` 里，这 30 条完全相同的输入，21 条出 h/d、9 条不出。**

**同样的输入，时灵时不灵，这不是"引擎给不出基线"，是 adapter 的 `find_math_metrics`（`src/lib.rs:288` 那段 ~40 行的 Typst frame 遍历）对某些 frame 布局返回了 `None`。** 报告的"待确认根因"（建议里写着"确认是 MiTeX 输出、Typst frame 结构还是 adapter 提取错误"）和"一票否决"**自相矛盾**——一个东西不可能既是"先修这个"又是"这个杀死整条路"。

**任务书里 h/d 的否决**（那一节）针对的是**一条路径根本给不出 h/d**（它举的 org-mode 例子：只有栅格 PNG、基线数据压根不存在）。**A 给出真实的 Typst frame 基线，301/310，而且每一个难的结构类别——分数/根号/矩阵/对齐/大运算符/伸缩定界符/unicode——都 100% 有**（去核 `missing_h_d_by_category`：只有 `custom-macro: 9` 和 `malformed: 18`；那 18 个畸形输入本就不该有基线）。**真实缺口 = 9 个结构相同的合成宏 + 一个自认可修的提取 bug。**

**要求**：把 A 的 h/d 缺口从"一票否决"改写为"**必修 caveat**"——定位并修 `find_math_metrics` 对那 9 个 frame 的提取，或明确写出根因是 MiTeX/Typst/adapter 哪一层。**它是"锁定 M3 前必须修好"，不是"否决 A"。**

## 二、【核心】auto-linebreak 被错误地列成了门槛

**报告第 8 行**把"窄宽公式可读性"（= 自动折行）列为四个门槛之一，说没有路径同时满足。**这违背任务书**——`codex-M-1-spikes.md` 任务 03 白纸黑字：

> "不能断的路径不等于出局（原型现在就是横滚，三家参照物也都横滚），但这个代价必须写在选型建议里，**由我来权衡**。"

**auto-linebreak 是留给人权衡的 caveat，不是门槛。** 报告正文（遗留风险/降级预案/折行一节）处理对了，**但头条结论把它抬成了门槛**。

**要求**：头条结论删掉"窄宽公式可读性"这个门槛项，把 auto-linebreak 明确降为 caveat，注明"KaTeX/路径 C 继承此残疾；A（Typst）与 B（MathJax 系）能不能断需后续单独评估；权衡交人"。

## 三、【应修】B（ReX）"Windows 上死了"说过头了

第 11 行的构建失败**按任务书规则记录是允许的**（"依赖已死/无法编译，如实记录后跳过"），这点没问题。**但 `rex-svg-probe/Cargo.toml` 只试了 `cairo-renderer` 那一个 feature**——也就是上游的 Cairo 示例。它**没有证明 Cairo 是 ReX 唯一的 SVG 出路**：Cairo 在 Windows MSVC 上能用 vcpkg/gvsbuild 构建（Inkscape/GIMP 都在 Windows 上发它），而 ReX 历史上有非 Cairo 的输出后端没被探。

**要求**：把"ReX 在 Windows 无可部署路径"（遗留风险第 4 条）改写为"**本轮只探了 Cairo 示例 feature 即失败；未证明这是唯一 SVG 路径，未评估 vcpkg 构建 Cairo 或非 Cairo 后端**"。**ReX 是唯一纯 Rust、原生 SVG 的引擎——A 万一 caveat 致命时的天然备胎，标记为「本轮未充分评估」，不是「判死」。**

## 四、【改】总判决：no-go → go-with-caveats（选 A）

**要求**：把结论从 `no-go` 改为 **`go-with-caveats`，选 A（MiTeX + Typst）作为 M3 引擎**，携带两条 caveat：
1. **9 宏的 h/d 提取 bug 必修**（锁定 M3 前定位并修 `find_math_metrics`，或明确根因）；
2. **无自动折行**（KaTeX/C 继承；A/B 的折行能力待后续评估；代价交人权衡）。
备胎与后续：**ReX 保留为未充分评估的纯 Rust 备胎，C 保留为离屏浏览器/HTML 研究方向。** 更新 `docs/spikes/README.md` 的 03 行（no-go → go-with-caveats）。

## 五、不要做

- 不要为了让判决好看而改语料期望值或 benchmark 数字。**分歧和缺口是产出。**
- 不要碰 04（已定案）、05、06、README 的其它行。
- 不要进 M0。
- 不要重跑 benchmark 或新增 workspace 成员；`vendor/alacritty_terminal` 保持在根 members，spike 保持在根 workspace 外（`cargo metadata` 自证）。

## 六、交付要求

每条修改说明：**改了什么、为什么（对着任务书哪一句 / 哪个 artifact 数字）**。完成后 `cargo test --manifest-path spikes/03-math-engine/Cargo.toml --locked` 仍绿，`cargo metadata` 自证 spike 仍在根 workspace 外。
