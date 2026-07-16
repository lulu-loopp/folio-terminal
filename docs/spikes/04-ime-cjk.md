# Spike 04：Windows 中文 IME 与 CJK cell 宽度（04b 返工版）

## 结论

**no-go（等待真人 IME 证据；winit 尚未被技术性否决）**。

原报告关于人工门的判定规则保持不变：三家核心项全 PASS 才能改判 go-with-caveats；协议级失败转向直接 TSF；少于三家或缺肉眼结果维持 no-go。没有用 `SendInput`，没有把未测项写成“应该可用”。

04b 同时撤回原报告最响亮但错误的“22/22 cell oracle 匹配”结论。真实结论是：

1. grapheme 候选目标自身是 22/22，但 vendored `alacritty_terminal::Term::input` 的真实 cell 占用只与其匹配 **15/22**；七条 emoji/ZWJ/VS 序列分歧是本 spike 的核心产出。
2. `bt-term` 当前沿用 alacritty 的逐 Unicode scalar 宽度，没有 grapheme 聚类；`👨‍👩‍👧‍👦` 实际占 8 cells，而候选产品目标是 2。**是否在 M0 给 bt-term 增加 grapheme 聚类/cluster width 是必须裁决的事项。** 本 spike 只读对照，没有修改现有 crate。
3. WT #370 暴露的 Ambiguous Width 没有 DESIGN 决策：`A☆中│Ｂ` 按 narrow 是 7，按 CJK-wide 是 9。原语料暗中选择 narrow，现已改为显式规格缺口。
4. cosmic-text 有三条**已知视觉失败**：`❤︎`、`☆`、`✏` 的自然字形宽于 1-cell slot，被裁剪。宽度数字正确不代表能正确画出。

因此当前不能批准 M0 以 winit IME 起步，也不能把现有 bt-term 宽度行为当作 emoji/ZWJ 产品目标。

## 04b 修改、原因与会红证据

| 修改 | 为什么 | 坏掉时如何会红 |
|---|---|---|
| corpus 同时保存 `expected_cells`、`expected_cells_bt_term` 及两套来源 | 候选产品策略与现状权威必须分离 | 改任一 bt-term 数字，vendored `Term::input` 对照测试失败；改候选数字，UAX/UTS 候选测试失败 |
| spike 以 dev-dependency 只读路径依赖 vendored alacritty | 不能再用 grapheme 实现给自身发证书 | `vendored_term_input_matches_only_fifteen_candidate_cases` 逐字符调用 `Handler::input`，断言 15/22 和精确七条差异 |
| `terminal_policy_cells` 改名为 `candidate_grapheme_cells` | 它是候选策略，不是 bt-term 权威 | 全树不再存在旧名；测试和 artifact 明确使用 `candidate_cells` |
| narrow 与 CJK-wide Ambiguous 策略同时测量 | 原 narrow 是未申报产品决策 | `ambiguous_width_has_unresolved_narrow_and_cjk_wide_results` 固定同一字符串 7/9，任一侧行为变化都会失败 |
| 清单第 10 条改为“蓝框/系统候选框相对关系跨 DPI 不变” | 黄色 preedit 用自然 advance，必然与 cell 几何漂移；不能把探针限制误判为 winit 缺陷 | 真人跨 DPI 后若候选框相对蓝框漂移即 FAIL；黄色字形末端与蓝框不重合不再构成失败 |
| F4 依次写入 1～10 的 `checklist_item` | 一条 preedit/commit + 一次移动不能冒充十项完成 | 严格审计缺任一 marker 就失败；审核式“最低 8 条日志”由回归测试明确拒绝 |
| 脚本成功输出降格为“最低日志/marker 门通过” | 自动绿不能代替肉眼表 | `run-manual.ps1` 明示最终结论仍需表格；报告判定规则拒绝“只有日志” |
| 更正 target clause、UTF-16 cursor 与 IME 身份边界 | 原能力表一处事实失准，另有两个未披露 caveat | 清单第 2 条按 target clause 观察；非 BMP 与身份真实性保留为人工/后续 native probe 风险，不伪造 PASS |

## 证据边界

