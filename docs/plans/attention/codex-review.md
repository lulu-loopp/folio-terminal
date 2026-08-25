# Codex 评审：通知与注意力块方案

评审日期：2026-08-25。依据为 `plan.md`、`evidence-2026-08-25.md`、仓内当前实现与已记裁决；外部协议只采用官方文档。

## 总结论

方向成立：先补焦点上报、把 BEL 降回一次性事件、再给队列接真正可撤回的生产者，这三件事的因果顺序是对的。实测 B/D 也足以支持“焦点是 Claude Code 是否发 BEL 的一个独立闸门”。

但当前方案不能直接施工。有六个会造成错误语义或错误状态机的必改项：`RequestAttention` 被过度解释成“等你回答”、一个 bool 无法表达“用户已回答但程序尚未发 no”、A0 把 ConPTY 的正确行为判成失败、焦点判据漏了真实键盘 owner、hook 把普通 `Stop` 当等待输入、三档通知漏了“窗口聚焦但请求来自非活动 tab”。九个生产片的依赖也要重排，尤其 A2 不能先于 A3 单独落地。

## 留给 Codex 的四问

### Q3：命名管道安全模型

**结论：必改。威胁是真的；“地址是我们自己发的，所以不会被冒充”不成立。** 管道名是定位信息，不是身份。Windows 的默认命名管道 DACL 甚至给 Everyone/anonymous 读权限；官方也明确要求需要隔离时自行提供安全描述符，并建议用 logon SID 隔离登录会话（[Microsoft：Named Pipe Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)）。同一用户下的另一个进程至少可以连接和伪造；若它读到了 pane 的环境，随机管道名也不再是秘密。

路线 A 可以做，但安全合同必须改成下面这组，而不是宣称“不可冒充”：

1. `FOLIO_PANE` 只供诊断，绝不作为授权或路由依据。服务端在创建端点时就把它绑定到一个 `LeafSession`；消息里不接受 window/tab/seat，也不允许调用者改目标。
2. 每 pane 使用不可预测的端点名/至少 128-bit capability；`PIPE_REJECT_REMOTE_CLIENTS`、`FILE_FLAG_FIRST_PIPE_INSTANCE`；显式 DACL 只给当前 logon SID（及确有需要的 SYSTEM），不用默认描述符。
3. 这个 capability 只准两个幂等动词 `wait`/`clear`，限制单帧长度、连接数和速率。即使被同用户进程偷到，最大影响也只能是给这一 pane 制造/撤销一条注意力声明，不能打字、开 pane、读转录或寻址别的 pane。
4. pane 生命周期拥有端点；leaf 迁窗/换 seat 时端点随 `LeafSession` 迁移，leaf 消失立即关 server 并让旧 capability 永久失效。`FOLIO_PANE=<window>.<tab>.<seat>` 会在 tear-out、合并或座位重排后陈旧，不能成为稳定地址；仓内普通进程内路由已经用 `LeafId { tab, seat }`（`main.rs:491-493`）而不是 window 坐标，但它仍是会随结构动作改铸的内部地址，对外应暴露 opaque capability。
5. Folio 不从 cwd、仓库、`.claude/` 发现或加载 hook；安装器只写用户明确批准的用户级配置。这满足 P3-4 的“永远不能从工作区/仓库读自动化”。仓库里的程序仍能像发 BEL/OSC 一样调用它继承到的 pane-local `wait`，所以必须靠第 3 条把能力封死。
6. **不能与未来 P3-4/mailbox 共用同一份授权。** 可以复用管道库、事件循环和生命周期代码，但强能力必须另发 capability/另开 endpoint。否则一个只该“按门铃”的项目 hook 会自然升级成“向别的终端打字”，正面撞上 P3-4/P3-11 的 RCE 裁决。

因此，当前 attention-only 威胁可通过“明确承认同用户边界 + 把影响封到本 pane 的一位状态”接受；若目标是抵抗同用户恶意进程，环境变量中的名称/令牌不够，需改为不可转授的继承 handle 或更强 broker 身份模型，不能在本片顺手承诺。

### Q6：`RequestAttention` 有没有更好的既有先例

