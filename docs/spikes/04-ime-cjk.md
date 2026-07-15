# Spike 04：Windows 中文 IME 与 CJK cell 宽度

## 结论

**no-go（等待真人 IME 证据，不代表 winit 已被技术性否决）**。

当前不能批准 M0 以 winit IME 起步。可交互探针、结构化日志、严格审计器、CJK/emoji/ZWJ 宽度语料和 cosmic-text 约束原型均已完成并通过自动门禁，但中文输入最关键的三家 IME 实测尚未由真人完成。自动烟测没有伪造键盘输入，并被严格审计器正确判为失败。因此，“M0 能不能用 winit 的 IME 起步？”在本轮交接点的明确答案是：**不能，必须先完成下述人工门；若三家均通过，可改判 go-with-caveats，若出现协议级失败则改判 no-go 并让 M0 直接评估 TSF。**

这是提示词规定的人工停点。本轮不启动 spike 03/05/06，也不进入 M0。

## 证据边界

| 项目 | 证据来源 | 当前状态 |
|---|---|---|
| winit 窗口创建、绘制、定时退出 | 本机自动烟测 | PASS |
| 每个 `Ime::` 事件、每次 `set_ime_cursor_area`、每帧状态的 JSONL | 代码与无输入烟测 | PASS（日志格式/顺序）；真实 IME 内容未覆盖 |
| Microsoft Pinyin 全拼/双拼/编辑/候选框跟随 | 真人实体键盘 + 肉眼 + JSONL | **未测** |
| 微信输入法 2.1.0.36 同上 | 真人实体键盘 + 肉眼 + JSONL | **未测** |
| 搜狗拼音或另一家指定主流第三方 IME 同上 | 真人实体键盘 + 肉眼 + JSONL | **未测；本机未安装** |
| cosmic-text 的 CJK/emoji/ZWJ 整形及 cell 约束 | 本机自动审计 | PASS（数值协议）；视觉抽查待真人 |

未测试项不按“应当可用”计入结论。探针的 `--ime-name` 是操作者声明的标签；winit 事件本身不提供当前输入法的可验证身份。

## 产物

- 独立 workspace：`spikes/04-ime-cjk/`，不属于根 workspace，也不依赖或修改现有 BetterTerminal crate。
- 交互探针：`src/bin/ime-probe.rs`。显示 committed/preedit、byte cursor range 和候选区；蓝框严格等于传给 `set_ime_cursor_area` 的物理像素矩形。
- 日志审计器：`src/bin/ime-log-audit.rs`。校验 schema、连续 sequence、启动/退出、帧、候选区调用、UTF-8 byte cursor 边界；`--strict-ime` 额外要求 Enabled、非空 Preedit/Commit 和候选区移动。
- 人工执行入口：`run-manual.ps1`；已有日志不会被覆盖，探针或审计非零退出会终止并保留证据。
- 逐项人工清单：`spikes/04-ime-cjk/MANUAL-CHECKLIST.md`。
- 宽度语料：`corpus/cjk-width-cases.json`，22 例，覆盖 CJK、East Asian Ambiguous、全角、组合字符、emoji、VS15/VS16、keycap、肤色、国旗、ZWJ，以及 Windows Terminal #370 的 `☆` 与 #900 的 `✏` / `✏️`。
- 可复查数据：`docs/spikes/artifacts/04-cjk-shaping.json`。

## winit IME 能力边界

本 spike 锁定 `winit 0.30.13`。其 Windows backend 通过 `WM_IME_*` 和 IMM32 兼容 API 处理组合输入：源码路径会调用 `ImmGetCompositionStringW` 读取组合串，并用 `ImmSetCandidateWindow` / `ImmSetCompositionWindow` 实现候选区定位；`set_ime_allowed` 对应 IME context 的关联控制。这不是一个应用自持的 TSF text store。

winit 当前足以表达的最小终端输入协议是：

