# M-1 的四个 spike（03 / 04 / 05 / 06）

> 复制分隔线以内的内容给 Codex。四个任务**彼此独立、都不碰现有 crate**，可一个会话做完，也可分四个会话并行（每个会话带「通用上下文」+ 对应任务）。
> 它们和 `codex-M-1-baseline.md` 的任务 00 也不冲突，可以并行。
>
> **优先级：任务 04 最高**——它决定 M0 的技术栈。

---

## 通用上下文（每个会话都要带）

你在实现 BetterTerminal——为 AI agent 时代设计的 Windows 原生终端（Rust + wgpu）。

**当前状态**：M-1 风险消减阶段的**第一部分已通过审核**（语料基建 + 双平面原型，三门 G1/G2/G3 全过，224 tests）。工程基线（CI / lint 政策 / 规范）刚刚落地。现在做 M-1 的**剩余工作**：一次 hygiene + 四个 spike。做完才能进 M0。

**必读**：
1. **`CONVENTIONS.md`** —— 工程约定，**新增，开工前必读**。规则分【事故】/【预防】/【提示】三级，【事故】级的每条都注明了它来自哪次返工。特别注意 §0（按规格推导）、§3.2（默认值掩盖 bug——这是一个**族**）、§3.3（断言要能失败，**射程覆盖构建配置本身**）、§8（lint 阶梯）。
2. `docs/DESIGN.md` —— 技术规格 v3.3，**唯一权威**。重点 §1.3、§2、§5、§8、§9、§10。
3. `docs/reviews/` —— 五份审核记录。`claude-review-M-1-part1-signoff.md`（第一部分验收 + M0 携带事项）、`claude-review-conventions.md`（工程审核，含代码质量清单）。
4. `docs/spikes/01-corpus.md`、`02-dual-plane.md` —— 已完成部分，照它们的格式写报告。

**硬性规则**（前三轮审核换来的，`CONVENTIONS.md` 有完整版）：

- **以规格与 upstream 契约为准推导，不要照复现步骤做局部修补**。前两轮的返工惨案全部源于此，其中一条审核方给的复现步骤**本身就是错的**，照着修反而改坏了 VT 状态机。审核给的是**症状**，规格才是**权威**。认为规格有问题 → 写「偏离申请」，不要自行偏离。
- **no-go 是有价值的产出**。首次交付把零件级单测误报成"三门全过"，被打回重做。如实的 no-go 会被接受。
- **声明"已修复"必须指出哪个测试在修复前会红**。答不上来的按未修复处理。
- 探针/试验工程不要污染主 workspace。**`vendor/alacritty_terminal` 必须留在 `workspace.members`**（CI 用 `cargo metadata` 检查，移出视为回归）。
- 做完停下等审核。

---

---

## 任务 04：Windows 中文 IME + CJK 宽度（**四个 spike 里优先级最高——它决定 M0 的技术栈**）

M0 要在 winit + wgpu + cosmic-text 上搭窗口。这个 spike 的作用是：**在搭之前，确认这套栈在中文输入下到底能不能用**。如果 winit 的 IME 不够用，M0 就得直接上 Windows TSF——这个结论必须现在拿到，不能等 M0 搭完再发现。

- winit 最小窗口：`Ime::Preedit` / `Ime::Commit` 事件、`set_ime_cursor_area` 候选框跟随。
- **实测至少三家输入法**：微软拼音 + 搜狗 + 微信输入法（或其他主流第三方）。逐项记录：预编辑文本、候选框跟随、中途切换输入法、中英切换、退格删预编辑、Esc 取消、双拼/全拼。
- 列出 **winit IME 的能力边界**：哪些场景必须绕过 winit 直接调 TSF。**这是本 spike 最重要的产出。**
- cosmic-text 侧：CJK / emoji / ZWJ / 变体选择符的整形结果，与终端 cell 宽度策略的对齐方案（**宽度权威在 bt-term，cosmic-text 只做整形**，其自然 advance 只能被约束/居中/裁剪——DESIGN.md §2）。记录字体初始化耗时。
- 对照 `docs/research/01-existing-terminal-pain-points.md` 的 WT #370（CJK 错位）、#900（emoji 半宽）案例集。
- 报告：`docs/spikes/04-ime-cjk.md`。**结论必须明确回答：M0 能不能用 winit 的 IME 起步？**

