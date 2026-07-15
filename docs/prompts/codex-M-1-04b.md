# Spike 04b —— 交给真人之前必须修的两处 + 撤回一个结论

> 前置：`d88514f spike: add Windows IME and CJK probe`
> 这不是推翻 04。**no-go 的形状是对的**，工具是真的，第一节说明哪些不要动。

## 零、做对的，别动

审核实测确认：

- **no-go 是「诚实的未覆盖」不是托词**：`04-ime-cjk.md:105-109` 明确写了"还差什么证据、拿到后怎么改判"（三家全 PASS → go-with-caveats；协议级失败 → 转 TSF；证据不足 → 维持）。**你拒绝了 SendInput，也没把未测项写成"应该没问题"——这正是任务书最想要的。**
- **IMM32 那条属实且是本 spike 最有价值的产出**：审核去 `winit-0.30.13/src/platform_impl/windows/ime.rs` 逐行核了，只有 `ImmGetCompositionStringW` / `ImmSetCandidateWindow` / `ImmSetCompositionWindow` / `ImmAssociateContextEx`，**全文无任何 TSF 符号**。
- **`JsonlLogger::create` 用 `create_new(true)`**（`lib.rs:94`）→ 已有日志不被覆盖。**你把"FAIL 证据不许被重跑冲掉"这条纪律焊进了工具**，而不是写在文档里祈祷。
- **每条 emit 都 flush**（`lib.rs:124`）：对一个"崩溃本身就是证据"的探针，牺牲吞吐换证据完整性是对的。
- **`keyboard_input` 带 `preedit_active`**（`ime-probe.rs:515`）→ 重复提交/Backspace 穿透**在日志里可判**，不依赖肉眼。清单第 3、5 条能成立全靠它。
- **`handle_key` 里 `state.preedit.is_empty()` 的守卫看着像启发式，其实正确**：原始 KeyboardInput 在 `:510` 已**无条件先进日志**，守卫只管画面回显、不吃证据。判定走日志、守卫走 UI，分层是对的。
- **人工清单质量远高于及格线**：10 条每条都有「怎么操作/该看到什么/日志应有/失败判据」；第 9 条把"**只写'应该可用'而未实测**"本身列为 FAIL；结尾"任何 FAIL 都不要自行解释为'输入法问题'"。
- lint 门有牙：审核种 `todo!()` → clippy 报错退出非零。spike 的 `[lints]` 写在 package 级，**没踩进 CONVENTIONS §3.3 那个"一条都没生效的空门"**。

---

## 一、【阻断】撤回"22/22 匹配"——它是自证的，真实数字是 15/22

**这是本 spike 本该发现的头号事实，而那个最响亮的数字正好把它盖住了。**

两套权威白纸黑字：

| | 出处 | 算法 |
|---|---|---|
| **真权威** | `vendor/alacritty_terminal/src/term/mod.rs:1203` | `c.width()` —— **逐 char**，零宽字符 `push_zerowidth` 挂到前一个 cell |
| **spike 造的** | `spikes/04-ime-cjk/src/lib.rs:273` `terminal_policy_cells` | `graphemes(...).map(width).sum()` —— **逐字素簇** |

**你造了第二套权威，然后和自己比对，得到 22/22。** 审核实测的真实分歧 **7/22**：

| 用例 | spike oracle | **bt-term/alacritty 实际** |
|---|---:|---:|
| `❤️` heart-emoji-vs16 | 2 | **1** |
| `1️⃣` keycap | 2 | **1** |
| `👍🏽` skin-tone | 2 | **4** |
| `👩‍💻` woman-technologist | 2 | **4** |
| `👨‍👩‍👧‍👦` family-zwj | 2 | **8** |
| `🏳️‍🌈` rainbow-flag | 2 | **3** |
| `A✏️B` wt-900-pencil-emoji | 4 | **3** |

**而分歧本身就是产出**：我另外核了，`grep grapheme` 在 `vendor/alacritty_terminal` 与 `crates/bt-term` **零命中**——**bt-term 今天没有 grapheme 聚类**。也就是说：**我们自己的终端现在就有我们要修的那个 bug**（`👨‍👩‍👧‍👦` 拆成 8 个 cell）。**那就是 WT #900 本体，是任务书点名要你对照的案例集。**

