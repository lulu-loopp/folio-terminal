# M-1 第一部分 —— 验收通过（第三轮）

日期：2026-07-15 · 前置：`claude-review-M-1-part1.md`（一轮，不通过）、`claude-review-M-1-part1-rework.md`（二轮，2 个新 BLOCKER）

## 结论

**通过。可进入 M0。** 无阻塞项。

两个 NEW-BLOCKER 均**从规格推导**而非照复现步骤字面修补；早先 5 个 BLOCKER 无一回退；vendor 已纳入一键测试且 upstream 0 failed。剩余 3 条低危 MINOR，不阻塞。

## 测试

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **224 passed / 0 failed / 0 ignored**（一轮 24 → 二轮 38 → 现在 224） |
| `clippy --all-targets -- -D warnings` | 0 warning（已 touch 源文件强制重跑，非缓存回放） |
| `#[allow]`/`expect(clippy)` 抑制 | **0 处**（干净不是靠抑制换来的） |
| `todo!()`/`#[ignore]`/FIXME | 0 处 |

**vendor upstream 测试——结构性解决**：`vendor/alacritty_terminal` 已加入 `workspace.members`，`--workspace` 直接覆盖：alacritty 单测 134 + ref 45 + doctest 1 = **180 passed / 0 failed**。二轮挂掉的 `intermediate_reset_on_dcs_exit` 随 vte 补丁撤销而恢复。**不再需要"拷出来单跑"——回归防线已内建**，这比修 bug 本身更有价值。

## NEW-B1｜vte 补丁 → ✅ 按正确方式关闭

- **vte 补丁整体 revert**：`vendor/` 下只剩 `alacritty_terminal`；`Cargo.lock` 中 vte 0.15.0 来自 crates.io。
- **未终止字符串可被 ESC 重同步**：APC/DCS/SOS/PM **四类全部** resync=true；RIS 亦可恢复。二轮"永久卡死"消失。
- **ED3 仅来自结构化上报**：全 workspace grep 无任何裸字节嗅探；唯一来源是 `clear_screen(ClearMode::Saved)`。
- **错误的回归测试已删除**，改写为 `ed3_is_only_emitted_by_the_vt_clear_history_action`。
- **OSC/APC 自相矛盾消除**，实测四种组合全部符合 DEC STD 070：

```
ESC 终止的 APC + ED3   → 清空 ✅（ESC 终止 APC，ED3 是真命令）
APC 纯数据里的 [3J     → 不变 ✅
ESC 终止的 OSC + ED3   → 清空 ✅（与 APC 一致）
OSC 纯数据里的 [3J     → 不变 ✅
```

> **这正是审核方自我更正后的正确答案。** Codex 没有迎合二轮那条**错误的复现步骤**，而是回到标准推导，并让 OSC/APC 归于同一模型。元原则被正确执行——这是本轮能通过的主要原因。

## NEW-B2｜Live 锚点 rebase → ✅ 按通用算法关闭

`capture_rows_transaction` 现为通用算法（按"该锚点之前被移除了几行"整体上移），非 row-0 特判。探针实测（全部覆盖 row≠0）：

```
滚动：8×2 anchor Live{row:1}→"two"，喂 "\r\nthree" → Live{row:0} 仍指向 "two" ✅（二轮漂移到 "three"）
收缩：8×4 anchor Live{row:3}→"r4"，resize(8,2) → Live{row:1} 仍指向 "r4" ✅；row:2→row:0 对偶也对
越界：anchor_y(row=50, live=2) = Err(LiveOutOfBounds) ✅（二轮返回 Ok(92160) 投影到网格外）
```

审核方追加的对抗性探针（Codex 未测的路径）全部通过：单次 feed 滚出 3 行的批量 rebase、alt-screen 锚点不被 primary 滚动波及、alt 进出不污染 primary、无行移出时的陈旧锚点返回 Err、DECSTBM 不捕获、256KiB 分包不变性（整喂 vs 7 字节切片，2000 行结果完全一致）。

## MAJOR/MINOR