| 项目 | 证据来源 | 当前状态 |
|---|---|---|
| winit 窗口创建、绘制、定时退出 | 本机自动烟测 | PASS |
| `Ime::`、候选区调用、帧、清单 marker 的 JSONL | 自动测试与无输入烟测 | PASS（格式/门禁）；真实 IME 内容未覆盖 |
| Microsoft Pinyin 全拼/双拼/编辑/候选框跟随 | 真人实体键盘 + 肉眼 + JSONL | **未测** |
| 微信输入法 2.1.0.36 同上 | 真人实体键盘 + 肉眼 + JSONL | **未测** |
| 搜狗拼音或另一家指定主流第三方 IME 同上 | 真人实体键盘 + 肉眼 + JSONL | **未测；本机未安装** |
| alacritty/bt-term 现状 cell 占用 | vendored `Term::input` 路径依赖测试 | PASS：实际测得 15/22 与候选一致 |
| cosmic-text 数值整形 | 自动 artifact | 已测；三条明确视觉失败，截图级审查未完成 |

`--ime-name` 只是操作者声明，不能证明当前活动 TIP。winit 不提供可验证身份，但 Win32 可以通过 `GetKeyboardLayout` / `ImmGetDescriptionW` 取证。本 spike 为保持 `unsafe_code = "deny"` 且不扩张 native FFI 面，选择不实现；代价是三家身份仍需操作者与环境截图佐证，而不是“Windows 做不到”。

## 双 oracle：候选目标与 bt-term 现状

DESIGN 的不变量仍是 bt-term 的 cell 占用控制光标、选择和后续 cell；cosmic-text 只能被约束。问题在于 bt-term 今天的实际算法并不是 grapheme 策略：vendored `Term::input` 对每个 `char` 调 `c.width()`，零宽 scalar 附着到前一 cell，宽 scalar 各自推进。`vendor/alacritty_terminal` 和 `crates/bt-term` 没有 grapheme 聚类路径。

corpus 现在并列两套数值：

- `expected_cells`：候选产品目标。每条注明 UAX #11、UTS #51、wcwidth 约定或“PRODUCT DECISION PENDING”，不再只写裸数字。
- `expected_cells_bt_term`：由 `alacritty_terminal 0.26.0 Term::input` 实际喂入后读取 cursor cell advancement；逐条注明来源。

七条真实分歧：

| 用例 | grapheme 候选目标 | bt-term/alacritty 实际 |
|---|---:|---:|
| `❤️` heart-emoji-vs16 | 2 | **1** |
| `1️⃣` keycap | 2 | **1** |
| `👍🏽` skin-tone | 2 | **4** |
| `👩‍💻` woman-technologist | 2 | **4** |
| `👨‍👩‍👧‍👦` family-zwj | 2 | **8** |
| `🏳️‍🌈` rainbow-flag | 2 | **3** |
| `A✏️B` WT #900 | 4 | **3** |

这不是为了让测试变绿而修改产品期望；两列都保留，分歧进入 M0 决策。路径 dev-dependency 不会把 spike 加进根 workspace，也没有改动 vendor 或 bt-term。

### WT #370：待裁决的 Ambiguous Width

`A☆中│Ｂ` 在候选 narrow 模式下是 7 cells，在 `width_cjk`（Ambiguous 视为 wide）下是 9。`☆` 和 box-drawing 都受语境策略影响。DESIGN 没有规定固定 narrow、固定 wide、locale 驱动还是用户可配置；因此原“#370 已覆盖”撤回，当前状态是**已复现规格缺口，等待裁决**。

## cosmic-text 约束结果与已知视觉失败

原型仍对整行 `Shaping::Advanced`，以 grapheme byte range 映射 glyph，再把自然几何居中/裁剪进候选 slot；自然 advance 不推进候选后续 cell。这个算法验证的是候选视觉约束，不是 bt-term 现状权威。

本机数据（Windows 25H2 build 26200.8875，Rust 1.94.1，`cosmic-text 0.19.0`）：

