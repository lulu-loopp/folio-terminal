# M-1 剩余 spike 执行提示词（03/04/05/06）

> 复制分隔线以内的内容给 Codex。四个任务彼此独立，可以一个会话做完，也可以分四个会话并行（每个会话带上「通用上下文」+ 对应任务）。

---

## 通用上下文（每个会话都要带）

你在实现 BetterTerminal——为 AI agent 时代设计的 Windows 原生终端（Rust + wgpu）。

**当前状态**：M-1 风险消减阶段的**第一部分已通过审核**（语料基建 + 双平面原型，三门 G1/G2/G3 全过，224 tests，验收报告见 `docs/reviews/claude-review-M-1-part1-signoff.md`）。现在做 M-1 的**剩余四个 spike**。做完这四个才能进 M0。

必读：
1. `docs/DESIGN.md` —— 技术规格 v3.3，**唯一权威**。重点 §1.3（线程与资源契约）、§2（渲染管线与延迟指标）、§5（数学渲染管线）、§8（依赖策略）、§9（M-1 定义）、§10（风险登记）。
2. `docs/prompts/codex-M-1.md` —— M-1 原始任务书（这四个任务的原始描述在里面）。
3. `docs/reviews/claude-review-M-1-part1-signoff.md` —— 上一轮验收（含 M0 携带事项）。
4. `docs/spikes/01-corpus.md`、`02-dual-plane.md` —— 已完成部分的报告，照它们的格式写。

**硬性规则（前三轮审核换来的，务必遵守）**：

- **以规格与 upstream 契约为准推导，不要照着别人给的复现步骤或样例做局部修补。** 前两轮的返工惨案全部源于此：为了让某条复现步骤"字面通过"而改状态机、只修 row 0。审核给的东西是**症状**，规格才是**权威**。如果你认为规格本身有问题，写「偏离申请」等裁决，不要自行偏离。
- **spike 的产出是结论与数据，不是能用的产品代码。** 但结论必须有**可复现的实测数据**支撑，不能是"看起来可行"。
- **不许假装通过**：no-go 就写 no-go。上一轮首次交付把零件级单测误报成三门全过，被打回重做——如实的 no-go 比虚假的 go 有价值得多。
- 每个 spike 产出 `docs/spikes/0X-<名称>.md`，含：**结论（go / no-go / go-with-caveats）**、支撑数据、遗留风险、对后续里程碑的影响。
- 探针/试验工程不要污染主 workspace（放 `spikes/` 子目录或独立 workspace，在报告里说明位置）。**`vendor/alacritty_terminal` 必须留在 `workspace.members`**（这是上一轮建立的回归防线，任何把它移出的改动视为回归）。
- 做完停下，等待人工审核。

---

## 任务 04：Windows 中文 IME + CJK 宽度（**优先级最高——它决定 M0 的技术栈**）

M0 要在 winit + wgpu + cosmic-text 上搭窗口。这个 spike 的作用是：**在搭之前，确认这套栈在中文输入下到底能不能用**。如果 winit 的 IME 不够用，M0 就得直接上 Windows TSF——这个结论必须现在拿到，不能等 M0 搭完再发现。

- winit 最小窗口：验证 `Ime::Preedit` / `Ime::Commit` 事件、`set_ime_cursor_area` 候选框跟随。
- **实测至少三家输入法**：微软拼音 + 搜狗 + 微信输入法（或其他主流第三方）。逐项记录：预编辑文本是否正确、候选框是否跟随光标、中途切换输入法、中英切换、退格删除预编辑、Esc 取消、双拼/全拼。
- 列出 **winit IME 的能力边界**：哪些场景必须绕过 winit 直接调 TSF。这是本 spike 最重要的产出。
- cosmic-text 侧：CJK / emoji / ZWJ 序列 / 变体选择符的整形结果，与终端 cell 宽度策略的对齐方案（**宽度权威在 bt-term，cosmic-text 只做整形，其自然 advance 只能被约束/居中/裁剪**——见 DESIGN.md §2）。记录字体初始化耗时。
- 对照 `docs/research/01-existing-terminal-pain-points.md` 里的 WT #370（CJK 宽字符错位）、#900（emoji 半宽）案例集，看我们的宽度策略能否算对。
- 报告：`docs/spikes/04-ime-cjk.md`。**结论必须明确回答：M0 能不能用 winit 的 IME 起步？**

## 任务 03：数学渲染引擎选型

- 三条路径各做原型：
  - A. `mitex`（LaTeX→Typst）+ `typst` as lib → SVG
  - B. ReX 系（评估最活跃的 fork）
  - C. 嵌入 QuickJS（`rquickjs`）跑 KaTeX → SVG
  - 任一路径若依赖已死/无法编译，如实记录后跳过（这也是结论）。
- **样本 ≥300 条**：AMS 环境、矩阵、多行对齐、Unicode math、自定义宏、**畸形/恶意输入**（不闭合、指数爆炸、超长）、超大表达式。可脚本抓取 arXiv 摘要或 Wikipedia 数学条目 + 手工补恶意样本。样本集入库。
- 测量：渲染成功率、视觉质量抽查、p50/p95 耗时、内存峰值、二进制体积增量、沙箱可行性（禁文件/网络）、**进程级隔离**（Job Object 限时限内存后 kill）的可行性与开销（DESIGN.md §1.3 要求"超时即抢占"，"任务返回后再看时间"不算超时控制）。
- 报告：`docs/spikes/03-math-engine.md`，给出**选型建议 + 降级预案**（若全部 no-go，P0-1 按 R10 降级——只留块级 / 或退到 tui-math 字符网格排版）。

## 任务 05：延迟测量基建

- 实现"**事件 → present 提交**"打点框架。本阶段无真渲染器，在最小 wgpu 窗口上验证方法学：注入按键 → 清屏绘制一帧 → 统计分位数，**60/120/144Hz 各测**。
- 按 DESIGN.md §2：指标是 p50 < 8ms / p99 < 20ms，测量方法是**在持续输出洪水中注入可回显按键**（不是 `cat 100MB` 的静态场景）。
- 另做一次**光子侧基线**校准（高速摄像头或光敏电阻），量出"present 提交"与"实际上屏"的偏差。没有设备就如实说明并给出替代方案与误差范围。
- 产出可复用的 bench 工具（M0 起持续使用）。
- 报告：`docs/spikes/05-latency.md`。

## 任务 06：背压与取消验证

- 按 DESIGN.md §1.3 的容量契约搭最小管线：模拟 PTY 洪水源 → 有界 channel → per-session actor → coalescing 快照 → 假 worker（可取消、带 generation）。
- 验证：洪水下内存有界、输入通道不被饿死、过期任务取消率、shutdown 不死锁、**1-2 核降级路径**、不可见会话 1/8 保底份额、worker 队列满时的"同 span 替换 → 拒绝 + retry-on-idle"。
- 注意：双平面原型里已实现 `PARSE_QUANTUM=256KiB` 和分包不变性，**不要重复**——本 spike 关注的是**队列/背压/取消/公平性**这一层。
- 报告：`docs/spikes/06-backpressure.md`。

---

## 交接

四份报告完成后更新 `docs/spikes/README.md` 的汇总表（各 spike 的 go/no-go）。之后由 Claude 审核；审核通过才进 M0。

**特别提醒**：如果任务 04 的结论是"winit IME 不可用"，或任务 03 三条路径全 no-go，**这会改变 M0/M3 的技术方案**——请在报告里明确写出对里程碑的影响和建议的替代路径，不要自己闷头改架构。
