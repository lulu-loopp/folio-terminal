# 终端富内容渲染技术选型调研（2026-07）

> 目标：为"能渲染 LaTeX 的 Windows 终端"选型。14 次搜索 + 6 次深读。

## 零、关键定性判断

目标不是"让 Claude Code 发 sixel 图片"，而是"**终端自己**从字节流识别 LaTeX/Markdown 并就地渲染"。两条路架构完全不同：

- **图形协议路线**：要改造被托管的程序（Claude Code 的 ink 不会发协议），且 Windows 上要穿 ConPTY 的 DCS 过滤。
- **终端自渲染路线（推荐）**：终端本来就解析整个字节流，在解析层检测 `$...$`/`\[...\]`/`$$...$$`，作为富内容单元自己画。**不需要图形协议，绕开 ConPTY 限制**。Warp 即此思路。

## 一、图形协议现状

| 协议 | 原理 | 优缺点 | 支持 |
|---|---|---|---|
| Sixel | DCS 序列，6 像素竖条编码 | 兼容面广；256 色、无 alpha、慢、无动画 | xterm、mlterm、mintty、foot、WezTerm、Contour、iTerm2；**WT Preview 1.22（2024）已支持**；kitty/Ghostty 明确拒绝 |
| Kitty Graphics | APC `ESC _G`，base64 分块，共享内存零拷贝 | z-index 图层、placement 复用、能力查询、quiet 模式；**Unicode Placeholder（U+10EEEE）让不懂协议的 tmux/vim 重绘时自动搬图** | kitty、Ghostty、WezTerm、Konsole、Rio |
| iTerm2 Images | OSC 1337 File= | 最简单，真彩 | iTerm2、WezTerm、VSCode、Hyper |

- WezTerm 唯一三协议全支持，可作参考实现。
- Claude Code issue #2266 在讨论图形协议，建议探测链 `Kitty → iTerm2 → Sixel → Unicode 半块`。

## 二、已有的"终端显示公式"尝试

| 方案 | 原理 | 评价 |
|---|---|---|
| latex2sixel | latex→dvipng→img2sixel | 需完整 TeX 链，重慢；外部工具 |
| Terminatex (Py) | 渲染 LaTeX 字符串走协议显示 | 外部工具 |
| utftex / tui-math (Rust) | Unicode 数学字符直接排进字符网格 | 轻量跨终端；美观度有限，可作降级 |
| MathRender (VSCode 插件) | 面向 AI agent 渲染 LaTeX | 证明痛点真实存在 |
| Jupyter QtConsole | Qt widget 内嵌富显示 | 可行性证明，但 LaTeX 支持有限且非通用终端 |

**没有任何通用终端做到"自动检测流内 LaTeX 并高质量渲染"——产品空白点。**

## 三、核心架构问题：不破坏 TUI 的检测策略

### 三种范式
- **A. Warp Block Model（最值得借鉴）**：终端 = 有类型的 Block 列表（BlockList）。shell 集成（precmd/preexec hook + ANSI 包裹 JSON 元数据）划分命令边界。核心洞察："BlockList 不关心 block 里是什么，只需要它的高度"——文本 block 与富内容 block 共用 SumTree 虚拟化渲染（O(log n)）。Rust + GPU 自绘。
- **B. Wave（Electron + 文件类型路由）**：remark/rehype 把 Markdown 转 React 组件；但主要用于"文件预览 block"而非实时流检测。
- **C. Kitty Unicode Placeholder**：协议层解决图与 TUI 共存的唯一优雅方案。

### 检测策略（组合使用）
1. **状态机门控**：仅在主屏、行式输出模式启用检测；alternate screen（`ESC[?1049h`，vim/htop）时关闭。
2. **Shell 集成划 block 边界**：只对稳定的命令输出 block 检测。
3. **保守 pattern 匹配**：仅对"收到换行且不再被覆写"的稳定文本匹配，避免流式半截公式误渲染。
4. **Claude Code 特殊性**：ink 界面持续重绘，直接在其活动区检测会打架；应对**滚出 ink 活动区进入 scrollback 的稳定文本**做检测。

## 四、LaTeX 渲染引擎

| 引擎 | 形态 | 集成 | 评价 |
|---|---|---|---|
| KaTeX | JS 同步 | HTML+CSS/MathML | 极快、bundle 小；web 栈首选 |
| MathJax 3 | JS | HTML/SVG/MathML | 覆盖最全，v3 性能已追平 KaTeX |
| katex-rs | 纯 Rust | LaTeX→HTML/MathML，支持 WASM | 原生栈可用，无需 JS 运行时 |
| pulldown-latex | 纯 Rust | MathML Core | 需自己排版字形 |
| latex2mathml | 纯 Rust | LaTeX→MathML | 同上 |
| tui-math/utftex | 纯 Rust | 直接排字符网格 | 降级方案 |
| Typst | Rust | 语法非 LaTeX | 无轻量 math 引擎，不建议做主路径 |

