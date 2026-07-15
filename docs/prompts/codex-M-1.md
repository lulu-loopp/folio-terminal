# M-1 阶段执行提示词（复制给 Codex 用）

> 使用方法：把下面分隔线以内的内容整体复制给 Codex。建议在 `D:\Developer\BetterTerminal` 目录下启动。
> M-1 体量较大，如果单次会话跑不完，可按"任务拆分建议"分多个会话，每个会话粘贴对应小节 + 「通用上下文」。

---

## 通用上下文（每个会话都要带）

你在实现 BetterTerminal——一个为 AI agent 时代设计的 Windows 原生终端（Rust + wgpu）。当前阶段是 **M-1 风险消减**：只做风险验证的 spike 和核心逻辑原型，**不做 GUI 产品代码，不进入 M0**。

开工前必读（按顺序）：
1. `docs/DESIGN.md` —— 技术规格 v3.3，已经过 7 轮审核定稿。**这是唯一权威**：workspace 划分（§1.2）、设计不变量（§1.1）、内容生命周期事件表（§3.1）、ContentAnchor（§3.2）、文档/视口投影（§3.3）、双状态机与四版本（§4.1）、并发契约（§1.3）都必须严格遵循。
2. `docs/PROBLEM-LIST.md` —— 需求背景。
3. `docs/reviews/` —— 历轮审核记录，理解每个设计决策的来龙去脉（尤其 v4/v5 关于 resize 语义的讨论）。

硬性规则：
- 如需偏离 DESIGN.md 的任何决策，**不要自行偏离**：在报告中开一节「偏离申请」写明原因与替代方案，代码按原规格实现或留 TODO，等待审核裁决。
- 每个 spike 产出一份报告 `docs/spikes/0X-<名称>.md`，含：结论（go / no-go / go-with-caveats）、支撑数据、遗留风险。
- 测试必须可用 `cargo test` 一键运行；语料文件放 `corpus/`。
- 完成本会话的任务后**停止**，不要顺手开始下一项或 M0。人工审核会对照 G1/G2/G3 验收（见 DESIGN.md §9）。

## 任务拆分建议（依赖关系）

- 会话 A：任务 1（语料基建）→ 任务 2（双平面原型）。2 强依赖 1，必须同会话或先后完成。这是 M-1 的主线。
- 会话 B：任务 3（数学渲染 spike），独立。
- 会话 C：任务 4（IME/CJK spike），独立。
- 会话 D：任务 5（延迟基建）+ 任务 6（背压验证），可合并。

## 任务 1：语料录制与回放基建

- 建立 Rust workspace（按 DESIGN.md §1.2 的 crate 划分，先只创建本阶段需要的：`bt-term`、`bt-transcript`、`bt-doc`、`bt-viewport`、`bt-detect` 的空骨架 + 一个 `bt-corpus` 测试支撑 crate）。
- 写一个录制工具：通过 ConPTY（`portable-pty`）跑任意命令，把原始字节流带时间戳录成文件（格式自定，需支持精确回放，含 resize 事件标记）。
- 写回放框架：把语料喂给 `alacritty_terminal`（scrollback 配置为 0，见 DESIGN.md §8），支持按任意分包粒度切割字节流（模糊测试需要）。
- 录制首批语料：pwsh 日常命令、`cargo build` 洪水输出、vim 进出与编辑、htop/top 类持续重绘、Claude Code 一段真实会话（如环境没有则用 ink demo 或任何持续重绘底部区域的 TUI 替代并注明）、含大量 `$` 的 shell 输出、窗口 resize 序列。
- 报告：`docs/spikes/01-corpus.md`。

## 任务 2：双平面最小原型（M-1 主线，G1/G2/G3 的载体）

纯逻辑、无 GUI。实现 DESIGN.md 的核心协议：

