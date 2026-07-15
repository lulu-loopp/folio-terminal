## 总体判断

**有条件可开工，但不能按当前 M0→M4 原样推进。** 技术方向正确，尤其是“富内容只是可撤销视图”“检测基于终端最终状态”“核心可回放测试”。但当前设计在最关键的地方——**稳定文本身份、固定网格与变高块的共存、reflow 后的源映射**——还没有闭合。

如果直接实现，最可能在 M2/M3 才发现 BlockList 无法可靠承接 alacritty grid，导致重做终端核心数据模型。

---

## 1. 架构缺陷

### 1.1 四个设计不变量

| 不变量 | 结论 | 主要问题 |
|---|---|---|
| Text is Truth | 方向成立，定义不成立 | alacritty scrollback 是 VT 执行后的 cell 状态，不是“原始文本”；物理网格行也不是稳定的文本身份 |
| 基于网格稳定状态检测 | 必要但不充分 | 网格适合判断“当前最终显示了什么”，但不能单独提供 Markdown 边界、逻辑行身份和跨 reflow 映射 |
| 核心与 GUI 解耦 | 成立但当前被破坏 | `bt-blocks::Block::Rendered(TextureId, px_h)` 把 GPU 资源身份泄漏进纯核心 |
| damage 驱动渲染 | 应改写 | 可以 damage 驱动调度和场景更新，但不能假定 swapchain 内容跨 present 保留并做真正的局部重绘 |

### 1.2 最大的根本问题：存在两个互不一致的纵向世界

当前同时存在：

1. alacritty 的固定行高 grid/scrollback；
2. BlockList 的像素高度、变高块视口。

但没有定义二者谁拥有滚动位置、哪些行仍可被终端程序寻址、何时从 grid“退休”为 Block。

尤其是可见区静默 200ms 后变 Rich 块这一规则有根本矛盾：

- PTY 仍认为屏幕有固定的 `rows × cols`；
- 富内容块却可能改变该行高度；
- 程序随后可以通过 cursor-up、保存/恢复光标、清屏、进度条更新等重新写这一行；
- 撤销块会再次改变高度，造成视口跳动、鼠标坐标失效、光标与显示错位。

**建议采用双平面模型：**

- 活动屏幕始终是固定行高 terminal plane，允许检测和等高装饰，但不允许变高替换；
- 只有退出“可寻址活动窗口”、冻结为 transcript 的内容，才进入 variable-height history plane；
- 视口是“冻结历史块 + 固定高度 live screen tail”的组合；
- 若要求短输出立即显示公式，可在活动屏幕中先做等高 inline overlay，滚入冻结区后再升级为变高排版。

### 1.3 `StableRowId` 不是足够稳定的身份

[DESIGN.md 第 85 行](D:/Developer/BetterTerminal/docs/DESIGN.md:85)假定物理行有单调、不受淘汰影响的 ID，但：

- resize reflow 会合并/拆分物理行；
- scrollback 可以清除或淘汰；
- resize 可能把 history 拉回可见区；
- soft-wrap 与硬换行语义不同；
- 原文一旦随 alacritty history 淘汰，ID 仍在也无法复制源码。

需要的不是 `StableRowId + column`，而是类似：

```text
TranscriptId + logical text offset/grapheme offset + revision
```

冻结记录至少应保存：

- 逻辑行文本；
- soft-wrap/hard-break 信息；
- cell style/span；
- 原文 offset 到布局 fragment 的双向映射；
- revision/generation；
- 生命周期和淘汰 tombstone。

reflow 应重新生成“逻辑文本 → 当前布局”的投影，而不是尝试把旧物理 row range 映射到新 row range。

### 1.4 Block 数据结构无法表达行内公式和富 Markdown

`Rich { source: Range<StableRowId> }` 只能替换整行或整段，不能正确表示：

```text
结果为 $x^2 + 1$，因此……
```

这里需要同一逻辑行内的普通文字、公式 fragment、基线对齐和源 offset 映射。Markdown 的加粗、链接、代码 span、表格单元格同样不是一个垂直 `Rich` quad 能表达的。