**结论：必改语义；没有找到更合适、同时具备开/关的既有终端序列。** iTerm2 当前正式定义确为 `OSC 1337;RequestAttention=yes|once|no|fireworks`：`yes` 持续弹 Dock、`once` 弹一次、`no` 取消（[iTerm2 官方 escape codes](https://iterm2.com/documentation-escape-codes.html)）。方案对拼写的核对通过。

但它只说“请求用户注意”，**没有说“程序阻塞并等你回答”**。build 完成、下载完成也完全可以持续请求注意。把通用 `yes` 直接投影为 `Awaiting`，仍然是在把较弱事实升级成较强断言，和本方案刚从 BEL 身上纠正的错误同型。`yes/no` 证明它是可撤回状态，不证明状态的原因是等待输入。

其他既有先例更不合适：kitty `OSC 99` 有 id、更新和 close，但官方把它定义为桌面通知消息（[kitty desktop notifications](https://sw.kovidgoyal.net/kitty/desktop-notifications/)），应留在 `TerminalNotification` 通道；BEL/xterm urgency 仍是事件。故不建议换另一个 OSC，也不接受自造 OSC。

可施工的二选一必须写进方案：

- 若 UI/队列坚持叫“等你回答”，只有明确的 `folio attention wait|clear`（及其用户批准 hook）可生产 `Awaiting`；通用 `OSC 1337 RequestAttention` 只能成为较弱的 `AttentionRequested`，不能进 P1-8 waiting 队列。
- 若坚持让任意程序的 `RequestAttention=yes` 直接入队，就必须把 claim、徽章和 toast 文案改成“需要注意/attention requested”，不得继续说“正在等你回答”。

`once` 可以兼容映射为现有一次性 Bell；`fireworks` 应明确忽略。红线 9 也应改成“`yes/no` 这一臂是状态，`once` 是事件”，不能笼统写“RequestAttention 是状态”。

### Q7：BEL 撤出入队门后，`mark_seen` 整片清铃是否仍成立

**结论：通过，保留整片清铃；但必须钉住它只清“一次性已看见账本”。** 当前 `TabState::mark_seen` 遍历所有 leaf（`main.rs:19052-19073`），`mark_leaf_seen` 只追平 revision 并调用 `clear_attention()`（`main.rs:21583-21594`）。对 BEL/失败/未读，这仍是正确的：tab 点是 fleet 聚合断言，活动 tab 的整棵 pane 树都上屏，切入 tab 就是整片被看见；只清 focused seat 会让同一 tab 的聚合点下一帧复活。

“队列逐 seat”不构成反例，因为队列 ticket 不是 `mark_seen` 的对象，今天也没有被它清。BEL 撤出队列后，两类账反而更干净：整片清的是 Bell/Failed/Unread；逐 seat 退的是 Awaiting episode。

必须补两条回归钉：

- `mark_seen` 清同 tab 每个 sibling 的 bell/failure/revision，但不清 `attention_requested`、不撤 ticket、不改“本次请求已应答”的代际状态。
- A1 不得为了省函数把新 `attention_requested` 塞进现有 `clear_attention()`；并把 `mark_seen` 注释里的“every claim”收窄为“every seen/one-shot claim”，否则注释会在 A1 后变成假话。

### Q8：焦点上报粒度是否漏位

**结论：必改。pane 粒度正确；漏掉的不是“聚焦模式上台位”，而是窗口真实的键盘 owner。** `focused_leaf` 只命名“键盘回到 shell 时落到哪一席”，并不证明键盘此刻在 terminal。仓里已有明确反例：preview/page 用 `focused_preview_seat`，网页自己有 `web_keyboard`；文件树、preview edit、search、menu/dialog 都能拿走键盘。`keyboard_owner_is_a_shell()` 正是现有统一判据（`main.rs:38563-38631`），而网页实现还明确写着 `focused_leaf` 永远不能代表 preview seat（`main.rs:38544-38560`）。

每个 terminal seat 的有效谓词应为：

```text
window_focused
&& tab_is_active
&& seat == tab.focused_leaf
&& keyboard_owner_is_a_shell()
```

`sessions.contains_key(seat)` 是地址守卫，不是第五个产品事实。聚焦模式不加位：它不另造键盘 owner，换台最终仍应落到 `focused_leaf`。独 preview、网页、文件树、搜索框、rename、menu/dialog 抢键盘时，owner 谓词会让原 terminal 收到 `CSI O`；归还时才收 `CSI I`。

一个 pane 从不可见 tab 变成可见、但仍不握键盘，**不发 `CSI I`**；协议报告的是输入焦点，不是可见性，方案倾向正确。

还漏了两个边沿要求：

1. 四个事实任一变化都要重算，包括 owner 在同一窗口内变化，不能只挂 `WindowEvent::Focused` 和 tab 切换。
2. 要定义“订阅边沿/初态”。ConPTY 从会话第一个字节就发 `?1004h`，子程序还可能再次发。一个在后台出生的 pane 若只等 `false→true` 的 UI 边沿，可能永远收不到初始 `O`。至少要用 `Unknown/Focused/Unfocused` 三态，并在 1004 首次/重新启用时同步当前答案；这项需由 A0 真机钉住。

焦点报告必须直接写目标 seat 的 `PtySession`，并标成 terminal-generated reply。绝不能走“用户输入”消费门；否则片 A0.5 发出的 `CSI O` 会被片 A2 当成“用户回答”而退票。

## 整份方案的技术矛盾与裁决冲突

| 级别 | 发现与结论 |
|---|---|
| **必改** | `plan.md:7` 声称“不推翻任何既有裁决”，但 Q2 同时改掉 2026-08-21 的 BEL 入队裁决和 2026-07-18 的 Enter-only 出队裁决。应改成“拟请求用户复议两条裁决，未裁前不得施工 A2”。 |
| **必改** | Q2 把“BEL 撤出”和“任意用户输入消费”绑成一个选择，逻辑并不对称：扩大出队门要求 BEL 先撤；BEL 撤出并不要求出队门扩大。用户完全可以选“BEL 撤、仍只 Enter”。必须拆成 Q2a/Q2b。 |
| **必改** | `attention_requested: bool` 不足以实现三主退出。用户输入拿走 ticket 后，raw bool 若仍为 true，下一轮 `settle_attention` 会立即重发新 ticket。需要 request episode/generation 与 acknowledged generation（或等价状态机）：`no` 结束 episode；用户输入只确认当前 episode；下一次 `no→yes` 才能再入队。 |
| **必改** | “任意用户输入字节”必须定义为带来源的用户动作，而不是任意 PTY write。键盘、IME commit、paste、被转发的 mouse 是否算要明列；焦点报告、颜色查询回复、协议回复绝不能算。当前键盘 Enter 在 `main.rs:63053-63065` 退票，paste、IME、mouse 分属别路，施工点不能粗暴下沉到 `write_pty_input`。 |
| **必改** | Claude hook 映射自相矛盾。官方定义 `Stop` 是“main agent finished responding”，不是“正在等批准”；`Notification` 还含 `auth_success`、`elicitation_complete/response` 等不等待输入的类型（[Claude Code hooks reference](https://code.claude.com/docs/en/hooks)）。方案刚证明“回合结束 BEL ≠ 等你”，却又用 `Stop → wait` 把同一事实升回 Awaiting。只应白名单真正需输入的 notification type（如 `permission_prompt`、`elicitation_dialog`）；`idle_prompt`/普通 Stop 是否算 waiting 必须另裁并匹配文案。 |
| **必改** | A0 ③的失败判据判反且与既有证据冲突。DESIGN §7.1.5i 已裁定 ConPTY 请求 1004 并把 `CSI I/O` 转成 `FOCUS_EVENT`；录音 D 又是“真 ConPTY + 写入 `CSI O` → Claude 改变行为”的端到端证据。被 ConPTY 消费不是 A0.5 作废条件。正确门是“目标程序观察到 focus transition，且普通程序输入不出现字面乱码”；仍需补 `I`、后台出生 pane 和真实 Folio 事件边沿。 |
| **必改** | 三档表漏一格：**窗口有焦点，但请求来自非活动 tab**。它既不满足档一，也不满足“可见但无焦点”的档二，更不是最小化/cloaked。C2 在写 8 行真值表前必须裁成 Nothing/Flash/Toast 中的一种；推荐 Flash + 窗内徽章、不 toast。 |
| **必改** | C3 的触发必须是“新 request episode 首次入队”的边沿，而不是每帧看见 ticket/raw yes；否则持续 `yes` 会连续 toast。其依赖不只是 A2+C2，还包括一个可用生产者 A3。 |
| **必改** | 可选 D 把 `Attached`、dead 接线、`closeOnExit` 三件不同生命周期问题塞进一片，且只挂 Q1。Q1 只裁第八记号；dead/closeOnExit 在 DESIGN §7.1.5b 已有独立裁决，不应被 Q1 一并授权，也不应借注意力块顺带施工。D 应拆出本方案，Q1 只留下 Attached。 |
| **建议** | A1 不是简单“给 `Osc1337Scanner` 扩表”。当前 `1337;` 在 `inline_image.rs:1067-1089` 直接进入 `InlineFileCapture`，需要先在共同前缀后分流 `File=` 与 `RequestAttention=`，并给 BEL/ST、跨 chunk、超限、未知 value、`fireworks` 原样忽略各一钉。方向可行，但施工描述应诚实。 |
| **建议** | 方案中的多处 `main.rs` 行号已漂移：`mark_seen` 当前为 19069，`answer_attention` 为 61076，`jump_to_attention` 为 61112，Enter 门为 63058，`settle_attention` 为 14297。开工前统一刷新，避免测试/评审指错实现。 |
| **通过** | BEL 撤回一次性 Bell、与 Awaiting 静态可分、toast 不排队不补发、`attention_is_consumed` 与 desktop reach 解耦、呼吸的 alt-screen 豁免不翻案，这五条红线均成立。 |

## 九个生产片的施工顺序风险

这里把 A0 视为 spike、D 视为块外可选项；生产片恰好九个。当前表的主要问题是 A2 只依赖 A1，却在正文又说应位于 A3 后或同批，二者必须统一。

| 片 | 评级 | 顺序结论 |
|---|---|---|
| **A0.5** | **必改后先做** | 保持最先，但先改 A0 的门；实现必须读真实 KeyboardOwner，并把 focus report 与 UserInput 分流。真机门应含 out、in、后台出生、普通 shell 四格。 |
| **A1** | **必改后可并行** | 可与 A0.5 并行；先落 parser + raw request episode，不落 UI。不能只加 bool，也不能先把泛化 `RequestAttention=yes` 命名为 Awaiting。 |
| **A3** | **必改前置** | 应移到 A2 前。路线 A 的安全合同与稳定 endpoint 先定；若路线 B 仍借 1337，必须先解决 Q6 的语义降级。没有真 producer 时不应先换队列语义。 |
| **A2** | **必改、与 A3 原子或后置** | 依赖应写成 Q2a/Q2b + A1 + A3。一次完成 admit/ack/withdraw/leaf-gone，保证 BEL 撤出时真信号已可用；trace 同时长出 episode/generation，不能只记 bool。 |
| **B1** | **通过** | 依赖 A2 正确。可用状态机夹具验 UI，但合入门应再加一个真 producer 的 waiting 数量，避免徽章只在测试里见过。 |
| **B2** | **建议解耦** | 命令面板本体尚未存在，不能让它阻塞 attention 主链。`Ctrl+Shift+A` 已可用；把面板 row 挂到命令面板里程碑，当前块只登记动词和键位。 |
| **C1** | **通过并行** | 可与 A 线并行；只提供 `IsIconic/CLOAKED` 事实与查询失败政策，不提前决定 toast。 |
| **C2** | **必改后并行** | C1 后施工；先补“focused window + inactive tab”真值格，再写 Flash/Toast。`attention_is_consumed` 保持两参，赞成另起 `desktop_reach`。 |
| **C3** | **必改最后交汇** | 依赖应为 A2 + A3 + C2；只在新 episode admission 边沿投递一次。点击路由复用现有 `NotificationRoute` 通过。 |

推荐关键路径为：`A0(修门) → A0.5`；另一支 `A1 → A3 → A2 → B1`；桌面支 `C1 → C2`；最后 `A2 + A3 + C2 → C3`。B2 独立等待命令面板。这样不会出现“假队列已拆、真生产者尚未装”和“持续 yes 每帧 toast”两个中间坏版本。

## 最终裁定

- Q3：**必改**。
- Q6：**必改**；拼写/先例通过，语义映射不通过，且没有更好的既有 OSC 可替换。
- Q7：**通过**，附“不清 raw request/ticket/ack generation”回归钉。
- Q8：**必改**；pane 粒度通过，补 KeyboardOwner 与订阅初态，不补 focus-mode/visibility 位。

完成上述必改后，本方案可进入分片施工；在此之前只建议执行修正后的 A0/A0.5 真机焦点片，不建议落 A1/A2/A3 的产品语义。
