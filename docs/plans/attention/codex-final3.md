# Codex 终审第三轮（窄）：§13「v5 外科修」

复核日期：2026-08-25。范围只含 `codex-final2.md` 点名的三处闭合，以及 §13 自身是否新增矛盾；不重开其余产品裁决。

| 项 | 结论 | 窄确认 |
|---|---|---|
| R5 clear 语义冲突 | **闭合** | §12.1 R5 与 §13.1 已把原先混在 `receipt` 中的三种情况拆开：id 模式的同 key 精确回执按 key 无条件退、不过 ack 闸；无 id 的 kind 级迟到回声仍只退 `gen <= acked_strong` 的已答电平；`quota_auto_resume_fired` / `_disabled` 改为 `boundary-kind`，无回答也能无条件清 quota 槽。§12.1.5 V-a、quota 正常出口与 R5 不再互相给出相反结果，㉔/㉖也覆盖了两条原反例。 |
| trace schema：`claim episode=` 与 `src`/`via` | **闭合** | §11.1.5 的权威 schema 已把 `claim` 定为 `episode=<e\|->`，并给出 live episode / `last_episode` / `-` 的取值规则，“每行都有 `episode=`”不再有例外。`src=` 已收窄为 `pipe\|osc\|bel` 的传输类别，`via=` 单独记录具体序列或上游事件；turn-end 行两者必带，OSC 9/777/99 不再冒充裸 BEL，§12.4 ㉒有对应验收钉。 |
| turn-end 去重位置与时机 | **闭合** | §11.7 与 §13.3 已把 `announced_turn_end` 的含义改成“本回合结束已经决定过”：开关 On 且位为假时，第一条事实源求值后立即置真，包括 `reach=nothing`；后到重复源零效果零 trace。因而“先有焦得 Nothing、后失焦再来一源补出 Flash”的反例被直接封住，㉕覆盖该序列。 |

## §13 自身的新矛盾

**仍需修改。** §13.1.4 先把合同重述为“每一种 wait 都有一条正常出口，且在等待真的结束时可达”，但同表 `agent_needs_input` 行又明确承认：后台 agent 自行结束且用户从未 Answer 时，`agent_completed` 会被第二支 ack 闸拦住，只能落到边界或 TTL。§13.1.5 随后仍宣称新判据不会让出口变成空话，§11.4.2 的标题也仍是“每一种进 wait 的事件都在这张表里有出口”。这些话与已经写明的 agent 反例不能同时成立。

这不推翻上述 R5 三分，也不要求把无 id `agent_completed` 放过 ack 闸；最小修订是把合同和 §11.4.2 标题收窄为“每种 wait 都有正常出口或有界兜底，并明列不可精确正常退出的分支”。若仍要保留“每种都有可达正常出口”的强合同，则必须先取得 background session id、让 agent kind 进入 id 模式，不能同时把它留作施工后取证项。

## 总判定

**仍需修改，尚未完全闭合。** `codex-final2.md` 点名的三处原始反例均已闭合；但 §13.1 新增了一处“正常出口全称合同”与 agent 未答自结束分支的自相矛盾。完成上述文字/合同收窄（或先补齐 agent 的精确关联）后，才可判 §13 全闭合。