建议把模型拆成：

- 纵向 `HistoryBlockTree`：负责块高度和虚拟化；
- 块内 `RichLayout/DisplayList`：glyph run、inline formula、背景、边框；
- 独立 `SourceMap`：布局 fragment ↔ canonical transcript offset；
- GPU texture/cache 只存在于渲染层，核心保存 `ArtifactKey` 和测量结果。

另外，高度累计不要使用 `f32`。一千万行达到上亿像素后会出现可见精度损失；前缀和用 `i64` 固定点或 `f64`，绘制时转换为视口局部坐标。

### 1.5 稳定检测状态机不安全

[200ms + 光标在下方](D:/Developer/BetterTerminal/docs/DESIGN.md:105)只能叫 `QuiescentCandidate`，不能叫 `Stable`。很多非 alt-screen 程序会长期回写上方行；primary-screen TUI、进度条和 ink 类界面都不会被 alt-screen 门控覆盖。

建议至少改成：

```text
Mutable → QuiescentCandidate → Frozen → Decorated
                  ↘ rewritten        ↘ source invalidated
```

每个检测/渲染任务携带 `(source_id, revision)`；完成时 revision 不匹配就丢弃。稳定判定还应综合：

- shell command/output 边界；
- 行是否离开可寻址活动区；
- 最近 mutation；
- soft-wrap 完整性；
- 输出块是否闭合；
- resize generation；
- clear/eviction 事件。

Markdown 不能只靠行首模式做可靠分段，应使用真实 CommonMark parser，并将启发式限定为“是否值得尝试解析”的门控。

### 1.6 线程模型存在洪水和资源失控问题

“PTY 满速解析”不等于 GUI 不会卡：

- N 个解析线程可占满所有核心；
- 无界 `GridDelta`、检测任务、公式渲染和纹理上传会造成内存爆炸；
- 已过期公式任务仍可能消耗 CPU；
- 大量纹理上传集中到一帧会造成明显卡顿。

必须明确：

- 所有跨线程 channel 有界；
- UI delta 使用 latest-state/coalescing，而不是保证逐事件交付；
- worker 队列按会话和可见性设优先级；
- 任务可取消并带 revision；
- 每帧限制 GPU 上传预算；
- 纹理、公式缓存和 scrollback 均有硬预算；
- 输出洪水下保留主线程和输入写入线程的 CPU 配额。

### 1.7 damage 渲染表述需要修正

wgpu surface 并不提供通用的脏矩形 partial-present 语义。正确模型应是：

- damage 决定是否唤醒和哪些 CPU scene node 需要重建；
- 每次取得新的 surface texture 后，仍完整合成当前可见帧；或者维护自己的 retained offscreen target，再整面 blit；
- expose、DPI、resize、surface lost、device lost、光标闪烁、IME、选择变化也必须触发重绘。

