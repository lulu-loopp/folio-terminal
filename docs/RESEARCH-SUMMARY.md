# BetterTerminal 调研总结与定位建议（2026-07）

> 详细报告见 `docs/research/`：01 现有终端痛点 · 02 现代终端格局 · 03 富内容渲染技术

## 核心结论

1. **"渲染快"已是红海**，Ghostty/Alacritty/Kitty 都够快；真正的空白是 **AI agent 时代的富内容渲染与多会话可观测性**。
2. **没有任何通用终端能自动检测并渲染流内的 LaTeX/Markdown**——这正是本项目的初始动机，且被调研证实为真实空白（用户靠 glow/MathRender 等外部工具凑合）。
3. **技术上的决定性判断：走"终端自渲染"而非"图形协议透传"**。终端本来就解析整个字节流，在解析层检测 `$...$` 并自己画，不需要子进程配合，还天然绕开 Windows ConPTY 的 DCS 过滤限制。Warp 的 BlockList 架构证明了可行性。
4. **Electron 是前车之鉴**（Hyper/Tabby/Wave 全部因内存/延迟/agent 输出渲染 bug 被吐槽），但 Web 栈做富内容混排最快——所以采取两阶段策略。

## 产品定位

**"为 AI CLI 时代设计的 Windows 终端"** —— 三大支柱：

### 支柱一：富内容自渲染（核心差异化，没人做）
- 自动检测并渲染 LaTeX 公式（$...$、$$...$$、\[...\]）
- Markdown 就地美化渲染（代码块高亮、表格、标题）
- 检测策略：alt-screen 门控 + 稳定文本判定，不破坏 vim/htop/ink

### 支柱二：Agent 感知（第二差异化，仅小众工具在做）
- 多 agent 并行时的状态一览（完成/报错/等输入/卡权限）
- 会话元数据侧边栏（cwd、git 分支、agent 最新消息）
- 对 agent 高频流式输出的渲染鲁棒性（Wave 的反面教材）

### 支柱三：把 Windows 终端的老毛病做对（table stakes 做扎实）
- 输入延迟作为架构级指标（danluu 基准）
- CJK/emoji 宽度计算正确（中文用户日常痛点）
- 大输出不衰减、焦点管理健壮、秒开
- 配置更新不破坏用户自定义
- 剪贴板干净一致（含 OSC 52）
- 水平滚动 + 大容量回滚

## 技术路线（两阶段）

- **阶段一（MVP，~2 月）**：Tauri + xterm.js + KaTeX + ConPTY。快速验证核心卖点"检测 LaTeX → 就地渲染"，重点打磨检测策略（该逻辑与渲染栈无关，可平移）。
- **阶段二**：Rust + GPU（wgpu）+ BlockList 虚拟化（Warp 式），公式走 katex-rs→SVG→GPU 贴图；可选加 Kitty 图形协议（含 Unicode placeholder）支持第三方程序出图（需 ConPTY ≥1.22）。
