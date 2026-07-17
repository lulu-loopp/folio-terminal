# M0-β：中文 IME 输入（M0 第二片）

> 前置：M0-α 已关片（`81c7dd9`，五轮返工，结论 go）。本片把 spike 04 真人实测钉死的结论焊进 bt-app/bt-render：**在真窗口里用中文输入法打字，行为与渲染正确。** 权威证据：`docs/spikes/04-ime-cjk-manual-results.md`（三家 × 10 项真人门）+ `docs/spikes/04-ime-cjk.md`。

## 一、这一片「做完」长什么样

在 M0-α 的窗口里：
1. 切到微软拼音/微信输入法/搜狗任一家，敲拼音 → 候选框跟着光标走，选字提交 → **中文正确写进 shell、正确渲染成宽字符网格**（`echo 你好世界` 回显对齐）；
2. 组字期间按 Backspace/Esc → 编辑/取消的是**预编辑**，绝不穿透成 shell 的真退格（spike 04 硬规则 1）；
3. 上报预编辑的输入法（微软拼音/微信），预编辑串内嵌显示在光标处（下划线样式）；搜狗不上报预编辑（Gecko 也记录过的行为），**设计不得假设 preedit 存在**——只拿最终 commit 也必须正确；
4. 中英混排行（`ls 文档/`）宽窄单元格对齐、无豆腐块;
5. 英文模式打字与 M0-α 完全一致，零回归。

## 二、实测钉死的硬规则（逐条落地，出处见 manual-results）

1. **键盘层按 `logical == Key::Named(Named::Process)` 丢弃组字期按键，绝不读 `physical_key`**——实测微软拼音组字期 128 键、27 个 physical=Backspace，0 泄漏；碰 physical 就会让 shell 吃掉真退格（winit #4508 行为）。
2. **1×1 system caret 补丁（照 Chromium 对 LANG_CHINESE 的做法）**——只专治微软拼音的候选框连续跟随（三家中唯一 FAIL 项），对微信/搜狗无害。放 bt-platform（又是 Win32 unsafe，沿用隔离纪律）。
3. **`set_ime_cursor_area` 必须节流**——实测高频发坐标时我们自己的窗口重绘掉帧而候选框流畅；节流阈值自定并写明依据。
4. **预编辑渲染无需处理 target clause**——三家 cursor range 全程塌缩 caret（a==b），中文拼音整串转换不分段；只画串+光标位即可。
5. **失焦/切输入法时 IMM32 会当场提交半个组字**（三家一致的幽灵 commit）。本片裁决：照单全收——commit 事件到什么就给 PTY 什么（与 Windows Terminal 行为一致）；在报告里明说这个决定。

## 三、本片必须先还的债（M0-α 遗留风险，字面写在 M0-alpha.md）

- **字体回退必须在渲染任何非 ASCII PTY 输出之前恢复**。当前只加载 4 个 Consolas 文件，CJK 会静默豆腐块。要求：补 CJK 回退（系统等宽中文字体链，如 Microsoft YaHei/等线），**同时保住 M0-α 的启动收益**——不许退回全系统字体扫描；按需/延迟加载均可，报告里给启动打点前后对比证明没有回归。
- **宽字符渲染**：vendored alacritty 网格已按 `char.width()` 给出 WIDE_CHAR + spacer 标志,本片照原样消费：宽 cell 的字形按 2×cell 宽定位/推进（tracking 逻辑对宽 cell 的适配自己想清楚），光标落在宽 cell 上时覆盖两格。**不做 grapheme 聚类、不裁决 ambiguous width、不保证 emoji ZWJ 正确——那些是 M1 攻坚项**，本片只保证「alacritty 说几格就画几格,且画对」。

## 四、明确不做

- grapheme 聚类 / ambiguous width 裁决 / emoji 序列正确性（M1）
- 富内容、数学（M3）；分屏/标签/多会话；滚动回溯 UI（后续片）
- 双拼特判（与全拼同通路，spike 04 已验穿）
- 为搜狗「不上报预编辑」做任何 workaround（应用层无从观测，做不了也不该做）

## 五、硬规矩（沿用）

- 以 DESIGN.md 为权威；认为规格有问题 → 写偏离申请。
- 声明「已实现」必须指出哪个行为测试验证它。可测的都要测：Process 键丢弃、commit → UTF-8 → PTY 字节、宽 cell 字形 x 坐标（纯 CPU 整形测试,混排 CJK/ASCII,沿用零容差风格）、字体回退命中非 .notdef、节流逻辑。
- 真人交互（候选框跟随、三家实测）自动化测不了的，写成人工验收清单,如实区分「代码可证」与「留人工」。
- `cargo test --workspace --locked`、clippy `-D warnings`、fmt 保持绿;unsafe 一律进 bt-platform。
- 做完停下等审核。

## 六、交付

每项改动说明：做了什么、哪个测试验证。附「怎么手动验」清单（起窗口、三家输入法各验什么、看什么算过）。启动打点前后对比（证明字体回退没吃掉启动收益）。若 winit 的 IME 事件序列与 spike 04 记录有出入，如实记录，不硬凑。