| 指标 | 结果 |
|---|---:|
| `FontSystem::new` | 50,256 µs |
| 语料 | 22 |
| 候选实现 vs 候选 oracle | 22 / 22 |
| bt-term 实际 vs 候选目标 | **15 / 22** |
| glyph id 0 | 0 |
| 已知裁剪/视觉失败 | **3** |
| 单例 shape p50 / p95 / max | 43 / 10,018 / 16,062 µs |

三条裁剪不是中性统计：

- `heart-text-vs15` `❤︎`（U+2764 + VS15）：自然 27.46 px、slot 14 px、2 glyph。VS15 请求文本形态，但当前 fallback 形成 emoji 宽几何并被裁剪。
- WT #370 `☆`：自然 16.66 px、narrow slot 14 px，被裁剪；这与 Ambiguous Width 未决直接相关。
- WT #900 `✏`：自然 27.46 px、slot 14 px；`A✏B` 自然总宽 51.83 px，而候选 slot 总宽 42 px。宽度/字形不一致正是 #900 一族问题。

所以 22/22 只说明候选 width 函数复现了候选数字，**不代表视觉通过**。完整逐例数据在 `docs/spikes/artifacts/04-cjk-shaping.json`。

## 探针几何与人工 marker

探针没有假装已把自然字形约束渲染接入交互窗口：黄色 preedit 仍是 cosmic-text 自然 advance，蓝框是传给 `set_ime_cursor_area` 的 cell 几何。两者对 `汉` 等字符会自然漂移。04b 采用审核允许的第二方案：

- UI 明示不要把黄色文字末端当候选位置 oracle；
- 第 10 条只比较“蓝框与系统候选框”的相对关系跨 DPI 是否稳定；
- 第 6/7 条同样只以蓝框/候选框判跟随；
- 完成每项后按 F4，依次 emit 1～10；严格审计缺任何一项即失败。

marker 只是操作者声明“该项已访问”，不能证明肉眼 PASS。即使自动审计退出 0，没有结果表仍按证据不足处理。

## winit / IMM32 能力边界更正

本 spike 锁定 `winit 0.30.13`。Windows backend 只使用 `WM_IME_*` / IMM32：`ImmGetCompositionStringW`、`ImmSetCandidateWindow`、`ImmSetCompositionWindow`、`ImmAssociateContextEx`；不是 TSF text store。

winit 暴露 Enabled/Disabled、Preedit、Commit 和候选区矩形。原报告称 target-converted range 未暴露不准确：Windows backend 会读取 `GCS_COMPATTR`，把 `ATTR_TARGET_CONVERTED` / `ATTR_TARGET_NOTCONVERTED` 合并成一个 byte range；IME 未标 target 时才用 `GCS_CURSORPOS` 回退成零长度 caret。真正丢失的是：

- 多 clause 的全部边界；
- 每个 clause 的 converted/not-converted/display attributes；
- target 之外的 selection/reading/candidate 细节。

因此清单第 2 条不再把该 range 恒称“预编辑光标”，而是先按 target clause 解释。

另有一条可复现的上游 latent bug：IMM32 的 `GCS_CURSORPOS` 是 UTF-16 code-unit offset，而 winit 0.30.13 用 `text.chars().take(cursor)` 把它当 Unicode scalar 数量，再求 UTF-8 byte offset。BMP 拼音不受影响，非 BMP preedit 的 caret fallback 可能错误。这是 IMM32 兼容层有损的具体证据。

以下需求仍必须绕过 winit 高层接口直接做 TSF/native integration：`ITextStoreACP` 文档锁/edit session、surrounding text/reconversion、完整 clause/display attributes、候选列表/UI-less sink、input scope、可验证 TIP/profile 身份、应用自绘候选与 TSF-only 文本服务。

## 自动门禁与反向验证

