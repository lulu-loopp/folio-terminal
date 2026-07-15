# M-1 Task 00b —— 00 的小返工（与 spike 并行，M0 开工前合入）

> 前置：commit `2bc91de` "chore: harden M-1 baseline"（Task 00）。
> 权威：`docs/DESIGN.md` §1.2 crate 划分、**§3.1 生命周期事件表**；`CONVENTIONS.md` §0/§2/§3.2/§3.3/§5。
> 这不是推翻 00。00 的护栏是真的活了，下面第一节说明哪些不要动。

## 零、先说做对的，别动它们

审核实测确认（这些是真交付，返工时不要回退）：

- **00.1**：六个 crate 都有 `[lints] workspace = true`；toolchain 钉了完整三元组。**clippy 此前从未运行过，现在跑得起来且全绿**——`todo!()` 反向测试实测会红（退出码 101）。你把"MSRV 是地板、构建工具链是天花板"这个混淆讲清楚了，那正是根因。
- **00.4 有牙，实测过**：把 `(Flags::BOLD, CellFlags::BOLD)` 改成 `CellFlags::DIM` → G1 红；把 `NamedColor::Red => 1` 改成 `=> 9` → G1 红。从真 VT 字节到 `frozen styles` 的链路真的被钉住了。**"样式是只写数据"这个三轮未除的问题，这次是真解决了。**
- **00.7**：真 `vte::Parser`，且 `intermediates.is_empty()` 正确排除了 `\x1b[?6n`（`?` 在 vte 里进 intermediates）——这个细节容易漏，你没漏。
- **00.10**：`capture()` 的 4 个 unwrap 是靠**重构结构**消失的（`back_mut().filter()` + `pop_back()`），不是靠加校验——正是要的做法。`bt-detect:169` 裸 `1024` → `SUBPIXELS_PER_PX` 是任务书没要求的超额，方向对。
- **`height_tree.rs`** 用模块级 `#[expect(clippy::indexing_slicing, reason=...)]` 而不给 Fenwick 树套 `.get().ok_or()`——严格照"明确不做"执行，对。
- **00.6 点名的两个函数搬得比要求更彻底**：`ResizeEpoch` 进 `scheduling.rs` 且 `pub(crate)`，`TerminalAdapter` 上已无痕迹。

## 一、【返工】00b.1 round-trip 的射程只有 12 个 flag 中的 4 个

**实测证据（我亲手跑的，不是推断）**：把 `cell_capture.rs:69` 的 `(Flags::DIM, CellFlags::DIM)` 改成 `(Flags::DIM, CellFlags::HIDDEN)` —— **`cargo test --workspace` 241 个测试全绿，零报警**。即：冻结历史里"变暗"静默变成"隐藏"，没有任何东西会红。

**为什么这是 00.3 的漏网**：00.3 的立论是"upstream 重排 Flags 的位 → 测试全绿 → 冻结的历史静默改变含义"。现在换了个马甲：**我们自己的映射表写错 → 测试全绿 → 冻结的历史静默改变含义**。危害路径一模一样。

**先澄清一件事，免得你改错方向**：这**不是**恒真断言。`capture_flags` 和 `decode_flags` 是两张独立的表，单侧写错**是**能抓的——只要那个 flag 在样本里。所以问题是**射程**，修法是**穷尽**，不是重设计。

**要求**：
- `cell_capture.rs:201` 的 flags round-trip 改为**穷尽映射表全部 12 个 flag**，且必须能让**任意单个 flag 映射错**被抓到（不是只测 `Flags::all()` 的并集——那样 DIM→HIDDEN 仍可能因并集相同而漏）。想清楚：单点错怎么才必然可见。
- 颜色同理：29 个 `NamedColor` 变体目前只测了 2 个，改为穷尽。
- 依据：`CONVENTIONS.md` §3.2 规则 2 "多字段的键：每个字段都必须有一个能让它单独失效的测试"——**bitflags 就是 12 个字段的键**。
- 交付时请说明：你怎么验证新测试**会红**（照 §3.3，逐个 flag 手改一遍不现实，但至少给出你实际改坏验证过的那几个）。

> **这一半是任务书的锅**：00.3 只说"补 round-trip 测试"，没说"穷尽所有变体"。§3.2 规则 2 覆盖了，但要你把 bitflags 认成"多字段的键"是个跳跃。

