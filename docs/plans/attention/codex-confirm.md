# Codex 窄复核：§10 v2 增补闭合性

复核日期：2026-08-25。范围只含 `codex-review.md` 的六条必改、Q3/Q6/Q7/Q8，以及 §10 新增内容是否自相矛盾；不重开已通过的产品裁决。仓内调用点按当前 `main.rs` 复核；Claude Code 外部事实按官方 [Hooks reference](https://code.claude.com/docs/en/hooks) 复核。

## 逐条结论

| 原评审项 | 结论 | 窄复核 |
|---|---|---|
| 1. `RequestAttention` 不能直接等同「等你回答」 | **仍需修改** | §10.8 的弱 `AttentionRequested` / 强 `AwaitingInput` 分层方向正确，但两层是独立生产源，却被塞进一个只有 `attention_request: Option<u64>` 的单源状态机；见下文「状态机死角」。因此 Q6 的语义裁决已接受，承载它的数据模型尚未闭合。 |
| 2. bool 无法表示「已答、程序未撤」 | **仍需修改** | `Acknowledged(e)` 对同一代 `Settle` 是不动点，这个核心修正成立；但 episode 的铸造所有权、双源合成、退号 trace 与强凭据后到时的 toast 仍有缺口，当前十七边不能直接施工。 |
| 3. A0 把 ConPTY 正确行为判成失败 | **闭合** | §10.5 已把门改为「目标程序观察到 transition + 未订阅普通程序无乱码」，并补 `I`、后台出生和真实 Folio 边沿。没有重引入原矛盾。 |
| 4. 焦点判据漏真实 KeyboardOwner | **闭合** | §10.10 的四项谓词、任一事实变化即重算、`Unknown/Focused/Unfocused` 初态同步，以及 terminal-generated reply 与 UserInput 分流，逐项覆盖 Q8。 |
| 5. hook 把普通 `Stop`/整个 `Notification` 当 waiting | **仍需修改** | `wait` 白名单的语义和拼写已改对，普通 `Stop` 已排除；但 start/clear 生命周期没有配平，权限请求可把强断言永久留在 `Acknowledged`，吞掉下一次请求。 |
| 6. 三档通知漏「前台窗口、非活动 tab」 | **闭合** | §10.7 补足 8 行真值表，该格定为 Flash + 窗内徽章、不 toast；与既有三档裁决无新冲突。 |
| Q3 命名管道安全合同 | **闭合** | §10.6 六条合同覆盖 endpoint 绑定、128-bit capability、显式 logon-SID DACL、remote/first-instance 标志、最小动词/限流、pane 生命周期和强能力隔离，并诚实承认不抵抗同用户恶意进程。只有 §10.4.2/§10.6 对“唯一路线”的表述需收窄，安全合同本身通过。 |
| Q6 通用序列语义 | **仍需修改** | `yes/no` 为可撤状态、`once` 为 Bell、`fireworks` 忽略，以及弱/强文案分层均正确；问题只在双源生命周期没有被状态机表达。 |
| Q7 `mark_seen` 整片清铃 | **闭合** | §10.9 明确只清 seen/one-shot 账，不碰 raw request、ticket、ack generation，并要求同片修正文档注释；覆盖原回归钉。 |
| Q8 焦点上报粒度 | **闭合** | pane 粒度、KeyboardOwner、订阅初态和协议自答来源全部闭合。 |

## §10.2 状态机：核心不动点成立，但还有四个死角

1. **episode 的所有权与路线 A 冲突。** §10.2.1 写死 `SessionStatus.attention_request` 只由 OSC 解析器写、episode 只在 `bt-term` 的 `no→yes` 上升沿铸造；§10.8 又要求 `folio attention wait|clear` 的强源共用同一台机器。管道事件不经过 OSC 解析器，当前没有合法的铸号入口，也没有存放强源 live 状态的位置。应把 episode 协调器放到能同时接收两种 source edge 的层，或明确各源 generation 如何汇入一个 app 侧 episode；不能让 A3 反向伪造 `bt-term` 的流事实。

2. **弱/强是独立电平，不能只存一个电平和一个只升不降的 `grounds`。** 典型反例：OSC `yes` → 用户已答进入 `Acknowledged(e)` → hook 出现一个新的真实权限请求。按 e13，新的 `wait` 被当作同一 episode 的幂等 `Yes`，永不入队。另一个反例：弱 `yes` 与强 `wait` 同时成立，随后强 `clear`、弱仍为 `yes`；§10.8.2 的「grounds 不降」会继续显示「正在等你回答」，虽然唯一的强凭据已撤。至少要分别记录 weak/strong 的 asserted/generation，`clear/no` 只撤自己的源；对**精确已确认 generation**，`Acknowledged` 对 `Settle` 仍保持不动点；其他源后来出现的新 generation 不能被旧 ack 吞掉。

3. **C3 的唯一边 e4 与「强信号才 toast」冲突。** 若弱 `Requested` 先 e4 入队，它按规则不 toast；六秒后 `permission_prompt` 把 grounds 升为 `AwaitingInput`，§10.8 又要求不重发号，而 §10.2 规定 toast 只能在 e4，于是这次真正的 waiting 永远没有 toast。应把门写成「该 episode 首次获得 `AwaitingInput` 且尚未 toast」；它可以发生在 admission，也可以发生在 queued 状态的首次强升级，仍只投一次、仍不重排 ticket。

4. **e7/e14/e16 的副作用与 trace schema 不可实现。** `Requested(e)` 和 `Acknowledged(e)` 都没有 ticket，却在 e7/e14 被描述成 `withdraw`；trace 的 `withdraw` 行又强制 `ticket=<t>`。e16 说任意态都写 `expire ticket=<t> episode=<e>`，但 `Idle` 没 episode，Requested/Acknowledged 没 ticket。应限定只有 `Queued` 的 `No/LeafGone` 写 withdraw/expire；其余状态只是清/销毁，或另定义不带 ticket 的 trace 事件。这个修正不改变产品语义，只让十七边与字段合同一致。

因此，问题中特别点名的结论是：**`Acknowledged(e)` 对同一 generation 的 `Settle` 确实是不动点，没有 bool 版的立即重入队；但当前状态机仍不是闭合状态机，不能按现表直接落代码。**

## §10.3.2：八个 `write_pty_input` 调用点

**数量闭合，定性仍需修改。** 当前生产代码确有八个直接调用点：两处 `take_pty_writes`、files-row、PSReadLine repair、`send_mouse_input_to`、`send_user_input`、clipboard paste、IME commit。协议自答和 PSReadLine repair 不算 Answer；键盘、提交后的 IME、paste、files-row 算 Answer，这些结论成立。

鼠标一行没有逐调用图闭合：

- `send_user_input` 的同一个调用点既承载键盘，也承载 forwarded mouse button；表中把它只写成 `UserInputKind::Keyboard` 不准确。
- `send_mouse_input_to` 不只承载点击/滚轮，还承载 SGR mouse motion。若把 `answer_attention` 下沉到这个 helper，程序开启 motion tracking 后，用户仅移动鼠标就会退票。
- 所以 Q2b 的子格至少要拆成 `MouseButton`、`MouseWheel`、`MouseMotion`；现有裁决只足以覆盖前两者，motion 必须明确为不算，或另交用户裁。施工点应留在产生语义的事件入口，再携带细分后的 `UserInputKind`，不能在两个 write helper 上统一消费。

## §10.4：官方拼写正确，hook 生命周期未闭合

官方文档确认以下拼写均正确：`PermissionRequest`、`Elicitation`、`ElicitationResult`、`permission_prompt`、`idle_prompt`、`auth_success`、`elicitation_dialog`、`elicitation_url_dialog`、`elicitation_complete`、`elicitation_response`、`agent_needs_input`、`agent_completed`、`quota_auto_resume_fired`、`quota_auto_resume_stale`、`quota_auto_resume_disabled`、`StopFailure`。`Stop` 的定义也确为 main agent finished responding，排除它是正确的。§10.4.1 的「其余 24 个事件」是计数笔误：31 个 hook event 中，去掉被 wait/clear 使用的 6 个唯一 event（含 `Notification`）后应为 **25** 个。

真正未闭合的是 clear 配对：

- `PermissionRequest` / `permission_prompt` 发 `wait` 后，批准或拒绝权限并不会触发 `UserPromptSubmit`；表中没有可靠的 permission-resolved `clear`。用户按键只把 Folio 状态送入 `Acknowledged`，不会撤掉强源。
- 下一次 `PermissionRequest` 因 e13 的幂等规则会被吞掉。把 `permission_prompt` 作为 `PermissionRequest` 的延迟兜底还会产生另一难题：没有 request id 时，无法区分「同一请求的迟到重复」与「已 ack 后的新请求」。
- `agent_needs_input` 与 `quota_auto_resume_stale` 也只列了 start；`agent_completed`、`quota_auto_resume_fired/disabled` 被列为“不 wait”，却没有说明它们是否以及如何结束对应断言。

A3 开工前必须给每个 wait 源写出可靠的结束/关联规则。可选形状是带受限 opaque request id 的 generation，或能证明完整覆盖的成对 hook；仅靠两个无参数、全局幂等的 `wait/clear` 加当前名单不足以连续处理两次权限请求。此处还要与 §10.6 的「只准两个动词」同时修订：可以仍只有两个动词，但必须说清是否允许一个不具寻址能力的有界关联键。

## `terminalSequence` 新发现与分片依赖

**官方事实闭合。** 官方文档明确把 `terminalSequence` 限于 OSC 0/1/2/9/99/777 与 BEL，并明确拒绝 OSC 1337；也明确说 hooks 无 controlling terminal，应由 Claude Code 的 terminal write path 代发。因此 B′ 不能承载 `RequestAttention`，A1 的 OSC 1337 不能由 Claude Code hook 直接生产。

**“所以命名管道是唯一路线”仍需收窄。** 白名单事实只证明 `terminalSequence` 这一路不通；它不单独证明 `CONOUT$` 的实验结论，也不排除 loopback/broker/继承 handle 等其他 IPC。可以据官方指导作产品/工程选择，停止探索直接 tty 写入，但准确表述应是：**在当前上游版本、既定安全边界和本方案已选路线中，pane-local attention pipe 是 Claude Code 强状态的唯一已定义生产路线。** 未来若上游把 1337 加入白名单，该结论自然失效。§10.6 的安全合同不受此措辞修正影响。

对依赖的正确影响是：

- **A3 必须在 A2 之前或与 A2 原子落地**，否则 BEL 撤出后 Claude 场景没有强生产者；C3 继续依赖 A2 + A3 + C2。这两条闭合。
- **新事实不推出 A3 依赖 A1。** A1 是通用程序的 OSC 弱源，A3 是 Claude hook 的 pipe 强源；二者应并行喂一个共享的双源 episode 核心。若片单仍保留 `A1 → A3`，应说明这是打包顺序而非技术依赖；更干净的拆法是先抽共享状态核心，然后 A1/A3 并行，A2 再依赖该核心、Q2a/Q2b 与 A3。是否要求 A2 同批交付 A1 的通用弱源，可以保留现有产品范围，不影响这条因果关系。
- A0 的①②可以因“本方案选择不用直接 tty”而取消；不能写成官方白名单已经实验性证明 `CONOUT$` 失败。

## 总判定

**仍需修改，暂不能进入 A1/A3/A2 施工。** 六条必改中 A0、焦点 owner、三档补格已闭合；Q3、Q7 闭合；Q6 的产品语义已闭合但状态承载未闭合。阻塞施工的最小修订集是：补双源 generation/ack 合成与 trace 边、补强凭据后到的单次 toast 边、把鼠标 motion 从 Answer 中拆出、为 hook wait 写出可连续工作的 clear/关联规则，并收窄“管道唯一路线”及相应依赖表述。完成这些后，无需重开任何已通过的产品裁决即可转为通过。