- 启用/停用 IME；
- `Enabled` / `Disabled` / `Preedit(String, Option<(usize, usize)>)` / `Commit(String)`；
- 用一个窗口级矩形提示候选框位置；
- preedit 活跃时由后端抑制普通 `KeyboardInput` 文本路径，避免同一文本被提交两次。

winit 没有暴露下列能力；产品一旦需要，必须绕过 winit 的高层 IME 接口，直接实现 Windows TSF（可继续让 winit 管窗口）：

1. TSF `ITextStoreACP` 文档锁、异步 edit session、surrounding text、selection 与 reconversion；
2. 候选列表内容、当前候选、分页、reading string，或 UI-less candidate sink；
3. preedit clause、target-converted range、display attribute 等富组合标记；winit 只有一个可选 byte cursor range；
4. 输入法 profile/语言/实际 TIP 身份、input scope 和按 IME 制定兼容策略；
5. 应用自绘候选 UI、候选可访问性语义，以及 TSF-only 输入法不提供 IMM32 兼容行为时的兜底；
6. 需要 surrounding text 的预测、手写、语音或复杂文本服务。

BetterTerminal M0 若只要求“单一活动 cell 上的 preedit + 系统候选窗 + commit”，winit 在协议形状上可能足够，但仍必须由三家真实 IME 的人工门证明。若人工测试出现无 Preedit/Commit、候选框无法稳定跟随、DPI 偏移、切换后幽灵组合串或重复提交，应先把失败归类为 winit/IMM32 能力缺口；不得在 M0 用按键启发式掩盖。

## CJK / emoji 宽度约束原型

设计不变量是：**终端 cell 是宽度权威，cosmic-text 只负责整形，不能反向决定光标或下一 cell 的位置。** spike 中的 `unicode-width` 只用于把显式 oracle 变成一个可运行的候选策略；M0 接线时必须从 `bt-term` 的真实 cell 占用和 wide-char spacer 取得权威范围，不能把本函数升级成第二套权威。

原型算法：

1. 对整行用 cosmic-text `Shaping::Advanced` 整形，保留字体 fallback、combining 与 ZWJ 形成整体 glyph 的机会；
2. 以 grapheme byte range 把 layout glyph 映射回 cluster；
3. 每个 cluster 的目标 slot 宽度只等于 `terminal_cells × cell_width`；
4. 自然 glyph advance 小于 slot 时居中，大于 slot 时裁剪；自然 advance 永不推进后续 cell；
5. 光标、选择和命中测试只走权威 slot，不走 cosmic-text 的自然 advance。

本机数据（Windows 报告为 25H2 build 26200.8875，Rust 1.94.1，`cosmic-text 0.19.0`）：

| 指标 | 结果 |
|---|---:|
| `FontSystem::new` | 51,246 µs（独立 shaping 进程）；探针为 52,543 µs |
| 语料 | 22 |
| cell oracle 匹配 | 22 / 22 |
| glyph id 0 | 0 |
| 被 slot 裁剪的 cluster | 3（1-cell 的 `♥︎`、`☆`、`✏`） |
| 单例 shape p50 / p95 / max | 40 / 10,129 / 15,998 µs |

p95/max 由首次加载 ASCII 与全角字体 fallback 的冷缓存成本主导；这些数字不是 steady-state 帧预算。该 spike 的目标是对齐协议而非渲染性能。完整逐例数据见 JSON artifact。

这个原型仍有两个不能带入 M0 时隐去的风险：glyph id 0 只是 `.notdef` 的粗检测，不能替代截图视觉检查；跨多个 grapheme 的字体 ligature 可能让按 byte overlap 分配 glyph 产生歧义，生产实现需要按终端 cluster/cell range 建立一次性绘制片段，不能重复绘 glyph。

## 自动门禁与反向验证

