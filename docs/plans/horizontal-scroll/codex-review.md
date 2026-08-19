我会按“审计而非改写”来做：先完整读方案和架构文档，再沿标注项与六个问题逐条追到真实代码和测试。这个任务明显涉及多个 crate，但你没有要求代理并行，我会保持单线审阅，避免证据口径分裂。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppData\\Local\\Temp\\claude\\D--Developer-Better`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppDa
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -NoProfile -Command "Get-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppData\\Local\\Temp\\claude\\D--Devel`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppDa
本地读取目前被 Windows 沙箱的登录会话错误（1312）拦了一次；我在切换读取路径继续，不把它当作仓库问题。证据最终会以实际行号为准。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command Get-Location`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Location\" | env_u16_len=7905 | si_flags=256 | cre
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-ChildItem -LiteralPath 'D:\\Developer\\BetterTerminal' -Force | Select-Object -First 5"`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=C:\\Windows\\Temp | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-ChildItem -LiteralPath 'D:\\Developer\\BetterTerminal' -Force 
目前已经发现一个会改变结论的硬错误：原生 build 并没有可用的 `Line wrapping` 设置；它只存在于 mock-up。代码里的 `line_wrapping` 是公式块溢出归属，不会发 DECAWM。接下来我会把这一区分贯穿六问，而不是沿用方案的同名概念。另一个重点结论也已显形：现有冻结行投影会先扫描/铺开整条逻辑行再截取，因此“天然 O(视口宽)”不成立。
我已经把“代码里现有的能力”和“方案假定已有的能力”分开了。接下来收口两个最容易误判的部分：alt-screen 切宽的时序边界，以及所有横坐标/宽度泄漏点；这两处会直接决定台阶二能否按原文立项。
总 verdict：**有条件通过路线顺序，但不能按现文直接立项。** 台阶一可以立项，但必须先补齐横向窗口化投影和性能门；台阶二只能先做架构验证，原文的 alt-screen 切宽方案不成立；台阶三继续只记账。四个阻断性错误是：

- 原生产品目前没有 `Line wrapping` 设置。
- 台阶一现有布局路径是 O(逻辑行长)，不是 O(视口宽)。
- alt-screen 进出时切换 ConPTY 宽度与当前 vendor/resize/PSReadLine 时序不相容。
- “17 处中的 4 处”明显漏项，至少还包括 CSI 14/18、光标、IME、预编辑、选择/搜索、内联对象坐标。

## 1. 事实 1–6

总体 verdict：**有保留**。

1. **事实 1：成立。**

   DECAWM 关闭时 `wrapline()` 直接返回；窄字符持续覆盖末格，宽字符在末列直接放弃。证据：[vendor term/mod.rs:1457](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:1457)、[term/mod.rs:1574](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:1574)、[term/mod.rs:1616](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:1616)。

2. **事实 2：有保留。**

   “`Pane` 被生产、renderer 不消费”成立；但它不是终端 no-wrap 的半成品，而是 `MathBlockPlacement` 的公式块溢出字段：[bt-viewport/src/lib.rs:390](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:390)、[bt-viewport/src/lib.rs:398](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:398)。生产点也只读取 `MathLayoutOptions::line_wrapping`：[session.rs:6751](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:6751)。

   修正建议：删除“Line wrapping Off 时产生”的产品语义；明确它当前只是公式块布局选项。台阶一要新增 pane 级横向投影状态，不能把消费这个 enum 当成主体实现。

3. **事实 3：有保留。**

   `validate_shape` 的严格等式和稠密 `columns × rows` 成立：[bt-viewport/src/lib.rs:519](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:519)。但“约 17 处、其中只有 4 处要反向换算”不成立，详见问题 4。

4. **事实 4：不成立。**

   原生产品没有 `Terminal ▸ Line wrapping` 设置；那一行只存在于 mock-up。代码注释两次明确写着“still has no setting behind it”：[settings.rs:1482](/D:/Developer/BetterTerminal/crates/bt-app/src/settings.rs:1482)、[settings.rs:3117](/D:/Developer/BetterTerminal/crates/bt-app/src/settings.rs:3117)。DESIGN 也明确说明产品从未把 `MathLayoutOptions::line_wrapping` 设为 false，也不发送 `CSI ? 7 l`：[DESIGN.md:839](/D:/Developer/BetterTerminal/docs/DESIGN.md:839)。

   修正建议：台阶一/二是在新增产品能力及其设置，不是扩展现有 On/Off 语义；最好同时把公式侧字段改名，避免继续把两个“line wrapping”混为一谈。