**要求**：
- corpus 增列 **`expected_cells_bt_term`**，与 `expected_cells`（策略目标）并列，**两个数都要有出处标注**。
- **加一条 dev-dependency 路径依赖引 `vendor/alacritty_terminal`**（路径依赖**不会**把 spike 拉进根 workspace，`cargo metadata` 可自证），喂 `Term::input` 跑一遍取**真实 cell 占用**做对照。
- 报告把 **15/22 与那 7 条分歧**作为**主结论之一**报上来，并明确写出它对 M0 的含义：**要不要给 bt-term 上 grapheme 聚类，是 M0 的裁决项。**
- `terminal_policy_cells` 改名，名字里不许再暗示它是"终端策略"——它是**候选策略**，不是权威。

> **这一半是我的锅**，见第四节。

## 二、【阻断】清单第 10 条的判据不可达——它会把探针的 bug 记成 winit 的缺陷

**这条不修就交给真人，产出的证据是坏的。**

- `ime-probe.rs:130-135`：蓝框 x = anchor + `UnicodeWidthStr::width(prefix)` × 14 × scale —— **cell 几何**
- `ime-probe.rs:221-237`：预编辑文字用 cosmic-text 的**自然 advance** 画（20×scale 字号）

**两者必然漂移**——artifact 里 `汉` 自然 20.0px vs cell 14.0px，**每个汉字漂 6px**。

而 `MANUAL-CHECKLIST.md:31` 第 10 条要求真人确认"蓝框、preedit、候选框仍重合"。**他会看到不重合，然后记 FAIL——而那是探针的缺陷，不是 winit 的。**

**更讽刺的是 `lib.rs` 里有整套 slot 约束算法，探针没用它**——探针在视觉上违反了本 spike 自己的模型。

**要求**：探针改用 lib 的约束算法画 preedit（首选）；或把第 10 条判据改成"**蓝框与候选框的相对关系跨 DPI 不变**"。

## 三、【应修】

**3.1 `run-manual.ps1` 绿了 ≠ 清单跑完了。** 审核手造了一份 **8 条记录**的日志（boot/area/frame/enabled/1 条 preedit/第二个 area/1 条 commit/shutdown，`ime_name` 写 `"Totally Real Pinyin 9.9"`）→ `--strict-ime` **failures 空、退出码 0**。即：真人只做第 1 条 + 按一次 F2，脚本就打印成功，**第 2/3/4/5/8/9/10 条自动侧零覆盖**。
你的判定规则没错（`:109` 要求肉眼表），但**脚本的绿会被读成"过了"**。
**修**：探针加 checklist-item marker（如 F4 循环当前项号并 emit `checklist_item`），审计器断言 10 项都出现过。最低限度也要在清单和脚本输出里写明"**退出 0 只表示最低证据存在，逐项结论以肉眼表为准**"。

**3.2 oracle 真值来源未标注**（违 CONVENTIONS §0）。`corpus/cjk-width-cases.json` 的 22 条 `expected_cells` 是**裸字面量**，审核实测它**逐条等于 `unicode-width 0.2` 的 grapheme 求和**。所以 §3.3 那句"什么情况下它会红"的答案是：**"unicode-width 改行为时"，而不是"我们的策略选错时"**。**门会响，但它守的不是我们以为的东西。** 修：每条标 UAX #11 / UTS #51 / wcwidth 出处，与实现来源解耦。

**3.3 ambiguous width 是个未申报的产品决策。** `corpus:6` 的 class 直接叫 `ambiguous-narrow-policy`，`wt-370-ambiguous-star` expected 7 = narrow；审核实测 `width_cjk` 给 **9**。**WT #370 的本体就是"CJK 语境下 ambiguous 该判宽"**——你把 narrow 一侧钉进了测试，**DESIGN 里找不到依据，也没写偏离申请**。所以"#370 已覆盖"是**名义覆盖**。**这不是你选错，是你没把它作为发现报上来**（规格缺口是既有的，见第四节）。修：报告写明两种策略的数值差、指出规格缺口、请裁决。

