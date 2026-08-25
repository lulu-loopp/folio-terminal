# Codex 最终窄确认：§11 v3 终修与 CLI 普查折入

复核日期：2026-08-25。范围只含 `codex-confirm.md` 总判定点名的五项最小修订集，以及 §11.10 对 `evidence-cli-survey-2026-08-25.md` 的折入是否产生新矛盾；不重开任何已通过的产品裁决。

## 五项最小修订集

| 项 | 结论 | 窄确认 |
|---|---|---|
| 双源 generation / ack 合成与 trace 边 | **仍需修改** | §11.1 已把弱、强源分账，给出两条 ack 水位、app 侧 episode 协调器、派生态、完整事件网格，并正确闭合“旧 ack 吞新源”和“强撤弱留”两个反例；`withdraw` / `expire` 也已限定到有 ticket 的状态。剩两处施工级缺口：① `attention_request: Option<u64>` 和 `attention_waits: Vec<(key,g)>` 只保存 live generation，清空后没有保存 `next_weak_gen` / `next_strong_gen`；`AttentionLedger.episode: Option<u64>` 清到 `Idle` 后也没有保存 last/next episode，却要求 generation/episode 永不复用且下一次 `mint prev=` 指向旧 episode。当前字段草图无法实现这些不变量。② §11.1.5 把 `toast why=turn-end` 写成必带 `ticket`、`episode`，§11.7 又明确回合结束不铸 episode、不发 ticket；§11.9 ⑱还要求每一行都有 `episode=`。事件级 turn-end trace 因而无合法形状。应补持久递增计数器/last episode，并把 awaiting 与 turn-end 的 trace 字段按是否有 episode/ticket 分开。 |
| 强凭据后到的单次 toast 边 | **闭合** | §11.2 把唯一门改成 `queued && grounds == AwaitingInput && !toasted`，每次转移后求值；覆盖 admit 与 queued 后到强升级两处。`toasted` 随 episode 生灭、降级不清、旧 key 幂等，因此既补投一次，又不重铸/重排 ticket，也不重复投。 |
| 鼠标 motion 拆出 Answer | **闭合** | §11.3 已把 mouse button、wheel、motion 分开，明确 motion 不算 Answer；判断留在语义事件入口而非两个混合承载的 write helper，并以 `UserInputKind::is_answer()`、trace 枚举和 `?1003h` 真机门封住回归。 |
| hook wait 的 clear 配对 | **仍需修改** | §11.4 增加有界 key、逐源出口、TTL/SessionEnd/leaf-gone 兜底，方向正确，但原复核点名的“无 request id 时，迟到重复与下一次请求不可区分”仍未闭合。固定 `kind` 兜底键在旧 wait 尚挂着时会把下一次真实请求当旧 key 幂等吞掉；反过来，0 秒事件有 id、约 6 秒通知没有同一 id 时，又会把同一次请求铸成新 generation。§11.4.3 的“t5 不依赖 t4”只对 `key=B` 成立，对固定兜底键不成立。并且 `PostToolUse` 的定义与关联字段仍被明确留到“A3 开工前取回”，尚不是已确认的可靠出口。§11.10 的 codex 映射又只有 `user-prompt-submit` / `stop` clear，而普查列出的 `permission-request` payload 没有 request id，故同一缺口在第二家适配器上重现。需给无 id/跨事件无共同 id 的路径定义可判定的换代/去重规则，并完成 `PostToolUse` 关联事实确认。 |
| “管道唯一路线”及依赖表述收窄 | **闭合** | §11.5 已准确限定为“Claude Code 强层在本方案中唯一已定义的生产路线”，明确白名单只否掉 hook 代发 1337，不证明 `CONOUT$` 或其他 IPC 不可行，也不否定通用程序直接发 1337；依赖表已改为先 A-core、A1 与 A3 并行、A2 依赖 A-core + A3、C3 依赖不变。 |

## §11.10 的三项折入

| 折入项 | 结论 | 窄确认 |
|---|---|---|
| 进度环“有源，只是 Claude Code/codex 不发” | **闭合** | §11.10.1 将范围收窄到两家，并保留 pi / copilot CLI 的 OSC 9;4 生产事实；不改变 D4 的 Claude 场景诊断，也不与 §11.1–§11.9 冲突。 |
| A1 定位为通用性正门、八家零现发 | **闭合** | §11.10.2 同时收回“最快见效”的话术、把验收门限定为协议形状，并维持 A1 ∥ A3；`OSC 133` 八家全零和 `?1004` 对 A0.5 的支持也已分别折入，没有改变既有裁决。 |
| 适配器收敛为 A/B/C 三型 | **仍需修改** | 架构收敛本身不冲突，但事件映射有两处新矛盾：① pi 的 `agent_settled` 是“run fully settles/回合结束”语义，却在 §11.10.3 被映射为强层 `wait`；这与 §10.4.1、§11.6–§11.7 已钉死的“回合结束是 `Announced`，不升 `AwaitingInput`”同型冲突。② codex 的无 request-id `permission-request` 映射没有满足 §11.4 的连续 clear/关联合同，见上项。前者应按既有裁决走事件级/A8，除非另有更强的等待输入事件凭据；后者须先补关联规则。§11.10.4 的 `Attached` B1 被明确留裁且拆出本块，不算本次闭合矛盾。 |

## 总判定

**仍需修改，尚未完全闭合。** 五项最小修订集中，强凭据后到 toast、mouse motion、管道路线收窄三项闭合；双源核心的持久计数/trace 形状，以及 hook 的无 id 关联与可靠 clear 两项仍未闭合。§11.10 的进度与 A1 两项更正无新矛盾，但适配器折入新增了 pi 回合结束被升成 `wait` 的语义冲突，并在 codex 上复现无 id clear 缺口。

所需修订均是状态承载、trace schema 与事件映射的闭合，不需要重开任何已经通过的产品裁决。