5. **事实 5：成立，但要写清三平面差异。**

   冻结层确为已拼合的逻辑行：[bt-transcript/src/lib.rs:760](/D:/Developer/BetterTerminal/crates/bt-transcript/src/lib.rs:760)。普通 capture 按 `continues` 把物理行加入同一候选：[bt-transcript/src/lib.rs:839](/D:/Developer/BetterTerminal/crates/bt-transcript/src/lib.rs:839)，`normalize` 再拼成一个 `FrozenLine::text`：[bt-transcript/src/lib.rs:1016](/D:/Developer/BetterTerminal/crates/bt-transcript/src/lib.rs:1016)。

   但 staging 仍是物理 `CapturedRow`，live 仍是 vendor 网格；“冻结免费”不等于 staging/live 也已有可随机访问的逻辑行视图。

6. **事实 6：有保留。**

   正常收割批次内，WRAPLINE 是可信的继续关系：[session.rs:8145](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:8145)，也有生命周期测试：[lifecycle_matrix.rs:1238](/D:/Developer/BetterTerminal/crates/bt-term/tests/lifecycle_matrix.rs:1238)。

   但 resize/所有权边界会故意设置 `wrap_split`，保留字符和原行尾标志，却拒绝把下一批未知来源的行焊接上来：[bt-transcript/src/lib.rs:876](/D:/Developer/BetterTerminal/crates/bt-transcript/src/lib.rs:876)、[bt-transcript/src/lib.rs:895](/D:/Developer/BetterTerminal/crates/bt-transcript/src/lib.rs:895)。所以应改成：

   > 正常批次内可按 WRAPLINE 拼合；跨 resize/冻结的不可信边界只保真内容，不保证恢复为同一个逻辑行。

## 2. 台阶一成本及 O(视口宽)

Verdict：**不成立——现实现不是天然 O(视口宽)。**

当前代码只先筛选“与纵向窗口相交的逻辑行”，随后仍完整布局整条逻辑行：

- 相交后直接调用 `layout_frozen_line(&entry.line, column_count)`：[bt-viewport/src/lib.rs:2081](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:2081)、[bt-viewport/src/lib.rs:2154](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:2154)。
- 直到整条线的所有 `VisualRow` 都生成、验证后，才 `skip/take` 可见部分：[bt-viewport/src/lib.rs:2231](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:2231)。
- 布局先对全文做 URL 检测，再遍历全部 grapheme：[bt-viewport/src/lib.rs:3719](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:3719)。
- 每个 grapheme 又对 `styles` 做线性 `find`：[bt-viewport/src/lib.rs:3757](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:3757)，病理情况下甚至可能接近 O(grapheme × style span)。

因此十万字符逻辑行的首次显示、滚动或缓存失效会产生 O(行长) 时间和中间内存。

台阶一必须增加以下验收条件：

- 每帧只物化 `viewport_columns + 小量 overscan` 的 cells/anchors。
- 每条逻辑行缓存累计 cell-width/checkpoint，可由 `x_origin` 二分定位首个可见 grapheme。
- style、显式 OSC 8、隐式 URL 使用区间游标/索引，禁止每帧全文扫描。
- O(行长) 只允许发生在冻结、内容变更或首次建索引时；连续横滚必须是 O(视口宽)。
- 加十万 grapheme、密集 style/OSC 8、CJK/组合字符的基准和分配上限。

底部横条 verdict：**有保留。** 当前仅右侧预留了 8px 竖条车道，底边没有车道：[DESIGN.md:813](/D:/Developer/BetterTerminal/docs/DESIGN.md:813)、[DESIGN.md:847](/D:/Developer/BetterTerminal/docs/DESIGN.md:847)。而“只有离开原点后才浮现”存在启动悖论：条不可见时用户不能靠它离开原点。必须同时规定 Shift+滚轮、触控板横向手势或命令入口，以及条覆盖最后输入行时的交互规则。仅引用竖条 auto-hide 不足以结案。

## 3. 台阶二 alt-screen 切宽与 PSReadLine

Verdict：**不成立，不能作为默认路线立项。**

主要证据：