- `bt-term` 适配层：包装 alacritty_terminal（scrollback=0），实现**上滚移出行捕获**（评估现有 API/damage 信息是否够用；不够则 vendor + 最小补丁，并在报告记录补丁面）。
- `bt-transcript`：staging（StagingId、4K 配额、强制拆分/强制定稿规则）、冻结定稿、规范化（trailing blank 截断、grapheme 边界表、OSC 8 元数据保留）、tombstone。
- `bt-doc` / `bt-viewport`：HistoryDocument 与 ViewportProjection 分离、每视口 i64 定点高度树、布局缓存键 `(span, source_gen, detection_rev, LayoutKey)`、滚动锚定。
- `ContentAnchor`：三变体（History/Staging/Live）、按 Screen 分命名空间的全序、Bias、两步原子迁移（捕获时 Live→Staging、定稿时 Staging→History）、ED3/淘汰降级规则。
- `bt-detect` 最小版：只需块级 `$$...$$` 定界识别 + 四态门控（Mutable→Quiescent→Frozen→Decorated 的源码/装饰双状态机），渲染可用假的（返回固定高度的占位 artifact）——本阶段验证的是**协议**，不是真渲染。
- **验收 = 三道门**（写成 `cargo test` 套件）：
  - G1：DESIGN.md §9 列出的全部生命周期回放矩阵场景；
  - G2：同一转录在两个不同宽度视口同时投影，高度树/选区/滚动锚独立且正确；
  - G3：持久锚点两步迁移原子性、ED3 降级、worker 任务 generation 失效丢弃、四版本失效边界。
- 报告：`docs/spikes/02-dual-plane.md`（含三道门的通过情况，任何一门不过必须如实标注 no-go 并分析）。

## 任务 3：数学渲染引擎 spike

- 三条路径原型：A. `mitex`（LaTeX→Typst）+ `typst` as lib → SVG；B. ReX 系（评估最活跃 fork）；C. 嵌入 QuickJS（`rquickjs`）跑 KaTeX → SVG。任一路径若依赖已死/无法编译，如实记录后跳过。
- 样本 ≥300 条：AMS 环境、矩阵、多行对齐、Unicode math、自定义宏、**畸形/恶意输入**（不闭合、指数爆炸、超长）、超大表达式。可用脚本从 arXiv 摘要或 Wikipedia 数学条目批量抓取 + 手工补充恶意样本。
- 测量：渲染成功率、视觉质量抽查、p50/p95 耗时、内存峰值、二进制体积增量、沙箱可行性（禁文件/网络访问）、**进程级隔离**（Job Object 限时限内存后 kill）的开销。
- 报告：`docs/spikes/03-math-engine.md`，给出选型建议与降级预案。

## 任务 4：Windows 中文 IME + CJK 宽度 spike

- winit 最小窗口：验证 `Ime::Preedit/Commit` 事件、`set_ime_cursor_area` 候选框跟随，实测微软拼音 + 至少一家第三方输入法（搜狗/微信输入法）。
- cosmic-text 侧：CJK/emoji/ZWJ 序列的整形结果 vs 终端 cell 宽度策略（宽度权威在我们，整形只能被约束）的对齐方案原型；记录字体初始化耗时。
- 报告：`docs/spikes/04-ime-cjk.md`（含 winit IME 的能力边界：哪些场景必须绕过 winit 直接 TSF）。

## 任务 5：延迟测量基建

- 实现"事件→present 提交"打点框架（本阶段无真渲染器，先在最小 wgpu 窗口上验证方法学：注入按键→清屏绘制一帧→测量分位数，60/120/144Hz 各测）。
- 输出为可复用的 bench 工具，M0 后持续使用。
- 报告：`docs/spikes/05-latency.md`。

## 任务 6：背压与取消验证

- 按 DESIGN.md §1.3 的容量契约搭最小管线：模拟 PTY 洪水源 → 有界 channel → 解析线程 → coalescing 快照 → 假 worker（可取消、带 generation）。
- 验证：洪水下内存有界、输入通道不被饿死、过期任务取消率、shutdown 不死锁、1-2 核降级路径。
- 报告：`docs/spikes/06-backpressure.md`。

---

## 审核交接

全部（或阶段性）完成后，在 `docs/spikes/README.md` 汇总各 spike 的 go/no-go 结论。之后由 Claude 对照 DESIGN.md §9 的 G1/G2/G3 逐条审核，审核通过才进入 M0。