## 任务 03：数学渲染引擎选型

- 三条路径各做原型：A. `mitex`（LaTeX→Typst）+ `typst` as lib → SVG；B. ReX 系（评估最活跃 fork）；C. 嵌入 QuickJS（`rquickjs`）跑 KaTeX → SVG。任一路径依赖已死/无法编译，如实记录后跳过（这也是结论）。
- **样本 ≥300 条**：AMS 环境、矩阵、多行对齐、Unicode math、自定义宏、**畸形/恶意输入**（不闭合、指数爆炸、超长）、超大表达式。可脚本抓 arXiv 摘要或 Wikipedia 数学条目 + 手工补恶意样本。样本集入库。
- 测量：渲染成功率、视觉质量抽查、p50/p95 耗时、内存峰值、二进制体积增量、沙箱可行性（禁文件/网络）、**进程级隔离**（Job Object 限时限内存后 kill）的可行性与开销（§1.3 要求"超时即抢占"，"任务返回后再看时间"不算超时控制）。
- 报告：`docs/spikes/03-math-engine.md`，给出**选型建议 + 降级预案**（若全部 no-go，P0-1 按 R10 降级——只留块级 / 或退到 tui-math 字符网格排版）。

> 参考：`design/ui-mockup.html` 里已经用真 KaTeX 演示了目标效果（点 claude 标签，或在任意标签敲 `math`）。**那是产品要达到的排版质量基线**——你选的引擎要能产出同等质量。

## 任务 05：延迟测量基建

- 实现"**事件 → present 提交**"打点框架。本阶段无真渲染器，在最小 wgpu 窗口上验证方法学：注入按键 → 清屏绘制一帧 → 统计分位数，**60/120/144Hz 各测**。
- 按 DESIGN.md §2：指标 p50 < 8ms / p99 < 20ms，测量方法是**在持续输出洪水中注入可回显按键**（不是 `cat 100MB` 的静态场景）。
- 另做一次**光子侧基线**校准（高速摄像头或光敏电阻），量出"present 提交"与"实际上屏"的偏差。没有设备就如实说明并给替代方案与误差范围。
- 产出可复用的 bench 工具（M0 起持续使用）。
- 报告：`docs/spikes/05-latency.md`。

## 任务 06：背压与取消验证

- 按 DESIGN.md §1.3 的容量契约搭最小管线：模拟 PTY 洪水源 → 有界 channel → per-session actor → coalescing 快照 → 假 worker（可取消、带 generation）。
- 验证：洪水下内存有界、输入通道不被饿死、过期任务取消率、shutdown 不死锁、**1-2 核降级路径**、不可见会话 1/8 保底份额、worker 队列满时的"同 span 替换 → 拒绝 + retry-on-idle"。
- 双平面原型里已实现 `PARSE_QUANTUM=256KiB` 和分包不变性，**不要重复**——本 spike 关注**队列/背压/取消/公平性**这一层。
- 报告：`docs/spikes/06-backpressure.md`。

---

---

## 交接

四份报告完成后更新 `docs/spikes/README.md` 的汇总表（各 spike 的 go/no-go）。之后由 Claude 审核；通过才进 M0。

**特别提醒**：如果任务 04 的结论是"winit IME 不可用"，或任务 03 三条路径全 no-go，**这会改变 M0/M3 的技术方案**——在报告里明确写出对里程碑的影响和建议的替代路径，**不要自己闷头改架构**。