- 当前 `Term::resize` 会把活动网格和非活动网格同时 resize 到同一个尺寸：[vendor term/mod.rs:988](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:988)、[term/mod.rs:1043](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:1043)。进入 alt 后改成真实宽度，也会重排停放的 primary；退出再重排回 240。
- ConPTY 的公开 API 是一次全局字符尺寸变更，并让附着的 CUI 程序立即看到这个尺寸；没有“primary 240、alt 真实宽”的独立尺寸接口。[Microsoft ResizePseudoConsole](https://learn.microsoft.com/en-us/windows/console/resizepseudoconsole)
- BetterTerminal 自身已记录：ConPTY resize 没有 repaint begin/end、request ID 或输出批次身份，本仓只能用 200ms 窗口收敛：[DESIGN.md:24](/D:/Developer/BetterTerminal/docs/DESIGN.md:24)、[DESIGN.md:201](/D:/Developer/BetterTerminal/docs/DESIGN.md:201)。
- alt 切换是解析子程序输出后才观察到：[session.rs:1963](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:1963)、[session.rs:5024](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:5024)。子程序可能已在同一输出块里按旧宽度发出 `?1049h` 后的首屏；终端无法追溯性地让它改用真实宽。
- 现有 resize gate 明说任何 shell 初始化期 resize 都不安全：[main.rs:6725](/D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:6725)。普通路径本地立即 reflow、ConPTY 延迟 200ms：[main.rs:39650](/D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:39650)。若 alt 切换绕过延迟，就引入第二条未经钉死的 resize 路径；若不绕过，TUI 会先按 240 列启动。
- PSReadLine 修复债只在“当时已经有开放 OSC 133 输入区”时建立，alt-screen 明确不建立：[main.rs:6812](/D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:6812)。退出 alt 后，prompt/input marker 与 真实宽→240 resize 的先后存在竞态，现有 InvokePrompt 链不能保证覆盖。

微软历史问题也证明 alt buffer、normal buffer 与 resize/reflow 曾是强耦合区；该问题已经修复，不应当作“当前 Windows 仍有此 bug”的证据，但足以说明不能靠经验假定切换无害：[microsoft/terminal #3492](https://github.com/microsoft/terminal/issues/3492)、[#4200](https://github.com/microsoft/terminal/issues/4200)。

更稳替代：**alt-screen 始终保持 W_virtual，视口裁切/横滚；不要在 1049h/l 上 resize ConPTY。** 体验可以另设 TUI 兼容模式，但“进 alt 切真实宽”只能作为隔离实验，需要新的 parser pause、屏幕切换状态机、两网格宽度语义和完整生命周期矩阵后再决定。

PSReadLine/OSC 133 输入重排本身 verdict：**有保留。** 现有 `SemanticInputRegion` 只有 anchors、written rows 和 witness，不是保存完整样式与 prompt 的输入 surface：[session.rs:2963](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:2963)。resize 后的逻辑文本仅用于 witness rematch：[session.rs:3766](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:3766)、[session.rs:3824](/D:/Developer/BetterTerminal/crates/bt-term/src/session.rs:3824)。B→C 只覆盖 input，prompt 在 A→B 之外。原文“经语义区换算即可重折输入行”低估了实现，需要独立的 prompt+input 逻辑投影和光标模型。

## 4. CPR/mouse/OSC 是否只漏“第 5 处”

Verdict：**不成立，漏的不止一处。**

- CPR 已经直接从 vendor 虚拟网格光标生成，不应再加减 `x_origin`：[vendor term/mod.rs:2168](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:2168)。
- DSR 5 没有坐标；“CPR/DSR 一律换算”表述不准确。
- 鼠标必须做 `content_column = x_origin + visible_column`。当前行做了 frame→live 映射，列仍原样返回：[main.rs:6902](/D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:6902)，随后直接编码成协议字节：[main.rs:7001](/D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:7001)。
- CSI 18 `t` 会直接报告 vendor 的虚拟字符尺寸：[vendor term/mod.rs:3189](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:3189)。
- **明确漏掉的协议点是 CSI 14 `t` 像素尺寸查询。** vendor 产生 `TextAreaSizeRequest`：[vendor term/mod.rs:3180](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:3180)，但当前 adapter 的 `_ => {}` 把它丢掉：[adapter.rs:229](/D:/Developer/BetterTerminal/crates/bt-term/src/adapter.rs:229)。台阶二必须裁决继续不答，还是回答 `W_virtual × cell_width`；若回答真实视口像素，就破坏“子程序只有一个世界”的合同。
- 光标与 IME 必须从内容列减去 `x_origin`。当前 caret 超出 `frame.columns` 会直接隐藏：[bt-render/src/lib.rs:8257](/D:/Developer/BetterTerminal/crates/bt-render/src/lib.rs:8257)，IME 候选窗也直接使用 `frame.cursor.column`：[bt-render/src/lib.rs:8183](/D:/Developer/BetterTerminal/crates/bt-render/src/lib.rs:8183)。
- IME preedit 的折行宽度使用 `frame.columns`：[bt-render/src/lib.rs:523](/D:/Developer/BetterTerminal/crates/bt-render/src/lib.rs:523)。
- word/line selection、搜索高亮、OSC 8/link run 都依赖可见帧的稠密步长：[bt-viewport/src/lib.rs:729](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:729)、[bt-viewport/src/lib.rs:945](/D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:945)。
- OSC 8 本身不向子程序回传坐标；真正要求是裁切后的 cell 必须保留指向原始内容的 anchor。
- InlineImage、OSC 133 marker 等事件携带的 row/column 是内容网格坐标，投影时也必须转换：[adapter.rs:125](/D:/Developer/BetterTerminal/crates/bt-term/src/adapter.rs:125)。

修正建议：不要单纯放松 `validate_shape`。应明确三个字段：

- `content_columns`：vendor/ConPTY 世界；
- `viewport_columns`：稠密 `ViewportFrame` 的步长；
- `x_origin`：内容窗口起点。

`ViewportFrame::validate_shape` 仍应严格验证 `cells.len == viewport_columns × rows`，并通过类型化转换集中处理 ContentColumn↔ViewportColumn。

## 5. DECAWM Off 存“行尾巴”

Verdict：**不成立，属于纯增熵，不应并入台阶一。**

DECAWM Off 的标准状态只有固定网格和末格覆盖：[term/mod.rs:1457](/D:/Developer/BetterTerminal/vendor/alacritty_terminal/src/term/mod.rs:1457)。增加尾巴以后必须回答：

- CUP/HPA、退格、CR、插入/删除、EL/ECH 怎样修改尾巴；
- SGR、OSC 8、组合字符、宽字符怎样保存；
- resize、滚屏、alt-screen、选择、复制怎样处理；
- 光标返回网格内部后尾巴何时失效。

不回答这些，它不是终端状态，只是旁路字节日志；回答完则相当于实现第二套可变宽终端网格。若以后确有日志捕获需求，应作为独立、非 VT 语义的实验，不进入台阶一。

## 6. 排期和台阶一→二耦合

Verdict：**有保留。**

“台阶一排 Windows 落地块之后”合理：当前 resize/PSReadLine/多 pane 宽度链刚收敛，横轴又会改动 frame、hit-test 和 pane chrome，不宜同时扰动。

但台阶一不能做成一次性的 WRAPLINE 展平补丁。否则台阶二至少会重写：

- 横向 extent 与 `x_origin` 状态；
- windowed frame materialization；
- 内容列↔视口列转换；
- 光标、IME、选择、链接、内联对象映射；
- pane 横条及输入手势。

修正建议：台阶一就建立通用 `HorizontalProjection { content_extent, viewport_columns, x_origin }`。台阶一的内容源是 WRAPLINE 逻辑行；台阶二未来的内容源才是 W_virtual 网格。这样顺序不会造成翻倍返工。

## 必须修改的句子

| 方案行 | 原意 | 必须改为 |
|---|---|---|
| [14–16](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:14) | 已有 Line wrapping On/Off；Pane 在 Off 时产生 | 原生无此设置；现有 `line_wrapping` 与 `HorizontalOverflowOwner` 仅属于公式块 |
| [22](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:22) | “数据侧零改动” | 可说终端源存储不改，但投影索引、帧契约、锚点和横向状态有实质改动 |
| [24](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:24) | “冻结层免费” | 冻结文本已拼合，但仍需 cell-width/link/style 索引；resize 边界可能 `wrap_split` |
| [27](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:27) | “复用竖向锚点体系” | 复用 anchor 身份，但必须新增独立 x 轴窗口和类型化坐标转换 |
| [28](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:28) | 离开原点后才显示浮条 | 增加可启动横滚的手势/入口，并裁决覆盖输入行的命中与可见性 |
| [29](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:29) | 半价尾巴补丁可并入 | 从台阶一删除，另列为非标准实验 |
| [33–36](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:33) | 用户选择现有 Off；OSC 133 足以重排输入 | 改为未来新增模式；输入投影需要 prompt+input 内容模型，OSC 133 只是边界/见证 |
| [37](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:37) | alt 进入真实宽、退出虚宽 | 默认改为 alt 始终 W_virtual；切宽降为未获批准的兼容性实验 |
| [38–39](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:38) | CPR/mouse/OSC 四处换算；放松 width 等式 | 列出 CSI 14/18、mouse、cursor、IME、selection、links、inline objects；拆成 content/viewport/x_origin，保持帧稠密校验 |
| [40](/C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:40) | 240 列线性外推即可判定可接受 | 增加十万字符行的时间、临时分配、索引和横滚帧预算；588B/80col 不能证明病理行成本 |

最终裁决：**“一→二→三”的路线顺序成立，但立项状态必须写成：台阶一有条件批准；台阶二仅批准 discovery/spike，不批准原实现方案；台阶三继续记账。** 只有补上 O(视口) 窗口化、通用横轴契约，并把 alt-screen 切宽从默认方案撤下后，这份方案才适合作为正式架构规划。