# M-1 第一部分返工验收报告（第二轮）

日期：2026-07-15 · 前置：`claude-review-M-1-part1.md`（第一轮，判定不通过）

## 结论

**不通过，但"接近通过"——返工是真的。**

上一轮 5 个 BLOCKER 全部经**独立探针实测**确认修复、8 个 MAJOR 全部处理、主链真的装起来了、报告如实撤回了误报。**但引入 2 个新 BLOCKER**，其中一个比它修复的那个更严重。修掉即可进入下一步。

`cargo test --workspace` 38 passed（上轮 24）/ 0 failed / 0 ignored；clippy 带 `-D warnings` 通过；无 `todo!()`/`#[ignore]`。

⚠️ **但这个数字有欺骗性**：`vendor/*` 不是 workspace member，`--workspace` **从不运行 vendor 自带的测试**。把 `vendor/vte` 拷出来单跑，**upstream 自己的测试挂了一个**。38 passed 掩盖了一个真实回归。

## 上一轮问题的验收（全部经探针实测，非读码推测）

| # | 结论 | 证据 |
|---|---|---|
| B-1 DL/IL@row0 误捕获 | ✅ 修复 | `ESC[1M`→`frozen=[]`（上轮 `["a"]`）；IL/`ESC[3M` 同样为空；反向验证没修坏（正常上滚→`["one"]`、全屏 DECSTBM→`["a"]`、局部→`[]`）。修法即建议的意图参数。**假注释已改对** |
| B-2 宽高同变丢历史 | ✅ 修复 | `resize(6,2)`→`frozen=["r1","r2"]`（上轮永久消失）。**`infer_removed_top` 启发式已删除**，改 vendor 在 `Grid::resize` 前按真实公式复制移出行 |
| B-3 APC/DCS 内 ED3 抹历史 | ⚠️ 机制修对，但过度修正 → NEW-B1 | 裸字节嗅探彻底删除，改结构化上报 ✅；但同时改了 vte 状态机 |
| B-4 G1 主链不存在 | ✅ 修复 | bt-term now 依赖 bt-doc/bt-detect/bt-viewport；新增 `DualPlaneSession`。探针从**纯字节**驱动全链：`$$x$$`→冻结→检测→`DecorationLifecycle::Ready` |
| B-5 转录层无配额 | ✅ 修复 | 配额生效且**走 `delete_transaction` 同一管线**，探针确认 tombstone + 锚点降级到后继 |
| M-1~M-8 | ✅ 全部修复 | 四版本单一来源、`SourceLifecycle` 单一定义、`redetect` 重建装饰意图、`feed` 整 slice + 256KiB quantum + proptest、`anchor_y` 真投影函数、`live_row` 真实行号、启发式删除、staging 尾部改写接线 |

**报告已如实撤回**：02-dual-plane.md 明写"首次交付把零件级单测误报为三门全通过；该结论已撤回"，并主动披露 vendor 面扩大。**语料重录**：`claude-code-session.btcr` 209 字节 → **226,262 字节**真交互式会话（三次输入、alt-screen、两次 resize、真实 Claude Code TUI）。

## 三门独立核对

- **G1 实质 PASS**：每个场景都从字节驱动复现过；"全尺寸可寻址"确实改成断言网格内容而非 `dims` 数字。
- **G2 PASS**：`anchor_y` 是真投影。同一锚 `History{id:1,offset:6}` 在宽 4 → 18px、宽 10 → 0px，与手算一致；alt screen → `Err(IsolatedScreen)`。
- **G3 FAIL**：见 NEW-B2。

## 新 BLOCKER

### NEW-B1｜vte 补丁改坏 VT 状态机，未终止的 DCS/APC **永久卡死终端**

`vendor/vte/src/lib.rs`（+38/-5）把 `dcs_passthrough`/`sos_pm_apc_string` 的 ESC 语义改成"只有 `ESC \` 才结束"。