**3.4 IMM32 边界表有一处失准，且它影响清单第 2 条。** `04-ime-cjk.md:50` 把 "target-converted range" 列为"winit 未暴露"。实际 `ime.rs:29-69` 中 winit **正是**用 `ATTR_TARGET_CONVERTED`/`ATTR_TARGET_NOTCONVERTED` 算出那个 range——**它暴露的单个 range 就是 target clause**（IME 未分段时才回退 `GCS_CURSORPOS`）。
正确表述：**多 clause 边界与每 clause 的 display attribute 被压成了一个 range**，丢的是"哪几段已转换/未转换"。
**这个更正加强你的结论**，但它直接影响清单第 2 条判据——**左右键移动时变的是目标 clause 而非 caret，测试者按"预编辑光标"去看会看错**。

**3.5 补一条上游 latent bug 进边界表**：`ime.rs:75-78` 把 `GCS_CURSORPOS`（**UTF-16 code unit** 偏移）用 `text.chars().take(cursor)` 当 **char** 偏移使。BMP-only 的拼音不受影响，非 BMP 组合串会算错。**这是"IMM32 兼容层有损"的又一个具体、可引用的证据。**

**3.6 `--ime-name` 不可证伪。** `:22` 说"winit 事件本身不提供当前输入法的可验证身份"——对 winit 属实，但 **Win32 可以**（`GetKeyboardLayout` / `ImmGetDescriptionW`）。整个结论建立在"三家真实 IME"上，而三份日志的身份全靠**手打字符串**。不做的理由合理（`unsafe_code = "deny"`），但报告该写"**可以做、我们选择不做、代价是身份不可核**"，而不是"不提供"。

## 四、任务书没说清的（我的锅）

1. **第一节那条有一半是我的锅**：任务书写了"宽度权威在 bt-term"，但**同时**写了"四个 spike 都不碰现有 crate""探针不要污染主 workspace"。**两条合起来读，最自然的理解就是"别碰 bt-term"**。现在明确：**可以只读地路径依赖 `vendor/alacritty_terminal` / `crates/bt-term` 做 oracle 对照——路径依赖不进根 workspace，`cargo metadata` 可自证。**
2. **3.3 的规格缺口是既有的**：任务书点名 WT #370，但"ambiguous 判宽还是窄"DESIGN 里没有。**你的错只是没把它作为发现报上来，不是选错了。**
3. **3.6**：任务书没界定"三家 IME"的身份如何取证。

## 五、不要做

- 不要动 no-go 的判定规则（`:105-109`）——它是对的。
- 不要为了让"22/22"好看而改 corpus 的期望值。**分歧是产出，不是瑕疵。**
- 不要开 03/05/06（它们会并行派出，与你无关）；不要进 M0。

## 六、次要

- `04-ime-cjk.md:76` 写「♥︎」(U+2665)，corpus 里是「❤︎」(U+2764)。笔误，但报告要能被逐字复核。
- `docs/spikes/README.md` 汇总表仍写 04"未执行/不在本会话范围"，与已交付报告矛盾。
- **3 条被裁剪的 cluster 是已知视觉失败，不是中性统计**：`heart-text-vs15` ❤︎ 自然 27.46px 挤进 14px slot（CLIP，2 glyph——**VS15 请求文本形态，cosmic-text 却给了 emoji 形态字形**）；`wt-370` 的 ☆；`wt-900-pencil-text` `A✏B` 自然 51.83 vs 权威 42.0。**这正是 WT #900 那一族**。报告 `:76` 列成计数、`:116` 归入"待截图审核"——诚实，但摘要那句"22/22 匹配"读起来像全过。**宽度对了不等于画得出来**，这一点要在摘要里说。

## 七、交付要求

每条说明：**改了什么、为什么、怎么验证它会红**。完成后跑 `cargo test`、`cargo clippy --all-targets --locked -- -D warnings`，并**自证 spike 仍不在根 workspace**（`cargo metadata` 输出 workspace_members）。