- `cargo fmt --manifest-path spikes/04-ime-cjk/Cargo.toml -- --check`：PASS。
- `cargo clippy --manifest-path spikes/04-ime-cjk/Cargo.toml --all-targets --locked -- -D warnings`：PASS，0 warning。
- `cargo test --manifest-path spikes/04-ime-cjk/Cargo.toml --locked`：3 passed，0 failed，0 ignored。
- release 探针无输入烟测：1,686 ms 退出；7 条记录、1 帧、1 次候选区调用，非严格结构审计 PASS。
- 同一无输入日志启用 `--strict-ime`：退出码 1，明确报告缺少 Enabled、非空 Preedit、非空 Commit、候选区移动四项；证明自动烟测不能冒充真实 IME PASS。
- 故意把语料中“汉”的期望宽度由 2 改为 1：目标测试退出码 101，报告 `left: 2, right: 1`；随后恢复 oracle 并重跑全套测试。

## 真人验收交接

当前机器已发现 Microsoft Pinyin 与微信输入法 2.1.0.36；没有搜狗拼音，注册表中的“搜狗高速浏览器”不算输入法。人工执行者需要先安装搜狗拼音，或明确指定另一家主流第三方输入法作为第三家，然后对每家运行：

```powershell
cd D:\Developer\BetterTerminal\spikes\04-ime-cjk
.\run-manual.ps1 -ImeName "Microsoft Pinyin" -LogName microsoft-pinyin
.\run-manual.ps1 -ImeName "WeChat Input 2.1.0.36" -LogName wechat-input
.\run-manual.ps1 -ImeName "Sogou Pinyin <version>" -LogName sogou-pinyin
```

每家都必须按 `MANUAL-CHECKLIST.md` 做全拼、双拼、Backspace、Esc、中英切换、组合中途换 IME、F2 静态候选区移动、F3 动态跟随和跨 DPI（无条件则记 N/A）。交回三份 JSONL 和三行肉眼结果表。任何 FAIL 都保留原日志和复现动作，不覆盖重跑，不用 `SendInput`。

人工结果回来后的判定规则：

- 三家核心项全 PASS，双拼/DPI 只有有理由的 N/A：**go-with-caveats**，M0 可从 winit IME 起步，同时把上述 TSF 边界写成升级触发器；
- 任一家缺失或重复 Preedit/Commit、候选区跟随失败、切换后状态损坏：先复现并归因；若是 winit/IMM32 表达能力不足：**no-go**，M0 改为直接 TSF spike，不自行修改 DESIGN.md；
- 少于三家或只有日志没有肉眼结果：维持本报告的 **no-go（证据不足）**。

## 遗留风险

1. 三家 IME 与双拼、候选框、DPI 的真实交互尚未测试，这是当前 blocker。
2. 本机缺第三家输入法；安装和选择属于人工环境准备，不由代码伪造。
3. winit Windows backend 的 IMM32 兼容路径可能无法覆盖 TSF-only 或富文本服务；是否影响“最小终端输入”必须由人工证据决定。
4. cosmic-text 视觉质量、彩色 emoji、字体 fallback 一致性和 ligature 跨 cluster 行为尚未做截图级人工审核。
5. 字体冷启动约 52.6 ms；M0 若采用该栈，应在窗口展示前或后台预热，但本 spike 不改产品启动架构。

## 上游依据

- winit 0.30.13 `Ime` / `set_ime_allowed` / `set_ime_cursor_area` API：<https://docs.rs/winit/0.30.13/winit/event/enum.Ime.html>、<https://docs.rs/winit/0.30.13/winit/window/struct.Window.html>。
- winit 0.30.13 Windows IMM32 实现：<https://docs.rs/winit/0.30.13/src/winit/platform_impl/windows/ime.rs.html>。
- Windows Terminal #370（CJK 环境下 Ambiguous Width 的 `☆`）：<https://github.com/microsoft/terminal/issues/370>。
- Windows Terminal #900（`✏` 显示为半尺寸）：<https://github.com/microsoft/terminal/issues/900>。

## 偏离申请

无。没有用 `SendInput`，没有把未测项写成通过，也没有修改 DESIGN.md、现有 crate 或根 workspace。若人工门迫使 M0 从 winit IME 改为直接 TSF，将另行提交偏离申请，不能由本 spike 自行改架构。
