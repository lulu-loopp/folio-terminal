# Codex 终审第二轮（窄）：§12「v4 收口」

复核日期：2026-08-25。范围只含 `codex-final.md` 点名的无 id 换代/去重、双源持久计数与 trace 形状、pi 映射冲突，以及 §12 自身是否新增矛盾；不重开其余产品裁决。

| 项 | 结论 | 窄确认 |
|---|---|---|
| 无 id 换代 / 去重 | **仍需修改** | §12.1 的主路已经闭合原反例：R1/R3 把 id 模式钉成 kind 级声明，R2 从源头禁止 primary/fallback 双装，R4 用 `gen <= acked_strong` 让已答电平重举；§12.1.4 也把 t0–t6、t4 缺席的 t5′ 和未答重申逐格重走，codex 的无 id `permission-request` 不再依赖固定兜底键，`PostToolUse` 亦已从正确性前提降成有完备三支的验证项。**但 R5 新增了施工级冲突**：R5 把所有 `receipt` 统一规定为“只退已答电平”，§12.1.5 的 V-a 却规定 id 模式下按 key 精确退一条，二者对“精确回执到达而该 generation 尚未被本地 Answer 追平”给出相反结果。更直接的反例是 `quota_auto_resume_fired` / `quota_auto_resume_disabled`：它们结束等待时可以没有任何用户 Answer，此时必有 `gen > acked_strong`，按 R5 会被零 trace 吞掉，quota wait 只能继续挂到边界或 TTL；这与 §11.4.2“每一种 wait 都有正常出口”以及 R5“回执只做卫生”的表述冲突。应明确：id 模式的同 key 回执是否无条件精确退；ack 闸只适用于哪些无 id、可能迟到的 kind 级回执；自动恢复结束类事件要么改成按 kind 的权威结束边界，要么另给不会被 ack 闸吞掉的规则。故 t0–t6 已通，但 R1–R6 整体尚未闭合。 |
| 双源持久计数 | **闭合** | §12.2 已把 live 弱/强 generation 与 `next_weak_gen` / `next_strong_gen` 分开，并为 episode 补齐 `last_episode` / `next_episode`；clear、TTL、就地换代和 drop 均不回退游标，`mint.prev` 跨 `Idle` 仍取最近一次 episode。作用域也明确为 leaf 账本一生，既有 per-window `next_ticket` 单独保留。当前字段草图已能承载 I2、“永不复用”和 `prev=`。 |
| trace 形状 | **仍需修改** | 点名的空集已经解开：`why=awaiting` 必带 ticket/真实 episode，`why=turn-end` 不带 ticket、写 `episode=-`，与“事件级不铸 episode、不发号”一致。**但完整 schema 仍未满足它自己重申的合同**：§11.1.5 的 `claim tab=<i> was=<C> now=<C>` 仍没有 `episode=`，而紧接着 §11.1.5、§11.9 ⑱及 §12.2.4 都要求每一行都有 `episode=<e|->`。至少应把 `claim` 定成 `episode=<e|->`，或明确将其移出该合同。此外 `src=<pipe|bel>` 把裸 BEL 与 pi/codex 的 OSC 9/777/99 都记成 `bel`，却又声称 `src` 能查出“是谁报的”；应把它明确改称传输类别，或扩展值域，避免 trace 把 OSC 来源写成 BEL。 |
| pi 映射冲突 | **闭合** | §11.10.3 已就地撤销 `agent_settled -> wait`：pi 明列为无强层源、出列 A 型，`agent_settled` 只作为事件级 `Announced` 经 A8 三档到桌面，不铸 episode、不发 ticket、不进队列；§12.4 ㉓也以“无 `mint`、队列不变”封住回归。它与 §10.4.1、§11.6、§11.7及红线 14 已一致。顺带撤掉未取证的 copilot `agent_idle -> wait` 是沿用既有封闭名单，没有反向重开裁决。 |

## §12 自身的新矛盾

**仍需修改。** 新矛盾集中在两处：一是 R5 的统一 ack 闸与 V-a 的精确 clear、quota 自动结束出口互相冲突；二是 trace 宣称“每行都有 episode”但 `claim` 形状仍缺该字段，且 `src=bel` 的字段名/解释与实际覆盖的 OSC 来源不一致。另有一处文字需在施工前钉清：§11.7 说同一回合第二个 turn-end 源无条件去重，却把 `announced_turn_end` 写成“投出去时置真”；应明确第一条被接受的 turn-end 决定即置真（包括 `reach=nothing`），否则先有焦得到 `Nothing`、后失焦收到重复源时可能补出一次 flash，反撞“不补发”。

## 总判定

**仍需修改，尚未完全闭合。** pi 映射冲突与双源持久计数已闭合；turn-end 的两种 trace 形状以及无 id 的 t0–t6 主走查也已闭合。剩余修改不要求重开产品裁决，但必须先消除 R5 的 clear 语义冲突、补齐完整 trace schema，并钉死 turn-end 去重位置，之后才能判 §12 全闭合。