## 二、【返工】00b.2 `lifecycle.rs` 交付的是空壳（00.5 最高价值的那一项没达成）

**核实过的事实**：

- `LIFECYCLE_RULES` 的 7 条与 `AdapterEvent` 的 7 个变体**严格一一对应**，`event_kind()` 是全函数。
- 所以 `directive()` 永远返回 `Some`；而 `directive` 是**从 event 推出来的**，`session.rs:304` 的 `match (directive, event)` 配对永远自洽。
- **结论：这张表是 enum 的同义反复。改表不改变任何行为**，只能让一个永远走不到的分支返回错误。它是一层可以整个删掉而行为不变的间接。
- **DESIGN §3.1 有 14 行语义规则**（DECSTBM 掉出行不捕获、用户上滚只动 viewport、resize 变高不回填、变矮丢空行、soft-wrap 片段不定稿、最后一行永不冻结、ED3/配额同一删除管线……）——**`lifecycle.rs` 里一行都没有**。
- 而它头上写着 `/// DESIGN.md §3.1 executable lifecycle table`。按 `CONVENTIONS.md` §5，**这是撒谎注释**。

**要求（倾向 (a)，但你可以按 §0 从规格反驳）**：

- **(a) 让表真的承载判定**：`lifecycle.rs` 成为 §3.1 的可执行形式——输入是**事实**（什么事件、什么 cause、行的形状），输出是 directive。判定从 `adapter.rs` 搬进来。此时"改规格 = 改这张表"成立，表也才对得起它的注释。
- **(b) 如果你论证 (a) 不可行**：那就承认它只是 dispatch helper —— 删掉表和 `directive()`，session 直接 `match event`，**并且把那句注释删掉**。不许留一个自称是 §3.1 却不是 §3.1 的东西。

**为什么现在做而不是拖到 M0**：M0 第一件事就是往 `bt-term` 加真 PTY 代码，而 00.5 的全部理由就是"先拆好，新代码才落得进对的文件"。**§3.1 的家现在还没建好**，M0 的代码落在空壳旁边等于把 00.5 白做一遍。

> **这一条主要是任务书的锅**，我要认：00.5 说"§3.1 在规格里是一张表，在代码里就该是一张表"（指向 14 行语义规则），紧接着又说"现在它埋在一个 44 行的 match 里"（那个 match 是事件分派，指向另一个意思）。**描述自相矛盾**，你选了工程上最省事的读法，怪不得你。现在明确：**要的是 §3.1 那 14 行。**

## 三、【返工】00b.3 适配层仍在做 §3.1 判定、并直接执行转录侧变更

**位置**：`adapter.rs:201`、`:205`、`:222`、`:228`

- `:201` `cause == ScrollOutCause::Resize && row_is_blank(&row.cells)` → 不捕获。**这就是 §3.1"resize 变矮"那一行**（"光标下方空行被裁剪时直接丢弃"）。按 `CONVENTIONS.md` §2 的判据——"适配层只回答 alacritty 发生了什么，不回答我们要拿它怎么办"——**"这行是空的所以我们丢掉它"属于后者**。
- `:205 capture()` / `:222 clear_history()` / `:228 invalidate_staging()`：适配层**直接执行**转录侧变更，而 session 的 directive 分派只管 document 侧。**同一条规格的两半住在两层**，且执行顺序是"adapter 先做完转录、session 事后补 document"。

**要求**：与第二条一起做。`AdapterEvent` 携带**事实**（cause + cells），判定与转录写入都上移到 §3.1 表 / session。

注意这会让事件载荷变大（要带 cells）——**这是对的，不是代价**：适配层的职责就是"如实报告发生了什么"，把 cells 交出来正是如实。若你认为这破坏了原子性或顺序，请在交付说明里论证，别默默不做。

> **部分是任务书的锅**：00.6 只点名了 `decorations_allowed()` / `mark_resize_quiescent()`，没给出"适配层不得执行转录侧删除/捕获判定"的一般规则。要你从 §2 的判据自己推出来是合理期待（§0 就是这么要求的），但我本可以直说。

## 四、【返工】00b.4 两个幽灵变体（00.2 刚删掉一个，这里又进来两个）