- Web 栈 → KaTeX（或 MathJax）直接 DOM 渲染。
- 原生栈 → katex-rs/MathJax 渲染成 SVG/PNG 后 GPU 贴图（规避纯 Rust 数学排版的深坑）。
- 数学字体（STIX/Latin Modern/KaTeX fonts）必须内嵌。

## 五、UI 技术栈对比

| 维度 | xterm.js + Electron/Tauri | 原生 GPU（Rust + wgpu/Skia） |
|---|---|---|
| 富内容混排 | 极强（DOM/React 组件天然混排） | 需自建 BlockList + 虚拟化 + 贴图（Warp 已证可行） |
| 性能 | Electron 输入延迟差，2026 新终端基本弃用；Tauri 稍好但 webview 仍是瓶颈 | GPU 加速、低延迟、大输出流畅 |
| LaTeX 集成 | 低（KaTeX 现成） | 中-高（SVG→贴图） |
| 天花板 | 富内容高、纯终端性能低 | 双高，但研发成本大 |

注：xterm.js 不原生支持 kitty/sixel（#5711 讨论中）。

## 六、ConPTY 的能力与限制（Windows 关键约束）

1. **ConPTY 重绘而非透传**：自己维护屏幕缓冲再吐给宿主。
   - **DCS 过滤（#17313）**：过去只转发认识的 DCS，sixel 正是 DCS——Warp 曾称"这让 Windows 支持不可能"。**修复 PR #17510 已进 v1.22（2024）**，新系统已大幅缓解。
   - OSC flush 顺序问题。
2. **Passthrough 模式不成熟**（#1985/#1173）：`ENABLE_PASSTHROUGH_MODE` 边缘 case 多。
3. **性能开销**：VSCode #214529 记录 bash 明显变慢。

**重大启示：自渲染路线完全不依赖 DCS 透传——ConPTY 只要把纯文本给你即可。只有"第三方程序（matplotlib）走 sixel 出图"才受 ConPTY ≥1.22 约束。** winpty 已过时，ConPTY 是唯一官方 PTY。

## 七、三套方案

### 方案一：Web 栈快速验证（Tauri + xterm.js + KaTeX）
- 检测：alt-screen 门控 + 稳定行正则匹配 → DOM 富内容层叠加。
- 工作量 ★★☆☆☆，原型 2-4 周。风险：webview 性能天花板、DOM 层与网格滚动同步。天花板：中。

### 方案二：Warp 式原生（Rust + wgpu/Skia + BlockList + katex-rs→SVG→贴图）
- 工作量 ★★★★★（数月-一年）。风险：巨型工程、需强 Rust/图形能力。天花板：最高，绕开 ConPTY 限制。

### 方案三：混合务实（alacritty_terminal 核心 + 富内容视图 + Kitty/sixel 协议兜底）
- 双通道：自渲染检测 LaTeX + 支持子进程主动推图（ConPTY ≥1.22）。
- 工作量 ★★★★☆。天花板高、兼容性最好。

## 八、推荐

**两阶段：先方案一验证，再落地方案二/三。**

1. **0-2 月 MVP（方案一）**：核心假设"自动渲染 AI 输出的 LaTeX 显著提升体验"必须低成本验证。重点打磨**检测策略**（alt-screen 门控、流式稳定判定、不破坏 ink）——这是真正的产品难点且与渲染栈无关，逻辑可平移到原生栈。
2. **第二阶段转原生 Rust GPU**：自渲染是本需求最优解且绕开 ConPTY 限制；Warp 已证明架构可行；公式走 SVG→GPU 贴图规避排版深坑。资源允许则选方案三（加 Kitty 协议 + Unicode placeholder），同时服务"AI 公式自渲染"和"通用程序出图"。

## 来源
- https://akmatori.com/blog/terminal-graphics-protocols
- https://sw.kovidgoyal.net/kitty/graphics-protocol/
- https://www.arewesixelyet.com/ （WT 状态已过时）
- https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release/
- https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment
- https://www.warp.dev/blog/the-data-structure-behind-terminals
- https://github.com/anthropics/claude-code/issues/2266
- https://github.com/microsoft/terminal/issues/17313 · #1985
- katex-rs / pulldown-latex / tui-math / latex2mathml / latex2sixel / Terminatex
- https://github.com/ghostty-org/ghostty/discussions/2496
- https://github.com/xtermjs/xterm.js/issues/5711