- **upstream 自己的测试挂了**：`tests::intermediate_reset_on_dcs_exit` FAILED。因 vendor 不在 workspace，`--workspace` 从不跑它，所以没被发现。
- **实测永久卡死**（比它修的 bug 更严重）：未终止 APC/DCS 之后 `ESC[2J ESC[1;1H hello` 全被吞，**连 RIS(`ESC c`) 都救不回**，只有 CAN/SUB 能恢复。原版 vte 里任何 ESC 都能重同步。上一轮是"cat 二进制清空历史"，现在是"cat 二进制**永久冻结终端**"——正好违背它要服务的 §1.3。
- **补丁自相矛盾**：OSC 路径没改，所以 `ESC]0;t ESC[3J BEL` 照样清空历史，而 `ESC_p ESC[3J ESC\` 不清。同一威胁模型两种行为——证明补丁是**照着复现步骤调出来的**，不是从规格推导的。

> **审核方自我更正（重要）**：上一轮 B-3 的复现步骤**本身是错的**。`ESC_payload ESC[3J ESC\` 里的 ESC 按 DEC STD 070 / xterm **确实**终止 APC，`ESC[3J` **确实**是真 CSI ED3（正因如此 Kitty 图形协议强制 base64、载荷不许含裸 ESC）。B-3 的**真实缺陷只有一个：裸字节嗅探绕过状态机**，而那个已被正确修好。Codex 修对了实质，又为了满足复现步骤的字面结果去改状态机——**过度修正**。正确动作是**整体 revert vte 补丁**。

### NEW-B2｜Live 锚点在滚动/收缩后从不 rebase，持久锚点静默漂移

`bt-doc:170 capture_transaction` **只迁移 `point.row == live_row` 的那一行锚点**，存活行的锚点保留陈旧 row：

```
8×2 喂 "one\r\ntwo"，锚点 Live{row:1} → 指向 "two"
再喂 "\r\nthree" → 锚点仍 Live{row:1} → 现在指向 "three"。内容漂移，无告警。

8×4 锚点 Live{row:3}→"r4"；resize(8,2) 后 r4 在 row 1
锚点仍 Live{row:3} → 越界；anchor_y 返回 Ok(92160)，投影到网格外，不报错不 clamp。
```

- 违反 §3.2：类型叫 **Content**Anchor，前提就是跟随内容；持久锚点定义含选区端点、滚动锚。"滚动输出时保持选区"是最常见操作。
- **反证很强**：stock alacritty 自己在 `scroll_up_relative` 里就做 `selection.rotate(...)`。用 ContentAnchor 取代它却没接 rebase，**相对被包裹物是净回归**。
- **漏检根因**：G1/G2/G3 里**每一个锚点测试都用 `live_anchor(0, _)`**——row 0 恰好是唯一被旧代码覆盖的行。又是"零件过检、接缝没测"。

## MAJOR

- **vendor 不在任何测试覆盖内**：补丁面已从"单文件 3 hunk ~30 行"膨胀到 **alacritty 15 hunk +112/-4 ＋ vte +38/-5**。alacritty 那份逐 hunk 核对**是正当的**（结构化上报确需这些点，policy 没进 vendor）；vte 那份应整体撤销。无论如何 vendor 必须纳入 CI。

## MINOR

1. `Term::resize` 复制了 `Grid::shrink_lines` 的公式（隐式耦合，上游改动会静默漂移）；更稳是让 grid 自己上报移除集合。
2. `delete_history` 里的 `transition(Tombstoned)` 是仪式性代码（下一行就 remove，无可观测效果）。
3. `redetect_document` 忽略 `revision`（`let _ = revision`）→ G3 的"revision 变化→意图重建"断言接近平凡。
4. `anchor_y` 硬编码 18px/行且忽略 artifact 高度 → 与高度树不自洽。
5. `project_view` 每次新建 projection → `LayoutCache` 复用路径从未被验证。
6. spikes README/02 结论 `go-with-caveats` 应在 NEW-B1/B2 修复前下调。

## 返工指令

1. **【BLOCKER】整体 revert `vendor/vte` 补丁**，vendor 面回到 alacritty 单文件。ESC 中止 DCS/APC/SOS/PM 是标准正确行为，stock vte 是对的。
   - 删除/改写 `regression_ed3_bytes_inside_apc_or_dcs_are_data`：正确断言是"**ED3 只能来自解析器的 `ClearMode::Saved` 结构化上报，不得来自字节嗅探**"，而非"APC 载荷里的 ED3 不生效"。审核方已确认原复现步骤有误。
   - 加新回归：**未终止的 APC/DCS 后终端能被后续 ESC 重同步**（`ESC_stuck` + `ESC[2J ESC[1;1Hhello` → 可见 `hello`）。
2. **【BLOCKER】实现 Live 锚点 rebase**：捕获事务里除迁移 `row == live_row` 外，还须把 `row > live_row` 的 primary Live 锚点整体上移；resize 变矮同理。参考 stock alacritty 的 `Selection::rotate`。
   - 回归必须**覆盖 row ≠ 0**；**把现有所有 `live_anchor(0, _)` 至少补一个非 0 行的对偶用例**——这是本轮漏检的根因。
   - 越界 Live 锚点应 clamp 或报错，不能投影到网格外。
3. **【MAJOR】vendor 纳入测试**：加入 `workspace.members` 或加一个跑 vendor upstream 测试的 job。验收标准：**vendor 补丁后 upstream 测试 0 failed**。
4. **【偏离申请】** 若坚持保留任何 vte 改动，必须提交正式申请，论证为何偏离 DEC STD 070，并解决"未终止字符串永久卡死"与"OSC/APC 行为不一致"。否则按 1 revert。
5. 【MINOR】按上表依次处理。
6. 修完 1/2 后重写 02-dual-plane.md 的 G3 结论；更新 vendor 补丁面表格；遗留风险补 vendor 测试覆盖。

## 元教训（写给审核方自己）

**审核给的复现步骤是症状，不是规格。** 我上一轮 B-3 的步骤本身就是错的，Codex 照着它修反而修出更大的洞；NEW-B2 同理——因为复现步骤只用 row 0，它就只修了 row 0。两个新 BLOCKER 出自同一模式：**为了让复现步骤字面通过而做局部修正，而没有回到规格/标准去推导**。下一轮指令必须写明：以规格与 upstream 契约为准，复现步骤仅作参考。