| 项 | 结论 |
|---|---|
| vendor 纳入测试 | ✅ 结构性解决（workspace member） |
| vendor 补丁面 | ✅ 收敛：**单文件 15 hunk +109/-4**，逐条核对全是结构化事件上报，无 policy 泄漏 |
| `redetect_document` 忽略 revision | ✅ revision 进入 `DecorationIntent::Math`，G3 断言非平凡 |
| `anchor_y` 18px 硬编码 | ✅ 改为构造参数注入，artifact 行取 local_y=0，与高度树自洽 |
| `project_view` 未验证缓存复用 | ✅ 新增 `refresh_view`，断言二次 refresh `cache_misses` 不增长 |
| `Term::resize` 复制 `shrink_lines` 公式 | ⚠️ 未结构性消除但**实现正确且已如实披露**；审核方核对与上游等价。接受取舍（让 grid 上报会扩大 vendor 面），但 M0 必须加版本护栏 |
| `delete_history` 死赋值 | ⚠️ MINOR，`entry.source = Tombstoned` 后 entry 即 drop，无可观测效果 |

## 三门独立核对

- **G1 PASS**：从 `$$x$$` 字节起 → staging → tail rewrite（断言 after≠before）→ 定稿 → `DecorationIntent::Math` → `run_workers` → `Ready` → 投影高度。resize 矩阵用网格内容作 oracle。
- **G2 PASS**：同一锚点在 4-cell/10-cell 独立投影（18*SUB vs 0），选区/滚动锚独立，缓存复用已验证。
- **G3 PASS**（二轮 FAIL 已翻转）：四版本失效边界、stale worker 丢弃、redetect revision 入 intent、ED3 原子删除 + 锚点降级、存活 Live rebase、越界拒绝。

**早先修复无回退**（探针实测）：B-1 DL/IL@row0 不捕获且正常上滚仍捕获；B-2 宽高同变 frozen=["r1","r2"]；B-4 主链存活；B-5 配额生效；`SourceLifecycle` 单一定义、`transition()` 真被调用、无幽灵变体、`infer_removed_top` 确已消失。

## 新问题

**BLOCKER 0、MAJOR 0。** 三条 MINOR：

1. `bt-doc:331` 死赋值（绕过 `transition()`）——建议删除。
2. **G1/G2/G3 门测试自身仍只用 `live_anchor(0, _)`**——根因已被专项测试直接覆盖，故不升级；但建议 M0 顺手改为非 0 行。**row 0 是唯一会掩盖 rebase 缺陷的行，不应再作为默认取值。**
3. 规格用词偏差（观察项）：§3.2 写"原子迁移仅持久锚点"，实现对所有已注册 primary Live 锚点 rebase；spike 中无"非持久锚点"概念故等价，M0 引入时需回到此处加区分。

## M0 携带事项

1. 【顺手】删除 `bt-doc:331` 死赋值。
2. 【顺手】G2/G3 的 `live_anchor(0, _)` 至少各改一个为非 0 行。
3. 【M0 必须建的防线】`Term::resize` 镜像 `Grid::shrink_lines` 公式是**已知实现耦合**（非语义偏离，等价性已核对）。必须加**版本护栏**：pin alacritty 版本，升级流程强制 diff `grid/resize.rs::shrink_lines`。224 tests 能兜住行为，兜不住上游公式静默漂移。
4. 【M0 前置】`vendor/alacritty_terminal` **必须永久留在 `workspace.members`**——二轮漏检的根因就是它不在里面。移出视为回归。
5. 【M0 范围内待定】frozen 配额 100,000 是 spike 参数，需按真实内存数据定生产值；worker 仍是固定高度假 worker（真进程隔离/取消/公平调度属独立任务）；staging live-tail snapshot 目前只服务协议验证，扩展时不得改变冻结规则。

## 元原则（延续到 M0）

**以 DESIGN.md 与 upstream 契约为准推导；复现步骤仅作症状参考。** 本轮 Codex 面对一条审核方已承认写错的复现步骤，选择回到 DEC STD 070 推导并让 OSC/APC 归于同一模型——这是正确判断。
