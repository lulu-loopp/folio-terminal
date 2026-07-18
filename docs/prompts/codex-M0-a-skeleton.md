# M0-α：一个能跑 shell 的窗口（M0 第一片，纯地基）

> 前置：M-1 全部四个 spike 已有结论（见 `docs/spikes/README.md`）。00/00b/01/02 已交付；03 选 A、04 go-with-caveats、05/06 go-with-caveats。
> **这是 M0 的第一片，不是整个 M0。** 目标只有一个：**一个真窗口，开一个 shell，能打字、能看到输出、能 resize。** 其余（IME、富内容、分屏/标签、宽度正确性、数学）都不在本片，见「五、明确不做」。

## 一、这一片"做完"长什么样

在 Windows 上 `cargo run` 起一个 winit + wgpu 窗口：
1. 启动时 spawn 默认 shell（PowerShell）经 ConPTY；
2. 敲 `dir` / `cargo --version`，**输出正确显示为等宽文字网格**；
3. 键盘 ASCII 输入回传给 shell（含回车、退格、Ctrl+C）；
4. **resize 窗口 → ConPTY 收到新尺寸**，CUI 程序在新尺寸下绘制正确（DESIGN §3.1 变高不回填语义）；
5. 关窗口 → 子进程干净收尾，无僵尸。

**验收是"人能用它跑几条命令"**，不是单元测试绿。但下面的地基件仍要有行为测试。

## 二、复用 M-1，别重造

**核心纪律：这一片是把「窗口 + PTY + 渲染器」套在已建的栈外面，不是重写网格。**

- **VT 解析 + 网格 + 双平面 + 转录**：已在 `bt-term` / `bt-transcript` / `bt-viewport`（spike 01/02 建成，224 tests）。**字节喂给现有栈，渲染消费 `bt-viewport` 的投影**——不要绕过它直接读 alacritty 网格。
- `vendor/alacritty_terminal` 保持在根 workspace，scrollback=0（DESIGN §1.1，转录是历史唯一所有者）。

## 三、新建的 crate（按 DESIGN §1.2 命名，只建这一片要的）

- **`bt-pty`**：ConPTY 封装（`portable-pty`）。spawn shell、读/写字节、resize、退出。**容量契约（DESIGN §1.3）**：PTY→Term 每会话 1 MiB 环形缓冲，满则阻塞 read（背压传导子进程）——spike 06 已验过这套逻辑，M0 落成真管线。
- **`bt-render`**：wgpu 文字渲染。等宽 cell 网格、单字体、`cosmic-text` 整形 + 字形图集（glyph atlas）。**先只渲染纯文字网格**（前景/背景色、光标）；SGR 颜色能显示即可，富装饰不做。
- **`bt-app`**：winit 窗口 + 接线。键盘→PTY、PTY→term→render、resize→ConPTY。**单会话、单 pane、无标签**。

## 四、把 spike 结论焊进来（这一片相关的部分）

- **延迟（spike 05）**：`bt-render` 的呈现路径从第一天就按「事件→submit / 事件→present」可打点设计，**事件源做成可替换边界**（04 的窗口栈结论未最终锁死，别把 winit 焊进 `bt-render` 的公开接口——spike 05 已证明这条可行且保护 M0）。这一片不要求完整延迟基建，但要求接口不挡路。
- **背压（spike 06）**：PTY→Term 的 1 MiB ring + 满则阻塞，按 §1.3 落地（不是逻辑原型，是真 ConPTY）。
- **IME（spike 04）不在本片**。但当后续片做中文输入时，三条硬规则已实测钉死，写进那一片：① 键盘在组字期按 `logical == Named(Process)` 丢弃、**绝不碰 physical_key**（否则 shell 吃掉真退格）；② 中文 IME 候选框跟随需自建 1×1 系统 caret（照 Chromium，专治微软拼音连续跟随）；③ `set_ime_cursor_area` 须节流。详见 `docs/spikes/04-ime-cjk-manual-results.md`。

## 五、明确不做（留给后续 M0 片 / M1）

- **IME / 中文输入**（后续 M0 片，规则见上）
- **富内容渲染 / 检测**（M3）、**数学**（M3，引擎已选 A）
- **分屏 / 标签 / 多会话**（后续 M0 片）
- **宽度正确性 / grapheme 聚类 / ambiguous width**（**M1 裁决项**，本片直接用 alacritty 网格原样的 char-by-char 宽度，不声称 CJK 正确）
- **滚动回溯 UI 打磨、配置系统、profile 发现、默认终端注册**（后续）

## 六、携带项（M-1 审核遗留，随本片首个 commit 落）

1. **G1 回放矩阵补一个字节驱动用例**：「resize 变矮丢弃空白行」（`docs/reviews/` 的 M-1 part1 signoff 列的携带项）。
2. **`docs/spikes/00-baseline.md` 加 supersession 头**（指向 00b）。

## 七、硬规矩（沿用）

- 以 DESIGN.md 为权威推导，不照复现步骤局部修补。认为规格有问题 → 写偏离申请。
- 声明"已实现"必须能指出哪个行为测试验证它。
- 不进 M1；不做宽度正确性；`vendor/alacritty_terminal` 留在根 members，`cargo metadata` 自证。
- 做完停下等审核。

## 八、交付

每个 crate 说明：**做了什么、行为测试验证哪条**。跑 `cargo test`、`cargo clippy --all-targets --locked -- -D warnings`。附一段"怎么手动验"（起窗口、跑哪几条命令、resize 看什么）。**若 winit 的 ASCII 键盘/resize 与 04 之外有新坑，如实记录，不硬凑。**
