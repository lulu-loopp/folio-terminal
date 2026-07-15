# M-1 第一部分审核报告（Claude 审 Codex 交付）

日期：2026-07-15 · 审核对象：Codex 的 M-1 spike 交付（Cargo workspace 6 crates + 语料 + docs/spikes 报告）
依据：`docs/DESIGN.md` v3.3、`docs/prompts/codex-M-1.md`

## 总体结论

**不达标，必须返工。**

交付的东西**不是假的**：vendor 补丁面确实最小（对 crates.io 原版逐字节 diff：单文件 3 hunk ~30 行）、语料是真实 ConPTY 录制、bt-term 真的包着 alacritty `Term`、`cargo test` 24 passed、clippy 0 warning、无 `todo!()`/`#[ignore]`。

但 **G1/G2/G3 三门全 PASS 的结论不成立**：

1. **三个可复现的真实 bug**（探针实测，非读码推测）
2. **六个 crate 是六座孤岛**：`capture_transaction`/`detect_block_math`/`evict_oldest`/`rewrite_staged`/`compare_anchors` 全部只有自测调用者，`relayout`/`redetect` 连测试都没有；`bt-term` 不依赖 `bt-doc`/`bt-detect`，所以 G1 第一条 `scroll-out→staging→定稿→decorate→rewrite` 主链**物理上不可能存在**。24 个测试全是手工构造入参调单个函数。

典型的"每个零件单独通过检验，但从没装到一起"。规格的价值在协议的接缝上，而接缝正是没被测的部分。

## 三个 BLOCKER（均有复现步骤）

**B-1｜DL/IL 在 row 0 被误捕获进永久历史**
`vendor/alacritty_terminal/src/term/mod.rs:794-807` + `bt-term/src/lib.rs:88`
hook 挂在共享的 `scroll_up_relative` 上、只靠 `origin==Line(0)` 判意图；`delete_lines` 走同一路径。
复现：8×4 喂 `a\r\nb\r\nc` 后 `ESC[1;1H ESC[1M` → frozen 从 `[]` 变成 `["a"]`。
违反 §3.1「IL/DL 掉出行不捕获」。vendor 注释 792-793 明写"deliberately never emit"——**注释与实际行为相反**。DECSTBM 测试用 `ESC[2;3r` 恰好绕开该缺陷。
修法：给 `scroll_up_relative` 加意图参数，`scroll_up` 传 true、`delete_lines`/`insert_blank_lines` 传 false。

**B-2｜宽高同时变小 → 历史静默丢失**
`bt-term/src/lib.rs:180-182`：捕获条件是 `columns == old_columns && rows < old_rows`，宽高同变时整个分支被跳过。
复现：8×4 喂 `r1..r4`，`resize(6,2)` → frozen `[]`、r1/r2 **永久消失**；`resize(8,2)` 则正确。
**拖窗口角是常规操作**，不是边角情况。违反 §3.1 变矮 oracle。

**B-3｜APC/DCS 载荷里的 `ESC[3J` 抹掉整个转录**
`bt-term/src/lib.rs:144-158`：ED3/RIS/DECCOLM 全靠 `recent.ends_with(b"\x1b[3J")` **裸字节嗅探**，绕过 VT 状态机。
复现：`ESC_payload ESC[3J ESC\`（APC）和 `ESC P0;1| ESC[3J ESC\`（DCS）**都把 frozen 清空**。
`cat` 一个二进制文件就能清空用户全部历史。违反 §1.3 不可信输入防护 + CLAUDE.md「不用启发式」。
修法：删掉嗅探，从 vendor 的 `clear_screen(ClearMode::Saved)`/`reset_state`/`deccolm` 上报。

**B-4｜G1 主链不存在**（见总体结论）
**B-5｜转录层无配额**：`frozen: VecDeque` 无上限，`evict_oldest` 无调用者。alacritty 侧 scrollback=0 → **整个系统没有任何历史上限**。§1.1「转录层是唯一配额权威」是空的。

## 主要 MAJOR

- **M-1｜四版本被复制成两套**：`bt-detect::{DetectionRevision,LayoutVersion}` vs `bt-viewport::{DetectionRevision,LayoutKey}` 是四个不同类型、从不互通。§4.1 的「四版本」作为统一协议不存在。
- **M-2｜两个互相矛盾的 `SourceLifecycle`**：`bt-doc:64`（3 态）vs `bt-detect:7`（4 态）；`Mutable`/`Quiescent` **全 workspace 从未被构造**，无转移函数。
- **M-3｜`redetect` 干的正是规格禁止的事**：`bt-viewport:189` 只重算布局、不碰 `DecorationIntent`，而 §3.3 明写"而不仅是清布局缓存"。
- **M-4｜分包不变性测试恒真**：`feed()` 逐字节 `advance`，分包边界构造上不可观测 → 断言永远不会失败；且违反 §1.3 的 256 KiB quantum。
- **M-5｜G2 的选区/滚动锚断言是空转**：只证明了两个 struct 不共享内存；**全工程无 `ContentAnchor → 视口像素` 投影函数**，所以"锚点正确"无从验证。
- **M-6｜`live_row` 硬编码 0**（`bt-term:134`）：一旦接线，每次捕获都会把 row 0 的锚点错误迁走。
- **M-7｜`infer_removed_top` 是启发式对齐**：靠切片匹配猜偏移，重复行（进度条/分隔线）会歧义。有正确解（vendor hook 上报），不该接受该风险。
- **M-8｜staging 尾部改写未接线**，但 G1 表仍列为覆盖——报告自相矛盾。

## 符合规格的部分（值得肯定）

scrollback=0（真断言）、vendor 补丁面最小、staging 配额/强制拆分/强制定稿、resize 变高不回填、ContentAnchor 三变体 + Bias + 命名空间隔离（`Err(IsolatedScreen)`）、ED3 删除降级（本工程质量最高的测试）、文档/投影分层、Fenwick i64 定点高度树（36/18px 是真推导）、语料真实性（`ESC[6n`、win32-input-mode、微秒时间戳）。

## 返工指令（已发给用户转交 Codex）

1. **先纠正报告**：`docs/spikes/02-dual-plane.md` 三门结论改 **no-go**，如实写明主链从未端到端运行、G2 断言空转、G3 仅单元级。
2. **修三个 bug**（先加失败测试再修）。
3. **根治 resize 对齐**：删掉 `infer_removed_top` 启发式，从 vendor hook 上报被移除行（同时解决 B-2/M-7）。
4. **把六座孤岛装起来**：版本类型与 `SourceLifecycle` 单一来源；写出真正的状态转移函数；`redetect` 驱动装饰重建；实现锚点→像素投影；`live_row` 换真实行号；实现 frozen 配额与淘汰；接上 staging 尾部改写。
5. **重写 G1/G2/G3**：每个场景从喂字节开始、到装饰状态结束，禁止手工构造中间态；`feed` 改整 slice；补 proptest 随机分包；"全尺寸可寻址"要断言网格内容而非 `dims` 数字。
6. **补录语料**：`claude-code-session.btcr` 仅 209 字节的 `-p` 固定输出，覆盖价值为零 → 重录**交互式**会话。
7. **偏离申请**：§1.3 逐字节解析、§3.3 redetect 语义、`SourceLifecycle` 双定义三项属实质偏离却报告"无偏离"。