Mailbox 也不能硬编码；wgpu 文档要求从 surface capabilities 选择，只有 Fifo 保证可用，低延迟还与 `desired_maximum_frame_latency` 有关。[wgpu PresentMode](https://docs.rs/wgpu/latest/wgpu/enum.PresentMode.html)、[SurfaceConfiguration](https://docs.rs/wgpu/latest/wgpu/type.SurfaceConfiguration.html)

---

## 2. 被低估或遗漏的重大风险

### 已列风险中被低估的

- **R1：中等。** 最新 `alacritty_terminal 0.26` 已提供 `TermDamage` 和 damaged-line iterator，所以“不暴露 damage”已不完全准确；但它不能解决逻辑文本身份、冻结、reflow ancestry 和 BlockList 映射。[官方 API](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/term/index.html)
- **R2：严重，且是架构风险。** 不是增加语料和 500ms debounce 就能兜底；必须先定义活动屏幕与冻结历史的边界。
- **R3：严重且暴露太晚。** 50 条公式不足以覆盖 AMS 环境、自定义宏、Unicode math、错误输入、超大表达式。应在 M0 前定案。
- **R4：不只是性能风险。** cosmic-text 能做 shaping，但不能决定终端 cell width。cell 占用必须由终端 Unicode/width 策略决定，shaper 的自然 advance 只能被约束、居中或裁剪。

### 缺失风险

1. **源映射和滚动锚定风险。** Rich 块完成渲染、失败或撤销时高度变化；若发生在视口上方，必须保持首个可见 source anchor，否则页面会跳。
2. **不可信输出 DoS。** 恶意/异常 LaTeX、Typst、SVG、Markdown 可制造极慢解析、巨大位图和缓存爆炸。需要最大源码长度、最大像素、超时、禁用外部资源、字体/包访问沙箱。
3. **Windows IME 风险。** winit 有 Preedit/Commit 和光标区域 API，但“收到 IME 事件”不等同于满足 TSF、候选框、多种中文 IME和快捷键冲突的产品要求。[winit IME API](https://docs.rs/winit/latest/winit/event/enum.WindowEvent.html)
4. **GPU/驱动风险。** device lost、TDR、远程桌面、混合显卡、多显示器 DPI、HDR、VRR、DX12 不兼容设备及软件回退未设计。
5. **Agent 状态误判风险。** Windows 进程树无法可靠识别 WSL 内前台 Linux 进程；权限提示文案会随 agent 版本、语言和主题变化。需要优先设计显式 agent side-channel/adapter，而不是把启发式作为主要来源。
6. **持久化隐私风险。** scrollback 常含 token、路径和命令输出。持久化必须有默认策略、容量、清理和崩溃安全写入规则。
7. **可访问性风险。** egui 的 chrome 可以接 AccessKit，但自绘终端和 Rich Markdown 仍需自己发布文本、选择、光标和语义节点。
8. **范围风险。** “完整终端正确性 + 原生排版 + Markdown + 多 agent 编排”是三个大型项目，现有 M0–M4 没有明确砍功能的退出条件。

---

## 3. 依赖选型审核

- **alacritty_terminal：可用，但必须包适配层并固定版本。** 不要让其物理 row/index 类型进入 `bt-blocks` 公共模型；升级时需要 corpus diff 和 reflow 回归。
- **portable-pty：适合作为 M0 起点，不应视为 Windows 生命周期问题的终点。** API 仍以阻塞 reader 为主；ConPTY 关闭、取消 IO、进程树、WSL、不同 Windows build 行为需自行封装。官方 crate 确实来自 WezTerm，当前 0.9.0，但产品验证应覆盖原生 ConPTY 边界。[portable-pty 文档](https://docs.rs/portable-pty/latest/portable_pty/)
- **wgpu：合理。** 需要 DX12/Vulkan/软件回退矩阵、device-lost 重建和 present-mode 能力探测；它不直接兑现“waitable swapchain”承诺。
- **winit：合理的窗口基座，但 IME 必须做 Windows 专项 spike。** Quake 全局热键、toast、任务栏激活、窗口层级和原生菜单还需要 `windows` crate。
- **cosmic-text：候选合理，不应直接作为“终端宽度正确性解决方案”。** 它提供 shaping、fallback 和 rasterization，但当前仍有字体初始化慢、Hangul Jamo 等开放问题，需真实 CJK/emoji corpus 验证。[cosmic-text 文档](https://docs.rs/cosmic-text/latest/cosmic_text/)、[问题列表](https://github.com/pop-os/cosmic-text/issues)
- **resvg：合适。** 纯 CPU SVG→pixmap 路径成熟，但应限制输出尺寸和外部资源，并按可见性调度。[resvg API](https://docs.rs/resvg/latest/resvg/)
- **toml_edit：合适，但“永不重排”承诺过强。** 官方明确说明 dotted-key 顺序不完全保留；还需解决并发外部编辑、原子替换和备份。[toml_edit 限制](https://docs.rs/toml_edit/latest/toml_edit/#limitations)
- **egui：适合早期 chrome，不宜假设以后低成本替换。** 它已有 AccessKit 和 wgpu/winit 集成，但输入焦点、快捷键、IME、状态模型和无障碍一旦深入会形成耦合。应在接口层隔离，并在 M2 设一次正式去留门。[egui 官方说明](https://github.com/emilk/egui)、[egui-winit AccessKit](https://docs.rs/egui-winit/latest/egui_winit/)

---

## 4. 里程碑顺序

当前顺序把三个最高风险——BlockList、数学引擎、稳定检测——推迟到了 M2/M3，不合理。

建议增加 **M-1 风险消减阶段**：

1. 真实 Claude/ink ConPTY 录制回放；
2. 活动固定网格 + 冻结变高历史的最小原型；
3. inline formula、resize reflow、撤销、滚动锚定、选择复制闭环；
4. 三条数学管线 spike；
5. Windows 中文 IME + CJK/emoji + event-to-photon 基准；
6. 输出洪水下有界队列和过期任务取消。

之后调整为：

- **M0：** 可用终端 + canonical transcript 身份模型；
- **M1：** 正确性、生命周期、IME、延迟和资源上限；
- **M2：** BlockList 产品化 + shell/agent 显式信号；
- **M3：** LaTeX；Markdown 先限于代码块、标题和安全的排版子集；
- **M4：** 多会话 UX、总览、通知、持久化。

验收标准也需修改：

- “使用一整天不崩”只能是 dogfood，不是唯一验收；
- `vtebench` 衡量吞吐，不衡量 VT 正确性；
- `cat 100MB` 不是很好的输入延迟场景，应在持续输出时注入可回显按键并测量 p50/p95/p99；
- “按键→上屏 <8ms”必须说明是事件到 submit、present，还是光子测量，并按 60/120/144Hz 分开；
- 一千万行应同时规定内存、磁盘、启动恢复和搜索指标，否则只有 60fps 没意义。

---

## 5. Q1–Q4 建议

**Q1：行内 `$...$` 默认关闭。**

默认档只开启 `$$...$$`、`\[...\]` 等强定界块；行内公式放入 aggressive，或仅在已确认的 agent/Markdown 输出块中开启。`$` 在 shell、PowerShell、awk 和日志中太常见，渲染成功也不代表语义判断正确。

**Q2：egui 可以起步，但 M2 必须设去留门。**

早期 chrome 使用 egui合理；必须从第一天启用 AccessKit、隔离 UI action/state，并单独为自绘终端构建 accessibility tree。“chrome 层薄所以容易替换”目前过于乐观。

**Q3：Markdown 走富排版文本，不走整块位图。**

但必须是带 source map 的 retained layout/display list，而不是把 cosmic-text 输出当普通纹理。公式可以是 inline raster artifact。MVP 建议先做不改变或少改变几何结构的增强，再开放表格和段落 reflow。

**Q4：第一版仅恢复布局和可选的 scrollback，不做进程存活。**

明确称为“历史恢复”，不要暗示任务仍在运行。scrollback 持久化应可关闭、限额、原子写入；长任务存活先建议用户使用 WSL/tmux。daemon 模式涉及 PTY 所有权、升级、崩溃隔离和安全边界，应作为独立架构阶段。

---

## 6. 最终结论与只改三处的选择

**结论：可以开始做风险原型，暂不建议按当前方案进入全面实现。**

如果只能改三处：

1. **重写 BlockList 与稳定性模型：** 固定高度活动屏幕 + 冻结历史平面；用 logical transcript span/revision 取代 `StableRowId` 物理行锚定。
2. **把 Block/reflow、数学渲染、Windows IME 三个 spike 前移到 M-1：** 在 workspace 和 UI 大规模铺开前形成端到端闭环。
3. **补齐有界并发与渲染契约：** 有界队列、revision/cancellation、滚动锚定、缓存/上传预算、完整帧合成和 GPU 恢复。

完成这三项后，整体方案的技术路线是成立的，也具备继续投入的价值。