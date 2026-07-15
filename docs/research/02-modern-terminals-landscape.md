# 现代终端功能与口碑调研（2026-07）

> 调研方法：8+ 次搜索覆盖 Warp/Ghostty/WezTerm/Kitty/Alacritty/Rio/Tabby/Hyper/Wave/Extraterm 及 AI agent 场景；深读 Termdock、Wave 评测、dasroot.net、yaw.sh、Agent-Aware Terminals、superterm.dev、cmux 等 7 篇文章。

## 一、各终端：亮点 + 吐槽点

### Warp
- 亮点：Block 交互模型（命令+输出成对折叠/复制/re-run）；AI 原生（内联补全、Agent Mode 多智能体、云端 Oz）；2026-04 客户端已开源（MIT/AGPL），新增离线模式。
- 吐槽：历史上强制登录引发反感；关闭遥测则 AI 不可用；体积 ~300MB（Ghostty 的 6 倍）、订阅 $18–180/月；核心 Oz 仍闭源。

### Ghostty
- 亮点：Zig + Metal，读文本速度约 iTerm2/Kitty 的 4 倍，~45MB 内存、120fps、亚 2ms 延迟；真平台原生 UI；libghostty 被十余项目复用。
- 吐槽：无会话管理、无 AI；**无 Windows 版**；SSH terminfo 兼容问题；无 agent 感知能力。

### WezTerm
- 亮点：Lua 高度可编程；内置 multiplexer；图形协议支持最广（**唯一三协议全支持**）；内置 SSH。
- 吐槽：Lua 学习曲线陡；内存 ~320MB；发布节奏不规律。

### Kitty
- 亮点：Kitty Graphics Protocol 事实标准（动画、分层、placement）；kittens 插件；IME 好评；60–100MB。
- 吐槽：维护者风格强硬常 "won't fix"；反 tmux 哲学；无 GUI 配置。

### Alacritty
- 亮点：极简极致性能，~30MB；其 VT 解析器/grid 被 Ghostty、Rio 复用。
- 吐槽：故意不做标签/分屏/连字/图像，功能不足需 tmux 补齐。

### Rio
- 亮点：Rust + WebGPU；支持 iTerm2/Kitty/Sixel + RetroArch 着色器；目标桌面+浏览器（WASM）。
- 吐槽：跨平台一致性不足、缺右键菜单、分屏不可调；被质疑"又一个 Rust 重写"。

### Tabby
- 亮点：高度可配置、SSH/Serial/分屏一体化。
- 吐槽：Electron 闲置 300MB+、启动慢；插件生态声量渐弱。

### Hyper
- 吐槽为主：Electron 拖累（2016 i7 启动 2s+、输入延迟明显）；开发基本停滞；"Electron 终端反面教材"。

### Wave Terminal
- 亮点：Block 工作区 + AI block（自带 key 接 Claude/OpenAI/本地模型）；**内联文件预览杀手级**（CSV→表格、PDF/PNG 原生显示）；持久化 SSH；开源免费。
- 吐槽：**跑 Claude Code/Codex 等 CLI agent 时孤立光标、卡顿重绘、状态栏错乱**；Block 范式与传统肌肉记忆冲突；Electron 内存 400–800MB。

### Extraterm
- 亮点：命令面板、`from` 命令复用历史输出、Colorizer 关键词高亮、Cursor Mode 编辑历史输出；仍在维护（2026-05 v0.82.0）。
- 吐槽：知名度和生态小。

## 二、"现代终端必备"清单（Table Stakes）

| 类别 | 功能 |
|---|---|
| 渲染 | 24-bit 真彩（41/44 终端已支持）、GPU 加速、连字、完整 Unicode/CJK/Emoji |
| 布局 | 标签页（33/44）、分屏（23/44）、大容量可搜索回滚 |
| 图形 | 至少支持 Kitty/Sixel/iTerm2 之一 |
| 交互 | 顺畅复制粘贴、正则搜索、Shell 集成（命令边界/退出码检测） |
| 远程 | SSH 集成或持久会话、tmux control mode 或等效 |
| 配置 | 合理默认值 + 可编程配置 |
| 性能 | 低内存、低输入延迟、高帧率滚动（应对 AI agent 流式输出） |

**共识：单纯"渲染快"已是红海（Ghostty/Alacritty/Rio/Kitty 都够快），战场已转移到多会话编排和 AI 时代可用性。**

## 三、空白需求（差异化机会）

2026 年已出现一批专为 agent 时代设计的新项目（cmux、superterm、herdr、agterm、pilotty），证明主流终端在以下方面空白：

1. **Agent 状态感知与统一通知（最大空白）**：多 agent 并行时一眼看出哪个完成/报错/等输入/卡权限墙。仅 cmux（视觉光环+侧边栏）、superterm（火花线图）在做，Ghostty/WezTerm/Kitty 全空白。
2. **跨 Pane 元数据侧边栏**：git 分支、PR 状态、cwd、端口、agent 最新消息摘要。
3. **可编程 UI / Agent 反向操控终端**：仅 cmux 有 Unix socket API。
4. **ANSI/流式渲染鲁棒性**：CLI agent 的高频富 ANSI 输出让 Electron 终端出视觉 bug——"正确稳定渲染 agent 的 TUI 输出"本身是未解难题。
5. **Markdown/富文本原生渲染**：agent 大量输出 Markdown，没有终端能就地渲染（用户靠 `glow`）。
6. **会话持久化 + 长任务可恢复**：不止 tmux 式文本恢复，还包括 agent 进度。
7. **多设备/远程监控**：superterm 主打手机监控解除 agent 阻塞。
8. **"注意力管理"范式**：把并行 agent 当"团队"管理而非多个 shell 窗口。

## 来源
- https://www.termdock.com/en/blog/best-terminal-emulator-ai-cli-2026
- https://moltamp.com/blog/wave-terminal-review-2026/
- https://dasroot.net/posts/2026/03/linux-terminal-emulators-alacritty-kitty-wezterm/
- https://yaw.sh/blog/best-terminal-for-claude-code/
- https://codex.danielvaughan.com/2026/04/29/agent-aware-terminals-codex-cli-warp-cmux-ghostty-choosing-terminal-emulator/
- https://superterm.dev/ · cmux (dev.to) · Terminal Trove 对比表 · Kitty/Ghostty/Rio/Tabby 官方与社区讨论