- `cargo test --manifest-path spikes/04-ime-cjk/Cargo.toml --locked`：**6 passed，0 failed，0 ignored**。
- `cargo clippy --manifest-path spikes/04-ime-cjk/Cargo.toml --all-targets --locked -- -D warnings`：0 warning。
- 候选 oracle 故障注入：把“汉”的候选期望 2 改 1，候选测试退出 101。
- bt-term oracle 故障注入：把任一 `expected_cells_bt_term` 改错，真实 `Term::input` 对照立即报告对应 id 的 actual/expected 差异。
- 自证式实现防回归：精确断言只有 15 条匹配及七条 mismatch id/数值；若未来 alacritty 聚类或宽度库变化，测试必须红并要求重新裁决，不能静默变成“22/22”。
- 最低日志反向门：构造含 boot/area/frame/enabled/preedit/第二 area/commit/shutdown 的“Totally Real Pinyin 9.9”日志，并只给 item 1/6 marker；严格审计拒绝，明确缺 `[2, 3, 4, 5, 7, 8, 9, 10]`。
- 无输入烟测仍被严格审计拒绝；不得冒充真实 IME PASS。
- 根 workspace `cargo metadata` 中 spike package/member 数均为 0；vendored alacritty 仍是根 workspace member。

## 真人验收交接

本机已发现 Microsoft Pinyin 与微信输入法 2.1.0.36，没有搜狗拼音。需安装搜狗或明确指定另一家主流第三方输入法，然后逐家运行：

```powershell
cd D:\Developer\BetterTerminal\spikes\04-ime-cjk
.\run-manual.ps1 -ImeName "Microsoft Pinyin" -LogName microsoft-pinyin
.\run-manual.ps1 -ImeName "WeChat Input 2.1.0.36" -LogName wechat-input
.\run-manual.ps1 -ImeName "Sogou Pinyin <version>" -LogName sogou-pinyin
```

每做完清单一项并记录肉眼结果后按一次 F4。交回三份 JSONL、三行结果表，并用系统输入法 UI/截图佐证实际身份。已有日志不会被覆盖。脚本退出 0 只代表最低结构和十个 marker 存在。

判定规则不变：

- 三家核心项全 PASS，双拼/DPI 只有有理由的 N/A：**go-with-caveats**，M0 可从 winit IME 起步；
- 任一家出现协议级 Preedit/Commit、候选跟随或切换损坏，且归因为 winit/IMM32 表达能力不足：**no-go**，先提交 TSF 方案裁决；
- 少于三家、身份无法佐证或只有日志没有肉眼表：维持 **no-go（证据不足）**。

## 遗留风险

1. 三家 IME、双拼、候选框与 DPI 的真人交互仍未完成。
2. bt-term 的 char-by-char 宽度与七条 grapheme 产品目标冲突；M0 前必须裁决是否以及在哪一层聚类。
3. Ambiguous Width 缺规格决策；WT #370 不能以单一 narrow oracle 宣称解决。
4. 三条裁剪是已知视觉失败；字体 fallback、彩色 emoji 与 cluster-aware 绘制仍需后续原型。
5. 非 BMP preedit 可能触发 winit UTF-16 cursor offset bug。
6. 输入法身份当前不可由 JSONL 自证；需要环境佐证，或以后增加受控 native 身份探针。

## 上游依据

- winit 0.30.13 `Ime` / cursor area API：<https://docs.rs/winit/0.30.13/winit/event/enum.Ime.html>、<https://docs.rs/winit/0.30.13/winit/window/struct.Window.html>。
- winit Windows IMM32 实现：<https://docs.rs/winit/0.30.13/src/winit/platform_impl/windows/ime.rs.html>。
- Unicode Standard Annex #11：<https://www.unicode.org/reports/tr11/>。
- Unicode Emoji / UTS #51：<https://www.unicode.org/reports/tr51/>。
- Windows Terminal #370：<https://github.com/microsoft/terminal/issues/370>。
- Windows Terminal #900：<https://github.com/microsoft/terminal/issues/900>。

## 偏离／裁决申请

没有自行偏离现有 DESIGN 决策，也没有修改现有 crate。请求后续审核裁决两个规格空白：

1. **Ambiguous Width**：固定 narrow、固定 CJK-wide、locale 驱动，还是用户可配置；在裁决前 corpus 同时保留 7/9，不把任一侧冒充权威。
2. **Emoji/ZWJ grapheme cell 聚类**：是否在 M0 修改 bt-term 的输入/cell 占用协议，使七条候选目标成为真实权威；在裁决前保留 15/22 差异，不为追求绿灯改产品目标。

本轮不启动 03/05/06，不进入 M0。