**位置**：`session.rs:33-34` 定义，`:303`、`:359` 构造点。

`MissingLifecycleRule` 和 `LifecyclePayloadMismatch` **结构上不可构造**（理由见第二条）。grep 确认全 workspace 无任何测试构造它们。

**这正是首轮 M-2 骂过、00.2 刚要求你删掉 `Tombstoned` 的那个模式**——从未被构造的分支。也违反 `CLAUDE.md`"不要为不可能发生的场景加错误处理"。

而 `docs/reviews/00-baseline.md:24` 写"缺表项或 payload 不匹配会返回 `SessionError`，不再静默继续"——**这句话暗示它是活的护栏，实际它永远不会响**。按 §3.3，这是空门。

**要求**：随第二条一并处理。表若留下且真驱动行为，就让类型使不匹配**写不出来**（而不是运行时报错）；表若不留，两个变体一起删。

**注意 `MissingStagingSource` 不同**：它跨 crate 边界（transcript 给的 mappings vs session 自己的 map），留 `Result` 合理，**别一起删**。

## 五、【返工】00b.5 §2 的 crate 边界目前是空门

`CONVENTIONS.md` §2 说"适配层不得依赖 bt-doc / bt-detect / bt-viewport，CI 可 grep 它们的 use"。但 `bt-term/Cargo.toml` 依赖这三个 crate，所以在 `adapter.rs` 里写 `use bt_doc::...` **编译照过**，而 `.github/workflows/ci.yml` 里**没有这个 job**（唯一的静态检查是 vendor 的 `cargo metadata` 守卫，不覆盖这条）。

`docs/reviews/00-baseline.md:68` 称这些清理"由……静态依赖检查共同守护"——**这句话在 CI 里没有对应物**。

**要求**：CI 加门，并**照反向测试的规矩证明它会响**（在 CI 里种一行违规 use，要求 job 变红，再撤掉）。**目前 `adapter.rs` 确实干净，所以这个门今天就该是绿的——但绿的门和不存在的门长得一模一样，这正是 §3.3 存在的理由。**

## 六、不要做

- 不要动 00.1 / 00.4 / 00.7 / 00.10 的成果。
- 不要给 `height_tree.rs` 套 `.get().ok_or()`（"明确不做"仍然有效）。
- 不要开 spike，不要碰 `Term::resize` / `shrink_lines`，不要进 M0。
- 不要为 `dpi_milli` 造语义消费者——见下。

## 七、留给 M0（登记，本轮不做）

- **`dpi_milli` / `font_rev` / `theme_rev` 目前只接了缓存身份，没接语义**：`project()` 只读 `width_cells`，改 `dpi_milli` 会 invalidate 缓存然后重算出**完全相同**的高度。**你现在不给它造消费者是对的**（高度还是 `SPIKE_CELL_HEIGHT_SUBPIXELS` 常量，真字体度量没接进来之前它不可能影响任何测量值，硬造只会得到第二个只写机制）。但**登记为 M0 硬性携带项**：接真 cell height 时 `dpi_milli` 必须真正参与计算，否则它当场退回死规格。
- **00.8 只做了一半**：`lifecycle_matrix.rs`（G1）拆成 15 个独立函数，**拆得很好**；但 `tests/multiview.rs` 的整个 G2 仍是**一个 64 行函数**、约 14 个断言（正是任务书点名的那个反例的形状），`anchor_protocol.rs:137` 用 `for boundary in 0..4` 把 4 个场景塞进一个函数。可带进 M0。
- 小残留：`bt-transcript:255/261` 的无 else `if let`（刚 push 过必非空）；`session.rs:487` 的 `let Some(row) = ... else { return }`（`rows: NonZeroU32` 下永远 `Some`）以及那里**硬编码的 `visible_row(0)`**（逻辑对，但没注释说明"行滚出后续接片段就在顶行"，正是 §3.2"row 0 是唯一会掩盖缺陷的行"那一族的形状）；`bt-transcript:330` 对即将 return 的临时 `Vec` 调 `shrink_to_fit()`（无意义）。

## 八、交付要求

- 每条说明：**改了什么、为什么、怎么验证它会红**。
- 第二条若选 (b)，把论证写清楚——我会当真读。
- 完成后跑：`cargo test --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo fmt --check`，并报告新 CI job 的反向测试结果。
