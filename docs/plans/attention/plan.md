# 通知与注意力块方案 v1(2026-08-25 起草,送 Codex 评审;不写产品代码)

> **读法**:正文 §0–§9 是 v1;**§10 是 v2**(评审折入);**§11 是 v3 终修**(窄复核 `codex-confirm.md` 的五处 + 用户 2026-08-25 的 A 组八条裁决)。**三者冲突时,序号大的为准。** 被改动的正文处已就地改,并留「v1 原文记作 / v2 原文记作」记号。

**由头**:用户把这一块提为当前最重要的块之一,原话「目前 claude code 的通知提示还是存在问题的」,并给了一张实机截图(竖栏 Icons 态,三枚 tab 图标,**前两枚右上角带橙点,而第一枚正是活动 tab**)。

本方案做四件事:①把「还是存在问题」拆成**可指认、带代码证据**的缺陷清单;②用**真 claude 进程的真字节流**回答「Claude Code 到底发什么」这个到今天为止只有散文没有数据的问题;③给 `awaiting` 一个诚实的信号源;④把 P1-8 注意力队列与 §7.6 桌面通知这两条早就各自有一半的线接起来。

**上位法**:DESIGN §7.1.5b(会话状态分类学 + P1-8,用户裁决 2026-07-18,三轮勘误 + 2026-08-21 入队门修正)、§7.6(桌面通知)、UI-UX §8(「等你」这一态的诚实处理)、PROBLEM-LIST P1-2 / P1-4 / P1-8 / P3-4 / P3-10、HANDOFF-2026-08-21 §4 g(提示三档,用户明说要先专题讨论)与 §5 第 5 条(`awaiting` 真信号源)与 §7 第 7 条(备用屏呼吸关掉后 tab 什么都不显示)。**v2 更正(评审必改①,2026-08-25)**:本方案**请求用户复议两条既有裁决** —— 2026-08-21 的「BEL 入队」与 2026-07-18 的「Enter-only 出队」,拆成 Q2a / Q2b 分别裁(§10.3);**未裁前片 A2 不施工**。其余上位法一条不动。**v1 原文记作**「本方案不推翻其中任何一条裁决;要用户拍板的一共四处(Q1/Q2/Q4/Q5),全部集中在 §9」—— 前半句不成立,后半句的处数以 §10.13 为准。

---

## 0. 一句话结论

**今天的「等你回答」不是「等你回答」,它是「这个 pane 曾经在你不看的时候响过一声铃」。** 一个回合末尾的一次性事件,被投影成了一个本该表示「它站在那儿等你」的持久状态。**该亮不亮**、**不该亮常亮**(号退不掉)、**亮了不知道为什么**(Bell 与 Awaiting 同色、静止时不可分)三件事,是同一个错位的三个面。

**而「该亮不亮」有一个实测钉死的、Folio 侧的、便宜的成因**:Claude Code **自己打开焦点上报 `?1004h`**,并且**按「用户还在不在看」决定要不要响铃** —— 单变量对照(录音 B vs D,同一句 prompt、同样长的回合,唯一差别是发不发 `ESC [ O`)是**零铃 vs 一铃**。而 `bt-app` 从不发 `\e[I`/`\e[O`(DESIGN §7.1.5i 记的全仓零命中)。**于是你切到副屏去用别的东西时,Claude Code 以为你还坐在它面前,于是不打扰你** —— 这正是用户那句抱怨的原始场景。**这一条是本块里最便宜、最该先做的一片(片 A0.5)。**

---

## 1. 现状审计:七态今天各由什么驱动

### 1.1 它不是一个枚举,是三条各自独立的通道

| 通道 | 类型 | 位置 |
|---|---|---|
| **呼吸**(busy) | 派生谓词,不落字段 | `session_is_breathing` `crates/bt-app/src/main.rs:13781` = `status.working && !status.alternate_screen` |
| **进度环** | `Option<ProgressState>` | `crates/bt-term/src/session.rs:743-753`;`Normal(u8)`/`Error`/`Indeterminate`/`Paused` |
| **点** | `StatusClaim` 五值 | `crates/bt-app/src/main.rs:13593-13614`,`Silent < Unread < Bell < Failed < Awaiting`(`Ord` 的声明序 = 响度序,`loudest_claim` `:14155` 靠这个) |

三条通道各画各的,`TabMarkState`(`crates/bt-app/src/seats.rs:6453-6482`)是它们汇合的唯一结构;横 strip(`seats.rs:8900-9003`)与竖栏 Icons 态(`rail_chrome` `seats.rs:9625`,点几何 `:9962-9979`)**读同一份**,这条纪律没问题、别动。

### 1.2 逐态的驱动与虚实

| §7.1.5b 的态 | 今天由什么驱动 | 证据 | 虚实 |
|---|---|---|---|
| **busy**(呼吸) | OSC 133 `C` → `working=true`,`D` → `false`,**两者都只认主屏** | `bt-term/src/session.rs:3339-3346`(`C`)、`:3365-3381`(`D`) | **真**,但对 agent 场景恒哑 —— 见 D4 |
| **进度环** | OSC `9;4;<st>;<pct>` | `bt-term/src/inline_image.rs:1294-1308` → `parse_progress` `:1375-1396`;任务栏镜像 `main.rs:14421-14528` + `bt-platform/src/lib.rs:3066-3167`(`ITaskbarList3`) | **真且完整**,但**没有程序在发**(实测 §2:claude 一条都不发) |
| **未读·完成**(蓝点) | 输出增量 `output_revision` 越过 `last_seen_revision` 且 tab 非活动 | `main.rs:13745-13751`、`13679-13681` | **真** |
| **未读·失败**(红点) | OSC 133 `D;<n>`,`n≠0` | `bt-term/src/session.rs:3382-3384` | **真**,同样只认主屏 |
| **等你回答**(橙点脉动) | **只有一个入口:BEL** —— `attention_is_asked(bell_latched, tab_is_active, window_is_focused)` | `main.rs:13963-13965`;`bt-shellint` 全仓零命中,那个 crate 不存在 | **形同虚设**:它是「铃响在没人看的地方」的别名,不是「它在等你」 |
| **bell**(橙点不脉动) | 裸字节 `0x07` 在 ground 态 | `bt-term/src/adapter.rs:382` | **真**,但实测证明 Claude Code **按焦点决定响不响**,而 Folio 从不供给焦点事实(§2 / D5 / D10) |
| **dead**(图标变暗去色) | **无** | `grayscale: false` 硬编码 `main.rs:18975`,注释自陈「Prepared and unwired」;`reap_exited_tabs` 在 PTY 退出当场关 tab;`closeOnExit` 设置在 `bt-persist/src/settings.rs` 零命中 | **完全未接线**,七态实际是六态 |

### 1.3 用户那张截图里,两枚橙点当时代表什么

**橙只有两种可能**:`StatusClaim::Bell` 与 `StatusClaim::Awaiting` **共用 `palette.status_warn`**(`main.rs:13627`),唯一的差别是 `pulses()`(`main.rs:13640`,只有 `Awaiting` 为真)。静止的截图里这个差别不存在。

**但第一枚点是活动 tab 上的点,而这件事把答案定死了。** `settle_attention`(`main.rs:14067-14130`)每一帧对每个 leaf 跑:活动 tab + 窗口有焦点 ⇒ `consumed` ⇒ `leaf.session.clear_attention()`,`bell_latched` 当场退。所以**一枚平橙点在活动 tab 上活不过一帧**。能长期挂在活动 tab 上的橙点**只能是 `Awaiting`** —— 因为 `claim()`(`main.rs:13702-13720`)把 `awaiting` 放在所有抑制之上,而 `clear_attention` 碰不到号(`attention_ticket` 只由 `answer_attention` 取走,`main.rs:13983` 的注释写得很清楚:「**nothing here takes a place away**」)。

**于是截图判读**:第一枚 = 一张**没人退得掉的号**。它的真实语义是「这个 pane 在你不看的某一刻响过一声铃」,而 UI 说的是「等你回答」。**两者不是同一句话**,而这正是用户说的「亮了不知道为什么」。第二枚在非活动 tab 上,Bell/Awaiting 静态不可分,**它是哪一种,这张截图回答不了** —— 这本身就是缺陷 D2。

---

### 1.4 缺陷清单(逐条带证据)

**D1 —— 号发得出、退不掉(「看过了不熄」)。**
唯一出口 `Runtime::answer_attention`(`main.rs:60187`),唯一调用点挂在 `Key::Named(NamedKey::Enter)`(`main.rs:62164`)。四条现实路径全部退不掉:
① 你在别的程序里干活(窗口无焦点)时 pane 响铃 → 发号(这是设计要的);② 你回来、切过去、读完了、**没在那个 pane 里按 Enter** → 号还在;③ 在 Claude Code 里回答权限提问按的是 `1`/`y`/`Esc`/`↓`+Enter,**只有最后一种碰得到这扇门** —— 你回答了 agent,号照样在;④ `npm run build` 跑完响的铃,你根本不会再往那个 pane 里按 Enter。
**「回答才消费」这条裁决本身没错,错的是把 BEL 当成了「它在等你回答」。** 一个不需要回答的事件拿到了一张只有回答才能退的号。

**D2 —— 两个断言共用一种橙,唯一的区别是动效,而动效有三种情形不在。**
`dot_color` 把 `Bell | Awaiting` 映到同一个 `status_warn`(`main.rs:13627`);区别只有 `pulse`(`main.rs:18981`)。而 `pulse` 的值是 `wait_halo_opacity(elapsed, motion)`,**reduced-motion 下它返回 `0.0`**(`main.rs:14318-14328`)。所以在**关了动画的 Windows 上,Bell 与 Awaiting 逐像素相同**;截图/录屏上也相同。§7.1.5b 的「一点一断言」在实机上退化成「一点两断言」。

**D3 —— 「等你回答」没有生产者。**
DESIGN 指名的 `bt-shellint` 不存在(HANDOFF §2 早已记「那四个 crate 在 `Cargo.toml` 里根本不存在」)。入队门 `attention_is_asked` 的第一个参数就是 `bell_latched`。**这不是隐藏的 bug,是记了两处的挂账**(HANDOFF `:373-374`、`:391`);本方案 §3 是它的还款。

**D4 —— 长驻 agent 的 tab 什么都不显示,而且原因有两个分支。**
Claude Code 全程住在备用屏(实测:`?1049h` 在会话第 86 字节,`?1049l` 在第 191101 字节,中间一次没退)。
- **装了 shell 整合**(用户的机器就是):shell 在启动 claude *之前*于主屏发 `133;C` ⇒ `working = true` 且**一直不落**(`D` 只认主屏,而 claude 活着的时候主屏没有 `D`)。于是:`session_is_breathing = working && !alternate_screen` = **false**(不呼吸);而 `claim()` 里 `work_in_flight = working || progress` = **true** ⇒ `unread` 被抑制 ⇒ **也没有蓝点**。加上不发 9;4(无环)、以及 D5/D10 那条闸门让它多数时候不响铃(无橙点)—— **这个 tab 上一个像素都没有。** 两处抑制读的是两个不同的事实,合起来把 tab 涂黑,而它们各自都是对的。
- **没装 shell 整合**:`working` 永远为 false ⇒ 只要 claude 在画面上动,`output_revision` 就涨(`main.rs:13745`,alt 屏不豁免)⇒ 后台 tab 挂一枚**蓝点**,而蓝点的断言是「**完成**·未读」。**一个正在跑的 agent 被投影成「跑完了」。**
两个分支给出两个不同的错答案,而用户看到哪一个取决于他装没装 `folio.ps1`。

**D5 —— 该亮不亮:铃的闸门是「用户还在不在看」,而那个事实 Folio 从不供给。**
见 §2 与 `evidence-2026-08-25.md`。单变量对照:录音 B(短回合、不发焦点事件)**零 BEL**,录音 D(同一句 prompt、同样短的回合,只多发一次 `ESC [ O`)**一枚 BEL**,签名与录音 C 那枚逐字节相同。`preferredNotifChannel: terminal_bell` 三段录音全部继承了用户真实的那份 settings。
**结论**:Claude Code 自己开 `?1004h` 并按焦点决定要不要响;Folio 从不发焦点事件 ⇒ 它永远以为你坐在它面前 ⇒ 你切走的那一刻它不响。副屏用 Claude Code 正是这个块的原始动机,而这就是那个场景的成因。
(第二条独立触发条件:录音 C 不发焦点事件、回合约 60s,也响了一枚。本方案不去猜它的阈值 —— 能修的是焦点那一条。)

**D6 —— 桌面通知与注意力账本没接线,而且只有两档。**
toast 全链路是通的(`crates/bt-app/src/notify.rs` 全文、`bt-platform::Notifier` `lib.rs:3295-3411`、点击回原 pane `main.rs:open_from_notification`、设置 `terminal_notifications` 默认 On `bt-persist/src/settings.rs:645`),**但它只由 `OSC 9` / `OSC 777;notify` 触发**(唯一投递点 `main.rs:52596`)。注意力队列的一张号永远变不成桌面上的一条消息。
而 `reaches_the_desktop(enabled, tab_is_active, window_is_focused)`(`notify.rs:107-109`)只有两档:窗口没焦点就发。**副屏上完全可见的窗口与最小化的窗口今天同档**,这与 HANDOFF §4 g 用户要的三档正相反 —— 而副屏用 Claude Code 正是这一块的原始动机。

**D7 —— 徽章不存在,于是队列没有可发现的入口。**
`N waiting` 药丸只活在 `design/ui-mockup.html:4548`;Rust 侧零命中;`waiting_badge` 设置键零命中。HANDOFF `:400` 自己记着这条。后果:`Ctrl+Shift+A`(`crates/bt-app/src/shortcuts.rs:744-749`)是取用队列的唯一动词,而它没有任何看得见的入口 —— UI-UX §1.0「现代 app 该有的样子」的反面。

**D8 —— dead 未接线,`closeOnExit` 不存在。** 见 1.2 表末行。

**D9 —— 平铃太容易灭,号太难灭。**
`TabState::mark_seen`(`main.rs:18822-18843`)在切到某个 tab 的那一刻,把**该 tab 里每一个 leaf** 的 `bell_latched` / `failure_exit_code` 一起清掉。三座位 tab 里 B pane 的铃,在你为了看 A 而切过来的那一刻就没了。这条**有意为之且写了理由**(tab 的断言是整个 fleet 的),但与 D1 并排看就是同一枚点的两种含义各走极端。

**D10 —— 焦点上报从不发出,而它正是 D5 的成因(不是一笔将来的账,是今天在咬人的那一条)。**
Claude Code 自己开了 `?1004h`(录音 B 第 62 字节,在 ConPTY 第 7 字节那次之后的第二次);而 `bt-app` 从不发 `\e[I`/`\e[O` —— DESIGN §7.1.5i 记的全仓 grep 零命中,那一节还专门为这一天留了话:「`TerminalModes::focus_reporting` 这一位保留,它是真的模式,焦点上报做出来那天要读的就是它」,并且把 `1004` **撤出**了提示符归零名单,正是为了不挡这条路。
**焦点的粒度是 pane 不是窗口**:一个三座位 tab 里只有一个 pane 握着键盘,另外两个 claude 就应该被告知「你没有焦点」—— 于是它们该响的铃会响,而这正是队列存在的那一刻。这条比「窗口有没有焦点」更值钱,也更符合协议本意。

---

## 2. 实测:Claude Code 到底发什么

### 2.1 台子

不用起 `folio.exe`,因为要量的是**子进程发出的字节**,不是这台终端的解释。用仓里现成的 `bt-record`(`crates/bt-corpus/src/bin/bt-record.rs`)—— 真 `portable_pty` + 真 ConPTY(`conpty_source=sidecar 1.25.260710002-preview`,与产品同一份 `conpty.dll`),原始字节同时进 `.btcr` 与 stdout。`--input-plan` 按毫秒注入按键,所以整场是可重放的。

- **隔离**:`APPDATA` 指向 scratchpad 下的空目录;`CLAUDECODE` / `CLAUDE_CODE_*` / `AI_AGENT` 全部 unset(不 unset 会继承 `CLAUDE_CODE_CHILD_SESSION`,子进程自己会报「Transcript saving is off」并且**吃掉早期按键** —— 录音 A 就是这么废的,留档在 §2.2)。`~/.claude/settings.json` 用真的那份(要的就是 `preferredNotifChannel: terminal_bell` 真的生效)。
- **不 kill 非自起进程**:全程只结束自己 spawn 的那一个 `claude.exe`,靠 input-plan 里的两次 `0x03`。
- **证据**:原始字节 + 按键计划 + 判读脚本在
  `C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\ccea9546-63d0-4a20-ba77-75caa4e8533c\scratchpad\attn\`
  (`claude-{a,b,c,d}.raw` / `.btcr`、`plan-{a,b,c,d}.txt`、`seq.py` / `scan.py` / `text.py`)。
  **scratchpad 会随会话消失**,所以逐字节的判读结果与复现命令落进了 **`docs/plans/attention/evidence-2026-08-25.md`** —— 那份文件是本节的凭据,`.raw` 不进仓(它带着真实会话 URL 与组织名,而每一条断言都能由那条命令重跑出来)。

### 2.2 四段录音

| 录音 | 场景 | 结果 |
|---|---|---|
| **A** | 继承了 `CLAUDE_CODE_CHILD_SESSION` 的启动;14s 注入按键 | **按键被吃**(TUI 尚未接管),回合没跑成。**留档的价值**:证明「起真 agent 做实测」这件事本身有一道坑 —— 注入必须等 TUI 站稳(≥30s),且必须清 `CLAUDE_CODE_*` |
| **B** | 干净启动;35s 注入 `reply with only the word ok`;回合约 10s;之后静置约 45s | 回合**跑完了**(`● ok` 在转录里);**BEL 计数 = 0**;OSC 9;4 = 0;OSC 133 = 0;OSC 777 = 0 |
| **C** | 干净启动;35s 注入 `use the Bash tool to run: sleep 40`;回合约 60s;之后静置约 145s | **裸 BEL 恰好一枚**,在偏移 `963283`,上下文是 `…\x1b[?25h\x07\x1b(B\x0f\x1b[?1000h…` —— **显示光标、响铃、重新开鼠标追踪**,即**回合结束、提示框交回给你的那一刻**。静置的 145s 里**再没有第二枚**(60s 空闲提醒没有变成第二声铃) |
| **D** | **同 B**,但在注入 prompt 前先发 `ESC [ O`(焦点离开),整场不发 `ESC [ I` | **裸 BEL 一枚**,偏移 `162272`,签名与 C 那枚**逐字节相同**。单变量对照 ⇒ **焦点是闸门** |

### 2.3 逐条结论

1. **Claude Code 不发 OSC 133,不发 OSC 9;4,不发 OSC 777。** 三段有效录音全部零命中。所以进度环通道对它**永远是空的**,`133 C/D` 对它也是 —— 它只发一次 `133`?不,**一次都不发**。今天 tab 上关于一个 claude pane 的一切,来源只有:它住在备用屏这件事、它写了多少字节、以及那一声铃。
2. **BEL 是回合末尾的事件,而它的闸门是焦点。** B 与 D 同一句 prompt、同样短的回合,唯一差别是 D 先发了一次 `ESC [ O` —— **零铃 vs 一铃**。两枚裸 BEL(C 与 D)的上下文签名逐字节相同:`…\x1b[?25h\x07\x1b(B\x0f\x1b[?1000h…`,即**显示光标、响铃、重开鼠标追踪** = 输入框交回给你的那一刻。C 在不发焦点事件时也响了一枚,所以时长是第二条独立触发条件,本方案不猜它的阈值。
**这一条同时给出两件事**:①「该亮不亮」的成因(D5/D10);②**BEL 不是「我在等你」的证据** —— 它在回合**结束**时响,而 agent 结束之后是不是「站在那儿等你」取决于你,不取决于它。
3. **标题(OSC 0)是一个转轮,而且转轮本身零语义。** 录音 C 的 49 次改题里,`✳`/`◐`/`◑` 三个字形循环出现(`✳` 在忙碌中段出现两次),**没有哪个字形专属于空闲**;空闲时发生的是**改题停了**,而改题停 = 帧停,帧停既可能是「等你」也可能是「它在想事、这一秒没画」。**DESIGN 早就写了「标题转轮停止不推荐当 awaiting」,这段录音是它的证据。** 标题的另一半有用:`OSC 0;<spinner> <任务名>`(`◐ Sleep 40`)—— **任务名是真的**,归 P1-12 会话命名,不归本块。
4. **Claude Code 自己开 `?2004h` 与 `?1004h`**(录音 B 偏移 54 / 62),也就是它**要**括号粘贴与**焦点上报**。焦点上报这条今天在 Folio 里是死的(D10)。
5. **它全程住在备用屏。** 这一条把 D4 的两个分支钉死。

---

## 3. `awaiting` 的真信号源(片 A)

### 3.0 先把已经在咬人的那一条修掉:发焦点上报(片 A0.5)

**这一片不属于「造一个新信号源」,它属于「把一个我们本来就欠程序的事实还给它」** —— 但它是本块里最便宜、最可能一条修好用户那句抱怨的一片,所以排在最前面。

- **做什么**:`TerminalModes::focus_reporting` 为真的 pane,在**它失去/得到键盘**时收到 `\e[O` / `\e[I`。
- **粒度是 pane 不是窗口**,理由在 D10:一个三座位 tab 里只有一个 pane 握着键盘,另外两个 agent 应当知道自己没有被看着。判据 = `窗口有焦点 && 是活动 tab && 是 focused_leaf && keyboard_owner_is_a_shell()`,**四位**,今天全都现成。
  **v1 原文记作**「判据 = `窗口有焦点 && 这个 leaf 是 focused_leaf && 它的 tab 是活动 tab`;这三个位今天全都现成」—— **必改**(评审 Q8):`focused_leaf` 只说「键盘回到 shell 时落到哪一席」,不证明键盘此刻在 terminal。文件树 / preview / 搜索框 / menu 都能拿走它。详见 **§10.10**。
- **边沿而非电平**:只在答案**变**的那一帧发一次,不每帧发 —— 与 `BT_ATTENTION_TRACE`「按决定写行」同一条纪律。**并且要定义订阅初态**:后台出生的 pane 只等 `false→true` 的 UI 边沿会永远收不到初始的 `O`,所以用 `Unknown/Focused/Unfocused` 三态,在 `?1004h` 首次/重新启用时同步一次(§10.10)。
- **协议前置已经备好**:§7.1.5i 把 `1004` 撤出了提示符归零名单,理由原文就是「焦点上报一旦实现,在提示符上关掉 ConPTY 要的那个 1004 就成了活的错误」。**那一天就是这一片。**
- **要量的两件**(spike,与 A0 同批)。**v1 原文记作**:「① ConPTY 会不会把 `\e[I`/`\e[O` 吃掉翻成 `FOCUS_EVENT` —— **若被吃掉,这一片当场作废**;② 一个不认识这两串的程序收到它们会不会把它们当字打出来。」
  **①必改(评审必改 M6):判反了。** §7.1.5i 已裁 ConPTY 请求 1004 并把 `CSI I/O` 转成 `FOCUS_EVENT` —— 那是它把焦点事实**送进**子程序的方式,是通道**正常工作**的样子;录音 D 就是「真 ConPTY + 写入 `CSI O` ⇒ Claude Code 改变行为」的端到端反证。**正确的门 = 「目标程序观察到一次 focus transition,且普通程序输入不出现字面乱码」**(②原样保留为副门)。**并补三个测试点**(`I` 边、后台出生的 pane、真实 Folio 事件边沿)—— 全部写在 **§10.5**。
- **验收**:重跑录音 B 的场景,但由 Folio 真的把焦点交出去 —— 该有一枚 BEL。这是一条能在真机上一眼看完的门。

**它不能取代 §3.2**:焦点上报让 BEL **该响的时候响**,但 BEL 仍然是事件不是状态,仍然不能说「我还在等」。两件事都要做,顺序是先这个。

### 3.1 判据三条(先立标准,再挑通道)

一个够格当 `awaiting` 的信号必须同时满足:
① **是程序主动说的**(DESIGN「显式 side-channel 优先」、UI-UX §8「这不是猜,是它说的」);
② **是状态不是事件** —— 有开、有关,不是一次性的响一下。「等你」这句话必须能被说完也能被**收回**;
③ **退出有三个主**:程序自己撤、用户回答、进程消失。缺任何一个都会长出 D1。

**BEL 只满足 ①。** 这是它今天的全部问题,也是本方案对它的处置(§4 B2)。

### 3.2 首选通道:`OSC 1337;RequestAttention=yes|no`

**不发明序列。** iTerm2 定义的 `OSC 1337;RequestAttention=<yes|no|once|fireworks>`(**这条先例的准确措辞与实装面请 Codex 复核,见 Q6**)语义**恰好**是判据②③:`yes` 进入一个持续的求关注态、`no` 由程序自己撤回、`once` 是一次性的(正好映到既有的 `Bell`)。仓里已经有 `Osc1337Scanner`(`crates/bt-term/src/adapter.rs:312/551`,今天为 iTerm2 内联图片而存在),**这是一次扩表而不是一条新解析路径**。

落地形状(片 A1):
- **v2 更正(评审必改③)**:`SessionStatus` 长的**不是一位 bool**,是一个 **episode 号** `attention_request: Option<u64>` —— `=yes` 在 `None → Some` 的上升沿铸一个新号、`=no` 清回 `None`;bt-app 侧另有 `attention_acked: Option<u64>`。状态机、四个状态与全部边画在 **§10.2**。
  **v1 原文记作**「`SessionStatus` 长一位 `attention_requested: bool`,由 `=yes` 置、`=no` 清」—— 一位 bool 表达不了「用户已答但程序尚未撤」,下一帧 `settle_attention` 会立刻重发一张新号(评审必改③)。
- **与 `bell_latched` 并排,互不覆盖**。`=once` 走既有 BEL 那条路(置 `bell_latched`),一个字不新写。
- 与备用屏的关系:**照 §7.1.5b 已有的不对称** —— 环与点是全屏程序**故意**要够到的通道,一律照收;呼吸的豁免不动。
- 与 `TerminalNotification`(OSC 9 / 777)的关系:**两件事,别合**。通知是一条**消息**,`bt-term/src/session.rs:712-718` 早就写明它「carries no claim」;`RequestAttention` 是一个**状态**。

### 3.3 谁来发:Claude Code 今天一条都不发,所以要给它一条能发的路

**路线 A(推荐)—— `FOLIO_PANE` + 每 pane 一条命名管道 + `folio attention` 动词。**
这是 P3-4「可编程 API」的最小切片,DESIGN 已经给它留好了位置与三条硬规矩。Folio 给每个 pane 注入 `FOLIO_PANE=<window>.<tab>.<seat>` 与 `FOLIO_SOCK=<pipe 名>`;`folio attention wait|clear` 往管道写一行。Claude Code 侧**不用改一行代码**,靠它自己的 hooks 接上 —— **映射见 §10.4.1 的三张名单**(事件级 `PermissionRequest` / `Elicitation` 优先,`Notification` 按 type 兜底,hook 一律 `async: true`)。
  **v1 原文记作**「`Notification` → `wait`、`Stop` → `wait`、`UserPromptSubmit` → `clear`」—— **必改**(评审必改 M5):`Stop` 的官方定义是「main agent finished responding」,把它映成 `wait` 就是把本方案刚用四段录音证伪的那件事升回 `Awaiting`;`Notification` 也不能整个映,它含 `auth_success` / `agent_completed` 等不等输入的 type。
- 好处:与 tty 无关(hook 的 stdout 被 Claude Code 收走,这条路绕开了这个问题);将来 P3-11 mailbox 与 P3-4 自动化**可以共用管道库与生命周期代码**。
  **v1 原文记作**「地址是我们自己发的,不会被别的程序冒充」与「将来 P3-11 mailbox 与 P3-4 自动化共用同一条地基」—— **两句都必改**(评审 Q3):管道名是**定位信息不是身份**,Windows 默认命名管道 DACL 连 Everyone/anonymous 都给读权限;而强能力**绝不能与本片共用同一份授权**。安全合同的六条写死在 **§10.6**。
- 代价:要建那条地基(命名管道生命周期、**显式 DACL**、pane 消失后的清理),并且要按 §10.6 的合同施工(Q3 已由 Codex 答完,不再是开放问题)。

**路线 B —— hook 直写 `CONOUT$`。作废。**
**v1 原文记作**:「hook 是 Claude Code 起的子进程,与 agent 共用同一个 ConPTY;Windows 上打开 `CONOUT$` 写字节即写进这条 pty。零新地基。必须先 spike(片 A0)……红门不过 ⇒ 路线 A 成为唯一。」
**v2 作废它,理由是官方已经提供了替代品**(§10.4.2):hooks 的 JSON 输出有一个 `terminalSequence` 字段,「Claude Code emits it for you through its own terminal write path … race-free … and works on Windows where there is no `/dev/tty`」。**片 A0 的问题①②因此不必再量。**
**但那条官方通道发不出 `RequestAttention`** —— 白名单逐字点名拒绝 OSC 1337。所以:

**路线 A 不是「推荐」,它是 Claude Code 场景下的唯一可行路。**(详见 §10.4.2;它同时是「A3 必须前置于 A2」的硬理由。)

**路线 C(记账,不排期)** —— 上游诉求。**v2 把它改小改准**(§10.4.2):不是「把 `preferredNotifChannel` 扩一档 `terminal_attention`」,而是**「把 OSC 1337 加进 hooks 的 `terminalSequence` 白名单」** —— 那张白名单里已经有五条 OSC,加一条是一行改动、不引入新概念,而且它让**每个终端**都能用自己的通道接住 Claude Code。两条都记账,都不占本块工期。

### 3.4 启发式兜底与它的降级论证

**明确不做的两件**:
- **读屏找「在问 y/n」** —— UI-UX 给它留的位置是「信号优先级的最后一档、明确标注最低可信度」,而那条例外的前提是「不存在协议真相」。**本块的目标正是造出协议真相**,在造出来之前先上一个猜的档位,等于给自己留一个永远不会被拆掉的补丁。
- **读标题转轮停没停** —— §2.3 第 3 条已经把它证伪:转轮停 = 帧停,而帧停两义。

**唯一可讨论的一档**:`输出安静 N 秒 + 备用屏 + 该 pane 收过用户输入` ⇒ **不画「等你回答」,画一个不同的、更弱的记号**。本方案**建议连这一档也不做**,理由是 P1-2 已裁的那句话:「『等你』一般情况下无法知道,否则如实显示为『运行中』」;而「运行中」这句话在备用屏上已被 §7.1.5b 作废。于是长驻 TUI 的诚实答案不是一个更弱的「等你」,是**一个说别的话的记号** —— 见 3.5。

### 3.5 第八记号提案(填 D4 那个洞;**需用户裁**)

**`Attached`——「这个 pane 里住着一个全屏程序」。** 不呼吸、不带色、不进 `StatusClaim` 的严重度序(**不抢点位**),形状建议是 mark 底边一条 1px 的次级墨色发丝线,或 mark 本身一档很轻的降透明。它只说一件这台终端**确实知道**的事:`alternate_screen == true`。
- 它填的是 D4:一个 tab 从「一个像素都没有」变成「有人在里面,但它的命令边界我读不到」。
- **代价**:第八个视觉断言,而 §7.1.5b 的整张表是用户逐条裁过的。
- **替代**:什么都不画(今天),把这个洞完全留给 §3.2 的真信号源填 —— 那条路走通之后,一个装了 hook 的 claude pane 会在该说话的时候说话,不装的仍然沉默。
- **推荐**:**做**,因为「沉默」与「这里没有东西」在一条竖栏上长得一模一样,而前者是假的。→ §7 Q1。

---

## 4. 注意力队列 P1-8 收尾(片 B)

### B1 徽章(D7)
标题栏齿轮左侧一枚安静小徽章:5px 橙点 + `N waiting`,21px 药丸,次级墨色 hover 升满,**零等待 = 零占位卸载**(小样 `design/ui-mockup.html:4548` 就是定妆稿)。设置 `waiting_badge` 默认 On,**只关 UI 不关能力**(用户勘误原文),关掉后 `Ctrl+Shift+A` 照常。i18n 两条串。
实现教训照抄小样那条:显隐必须验计算样式而不是属性 —— 在原生这边的对应物是**别用「有没有构造那个矩形」当断言,用「它有没有进 chrome 的命中表」**。

### B2 出队门与入队门(**本方案最重要的一条,需用户裁**)

**裁决请求:把 BEL 从入队门里撤出去。**
- 撤之后:BEL 只剩 `StatusClaim::Bell` 那一态(平橙点、看一眼就灭 —— 今天已经是这样,`clear_attention` 那条路一个字不改)。**D1 的四条路径里有三条当场消失**,截图上那枚活动 tab 的橙点也当场消失。
- **这与 `44f9f2d` 是同一种修法**:那次的教训是「病在入队不在出队」,而这次要说的是更前面一句 —— **这个号本来就不该发**。给「回答才消费」这条门配的钥匙是「它在等你回答」这个状态,不是「它响了一下」这个事件。
- 撤之后队列一度是空的,直到 §3 的真信号源落地。**这不是退化,是把一个假的填充物拿掉**;时序上 A2 必须排在 A1/A3 之后或与之同批合入。

**新的两扇门(号由 `attention_requested` 发时):**
| 门 | 谁开 | 理由 |
|---|---|---|
| 程序撤 | `RequestAttention=no` | 它说完了。这是判据②的另一半 |
| 用户答 | **那个 seat 里的一次「带来源的用户动作」**(不再只是 Enter) | 「看一眼不解除阻塞」原样成立 —— 看不产生字节。而「我往它嘴里塞了字节」是「我回答了它」唯一诚实的反面,`1`/`y`/`Esc` 都算。**v1 原文记作**「任意用户输入字节」—— 那句话会把终端自答(焦点上报、颜色查询回复、PSReadLine 修复)一并算成回答,**必改**;逐路的算/不算与施工点见 **§10.3**(施工点**不许**下沉到 `write_pty_input`) |
| 进程走 | leaf 消失 | 已经是了(号住在 `LeafSession` 里),但要一条钉盯着 |

**出队门加宽这件事,只对真信号发的号成立。** 如果用户否决 B2 的撤 BEL(§7 Q2 的 B 选项),那么「任意字节即回答」不能套在 BEL 上 —— 一声铃后你在那个 pane 里按任何键都会退号,那就等于「看一眼就消费」,与既裁语义正面冲突。**两件事必须一起裁。**

### B3 动词
`Ctrl+Shift+A` 已装(`shortcuts.rs:744-749` → `jump_to_attention` `main.rs:60223`),归 P2-7 正式键位表;命令面板(`Ctrl+Shift+P` 今天是存根)加一条 `Go to next waiting session`。**jump 不消费,一个字不改**(`main.rs` 那段注释「Arriving is looking, and looking is not answering」是裁决原文的转写)。

### B4 号的生死
`attention_next_ticket` 每窗一个、永不复用(`main.rs:6193`);跨窗 tear-out **丢号不带号**(DESIGN `:351` 已裁,理由写得很完整)。要补的是一条钉:**leaf 被关掉/被移走时,号随它消失,且 `next_ticket` 不回退**。

### B5 `BT_ATTENTION_TRACE` 加两站
`withdraw … reason=program`(收到 `=no`)、`expire … reason=leaf-gone`。仍然**按「决定」写行不按「看见」写行**,`the_attention_trace_writes_one_line_per_decision_and_none_otherwise` 那条钉两头都要跟着长。

---

## 5. 系统级通知与提示三档(片 C)

### 5.1 现状:路修好了,没车走

`notify.rs` 全文是纯函数 + 一张真值表,`bt-platform::Notifier` 是真 WinRT toast,点击回原 pane 走 `NotificationRoute`(三个 id 塞进 `launch` 串,不留 map)。**缺的只有两件**:队列不喂它(D6 上半),以及它只有两档(D6 下半)。

### 5.2 三档(HANDOFF §4 g 的要点已由用户给出,本节把它落到能施工的判据上)

| 档 | 条件 | 做什么 |
|---|---|---|
| 一 | 窗口有焦点 **且** 是活动 tab | 什么都不加(今天就是) |
| 二 | 窗口**可见但无焦点**(副屏) | 窗内信号(点/环/徽章)+ `FlashWindowEx` 任务栏闪。**不弹 toast** |
| **二′** | **窗口有焦点,但请求来自非活动 tab** | **v2 补格(评审必改⑥)**:归第二档 —— 窗内徽章 + 那枚点,**不 toast**。8 行真值表与「前台窗口上 `FlashWindowEx` 是 no-op」这条诚实说明见 **§10.7** |
| 三 | 窗口**不可见** | toast |

**v1 原文记作**上表只有三行 —— 它漏了「窗口有焦点、请求来自非活动 tab」这一格:它既不满足档一(不是活动 tab),也不是档二的「可见但无焦点」(窗口有焦点),更不是最小化/cloaked。

**「窗口可见」这个新事实源必须诚实,而 Windows 上能可靠回答的只有一半。**
- 可靠:`IsIconic`(最小化)、`DwmGetWindowAttribute(DWMWA_CLOAKED)`(切到了别的虚拟桌面 / 被 shell 隐藏)。
- **不可靠**:「被另一个窗口完全盖住」。winit 的 `WindowEvent::Occluded` 在 Windows 后端**不发**;自己算要枚举 z 序 + 求区域差,那是启发式,而且每帧都要算。
- **建议**:第三档的判据 = `IsIconic() || DWMWA_CLOAKED`,并把「被完全遮挡」**明说为不做**、归第二档。这样三档全部站在能回答的事实上,代价是「窗口被 IDE 全屏盖住时只闪任务栏不弹 toast」——而任务栏闪在那种情形下是看得见的。→ §7 Q4 留裁决位(用户可能要「被盖住也算不可见」)。

### 5.3 `reaches_the_desktop` 要解耦(HANDOFF 已记这一笔)

今天它是 `enabled && !attention_is_consumed(...)`,而 `attention_is_consumed` 只有两个位。改成一个**新函数** `desktop_reach(enabled, tab_is_active, window_is_focused, window_is_hidden) -> Reach{Nothing, Flash, Toast}`,三档各一条臂。
**红线**:`attention_is_consumed` **一个参数都不许加**。它管的是账本(谁看过了),`desktop_reach` 管的是桌面(够不够得到你),`44f9f2d` 的教训就是这两句话缠在一起会把因果说反。今天 `notify.rs:93-101` 那段「读的是同一个谓词的另一面」的论证在两档模型下是对的,**三档之后它不再对**:第二档「可见但无焦点」既不是「看过了」也不是「够得到桌面」,它是第三个答案。这段注释要跟着改,别留着。

### 5.4 队列喂 toast

**只有真 `awaiting` 信号发的号配 toast。** BEL 不配 —— 它是事件,而事件已经有 `OSC 9` / `OSC 777` 那条正路。toast 文案 = `toast_title` 的三层链(carried → pane 名 → profile 名)+ 一句「正在等你回答」;点击走既有 `NotificationRoute`,**一行不改**。
**v3 澄清(A7/A8)**:上面这句话管的是**「号」那条道** —— 队列里的一张号配不配 toast,判据是 `grounds == AwaitingInput`(§10.8.2)、门在 §11.2。**事件那条道另有一条腿**:回合结束、`OSC 9`/`777` 的消息按 A8 走同一个 `desktop_reach` 三档(§11.7),**不进队列、不发号**。两条道共用 `Notifier` 与 `NotificationRoute`,都不排队不补发(红线 5),trace 上靠 `why=<awaiting|turn-end>` 分辨。
**不排队、不补发**(`raise_notifications` 那段注释已经写死了理由,原样成立)。

### 5.5 「Claude Code bell」设置行:实测改了它的诊断,没改它的存留

HANDOFF §4 g 写的是写 `~/.claude/settings.json` 的 `preferredNotifChannel=terminal_bell`。**§2 把这一行的诊断改了**:它本身是对的、也确实生效了,**买不到东西的原因在我们这边**(不发焦点上报),而那一半由片 A0.5 免费还回来。
所以这一行**留着不动**,并**加**一条:装 hooks(§3.3 路线 A/B 的那三条)。两者不冲突 —— 铃管「你走开了」,hook 管「我在等你」。
纪律**原样保留**:On 之前存原值、Off 恢复不删、状态从文件读、默认 Off、**问那个程序自己而不是拼路径**(§7.1.6j 的教训,`1070414` 那次现场逼出来的)。→ §7 Q5。

---

## 6. 分片(每片一条线可施工)

> **v2 已整表重排,以 §10.11 为准。** 下表是 v1 原文,留作对照。三处必改:**A3 移到 A2 之前**(否则 BEL 撤出后队列永久为空);**D 拆出本方案**(Q1 只裁第八记号);**A0 三问缩成一问**(①② 由官方 `terminalSequence` 文档回答,§10.4.2)。

| 片 | 内容 | 前置 | 门 |
|---|---|---|---|
| **A0** | spike 三问:①`CONOUT$` 从 hook 子进程写得进 pty 吗?②`async` hook 的时序抖动多大?③ ConPTY 会不会吃掉我们发的 `\e[I`/`\e[O`? | 无 | 证据文件 + 三句可引用的结论。①不过 ⇒ 路线 A 成唯一;③不过 ⇒ A0.5 作废 |
| **A0.5** | **发焦点上报**(§3.0):per-pane、边沿触发、只发给开了 `?1004h` 的 pane | A0 ③ | 真机门:切走 → Claude Code 该响的铃响了。**本块最便宜的一片,排最前** |
| **A1** | `OSC 1337;RequestAttention=yes/no/once` 进 `Osc1337Scanner`;`SessionStatus.attention_requested`;三个状态转移的钉。**不碰 UI** | 无 | 红形:今天的解析器对这串一言不发 |
| **A2** | 入队门改判(BEL 撤出 / 真信号进)、出队门按信号种类分、trace 加两站 | **用户裁决 Q2** + A1 | `BT_ATTENTION_TRACE` 端到端;截图那个场景变红再变绿 |
| **A3** | 投递:路线 A(`FOLIO_PANE`/管道/`folio attention` 动词)或路线 B(hook 脚本);安装提示照 §7.1.6j | A0/A1 | 真 claude + 真 Folio 的人工门 |
| **B1** | `N waiting` 徽章 + 设置 + i18n + 零等待卸载 | A2 | 命中表断言,不是矩形断言 |
| **B2** | 命令面板动词 + `Ctrl+Shift+A` 归 P2-7 | 命令面板本体 | — |
| **C1** | 「窗口可见」事实源(`IsIconic` + `DWMWA_CLOAKED`)+ 钉 | 无 | 可与 A 线并行 |
| **C2** | `desktop_reach` 三档 + `FlashWindowEx`;`notify.rs` 那段注释重写 | C1 | 真值表测试从 4 行长到 8 行 |
| **C3** | 队列 → toast | A2 + C2 | — |
| **D**(可选,需裁) | 第八记号 `Attached` / dead 接线 / `closeOnExit` | Q1 | — |

**并行性**:A 线(信号)与 C 线(桌面)之间只有 C3 一个交点,可以两条线同时开。B 线全部挂在 A2 后面。

---

## 7. 红线

1. **不许读屏猜「在问 y/n」。** `CONVENTIONS.md` §1;UI-UX 给启发式留的那一档,前提是「不存在协议真相」,而本块正在造协议真相。
2. **不许把标题转轮当信号。** §2.3 第 3 条是它的证据,不是意见。
3. **一点一断言不许继续退化。** Bell 与 Awaiting 若仍同色,必须有**静态可分**的差别(reduced-motion 下也在);做不到就合并成一态。
4. **`attention_is_consumed` 不许长第三个参数。** 桌面那一档另起函数(§5.3)。
5. **toast 不排队、不补发。**
6. **号不跨窗、序号不回退、不重发。**
7. **「装没装 X」问那个程序自己。**(§7.1.6j;`1070414` 的现场教训。)
8. **呼吸的备用屏豁免不许翻案来填 D4 的洞。** 那条豁免的理由(恒真的指示灯不是指示灯)与 D4 是两件事;D4 的正解是 §3,不是把恒真的灯点回来。
9. **凭据分三级,三级不许互相冒充**(**v3 就地改,A7,2026-08-25 用户裁**):**事件级 `Announced`**(裸 BEL / `RequestAttention=once` / `OSC 9;<text>` / `OSC 777;notify` / `OSC 99` / 回合结束)、**弱 `Requested`**(`RequestAttention=yes|no`)、**强 `AwaitingInput`**(管道 `wait`/`clear`)。**只有后两级铸 episode、发号**;事件级一条都不发号,也不能升 `grounds`。三级表与三条规矩在 **§11.6**。
   **v2 原文记作**「`RequestAttention` 的 `yes/no` 这一臂是状态、`once` 是事件;`OSC 9`/`777` 是消息,**三者不合并**(`bt-term/src/session.rs:712-718` 已立)」—— A7 把「不合并」换成「**在同一张凭据表里各占一级**」:它们仍然不许互相冒充(那条纪律一个字不动),但它们现在**都在**体系里,而不是有一条留在体系外没有名分。
   **v1 原文记作**「`RequestAttention` 是状态」—— 那句话把 `once` 也裹了进去,而 `once` 按定义是一次性的,它走既有 `bell_latched` 那条路(评审 Q6,折入 §10.8)。
10. **焦点上报不是用户输入。** 片 A0.5 写出去的 `\e[I`/`\e[O` 必须走终端自答通道(`take_pty_writes` 那条路,`main.rs:22560`/`22582`),**绝不许经过 `send_user_input` / `send_mouse_input_to`**;否则 A0.5 发的 `CSI O` 会被 A2 的出队门当成「用户回答了」而退号(评审 Q8,折入 §10.10)。
11. **`attention_ticket` 的「nothing here takes a place away」有且只有两个具名例外**,都在 A2 里长出来:程序自撤与 leaf 消失。**用户回答那扇门仍在函数外**(`answer_attention`)。加第三个例外前必须回来改这条红线(§10.2)。
    **v3 就地改**:「程序自撤(`=no`)」泛化为「**最后一个未答凭据被它自己的生产者撤回**」—— 弱层 `=no`、强层 `clear`、wait 的 TTL 到期、`SessionEnd`,四条都走这**同一个**例外,不是四个新例外(§11.1 的不变量 I3:`ticket.is_some() ⇒ asking`)。
12. **toast 门只有一处**(§11.2 那三行,求值在每次状态转移之后)。**C3 不得再立第二处判断**,C2 也不得在 `desktop_reach` 里替它决定投不投 —— `desktop_reach` 只回答「够到哪一档」,不回答「该不该投」。
13. **强层的 key 只是关联键,永远不是地址。** 它只能在**本端点内**指认一条自己发出的 wait;不能选 pane、不能跨 pane、不进 UI 文案、不进 trace(§10.6 第 3 条的延长,§11.4)。
14. **事件级凭据不铸 episode、不发号**(红线 9 的执行面)。回合结束、BEL、`once`、`OSC 9`/`777`/`99` 一条都不许进注意力队列 —— 它们够到桌面的那条路是 A8 的两条臂(§11.7),与队列那条道并排、不交叉。

---

## 8. 验收门

> **v2 加了三步(⑧⑨⑩),见 §10.14。** 下面八步原样有效。

**每片自己的红门写在片单里**;整块有一条不可省的**端到端人工门**:
真 `folio.exe` + 真 `claude`,开 `BT_ATTENTION_TRACE` 与 `BT_PTY_DUMP`(后者用来核对我们真的把 `\e[O` 发出去了),走完这八步并逐行核对:
⓪ 在一个 claude pane 里问一句、**立刻切到别的程序** → `.chunks` 里该有一枚裸 BEL(片 A0.5 的门,今天没有);
① 后台 tab 里 agent 开始一个长回合 → 该 tab 的记号是什么(D4 的答案);
② 回合结束(agent 说话了)→ `admit` 一行,点亮;
③ 切过去看一眼、不回答 → `look … kept=1`,**点不灭**;
④ 在那个 pane 里按 `1` 回答权限提问 → `answer`,点灭(今天做不到);
⑤ 窗口切到副屏、agent 再问 → **任务栏闪、不弹 toast**;
⑥ 窗口最小化、agent 再问 → **toast**,点它 → 回到那个 pane;
⑦ agent 自己撤(`=no`)→ `withdraw`,点灭。
**外加**:关掉 Windows 动画(reduced-motion)重跑 ②③,Bell 与 Awaiting 必须仍然分得出(红线 3)。

---

## 9. 留给 Codex 的问题

> **v2 状态(2026-08-25,以 §10.13 为准)**:
> **Q3 / Q6 / Q7 / Q8 已由 Codex 答完** —— 结论分别折入 §10.6 / §10.8 / §10.9 / §10.10,不再是开放问题。
> **Q2 作废**,拆成 **Q2a**(BEL 撤出入队门)与 **Q2b**(出队门扩到「带来源的用户动作」),见 §10.3.1;两题都**挡住片 A2**。
> **Q1 已收窄**:只裁第八记号 `Attached`,dead 接线与 `closeOnExit` 退回 DESIGN §7.1.5b。
> **新增一题**:`Notification` 的 `idle_prompt` 算不算 waiting(§10.13)。

- **Q1(需用户裁)**:第八记号 `Attached` 做不做?做 = 多一个视觉断言但备用屏 tab 不再是一片空白;不做 = D4 完全押在 §3 的信号源上,不装 hook 的人永远看不到东西。**推荐:做。**
- **Q2(需用户裁,本方案的核心)**:BEL 撤出入队门吗?撤 = 截图上那枚活动 tab 的橙点当场消失、D1 三条路径消失,代价是队列在 §3 落地前是空的。**推荐:撤,并与「任意字节即回答」这条一起裁。**
- **Q3(问 Codex)**:路线 A 的命名管道,安全模型怎么写才不违反 P3-4 的三条硬规矩(尤其「永远不能从工作区/仓库读自动化」)?一个 pane 的管道名泄漏出去,别的进程能冒充这个 pane 说「我在等你」——这是不是一个真的威胁,还是「它已经在你的管子里了」?
- **Q4(需用户裁)**:第三档的判据只认 `IsIconic || CLOAKED`,把「被别的窗口完全盖住」归第二档 —— 接受吗?
- **Q5(需用户裁)**:「Claude Code bell」那一行**留着 + 加一条装 hooks** 吗?铃管「你走开了」(A0.5 之后才真的有用),hook 管「我在等你」。**推荐:两条都写,纪律(存原值/恢复不删/问程序自己)原样。**
- **Q6(问 Codex)**:`OSC 1337;RequestAttention` 是否还有比它更合适的既有序列?本方案挑它的理由是「yes/no 是一对、程序能撤」;若有更好的先例请指出来 —— **这一条明确不接受「自己发明一个 OSC」这个答案**。
- **Q7(问 Codex)**:D9(`mark_seen` 整片清 fleet 的铃)在 BEL 撤出队列之后还成不成立?本方案倾向「成立、不动」,但那条注释的论证前提里有「tab 的断言是整个 fleet 的」,而队列的断言是**每个 seat 各自的** —— 两句话在一个 tab 上同时为真,值得一双外面的眼睛。
- **Q8(问 Codex)**:片 A0.5(发焦点上报)的粒度定在 pane,判据 = `窗口有焦点 && 是 focused_leaf && 它的 tab 活动` —— 有没有第四个位该进去(例如聚焦模式下「上台」的那一席、或独 preview pane 抢走键盘的那一刻)?另外:一个 pane 从**不可见的 tab** 变成可见但仍不握键盘,该不该发 `\e[I`?本方案倾向**不发**(协议问的是键盘不是眼睛),但这句话值得一双外面的眼睛。

---

## 10. 2026-08-25 v2 增补(评审折入)

**这一节是什么**:`codex-review.md` 的六条必改、技术矛盾表里另五条同级必改、两条建议与 Q3/Q6/Q7/Q8 四个结论,逐条闭合。正文被改动的地方**已经就地改了**,并在原处留了「v1 原文记作…」。**本节与 §0–§9 冲突时以本节为准。**

**外部事实自己重核过一遍**:评审引的 Claude Code hooks 官方文档,本节**逐字重取了原文**(`curl -sL https://code.claude.com/docs/en/hooks.md`,2026-08-25;不经任何转述),因为白名单要逐字进方案。核对结果里有**一条评审没看到、且推翻本方案一条选路的事实** —— 写在 §10.4.2,它是这一轮最重要的单条发现。

### 10.1 逐条闭合对照表

| # | 评审的话 | 级别 | v2 落点 | 结论 |
|---|---|---|---|---|
| M1 | `plan.md:7` 声称「不推翻任何既有裁决」,但 Q2 同时改了 2026-08-21 与 2026-07-18 两条 | 必改 | **正文 §0 上位法段就地改** + §10.3.1 | 改成「请求复议两条,未裁前 A2 不施工」 |
| M2 | Q2 把「BEL 撤出」与「出队门扩大」绑成一个选择,逻辑不对称 | 必改 | **§10.3.1**;正文 §9 Q2 拆成 Q2a/Q2b | 拆开,各自可独立裁(耦合只有一个方向) |
| M3 | `attention_requested: bool` 不足以实现三主退出 | 必改 | **§10.2**;正文 §3.2 就地改 | episode/generation 状态机,四态十七边 |
| M4 | 「任意用户输入字节」必须是**带来源的用户动作**;施工点不能下沉到 `write_pty_input` | 必改 | **§10.3.2**;正文 §4 B2 表就地改 | 八个 `write_pty_input` 调用点逐个定性 |
| M5 | hook 映射自相矛盾:`Stop` 是「回合结束」不是「等批准」 | 必改 | **§10.4** | 白名单只收真正等输入的事件/type;`Stop`/`idle_prompt` 出局 |
| M6 | A0 ③ 的失败判据判反,与 §7.1.5i 及录音 D 冲突 | 必改 | **§10.5** | 门改成「目标程序观察到 transition + 普通程序不出乱码」,补三个测试点 |
| M7 | 三档表漏「窗口有焦点但请求来自非活动 tab」 | 必改 | **§10.7**;正文 §5.2 表就地补 | 8 行真值表;该格 = Flash 档、窗内徽章、不 toast |
| M8 | C3 的触发必须是「新 episode 首次入队」的边沿,依赖含 A3 | 必改 | **§10.2.3 边 e4** + **§10.11** | 只在首次 admit 投一次 |
| M9 | 可选 D 把三件不同生命周期的事塞一片,且只挂 Q1 | 必改 | **§10.11** | **D 拆出本方案**;Q1 只留 `Attached` |
| Q6 | `RequestAttention` 被过度解释成「等你回答」 | 必改 | **§10.8**;正文红线 9 就地改 | 两层分开、合成规则写死;拼写与先例通过 |
| Q8 | 焦点判据漏了窗口真实的键盘 owner | 必改 | **§10.10**;正文新增红线 10 | 谓词补第四位 `keyboard_owner_is_a_shell()`;订阅初态三态 |
| Q3 | 管道安全模型:「地址是我们发的所以不会被冒充」不成立 | 必改 | **§10.6**;正文 §3.3 就地改 | 六条安全合同写死;不承诺抗同用户恶意进程 |
| Q7 | `mark_seen` 整片清铃在 BEL 撤出后是否仍成立 | 通过 | **§10.9** | 保留;附两条回归钉 |
| M10 | A1 不是简单「给 `Osc1337Scanner` 扩表」 | 建议 | **§10.11** 片 A1 行 | 承认要在共同前缀后分流,五个钉写进片单 |
| M11 | 多处 `main.rs` 行号已漂移 | 建议 | **§10.12** | 全表刷新,并把评审没点到的几处一并刷了 |
| — | 九片依赖重排,尤其 A2 不得先于 A3 | 必改 | **§10.11** | 新序 + 每片前置 |
| — | 五条红线(BEL 撤回一次性、静态可分、toast 不排队、`attention_is_consumed` 解耦、呼吸豁免不翻案) | 通过 | 正文原样 | 不动 |

---

### 10.2 【必改 M3】请求 episode 状态机 —— 替换 `attention_requested: bool`

> **v3 就地记**:本节的**四个状态**、**「只有上升沿铸号」**、**「`Acknowledged` 对 `Settle` 是不动点」**三条全部成立,一个字不撤。但它是一台**单源**机器,而 §10.8 的弱层与强层是**两个独立生产者** —— 窄复核点名的四个死角(铸号所有权、双源电平、后到的强凭据不 toast、`withdraw`/`expire` 的字段合同)都出在这里。**§11.1 用同一个骨架把它扩成双源**:字段从「一个 `Option` + 一位 ack」变成「两个 generation + 两条 ack 水位」,状态由派生式算出、不另存,十七条边扩成 §11.1.4 的那张网格。**本节与 §11.1 冲突时以 §11.1 为准**;下面的 §10.2.1–§10.2.6 留作对照,读它是为了读懂 §11.1 为什么长成那样。

**病在哪(一句话)**:一位 bool 说不出「用户已经答过了,但程序还没说 `no`」。用户答完、`answer_attention` 把号取走,下一帧 `settle_attention` 再看这一位仍是 `true`,于是**立刻重发一张新号** —— 与 2026-08-21 那次「badge 活得比它报告的东西更久」是同一类错,只是搬到了另一根柱子上。

#### 10.2.1 两个字段,分居两层

| 层 | 字段 | 谁写 | 为什么在这一层 |
|---|---|---|---|
| `bt-term`(流的事实) | `SessionStatus.attention_request: Option<u64>` | 解析器:`=yes` 在 `None → Some` 的**上升沿**铸一个新 episode 号;`=no` 清回 `None` | 它是**字节流的函数**,可离线重放、可在 `bt-term` 单元测试里钉死,不需要知道「用户」是什么 |
| `bt-app`(用户的事实) | `LeafSession.attention_acked: Option<u64>` | 一次「带来源的用户动作」(§10.3.2)把它设成当前 episode | 只有这一层知道什么是用户 |

**episode 号 per-session 单调递增、永不复用**,与 `attention_next_ticket`(per-window 队列序号,`main.rs:6279`)是**两个计数器,不许合并**:ticket 决定**队列次序**,episode 决定**这是不是同一次请求**。

**单调性是一条不变量,不是一个巧合**:`=no` 之后 `attention_acked` 里留着的是一个死 episode 的号,而下一个 episode 的号严格更大,所以 `acked != Some(episode)` 自动成立 —— **不需要在 `=no` 时回头清 ack**。少一条清理路径,就少一处能忘。

#### 10.2.2 状态机:四态

三值判据与**既有**的 `attention_ticket: Option<u64>` 的乘积,**不新增第二个 `Option`**:

```text
asserted = status.attention_request.is_some()
episode  = status.attention_request                 // Option<u64>
acked    = episode.is_some() && leaf.attention_acked == episode
asking   = asserted && !acked                       // ← 入队门的第一个参数
queued   = leaf.attention_ticket.is_some()
```

| 状态 | 判据 | 一句话 |
|---|---|---|
| **`Idle`** | `!asserted` | 程序没在请求。**leaf 出生即此。** |
| **`Requested(e)`** | `asking && !queued` | 在请求,**还没拿到号**(入队门说「你正被看着」) |
| **`Queued(e,t)`** | `asking && queued` | 在请求,持号 `t` |
| **`Acknowledged(e)`** | `asserted && acked` | **程序还在说 yes,但这一次已经被你答过了。** 不拿号、不 toast、不进队列 |

#### 10.2.3 状态机:全部边

事件五种:`Yes`(收到 `=yes`)、`No`(收到 `=no`)、`Settle{consumed}`(每帧 `settle_attention` 的那一次判断)、`Answer`(那个 seat 里的一次带来源用户动作)、`LeafGone`。

```text
┌──────────┐   ⟲ (e2) No 幂等    ⟲ (e3) Settle/Answer 无事可做
│   Idle   │◀────────── 三条 `no` 边全部回到这里:(e7)(e10)(e14) ──────────┐
└──────────┘                                                              │
   │                                                                      │
   │ (e1) Yes ── 铸 e = next_episode() ── 全机唯一铸号的边                   │
   ▼                                                                      │
┌─────────────────┐  ⟲ (e6) Yes 幂等,不铸新号                             │
│  Requested(e)   │  ⟲ (e5) Settle{consumed=true} → refuse reason=watched  │
│ 在请求,还没拿号  │                                        (e7) No ────────┤
└─────────────────┘ ─────────── (e8) Answer ────────┐                      │
   │                                                │                      │
   │ (e4) Settle{consumed=false} · admit + 发 ticket │                      │
   │      ← C3 toast 的唯一投递边沿                    ▼                      │
   ▼                                       ┌────────────────────┐          │
┌─────────────────┐                        │  Acknowledged(e)   │          │
│  Queued(e,t)    │ ── (e9) Answer ───────▶│ 程序还举着 yes,     │ (e14) No ┤
│ 持号,在队列里    │                        │ 但这一次你答过了     │          │
└─────────────────┘                        └────────────────────┘          │
   │                                                                       │
   └────────────── (e10) No · withdraw + 退号 ──────────────────────────────┘

   ⟲ (e11) Yes 幂等,ticket 不重铸(先到先服务)   ⟲ (e13) Yes 幂等 ← 本机存在的理由
   ⟲ (e12) Settle 对 Queued 是不动点             ⟲ (e15) Settle/Answer 幂等

   任意态 ──(e16) LeafGone ──▶ leaf 消失:expire reason=leaf-gone
                              号随之消失,next_ticket 不回退
   任意态 ──(e17) mark_seen ─▶ 不变(Q7 钉,§10.9)
```

逐边表(**这张表就是钉的清单**):

| # | From | 事件 | To | 副作用 / trace |
|---|---|---|---|---|
| e1 | `Idle` | `Yes` | `Requested(e)` | **唯一铸 episode 的边**(`no→yes` 上升沿) |
| e2 | `Idle` | `No` | `Idle` | 幂等,不写 trace |
| e3 | `Idle` | `Settle{*}` / `Answer` | `Idle` | 无 episode 可确认 |
| e4 | `Requested(e)` | `Settle{consumed=false}` | `Queued(e,t)` | `admit … ticket=t episode=e`;~~**C3 toast 的唯一投递边沿(M8)**~~ **v3 就地改**:admit 不再是 toast 的**唯一**边沿 —— 一个弱凭据先入队、随后强凭据后到的 episode 会永远收不到 toast(窄复核死角 3)。**唯一的 toast 门改成 §11.2 的那三行**(「持号 + `grounds==AwaitingInput` + 本 episode 未投过」),它在 admit 求值,也在 queued 中首次强升级时求值,**仍然只投一次、仍然不重排 ticket** |
| e5 | `Requested(e)` | `Settle{consumed=true}` | `Requested(e)` | `refuse … reason=watched` |
| e6 | `Requested(e)` | `Yes` | `Requested(e)` | **幂等重申**:不铸新号、不重发 toast |
| e7 | `Requested(e)` | `No` | `Idle` | ~~`withdraw … reason=program`~~ **v3 就地改**:`Requested(e)` 没有 ticket,而 `withdraw` 这一行强制带 `ticket=` —— 这一格改写不带 ticket 的 `clear`(§11.1.5)。`withdraw` 只属于 `Queued` |
| e8 | `Requested(e)` | `Answer` | `Acknowledged(e)` | 你抢在入队前答了 |
| e9 | `Queued(e,t)` | `Answer` | `Acknowledged(e)` | `answer … ticket=t`;**退号** |
| e10 | `Queued(e,t)` | `No` | `Idle` | `withdraw … reason=program`;**退号** |
| e11 | `Queued(e,t)` | `Yes` | `Queued(e,t)` | 幂等;**ticket 不重铸**(先到先服务;`attention_ticket` 的 `is_none()` 守卫原样) |
| e12 | `Queued(e,t)` | `Settle{*}` | `Queued(e,t)` | 无 |
| e13 | `Acknowledged(e)` | `Yes` | `Acknowledged(e)` | **本状态机存在的理由**:程序持续 yes 也不能再入队。**v3 就地收窄**:幂等只对**已被确认的那一个 generation** 成立 —— `=yes` 在已经 `Some` 上重申不铸新号,所以确实幂等;但**另一个源的新 generation 不是重申**,它必须能重新入队(§11.1.3)。窄复核的反例就是这一格被读宽了:「已答 → hook 报来一个真的权限请求 → 被当成同一 episode 的幂等 `Yes` 永不入队」 |
| e14 | `Acknowledged(e)` | `No` | `Idle` | **「下一次能再入队」的唯一开关**。**v3 就地改**:同 e7,这一格写不带 ticket 的 `clear`;并且它**不再是唯一开关** —— 双源之后,一个**新 generation**(哪怕来自另一个源)本身就能让 `asking` 重新为真、铸下一个 episode,不必等旧凭据被撤(§11.1.3 合成规则③,窄复核反例①的正解) |
| e15 | `Acknowledged(e)` | `Settle{*}` / `Answer` | `Acknowledged(e)` | 不拿号;幂等 |
| e16 | 任意 | `LeafGone` | — | `expire … reason=leaf-gone`;号消失,`next_ticket` **不回退**。**v3 就地改**:`expire` 这一行强制带 `ticket=`,而 `Idle` 没 episode、`Requested`/`Acknowledged` 没 ticket —— 按 §11.1.5,**只有 `Queued` 写 `expire`**,无号态写不带 ticket 的 `drop`,`Idle` 一行都不写(窄复核死角 4) |
| e17 | 任意 | `mark_seen` | **不变** | **Q7 钉**:整片清铃**不碰这台状态机**(§10.9) |

#### 10.2.4 三条不变量

1. **只有 e1 铸 episode。** 于是「用户答了但程序没撤」= `Acknowledged`,而 `Acknowledged` 对 `Settle` 是不动点 —— **bool 做不到的那件事,由这一条做成**。
2. **`no` 是唯一能把状态机送回 `Idle` 的程序动作;用户动作最多送到 `Acknowledged`。** 这就是判据③「三个主」在状态机上的样子:程序自撤(e7/e10/e14)、用户回答(e8/e9)、进程消失(e16)。
3. **`once` 不进这台状态机。** 它走 `bell_latched`,与本机并排、互不覆盖(正文 §3.2 原样)。

#### 10.2.5 它踩到的既有不变量,必须一起改

`attention_ticket`(`main.rs:14213-14220`)的 doc 现在写着 **「nothing here takes a place away」**,而 e10 要求 `settle_attention` **在程序自撤时退号**。这不是把那句话推翻,是给它两个**具名**例外,而且两个例外都不是用户那扇门:

- **例外一**:程序自撤(`=no`),`withdraw … reason=program`。
- **例外二**:leaf 消失,`expire … reason=leaf-gone`(v1 §4 B4 已经要这条钉,只是没说它住在 `settle` 里)。
- **用户回答那扇门仍在函数外**(`answer_attention`,`main.rs:61076`),一个字不改。

**A2 必须同时改那段 doc**,否则它在 A2 之后就是假话 —— 与 §10.9 对 `mark_seen` 注释的要求同型、同理由。已升为**红线 11**。

#### 10.2.6 `BT_ATTENTION_TRACE` 的字段(替换 v1 §4 B5 的「加两站」)

带 episode 之后,**每一行都要能回答「这是哪一次请求」**:

```text
admit    tab=<i> seat=<s> ticket=<t> episode=<e> grounds=<requested|awaiting> active=<0|1> focused=<0|1>
refuse   tab=<i> seat=<s> episode=<e> reason=watched active=1 focused=1
look     tab=<i> seat=<s> ticket=<t> episode=<e> kept=1
answer   tab=<i> seat=<s> ticket=<t> episode=<e> by=<keyboard|ime|paste|files-row|mouse>   ← v3 改:`by` 的取值换成 §11.3 的七值(`mouse` 拆成 `mouse-button`/`mouse-wheel`,`mouse-motion` 永不出现在这一行);整张 schema 以 §11.1.5 为准
withdraw tab=<i> seat=<s> ticket=<t> episode=<e> reason=program
expire   tab=<i> seat=<s> ticket=<t> episode=<e> reason=leaf-gone
claim    tab=<i> was=<C> now=<C>
```

`the_attention_trace_writes_one_line_per_decision_and_none_otherwise` 那条钉两头都跟着长:**e6/e11/e13 这三条幂等边一行都不许写** —— 否则一个每秒重申 `yes` 的程序会把 trace 刷满,而它什么决定都没改变。

---

### 10.3 【必改 M1/M2/M4】Q2 拆成 Q2a / Q2b,并明列什么算「用户动作」

#### 10.3.1 拆题(M2)与复议声明(M1)

评审的话原样成立:**扩大出队门要求 BEL 先撤;BEL 撤出并不要求出队门扩大。** 用户完全可以选「BEL 撤、仍只 Enter」。v1 把两件事绑成一次裁决,是把一个单向蕴含写成了双向的。

> **本方案请求用户复议两条既有裁决:**
> - **2026-08-21「BEL 入队」** —— 请求把 BEL 撤出入队门(**Q2a**);
> - **2026-07-18「Enter-only 出队」** —— 请求把出队门扩到「带来源的用户动作」(**Q2b**)。
>
> **未裁前,片 A2 不施工。** A0.5 / A1 / A3 / C1 / C2 不受这两条影响,可以先走(§10.11)。

**Q2a(需用户裁)—— BEL 撤出入队门吗?**
撤 = 截图上那枚活动 tab 的橙点当场消失、D1 四条路径去掉三条;代价是队列在 A3 落地前是空的。**推荐:撤。** 理由见正文 §4 B2,一个字不改。

**Q2b(需用户裁)—— 出队门从「Enter」扩到「带来源的用户动作」吗?**
- **扩**:`1` / `y` / `Esc` / `↓` 都能退号 —— 这是 D1 第③条路径(在 Claude Code 里回答权限提问按的根本不是 Enter)的正解。
- **不扩**:出队门仍只认 Enter。此时 D1③ 仍在,但**至少不再错**:真 `awaiting` 号的主路是程序自己 `=no` 退,Enter 是副路。
- **推荐:扩**,且**只对真信号发的号成立**(正文 §4 B2 末段那句话原样保留)。
- **两题之间唯一的耦合方向,写在这里以免被当成独立两票**:若 Q2a 被否决(BEL 仍入队),**Q2b 必须同时否决** —— 一声铃之后你在那个 pane 里按任何键都退号,就等于「看一眼就消费」,与既裁语义正面冲突。反过来不成立:Q2a 通过而 Q2b 否决,是一个自洽的组合。

#### 10.3.2 什么算「带来源的用户动作」(M4)

**必须按来源定,不能按字节定。** 仓里 `write_pty_input`(`main.rs:10503`)有**八个**调用点,逐个定性:

> **v3 就地改(闭合③)**:下表的 **#1 与 #5 两行的定性不准**,而不准的地方正好是鼠标。实测复核(2026-08-25,`grep` 全部调用点):
> - `write_pty_input` 的**八个调用点数量闭合**,这一条成立;但**#1 与 #5 都是 helper 内部那一行**,而两个 helper 各自承载**不止一种**语义。
> - `send_user_input`(`main.rs:55459`)有**两个**调用点:`63061`(键盘)与 **`60529`(被转发的鼠标按键,`UserInputKind::Mouse`)** —— 所以 #1 写成「只有键盘」不准。
> - `send_mouse_input_to`(`main.rs:55438`)有**三个**调用点:`62142`(SGR 滚轮)、`62161`(备用屏滚轮转方向键)、**`56298`(SGR mouse motion)** —— **它一个按键都不承载**,所以 #5 写成「点击/滚轮」也不准。
> - 于是 Q2b 的鼠标子格**拆成三格**,并且施工点**只能在产生语义的事件入口**,不能在这两个 helper 上统一消费(否则程序一开 motion tracking,用户把鼠标从 pane 上扫过就退号)。三格的定性见下表 #5a/#5b/#5c。

| # | 位置 | 是什么 | 算不算 `Answer` |
|---|---|---|---|
| 1 | `main.rs:55478` `write_pty_input`(在 `send_user_input` 里) | 键盘按键(调用点 `63061`)**与被转发的鼠标按键**(调用点 `60529`)。**v1/v2 原文记作**「`send_user_input(…, UserInputKind::Keyboard)` 键盘按键」 | 两者**都算**,但**分成两个 kind**:`Keyboard` 与 `MouseButton`(见 #5a) |
| 2 | `main.rs:63248` `Ime::Commit` | **IME 提交**(不是 preedit) | **算** —— 提交出来的就是你打的字 |
| 3 | `main.rs:63111` `paste_from_clipboard_into` | 剪贴板粘贴(`Ctrl+V` / `Shift+Insert` / 终端菜单粘贴) | **算** |
| 4 | `main.rs:49768` files 行插入路径 | 从文件树把一条路径插进 shell | **算** —— 一次用户手势,且落进的是具名的那一席 |
| ~~5~~ | ~~`main.rs:55456` `send_mouse_input_to`~~ | ~~**被转发的鼠标**(点击/滚轮进了追踪程序)~~ | ~~**Q2b 的一个子格,需用户裁。推荐:算。**~~ **v3 拆成 #5a/#5b/#5c**(下三行);**v1/v2 原文记作**上面这一行,它把「点击」写进了一个根本不承载点击的 helper |
| **5a** | 事件入口 `main.rs:60529` → `send_user_input(…, UserInputKind::Mouse)` | **被转发的鼠标按键**(按下 / 抬起,`MouseProtocolEvent::Press`/`Release`) | **算**(A3 已裁「鼠标算」,2026-08-25)。新 kind `MouseButton`。正方原样:`send_mouse_input_to` 的 doc 自己写着「a wheel forwarded into a program running there is as much a claim on it as a keystroke」 |
| **5b** | 事件入口 `main.rs:62142`(SGR)/ `62161`(备用屏转方向键)→ `send_mouse_input_to` | **被转发的滚轮** | **算**。新 kind `MouseWheel`。**为什么它没被终修单那句「只有按下/抬起算」排除**:按 SGR 线序,一次滚轮**就是一次 `Press`**(button 64/65,没有独立的抬起,`main.rs:62134` 写着 `MouseProtocolEvent::Press`)—— 「按下/抬起」在**线序上**本来就覆盖它,与 A3「鼠标算」不冲突。**若用户要把滚轮也撤出,只改这一格**:它与 #5c 同一个 helper,撤出后那条 helper 整条不碰 `answer_attention`,施工面反而更小 |
| **5c** | 事件入口 `main.rs:56298` → `send_mouse_input_to`(`MouseProtocolEvent::Motion`) | **被转发的 mouse motion**(程序开了 `?1002h`/`?1003h` 之后,指针每移动一格就是一串字节) | **不算(v3 终修,闭合③)。** 悬停扫过不是回答:你把鼠标从一个 pane 上划过去、甚至只是让指针停在那儿,**没有回答任何问题**。若把 motion 算进去,一个开了 motion tracking 的程序会让「注意力号」被**指针的路过**退掉,而这个 bug 只在特定程序上出现、只在鼠标经过时出现 —— 是最难被人工门抓到的一类。新 kind `MouseMotion`,它**不调 `answer_attention`** |
| 6 | `main.rs:22561` `take_pty_writes()`(读到输出之后) | **终端协议自答**(DA / DSR / 颜色查询 / **焦点上报**) | **绝不算** |
| 7 | `main.rs:22583` `take_pty_writes()`(空转时再排一次) | 同上 | **绝不算** |
| 8 | `main.rs:54149` PSReadLine resize 锚点修复 | **我们自己发的自愈输入** | **绝不算** —— 是终端为修自己发的,不是你 |

**IME preedit(`Ime::Preedit`)不算**:它一个字节都不进 pty(只更新 `window.preedit`),连门都碰不到。写出来是为了让「IME 算」不被读成「组字中途就算」。

**施工点在哪(M4 的另一半)**:

- **不许下沉到 `write_pty_input`。** 那个函数只看得见一个字节串和一句 `&'static str`,分不出上面八行 —— 与 Enter 门 doc 里那句现成的理由完全同型:「`send_user_input` is handed a byte string and cannot tell an answer from a paste that happens to end in a newline」(`main.rs:63056`)。
- **正解**:把既有的 `UserInputKind`(`main.rs:10687-10696`,今天只有 `Keyboard | Mouse`)**扩成来源枚举**,并让每条真用户路都经过它。
  **v1/v2 原文记作**「`Keyboard` / `Ime` / `Paste` / `FilesRow` / `Mouse`」—— **v3 就地改**:`Mouse` 一格拆三,枚举是
  ```rust
  enum UserInputKind { Keyboard, Ime, Paste, FilesRow, MouseButton, MouseWheel, MouseMotion }
  ```
  其中 **`MouseMotion` 是唯一一个不是 `Answer` 的成员**(#5c),而 `returns_view_to_live` 对三个鼠标成员**一律为 false**(既有语义原样:鼠标手势没有回到 live 那一半)。
  `answer_attention(seat, by: UserInputKind)` **由每条路自己在事件入口调**,`by` 直接进 trace(§11.1.5)。
- **「算不算」这件事必须写成枚举上的一个方法,不能写成调用点的记忆力**:`UserInputKind::is_answer()`(`MouseMotion` 为 false、其余为 true)。一个将来新增的输入路必须在**枚举上**回答这个问题,而不是在某个 `if` 里被忘掉 —— 与 `returns_view_to_live` 同型、同位置、同理由。
- **协议自答那三条(#6/#7/#8)根本不经过 `UserInputKind`**,所以它们不是「要记得排除的例外」,而是**结构上够不到那扇门** —— 这正是红线 10 想要的形状,也是片 A0.5 的焦点上报能安全存在的原因。
- **`answer_attention` 的取号语义不变**:仍是「一次动作确认当前 episode 并取走号」,只是入口从一个变成四(或五)个,且每个入口都带来源。

---

### 10.4 【必改 M5】hook 白名单 —— 只有真正在等输入的才映射为 waiting

**证据来源**:`https://code.claude.com/docs/en/hooks.md` 原文,2026-08-25 逐字取回(291 536 字节),**不经转述**。文档里 `## Hook events` 下共 **31** 个事件。下表的每一句「官方原文」都是从那份 markdown 里抄的。

#### 10.4.1 三张名单

> **v3 就地记(闭合④)**:三张名单的**语义与拼写窄复核已过**,一个都不撤。缺的是**配平**:下面第一张表里的每一个 `wait`,必须写明**它由什么 clear**,否则一次权限请求会把强断言永久留在 `Acknowledged` 里、吞掉下一次请求。**逐源的 clear 配对表在 §11.4**,连同两个动词之外那把**有界关联键**的合同。第三张名单的标题也就地收窄:**「绝不映射为 `wait`」不等于「不进本体系」** —— `Stop` / `StopFailure` / `agent_completed` / `quota_auto_resume_fired` / `quota_auto_resume_disabled` / `PostToolUse` 在 v3 里**都是 clear 源**,而它们一条都不会把 pane 送进 `Awaiting`。

**映射为 `wait`(它真的在等你输入) —— 事件级优先,notification 级兜底:**

| 来源 | 官方原文定义 | 延迟 | 说明 |
|---|---|---|---|
| **`PermissionRequest`**(事件) | "Runs when Claude Code is about to ask you for permission." | **0** | **首选。** 官方自己写着:"Use this event when you need a signal the moment Claude asks for permission. The Notification event's `permission_prompt` type reaches you only after the prompt has waited about six seconds." |
| **`Elicitation`**(事件) | "Runs when an MCP server requests user input mid-task." | **0** | MCP 表单/URL 请求要用户输入 |
| `Notification` matcher **`permission_prompt`** | "Claude needs you to approve a tool use and the prompt has waited about six seconds" | ~6s | **兜底**(装了 `PermissionRequest` 时它是冗余的,但幂等边 e6 让冗余无害) |
| `Notification` matcher **`elicitation_dialog`** | "An MCP server opens an elicitation form and you haven't typed for about six seconds" | ~6s | 兜底 |
| `Notification` matcher **`elicitation_url_dialog`** | "An MCP server asks you to open a browser URL and you haven't typed for about six seconds" | ~6s | 兜底 |
| `Notification` matcher **`agent_needs_input`** | "A background session starts waiting on your input. Fires only while agent view is open in a terminal" | — | 语义完全对口;**只在 agent view 打开时发**(官方注),需 ≥ v2.1.198 |
| `Notification` matcher **`quota_auto_resume_stale`** | "A claude.ai usage limit reset while your computer slept for more than about 30 minutes. **Claude Code waits for you to press `Enter` instead of continuing.**" | — | 字面就是在等一次按键;需 ≥ v2.1.234 |

**映射为 `clear`(不再等了):**

| 来源 | 官方原文定义 |
|---|---|
| **`UserPromptSubmit`**(事件) | "Runs when the user submits a prompt, before Claude processes it." —— 你已经回话了 |
| **`ElicitationResult`**(事件) | "Runs after a user responds to an MCP elicitation." |
| `Notification` matcher **`elicitation_complete`** | "An MCP elicitation form is submitted or dismissed" |
| `Notification` matcher **`elicitation_response`** | "An MCP elicitation response is sent back to the server" |
| **`SessionEnd`**(事件) | 会话没了(与 e16 `leaf-gone` 是两回事:会话结束时 pane 可能还在) |

**绝不映射为 `wait`(名单是封闭的:**其余事件与 notification type 一律不映射为 `wait`**):**

> **v3 就地改两处**:①「其余 **24** 个事件」是**算错了**。31 个 hook event 里,v2 用掉 6 个唯一 event(含 `Notification`)⇒ 应为 **25**;**v3 又把 `PostToolUse` / `Stop` / `StopFailure` 收进 clear 名单**(§11.4)⇒ 用掉 9 个,**其余 22 个事件**一律不映射。这三个数字都写出来,是为了不再出第三次算术错。②标题从「绝不映射」收窄为「**绝不映射为 `wait`**」:下表里有三个已经是 clear 源了,**不 wait ≠ 不出现**。

| 来源 | 官方原文定义 | 为什么不 |
|---|---|---|
| **`Stop`** | "Runs when the main Claude Code agent has finished responding." | **v3 就地补**:它**不映射为 `wait`** 这条不动;但它在 v3 里有两个别的身份 —— **① clear 源**(回合结束 ⇒ 该 pane 没有任何东西在等你输入,`clear all`,§11.4);**② A8 的事件级凭据**(回合结束要够到桌面:无焦闪、最小化 toast,§11.7)。两个身份都**不**把它升成状态。<br>**这正是本方案 §2 花四段录音证伪的那件事。** v1 §3.3 路线 A 写的 `Stop → wait` 是把「回合结束」升回 `Awaiting`,与本方案对 BEL 的纠正同型同错。**v1 原文记作**「`Notification` → `wait`、`Stop` → `wait`、`UserPromptSubmit` → `clear`」—— 第二条必删,第一条必须细分到 type |
| `StopFailure` | "Runs instead of Stop when the turn ends due to an API error." | 结束不是等待 |
| `SubagentStop` | 子 agent 结束 | 同上 |
| `Notification` `idle_prompt` | "Claude finished responding about 60 seconds ago and you haven't typed since" | ~~**另裁,见 §10.13。**~~ **v3:已裁(A6,2026-08-25 用户裁)—— 不算 waiting。** 它是上游版的「输出安静 N 秒」启发式,而 v1 §3.4 明确不做这一档。**§10.13 那条「次选:当作弱层 `Requested` 凭据」未被采纳** —— 裁决只说「不算」,而名单是封闭的;要把它加成弱层凭据须另裁 |
| `Notification` `auth_success` | "Authentication completes" | 不等输入 |
| `Notification` `agent_completed` | "A background session finishes or fails." | 不等输入。**v3 补**:它是 `agent_needs_input` 的 **clear 源**(§11.4)—— 不 wait,但必须出现,否则那条 wait 有进无出 |
| `Notification` `quota_auto_resume_fired` / `quota_auto_resume_disabled` | 自动继续了 / 结束等待 | 不等输入。**v3 补**:两条都是 `quota_auto_resume_stale` 的 **clear 源**(§11.4),理由同上 |
| `StopFailure` / `PostToolUse` | `StopFailure` 见上;**`PostToolUse` 的官方原文本节没有逐字抄录** | **v3 补**:两条都是 clear 源(§11.4)。`PostToolUse` 是 `PermissionRequest` **被批准并执行完**的回执。**A3 开工前必须按 §10.4 同一条纪律**(`curl` 原文、不经转述)**把 `PostToolUse` 的原文定义与它 payload 里的 tool 标识字段逐字取回**,并把 §11.4 那张表的第一行钉死 —— 它是整张 clear 表里唯一一处还靠推断的地方 |

**评审列的可疑拼写全部核对无误**:`permission_prompt`、`elicitation_dialog`、`idle_prompt`、`auth_success`、`elicitation_complete`、`elicitation_response` 六个都在原文里、无一拼错;原文另有评审没列的 `elicitation_url_dialog`、`agent_needs_input`、`agent_completed`、`quota_auto_resume_fired`、`quota_auto_resume_stale`、`quota_auto_resume_disabled` 六个,共 12 个 type。

#### 10.4.2 核对时撞见的一条事实:**`terminalSequence` 明确拒绝 OSC 1337**

这一条评审没看到,而它**推翻本方案的一条选路**。官方原文两处:

> `terminalSequence` … A terminal escape sequence for Claude Code to emit on your behalf … **Restricted to OSC `0`/`1`/`2`/`9`/`99`/`777` and BEL.** If the value contains anything outside the allowlist, the field is ignored. Use this instead of writing to `/dev/tty`, which is unavailable to hooks

> Hooks run without a controlling terminal, so writing escape sequences directly to `/dev/tty` fails. Instead, return the escape sequence in the `terminalSequence` field and Claude Code emits it for you through its own terminal write path. **This is race-free, works inside tmux and GNU screen, and works on Windows where there is no `/dev/tty`.**

> Anything outside the allowlist, including CSI cursor and color sequences, OSC palette sequences, OSC 8 hyperlinks, OSC 52 clipboard writes, **and OSC 1337**, is rejected and the field is ignored.

**三条后果,逐条落到片单上:**

1. **v1 片 A0 的问题①②撤销。** v1 要量的是「hook 子进程写不写得进 pty」与「`async` hook 的时序抖动多大」;上游已经把这条路做好了,而且**明说是 race-free 的、在 Windows 上工作**。**A0 从三问缩成一问**,只剩改正后的 ③′(§10.5)。v1 路线 B「hook 直写 `CONOUT$`」**作废**,它的位置由**路线 B′(`terminalSequence`)** 顶替。
   **v3 就地改一句措辞**:①② 撤销的理由是**产品选路** —— 官方给了一条它自己维护的、race-free 的通道,我们**选择不再走直写 tty 这条自己造的路**。**不能写成「官方白名单已经实验性证明 `CONOUT$` 写不进去」** —— 白名单一个字都没说 `CONOUT$`,那件事**至今没被量过,也不必再量**。**v2 原文记作**「它现在是一条自己造的、官方已经提供了替代品的风险路径」—— 这句话本身没错,但它容易被下一个人读成「实验已经证否」,所以补这一句把两件事分开。

2. **但 B′ 发不出 `RequestAttention`。** 官方白名单逐字点名 OSC 1337 是被拒的。于是**在 Claude Code 这个生产者身上,正文 §3.2 挑的那条通道无论 hook 怎么写都走不通**。B′ 能发的与本块有关的只有:`OSC 777;notify`(消息,红线 9 已判它不是状态)、`OSC 99`(kitty 通知,同上)、裸 BEL(事件)、`OSC 9;4`(进度,实测无源)、标题(转轮,§2.3 已证伪)。**一条都不满足判据②③。**

3. **所以路线 A 不再是「推荐」。** hook 是普通子进程,能执行任意命令、继承 pane 的环境变量,`folio attention wait` 完全不受 `terminalSequence` 白名单约束。这与评审「A3 必须前置于 A2」的重排指向同一处,并给了它一个更硬的理由:**没有 A3,claude pane 上的 awaiting 永远是零,把 BEL 撤出去就只剩一个空队列。**
   **v3 就地改这一条的措辞(闭合⑤)。v2 原文记作**「它是 Claude Code 场景下的**唯一可行路**」—— **说宽了**。准确的一句是:
   > **对 Claude Code 的强层,pane-local attention pipe 是本方案已定义的唯一生产路线;对通用层,`OSC 1337;RequestAttention` 仍是正门。**

   白名单事实证明的是**一件很窄的事**:`terminalSequence` 这条**代发**通道不许带 OSC 1337 —— **被堵死的是「hook 代发 1337」,不是「1337」**。它**不证明**:① `CONOUT$` 直写失败(见上条);② loopback / broker / 继承 handle 等别的 IPC 不可行;③ **别家 CLI 不能自己发 1337** —— 任何自己持有 tty 的程序照发不误,而**片 A1 服务的正是它们**。
   **成立范围**:当前上游版本 + 本方案既定的安全边界 + 本方案已选的路线。上游哪天把 1337 加进白名单(路线 C 的诉求),这句话自然失效 —— 而 **A1 的解析器当天就能接住**,一行不用改。这是 A1 与 A3 **并行**、而不是 A1 前置于 A3 的另一个理由(§11.5)。

**片 A1(`OSC 1337;RequestAttention`)照做不误,但要说清它是给谁的**:它的用户**不是 Claude Code**,是**任何自己写 tty 的程序** —— iTerm2 生态的现成脚本、构建工具、用户自己的脚本。它是一条**不需要装 hook 的通用路**,这正是它值得存在的理由,也是它不该被 A3 取代的理由。两条路服务两类生产者。

**路线 C 的诉求随之精确化。v1 原文记作**「请上游把 `preferredNotifChannel` 扩一档 `terminal_attention` 直接发 1337」。更小、更可能被接受的诉求是:**把 OSC 1337 加进 `terminalSequence` 的白名单** —— 白名单里已经有五条 OSC,加一条是一行改动,不引入新概念,而且它让每个终端都能用自己的通道接住 Claude Code。两条都记账,都不占本块工期。

#### 10.4.3 从原文抄下来的三条施工约束

- **信号 hook 必须 `async: true`。** 官方:async hook「starts the hook process and immediately continues without waiting for it to finish」。而 `PermissionRequest` 是**同步的决定门**(默认 600 秒超时),一个卡住的同步 hook 会把**每一次权限询问**都拖住。async hook 不能返回 decision —— 而我们本来就不返回 decision,只要副作用。
- **`terminalSequence` 只在交互式会话、且 Claude Code 界面在屏上时才写**;`-p` 与 Agent SDK 下忽略。对本块无碍(pane 里就是交互式),但若将来用 B′ 发任何东西,这一条要写进门。
- **官方 Security considerations 是 §10.6 第 5 条的外部佐证**:"**`-p` or SDK session**: Claude Code never shows the dialog and treats the folder as trusted, so hooks committed in a repository's `.claude/settings.json` run in a folder you've never trusted."—— 所以 **Folio 的安装器只写用户级配置,绝不从工作区/仓库发现或加载 hook**,这不是我们的洁癖,是上游自己标出的攻击面。

---

### 10.5 【必改 M6】A0 ③ 的判据判反了

**v1 原文记作**:「③ ConPTY 会不会把 `\e[I`/`\e[O` 吃掉翻成 `FOCUS_EVENT`,而 Claude Code 读的是 VT 流不是 `ReadConsoleInput` —— **若被吃掉,这一片当场作废**」。

**这句话把正确行为写成了失败条件。** 两条既有证据同时反对它:

- **DESIGN §7.1.5i 已裁**:ConPTY 自己请求 `?1004`,并把收到的 `CSI I` / `CSI O` **转成 `FOCUS_EVENT`** —— 这是它把焦点事实**送进**子程序的方式,是通道正常工作的样子,不是通道被堵死。
- **录音 D 是端到端反证**:真 ConPTY + 我们写入的 `ESC [ O` ⇒ Claude Code **改变了行为**(零铃 → 一铃)。字节确实到了、确实被理解了。若「被 ConPTY 消费」等于作废,录音 D 就不可能存在。

**改正后的门(A0 ③′),两扇:**

- **正门**:**目标程序观察到一次 focus transition**。可观察产物就是录音 D 的那枚裸 BEL 及其逐字节签名(`…\x1b[?25h\x07\x1b(B\x0f\x1b[?1000h…`)。
- **副门**:**一个没开 `?1004h` 的普通程序**(`cmd.exe`、裸 `powershell` 提示符)收到我们发的字节后**屏幕上不出现字面乱码** —— 不出现 `[I` / `[O`,提示行不脏。

**三个补测点(评审点名要补的,逐条给做法):**

1. **`I` 边。** v1 与录音 D 都只验了 `O`(离开)。必须补「发 `CSI I` ⇒ 程序观察到 focus-in」,否则 A0.5 只做了一半 —— 而「你回来了」这一半正是「回来之后它还该不该响」的依据。做法:**录音 E** = 录音 D 的脚本 + 在回合中途补一次 `ESC [ I`,比对铃是否消失/位置是否移动。用 `bt-record` 的 `--input-plan`,与 §2.1 同一条命令。
2. **后台出生的 pane。** 在**非活动 tab / 无焦窗口**里起一个 pane:它从出生起就没有键盘。按 Q8 结论用 `Unknown / Focused / Unfocused` 三态,在 `?1004h` **首次或重新启用**的那一刻同步一次当前答案,验证它收到的是一次 `O` 而不是永远的沉默。**只等 `false→true` 的 UI 边沿会让这种 pane 永远收不到初态。**
3. **真实 Folio 事件边沿。** 不是脚本注入,是**真 `folio.exe`**。五个动作各产生正确的一次边沿、且**每个动作只发一次**(不每帧发):`Alt+Tab` 走开;点副屏另一个 app;**在同窗内点文件树 / preview / 搜索框**(KeyboardOwner 变了、窗口焦点没变 —— 这一格正是 Q8 的必改点);切 tab;切 seat。用 `BT_PTY_DUMP` 核对真的写出去了、且只写了一次。

**A0 剩下的形状**:①② 由 §10.4.2 的官方文档回答,不必再量;A0 **只剩 ③′ 这一问**,而它同时是 A0.5 的红门。

---

### 10.6 【必改 Q3】命名管道的安全合同(写死,不再是开放问题)

**先把不能说的话删掉。** 「管道名是我们自己发的,所以不会被别的程序冒充」不成立:**管道名是定位信息,不是身份**。Windows 默认命名管道 DACL 甚至给 Everyone/anonymous 读权限,官方明确要求需要隔离时**自行提供安全描述符**,并建议用 **logon SID** 隔离登录会话([Named Pipe Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights))。同一用户下的另一个进程至少可以连接与伪造;若它读到了 pane 的环境,随机管道名也不再是秘密。

**没有替代通道可换。** §10.4.2 刚证明 `terminalSequence` 发不出 1337,路线 B 作废;**对 Claude Code 的强层,pane-local pipe 是本方案已定义的唯一生产路线**(**v2 原文记作**「路线 A 是 Claude Code 场景下唯一可行路」—— 按 §10.4.2 第 3 条 v3 收窄)。所以正确的做法不是换路,是**把合同写清楚并把能力封死**。

**路线 A 的安全合同(六条,施工时逐条对照):**

1. **`FOLIO_PANE` 只供诊断,绝不作为授权或路由依据。** 服务端在**创建端点时**就把它绑定到一个 `LeafSession`;消息里**不接受** window/tab/seat 字段,也不允许调用者改目标。
2. **每 pane 一个不可预测的端点名 / 至少 128-bit capability**;`PIPE_REJECT_REMOTE_CLIENTS`、`FILE_FLAG_FIRST_PIPE_INSTANCE`;**显式 DACL 只给当前 logon SID**(及确有需要的 SYSTEM),**不用默认描述符**。
3. **只准两个幂等动词 `wait` / `clear`**(**v3 就地补**:动词仍是**两个**,但各带一把**有界关联键** `--key=<opaque>` —— 它**不具寻址能力**,只能在**本端点内**指认一条自己发出的 wait,而端点在创建时就绑死了 pane。键的合同、长度上限、outstanding 上限与溢出规则在 **§11.4**;红线 13 盯着它不许长成地址),限制单帧长度、连接数与速率。即使被同用户进程偷到,**最大影响也只是给这一 pane 制造/撤销一条注意力声明** —— 不能打字、不能开 pane、不能读转录、不能寻址别的 pane。(这一条与 §10.2 的幂等边 e6/e11/e13 互相加强:重复的 `wait` 连一行 trace 都写不出来。)
4. **pane 生命周期拥有端点。** leaf 迁窗/换 seat 时端点随 `LeafSession` 迁移;leaf 消失立即关 server 并让旧 capability **永久失效**(与 e16 同一时刻)。`FOLIO_PANE=<window>.<tab>.<seat>` 在 tear-out、合并或座位重排之后会陈旧,**不能作为稳定地址**;仓内进程内路由已经用 `LeafId { tab, seat }`(`main.rs:491-493`)而不是 window 坐标,但那仍是会随结构动作改铸的内部地址 —— **对外只暴露 opaque capability**。
5. **Folio 不从 cwd、仓库、`.claude/` 发现或加载 hook**;安装器只写**用户明确批准的用户级配置**。这满足 P3-4 的「永远不能从工作区/仓库读自动化」,并有 §10.4.3 第三条的上游佐证。仓库里的程序仍能像发 BEL/OSC 一样调用它继承到的 pane-local `wait` —— 所以第 3 条那道封印是必须的,不是可选的。
6. **不能与未来 P3-4 / mailbox 共用同一份授权。** 可以复用管道库、事件循环与生命周期代码,但**强能力必须另发 capability、另开 endpoint**。否则一个只该「按门铃」的项目 hook 会自然升级成「向别的终端打字」,正面撞上 P3-4 / P3-11 的 RCE 裁决。

**承认边界,不越界承诺。** 当前 attention-only 的威胁,可以通过「**明确承认同用户边界 + 把影响封到本 pane 的一位状态**」接受。**若目标是抵抗同用户的恶意进程,环境变量里的名称/令牌不够** —— 那需要不可转授的继承 handle 或更强的 broker 身份模型,而那**不在本块顺手承诺的范围内**。这句话要原样进 A3 的片单。

---

### 10.7 【必改 M7】三档表补格 + 8 行真值表

**漏的那一格**:**窗口有焦点,但请求来自非活动 tab。** 它既不满足档一(不是活动 tab),也不是档二的「可见但无焦点」(窗口就是有焦点的),更不是最小化/cloaked。v1 的三行表对它无话可说,而它是**最常见的一格** —— 你在 tab 1 里干活,tab 3 里的 agent 问你问题。

**裁定:归第二档 —— 窗内徽章 + 那枚点,不 toast。**(采纳评审推荐。)
理由:你人就在这个窗口前,而它要说的事**就在同一个窗口里一格之外**。toast 会盖住你正在看的东西,去告诉你一件切个 tab 就看得见的事;而它的点击动作在同窗内退化成「切个 tab」,不值一次桌面打扰。窗内已经有三重可见:那个 tab 上的点、标题栏的 `N waiting` 徽章、以及 `Ctrl+Shift+A` 一按就到。

**`desktop_reach` 的 8 行真值表**(`C2` 的门,`Reach{Nothing, Flash, Toast}`):

| # | `window_is_focused` | `tab_is_active` | `window_is_hidden` | `Reach` | 说明 |
|---|---|---|---|---|---|
| 1 | T | T | F | **Nothing** | 档一:你正在看它。今天就是这样 |
| 2 | T | F | F | **Flash** | **v2 补的那一格。** 窗内徽章 + 点,不 toast |
| 3 | F | T | F | **Flash** | 档二:副屏可见但无焦点 |
| 4 | F | F | F | **Flash** | 档二 |
| 5 | T | T | T | **Toast** | **不可达组合**(最小化/cloaked 的窗口不持有键盘焦点);`hidden` 优先,答案照档三 |
| 6 | T | F | T | **Toast** | 不可达组合,同上 |
| 7 | F | T | T | **Toast** | 档三 |
| 8 | F | F | T | **Toast** | 档三 |

化简后就是三行代码,而**它读起来就是那三档**:

```text
if window_is_hidden      { Toast }        // 够不到你,只能上桌面
else if focused && active { Nothing }     // 你正在看
else                      { Flash }       // 在这台屏上,但不在你眼前
```

**第 5/6 行不是防御性编程,是全函数的定义域。** 一个三 bool 的纯函数必须对 8 个输入都有答案;把「hidden 优先」写在最外层,是因为**不可见就是够不到你,不管焦点位怎么说** —— 而这两行同时覆盖了「刚刚最小化、焦点位还没更新过来」的那一帧。

**第 2 行的诚实说明(必须写进 `desktop_reach` 的 doc,否则会被当成分支写错)**:`FlashWindowEx` 对**前台窗口**在 Win32 上是无操作 —— 第 2 行的可见产物是**窗内徽章与那枚点**,不是任务栏闪。把它写成 `Flash` 而不是 `Nothing`,是因为这一格的答案是「**在这台屏上,但不在你眼前**」,与第 3/4 行是同一句话;若下一秒你切走了,同一条臂立刻变成真的闪,而**谓词一个字都不用改**。写成 `Nothing` 才是撒谎:它会让「有焦窗口的非活动 tab」和「你正盯着的那一格」在函数里长得一模一样。

**v3 补:A8 挂在这张表的哪一格。** 用户 2026-08-25 裁「**回合结束 = 无焦任务栏闪(默认开)+ 最小化 toast(设置行默认开可关)**」。它**不新造判据**,就走上面这张 8 行表:**第 1 行 `Nothing`**(你正看着它,回合结束不打扰)、**第 2/3/4 行 `Flash`**(任务栏闪 —— 其中第 2 行按上面那条诚实说明,在前台窗口上 `FlashWindowEx` 是 no-op,可见产物是窗内那两枚既有记号)、**第 5–8 行 `Toast`**。设置行、去重规则、与队列那条道的分界写在 **§11.7**。**它一张号都不发**(红线 14)。

**`attention_is_consumed` 一个参数都不许加**(红线 4 原样)。`notify.rs:93-101` 那段「读的是同一个谓词的另一面」的论证在两档模型下是对的,**三档之后不再对** —— 第 2 行既不是「看过了」也不是「够得到桌面」,它是第三个答案。**那段注释必须在 C2 里重写,不许留着。**

**Q4 不受影响**(第三档判据是否只认 `IsIconic || CLOAKED`),它仍是一道待裁的题(§10.13)。补格改的是第一/二档之间的边界,Q4 管的是第二/三档之间的边界,两者不重叠。

---

### 10.8 【必改 Q6】`RequestAttention` 的语义:两层,以及它们怎么合成 `Awaiting`

**拼写与先例核对通过**:iTerm2 的正式定义确为 `OSC 1337;RequestAttention=yes|once|no|fireworks` —— `yes` 持续、`once` 一次、`no` 取消。**没有更合适的既有序列**:kitty `OSC 99` 有 id / 更新 / close,但官方把它定义为**桌面通知消息**,应留在 `TerminalNotification` 通道;BEL / xterm urgency 仍是事件。**不换序列,也不自造序列。**

**必改的是语义投影。** `RequestAttention` 只说「请求用户注意」,**没有说「程序阻塞并等你回答」**。build 完成、下载完成也完全可以持续请求注意。把通用 `yes` 直接投影成 `Awaiting`,是在把较弱事实升级成较强断言 —— **与本方案刚从 BEL 身上纠正的错误同型**。`yes/no` 证明它是**可撤回的状态**,不证明**状态的原因是等待输入**。

#### 10.8.1 两层

| 层 | 名字 | 谁生产 | 它断言什么 |
|---|---|---|---|
| **弱** | `AttentionRequested` | `OSC 1337;RequestAttention=yes`(任何程序) | 「这个 pane 想要你的注意」 |
| **强** | `AwaitingInput` | `folio attention wait`(路线 A 动词),或 §10.4.1 白名单里的 hook | 「这个 pane **在等你输入**」 |

两层**共用 §10.2 那一台状态机**(同一个 episode 计数器、同一张号、同样的三主退出);它们的差别**只在凭据强度**,不在生命周期。所以队列里每张号带一位:

```text
grounds: AttentionGrounds { Requested, AwaitingInput }
```

> **v3 就地记(闭合①)**:「共用同一台状态机」这句话是对的,但 v2 交给它的**数据模型是单源的** —— 一个 `attention_request: Option<u64>`,而铸号只在 `bt-term` 的解析器里。管道事件不经过 OSC 解析器,**没有合法的铸号入口**,也没有存放强层 live 状态的地方;A3 更不许反向去伪造 `bt-term` 的流事实。**§11.1 把这台机器扩成双源**:两个 generation(弱层住 `bt-term`、强层住 `bt-app` 的 pane 端点)、两条 ack 水位、一个 app 侧的 episode 协调器;`grounds` 由派生式算,不是锁存位。**A7 之后这张表还多一级**(事件级 `Announced`,§11.6),但那一级**不进这台状态机**。

#### 10.8.2 合成规则(强吃弱)

| 弱层在请求 | 强层在请求 | 号的 `grounds` | 文案 | 配 toast? |
|---|---|---|---|---|
| 否 | 否 | 不发号 | — | — |
| **是** | 否 | `Requested` | 「需要注意」/ "Attention requested" | **不配** |
| 否 | **是** | `AwaitingInput` | 「正在等你回答」/ "Waiting for you" | **配** |
| **是** | **是** | `AwaitingInput` | 「正在等你回答」 | **配** |

- **`grounds` 在一个 episode 内可升可降,但号与 toast 不跟着抖**:一个 pane 先发 1337 `yes`(弱)、六秒后 hook 又报了 `permission_prompt`(强),是**同一次请求**被证实了 —— 升级 `grounds`,**不铸新 episode、不重发号、不重新排队**(先到先服务不能因为证据变强而被打乱),并且**这一刻要补投那一次 toast**(§11.2)。强层撤了而弱层还举着,**`grounds` 降回 `Requested`、文案退回「需要注意」,号不退、也没有第二次 toast**。
  **v2 原文记作**「`grounds` **只升不降**……反过来不降:强层撤了但弱层还举着,`=no` 之前它仍是同一次请求」—— **v3 就地改(闭合①)**:窄复核的反例成立 —— 强层是唯一的强凭据,它撤了以后 UI 还在说「正在等你回答」,就是**在用一个已经不存在的凭据说话**。正解是把 `grounds` 写成**当前未答凭据的派生量**(`grounds = if 强层有未答凭据 { AwaitingInput } else { Requested }`,§11.1.2),而不是一个只升不降的锁存位;**「只升不降」这件事真正该锁的是 toast**,它由 episode 上的 `toasted` 一位管(§11.2)。**每个源的 `clear` 只撤自己那一层**,这是双源的第一条纪律。
- **§5.4「只有真 `awaiting` 信号发的号配 toast」现在有了准确的定义**:`grounds == AwaitingInput`。这一条把 v1 那句凭直觉写的话变成了可施工的判据。

#### 10.8.3 为什么不长第八个视觉断言

**不新增 `StatusClaim`。** 两个 `grounds` 共用 `Awaiting` 那一格的颜色与脉动,**只有文案分两句**(徽章 tooltip、toast 正文、命令面板行、`Ctrl+Shift+A` 的落点说明)。

这不违反红线 3。红线 3 说的是「一点一断言不许继续退化」,而 Bell 与 Awaiting 同色之所以坏,是因为那是**两个不同的事实**(一次性事件 vs 可撤回状态)共用一个像素 —— 静止时你分不出「响过一声」和「还在等着」。而 `Requested` 与 `AwaitingInput` 是**同一个事实的两种凭据强度**:两者都在说「这个 pane 要你」,共用一个像素**不制造歧义**。歧义在于「要你干什么」,而那句话本来就只在文案里说得清,**在一枚 5px 的点上说不清**。

**若用户否决这个设计**,评审给的另一条路是:把 claim、徽章与 toast 的文案**全部**改成「需要注意 / attention requested」,让任意 `RequestAttention=yes` 直接入队 —— 代价是**「正在等你回答」这句话从产品里消失**,而它正是用户抱怨的那件事想要的答案。本方案**不推荐**。

#### 10.8.4 `once` 与 `fireworks`

- **`once`** 映射为既有的一次性 `Bell`(置 `bell_latched`),**不进 §10.2 的状态机**。
- **`fireworks`** 明确**原样忽略**(不报错、不当未知值、不落任何状态),并配一个钉。

**红线 9 已就地改写**为「`yes/no` 这一臂是状态、`once` 是事件」。**v1 原文记作**「`RequestAttention` 是状态」—— 那句话把 `once` 也裹了进去。

---

### 10.9 【Q7 通过】`mark_seen` 整片清铃保留,附两条回归钉

**结论:通过,不动。** `TabState::mark_seen`(`main.rs:19069`)遍历所有 leaf、`mark_leaf_seen`(`main.rs:21591`)追平 revision 并调 `clear_attention()` —— 对 BEL / 失败 / 未读,这仍然是正确的:**tab 上那个点是 fleet 的聚合断言**,活动 tab 的整棵 pane 树都上屏,切进 tab 就是整片被看见;只清 focused seat 会让同一 tab 的聚合点下一帧复活。

**「队列逐 seat」不构成反例**:队列 ticket 不是 `mark_seen` 的对象,**今天也没有被它清**。BEL 撤出队列之后,两类账反而更干净 —— **整片清的是 Bell / Failed / Unread;逐 seat 退的是 Awaiting episode。**

**两条回归钉(A1/A2 各带一条):**

1. **`mark_seen` 清同 tab 每个 sibling 的 bell / failure / revision,但不清 `attention_request`、不撤 ticket、不改 `attention_acked`。** 这就是 §10.2.3 的边 e17,写成一条可跑的测试。
2. **A1 不得为了省一个函数把新的 `attention_request` 塞进现有的 `clear_attention()`。** 并且要把 `mark_seen` 注释里的「every claim」**收窄为「every seen / one-shot claim」** —— 否则那段注释在 A1 之后就变成假话。(与 §10.2.5 对 `attention_ticket` doc 的要求同型:**新语义落地的同一片里,必须把被它变成假话的注释一起改。**)

---

### 10.10 【必改 Q8】焦点谓词补第四位,加订阅初态

**pane 粒度通过。** 漏的不是「聚焦模式上台那一席」,是**窗口真实的键盘 owner**。

`focused_leaf` 只命名「键盘回到 shell 时落到哪一席」,**并不证明键盘此刻在 terminal**。仓里已有明确反例:preview/page 用 `focused_preview_seat`,网页自己有 `web_keyboard`;文件树、preview edit、search、menu/dialog 都能拿走键盘。`keyboard_owner_is_a_shell()`(自由函数 `main.rs:10236`,Runtime 上的 `main.rs:38584`,读的是 `keyboard_owner()` `main.rs:38593` 汇总的八个 owner)**正是现有的统一判据**;而网页那段实现里明确写着 `focused_leaf` 永远不能代表 preview seat。

**每个 terminal seat 的有效谓词(替换正文 §3.0 的三位判据):**

```text
window_focused
&& tab_is_active
&& seat == tab.focused_leaf
&& keyboard_owner_is_a_shell()
```

**v1 原文记作**「判据 = `窗口有焦点 && 这个 leaf 是 focused_leaf && 它的 tab 是活动 tab`;这三个位今天全都现成」—— **三位不够,第四位也现成**。

**`sessions.contains_key(seat)` 是地址守卫,不是第五个产品事实**,不进这个谓词。
**聚焦模式不加位**:它不另造键盘 owner,换台最终仍落到 `focused_leaf`。独 preview、网页、文件树、搜索框、rename、menu/dialog 抢键盘时,owner 谓词会让原 terminal 收到 `CSI O`;归还时才收 `CSI I`。
**一个 pane 从不可见 tab 变成可见、但仍不握键盘,不发 `CSI I`** —— 协议报告的是**输入焦点**,不是可见性。v1 的倾向是对的,现在有了理由。

**两个边沿要求:**

1. **四个事实任一变化都要重算**,包括 owner 在**同一窗口内**变化 —— 不能只挂 `WindowEvent::Focused` 和 tab 切换。点一下文件树就该发 `O`,而窗口焦点在那一刻一位都没动。
2. **必须定义「订阅边沿 / 初态」。** ConPTY 从会话第一个字节就发 `?1004h`,子程序还可能再次发(录音 B:第 7 字节一次、第 62 字节又一次)。一个**在后台出生的 pane** 若只等 `false→true` 的 UI 边沿,可能**永远收不到初始的 `O`**。至少要用 **`Unknown / Focused / Unfocused` 三态**,并在 1004 **首次或重新启用**时同步当前答案。**这一项由 A0 ③′ 的测试点 2 真机钉住**(§10.5)。

**焦点报告的写法(已升为红线 10):** 必须直接写目标 seat 的 `PtySession`,走终端自答通道(`take_pty_writes`,`main.rs:22560`/`22582`),并标成 terminal-generated reply。**绝不能走「用户输入」那条门** —— 否则片 A0.5 发出的 `CSI O` 会被片 A2 当成「用户回答了」而退票,而这个 bug 会**只在你切走的时候**发生,是最难被人工门抓到的一类。§10.3.2 的 `UserInputKind` 形状让这件事**结构上不可能**,而不是靠记得排除。

---

### 10.11 分片依赖重排

> **v3 就地记(闭合⑤的依赖面)**:本节的两条结构性变更(**A3 前置于 A2**、**D 拆出本方案**)都成立,不撤。但下表把 **A3 的前置写成了 `A1 + §10.6`**,而**新事实推不出「A3 依赖 A1」** —— A1 是通用程序的 OSC 弱源,A3 是 Claude hook 的 pipe 强源,两个源**并行喂同一个双源核心**。**§11.8 是 v3 的片表**:抽出 **A-core**(§11.1 的双源账本,不碰 UI、不接生产者),然后 **A1 与 A3 并行**,A2 依赖 A-core + A3 与两条已裁的门。若排期上仍想把 A1 先做完,那是**打包顺序,不是技术依赖**,要在片单里说出来。

**两条结构性变更:**
- **A3 移到 A2 之前**(评审必改)。理由在 v1 里只有「不该先拆假队列」,§10.4.2 之后有了更硬的一条:**Claude Code 只能走路线 A,没有 A3 就没有任何 claude pane 会进队列**,BEL 一撤队列就是永久空的。
- **可选 D 拆出本方案**(评审必改 M9)。v1 把「第八记号 `Attached`」「dead 接线」「`closeOnExit`」三件不同生命周期的事塞进一片、还只挂 Q1。**Q1 只裁第八记号**;dead 与 `closeOnExit` 在 DESIGN §7.1.5b 已有独立裁决,不该被 Q1 一并授权,也不该借注意力块顺带施工。**本块只保留 Q1(`Attached` 做不做),其余两件退回原处。**

**生产片恰好九个**(A0 是 spike,D 出块):

| 序 | 片 | 前置 | 相对 v1 的变更 | 门 |
|---|---|---|---|---|
| 0 | **A0**(spike) | 无 | **三问缩成一问**:①② 由官方 `terminalSequence` 文档回答(§10.4.2),不必再量;只剩改正后的 ③′ | §10.5 的正门 + 副门 + 三个补测点 |
| 1 | **A0.5** | A0 ③′ | 谓词补第四位 `keyboard_owner_is_a_shell()`;三态订阅初态;走 `take_pty_writes` 不走 `send_user_input`(红线 10) | 真机四格:out / in / 后台出生 / 普通 shell 不出乱码 |
| 2 | **A1** | 无(**与 A0.5 并行**) | 落 **parser + episode 号**,**不落 UI、不命名 Awaiting**;不是简单扩表(M10):`1337;` 今天在 `inline_image.rs:1087-1089` 直接进 `InlineFileCapture`,要**先在共同前缀后分流** `File=` 与 `RequestAttention=` | 红形 + **五个钉**:BEL/ST 两种收尾、跨 chunk 重组、超限、未知 value、`fireworks` 原样忽略 |
| 3 | **A3** | A1 + §10.6 安全合同 | **移到 A2 前**;路线 A 为唯一可行路(§10.4.2);hook 按 §10.4.1 白名单 + `async: true`;安装提示照 §7.1.6j「问那个程序自己」 | 真 claude + 真 Folio 人工门:权限提问出现 ⇒ 队列长一位 |
| 4 | **A2** | **Q2a + Q2b(用户裁)** + A1 + **A3** | **与 A3 原子或后置**;一次做完 admit / ack / withdraw / leaf-gone(§10.2.3 十七条边);trace 长 episode 与 grounds(§10.2.6);同时改 `attention_ticket` 与 `mark_seen` 两处注释(§10.2.5 / §10.9) | `BT_ATTENTION_TRACE` 端到端;截图那个场景先变红再变绿 |
| 5 | **B1** | A2 + **A3** | 合入门**加一条**:真 producer 产生的 waiting 数量 —— 避免徽章只在测试夹具里见过 | 命中表断言,不是矩形断言 |
| 6 | **C1** | 无(**与 A 线并行**) | 只提供 `IsIconic` / `DWMWA_CLOAKED` 事实与查询失败政策,**不提前决定 toast** | 事实源的钉 |
| 7 | **C2** | C1 + §10.7 补格 | 8 行真值表;`FlashWindowEx`;`notify.rs:93-101` 那段注释重写;`attention_is_consumed` 保持两参 | 真值表从 4 行长到 **8 行** |
| 8 | **C3** | **A2 + A3 + C2** | **只在新 episode 首次入队的边沿(e4)投一次**(M8);点击路由复用既有 `NotificationRoute` | 「持续 `yes` 不连发 toast」的钉 |
| 9 | **B2** | **命令面板本体** | **解耦,不阻塞主链**(评审建议)。命令面板尚不存在;`Ctrl+Shift+A` 已可用。本块只登记动词与键位,面板 row 挂到命令面板里程碑 | — |
| — | ~~D~~ | — | **拆出本方案**;Q1 只留 `Attached`;dead / `closeOnExit` 归 §7.1.5b | — |

**关键路径**:

```text
A0(修门) ──▶ A0.5
A1 ──▶ A3 ──▶ A2 ──▶ B1
C1 ──▶ C2
A2 + A3 + C2 ──▶ C3
B2 ──▶ 独立等命令面板
```

**这样排掉的两个中间坏版本**:①「假队列已拆、真生产者尚未装」(A2 先于 A3);②「持续 `yes` 每帧 toast」(C3 挂在电平而不是边沿上)。

**并行性(替换 v1 §6 末段)**:A0.5、A1、C1 三片**开工时互不依赖**,可同时开三条线。**v1 原文记作**「A 线与 C 线之间只有 C3 一个交点;B 线全部挂在 A2 后面」—— 前半句仍对,后半句要改:**B1 挂在 A2+A3 后面,B2 完全不挂在 A 线上**。

---

### 10.12 【建议 M11】行号刷新(2026-08-25 实测)

评审点名的五处全部证实漂移,并把它没点到的几处一并刷了。**开工前以本表为准。**

| 符号 | v1 写的 | 实测 | 文件 |
|---|---|---|---|
| `session_is_breathing` | 13781 | **14033** | `bt-app/src/main.rs` |
| `StatusClaim`(枚举) | 13593-13614 | **13824** | 同上 |
| `dot_color` | 13627 | **13853** | 同上 |
| `pulses()` | 13640 | **13870** | 同上 |
| `SessionFacts::claim` | 13702-13720 | **13932** | 同上 |
| `attention_is_consumed` | — | **14168-14170** | 同上 |
| `attention_is_asked` | 13963-13965 | **14193-14195** | 同上 |
| `attention_ticket`(「nothing here takes a place away」) | 13983 | **14213-14220** | 同上 |
| `next_attention_stop` | — | **14241** | 同上 |
| `settle_attention` | 14067-14130 | **14297-14362** | 同上 |
| `answer_attention_in` | — | **14374** | 同上 |
| `loudest_claim` | 14155 | **14385** | 同上 |
| `wait_halo_opacity`(reduced-motion 返回 0.0) | 14318-14328 | **14548** | 同上 |
| `TabState::mark_seen` | 18822-18843 | **19069** | 同上 |
| `grayscale: false`(dead 未接线) | 18975 | **19205** | 同上 |
| `fleet_claim_for` | — | **19223** | 同上 |
| `mark_leaf_seen` | — | **21591** | 同上 |
| `Runtime::answer_attention` | 60187 | **61076** | 同上 |
| `Runtime::jump_to_attention` | 60223 | **61112** | 同上 |
| Enter 出队门 | 62164 | **63058-63059** | 同上 |
| `attention_next_ticket`(字段) | 6193 | **6279** | 同上 |
| `write_pty_input` | — | **10503** | 同上 |
| `keyboard_owner_is_a_shell`(自由函数) | — | **10236** | 同上 |
| `keyboard_owner_is_a_shell`(Runtime) | — | **38584** | 同上 |
| `keyboard_owner`(八个 owner 的汇总) | — | **38593** | 同上 |
| `UserInputKind` | — | **10687-10696** | 同上 |
| `send_mouse_input_to` | — | **55438** | 同上 |
| `send_user_input` | — | **55459** | 同上 |
| `open_from_notification` | — | **53426** | 同上 |
| `settle_attention` 调用点 | — | **53728** | 同上 |
| `1337;` → `InlineFileCapture`(A1 要分流的那一处) | 1067-1089 | **1064-1092**(判定在 1067,分派在 1087-1089) | `bt-term/src/inline_image.rs` |
| `reaches_the_desktop` | 107-109 | **107-109**(未漂) | `bt-app/src/notify.rs` |
| 「同一个谓词的另一面」那段注释(C2 要重写的) | 93-101 | **93-101**(未漂) | 同上 |

---

### 10.13 留给用户的裁决清单(v2 之后)

> **v3 状态:本节全部已裁(A 组八条,2026-08-25 用户裁,全过)。** 逐条落点表在 **§11.0**。下表留作对照 —— 「推荐」那一列现在都是**裁决**,不是建议。

**必须先裁、否则片不能开工的:**

| 题 | 内容 | 挡住哪片 | 推荐 |
|---|---|---|---|
| **Q2a** | **BEL 撤出入队门吗?**(复议 2026-08-21 裁决) | **A2** | **撤** |
| **Q2b** | **出队门从 Enter 扩到「带来源的用户动作」吗?**(复议 2026-07-18 裁决)含子格:**被转发的鼠标算不算**(§10.3.2 第 5 行) | **A2** | **扩**;鼠标**算**。**若 Q2a 被否,Q2b 必须同否**。**v3:已裁 A3「扩」**;鼠标子格由 §11.3 拆三 —— **按键算、滚轮算、motion 不算** |
| **Q4** | 第三档判据只认 `IsIconic \|\| CLOAKED`,把「被别的窗口完全盖住」归第二档 —— 接受吗? | **C2** | 接受(winit 的 `Occluded` 在 Windows 后端不发,自己算 z 序是启发式) |
| **Stop 语义(新)** | `Notification` 的 **`idle_prompt`**(官方:「Claude 回完话约 60 秒且你没打字」)算不算 waiting? | **A3** 的白名单 | **不算 waiting。** 它是上游版的「输出安静 N 秒」启发式,而 v1 §3.4 明确不做这一档。**次选**:当作 §10.8 的弱层 `Requested` 凭据(它至少是程序自己说的,且它知道自己有没有在跑工具 —— 比我们自己数秒诚实)。**普通 `Stop` 无论如何不映射**,那是 §2 花四段录音证伪的东西 |

**不挡工期、但要拍板的:**

| 题 | 内容 | 推荐 |
|---|---|---|
| **Q1** | 第八记号 `Attached` 做不做?(**已按 M9 收窄:只裁这一件**,dead 接线与 `closeOnExit` 退回 §7.1.5b) | **做** |
| **Q5** | 「Claude Code bell」那一行留着 + 加一条装 hooks 吗? | **两条都写**,纪律原样(存原值 / 恢复不删 / 问程序自己)。**§10.4.2 让这一题的答案更清楚**:铃管「你走开了」(A0.5 之后才真的有用),hook 管「我在等你」,而且 hook 是唯一能管后者的 |

**已经答完、不再是开放问题的**:Q3(§10.6 六条合同)、Q6(§10.8 两层 + 合成规则)、Q7(§10.9 通过 + 两条钉)、Q8(§10.10 第四位 + 三态初态)。

**v1 §9 的 Q2 已作废**,由 Q2a / Q2b 取代;**v1 §9 的 Q1** 已按 M9 收窄。

---

### 10.14 v2 之后的验收门增补(接正文 §8)

> **v3 再加八步(⑪–⑱),见 §11.9。**

正文 §8 那八步原样有效,加三步:

- **⑧ 同窗非活动 tab**:窗口保持前台,在 tab 3 的 agent 上触发一次请求 ⇒ **窗内徽章 +1、tab 3 上点亮,不弹 toast**(§10.7 第 2 行)。
- **⑨ 已答但程序没撤**:回答权限提问之后**不要**让 agent 撤;让它继续举着 `wait` ⇒ 徽章**不许**在下一帧重新 +1(§10.2 边 e13)。这一步是 bool 版本必挂、状态机版本必过的那一格。
- **⑩ 同窗内换键盘 owner**:窗口焦点不动,点一下文件树 ⇒ `BT_PTY_DUMP` 里该 pane 收到**恰好一次** `\e[O`;点回终端 ⇒ **恰好一次** `\e[I`(§10.10 边沿要求 1)。

**外加**:关掉 Windows 动画(reduced-motion)重跑 ②③,Bell 与 Awaiting 必须仍然分得出(红线 3,原样)。

---

## 11. 2026-08-25 v3 终修(窄复核折入 + A 组裁决)

**这一节是什么**:`codex-confirm.md` 的**总判定**点名了五处「阻塞施工的最小修订集」,本节逐处闭合;同日**用户裁决了 A 组八条**(全过,其中 A7 / A8 是新加的两条),本节把它们正式写进方案。**本节与 §0–§10 冲突时以本节为准。** 被改动的旧文都已就地改并留了「v1 原文记作 / v2 原文记作」。

**闭合对照(总判定那五处):**

| # | 窄复核的话 | 落点 | 一句话 |
|---|---|---|---|
| ① | 补**双源 generation / ack 合成**与每条边的 trace | **§11.1** | 两个 generation + 两条 ack 水位;状态是它们的**纯函数**,不另存;`asking` 由假变真是**唯一**铸号边 |
| ② | 补**强凭据后到的单次 toast 边** | **§11.2** | 唯一 toast 门 =「持号 + `grounds==AwaitingInput` + 本 episode 未投过」,每次转移后求值一次 |
| ③ | 把**鼠标 motion 从 Answer 拆出** | **§11.3**(+ §10.3.2 就地改) | `UserInputKind` 拆七值,`MouseMotion` 是唯一不是 `Answer` 的那个 |
| ④ | 为 hook wait 写**可连续工作的 clear/关联规则** | **§11.4** | 逐源 clear 配对表 + 一把**有界关联键**;两个动词不变 |
| ⑤ | 收窄**「管道唯一路线」**及依赖表述 | **§11.5**(+ §10.4.2 / §10.6 / §10.11 就地改) | 强层对 Claude Code 唯一;**通用层 1337 仍是正门**;A1 ∥ A3 |

**另外两处**(窄复核在「四个死角」里点到、本节一并闭合):**死角 1**(episode 的所有权与铸号入口)在 §11.1.1;**死角 4**(`withdraw`/`expire` 的字段合同不可实现)在 §11.1.5。

---

### 11.0 A 组裁决落点(2026-08-25 用户裁,八条全过)

| 编号 | 裁决 | 对应旧题 | 落点 | 解锁了什么 |
|---|---|---|---|---|
| **A1** | **做** | Q1 第八记号 `Attached` | §3.5 原样成立 | `Attached` **不占本块工期**(片 D 已按 M9 拆出本方案):它成为 §7.1.5b 表上一条独立的扩条,本块只登记这条裁决。dead 接线与 `closeOnExit` 仍归 §7.1.5b,不借本块施工 |
| **A2** | **撤** | Q2a BEL 撤出入队门 | §4 B2 / §10.3.1 原样成立 | **片 A2 的第一道锁开了。** 撤之后队列在 A3 落地前是空的,这是有意的(§4 B2) |
| **A3** | **扩** | Q2b 出队门扩到「带来源的用户动作」 | §10.3.2 + **§11.3** | **片 A2 的第二道锁开了。** 鼠标子格由 §11.3 拆三格 |
| **A4** | **接受** | Q4 第三档只认 `IsIconic \|\| CLOAKED` | §5.2 / §10.7 原样成立 | **片 C2 的锁开了** |
| **A5** | **加** | Q5「Claude Code bell」行留着 + 加一条装 hooks | §5.5 原样成立 | 设置块两行;纪律原样(存原值 / 恢复不删 / 状态从文件读 / **问那个程序自己**) |
| **A6** | **不算** | `Notification` `idle_prompt` 算不算 waiting | §10.4.1 第三张名单就地改 | **片 A3 的白名单封口。** 「次选:当作弱层凭据」**未被采纳** —— 裁决只说「不算」,而名单是封闭的,要加须另裁 |
| **A7** | **收 `OSC 9` 与 `OSC 777` 通知序列进凭据体系** | 新 | **§11.6** + 红线 9 就地改写 | 凭据从两级变三级:事件级 `Announced` 有了名分,而它**仍然不发号** |
| **A8** | **回合结束 = 无焦任务栏闪(默认开)+ 最小化 toast(设置行默认开可关)** | 新 | **§11.7** + §10.7 就地补 | 片 C2 / C3 各长一条臂;**不进队列** |

**还没裁、也不挡工期的**:一条都没有 —— A 组之后,本块**没有留给用户的开放题**。将来若要把 `idle_prompt` 收成弱层凭据、或把滚轮撤出 Answer(§11.3),都要**新起一题**。

---

### 11.1 【闭合①】双源 episode 核心

#### 11.1.1 三个生产层,两个 generation,一个 episode 协调器

窄复核死角 1 说的是:§10.2.1 把铸号写死在 `bt-term` 的 OSC 解析器上,而 §10.8 又要求管道的强源共用同一台机器 —— **管道事件不经过解析器,没有合法的铸号入口**,而 A3 更不许反向去伪造 `bt-term` 的流事实。正解不是让一个源冒充另一个源,是**把铸号抬到能同时看见两个源的那一层**。

| 层 | 住哪 | 字段 | 谁写 | 为什么在这一层 |
|---|---|---|---|---|
| **弱层**(流的事实) | `bt-term` | `SessionStatus.attention_request: Option<u64>` | 解析器:`=yes` 在 `None → Some` 的**上升沿**铸一个新**弱 generation**;`=no` 清回 `None` | 它是**字节流的函数**,可离线重放、可在 `bt-term` 单元测试里钉死,**不需要知道「用户」是什么**(§10.2.1 原话,原样成立) |
| **强层**(管道的事实) | `bt-app`,**挂在 pane 端点上** | `LeafSession.attention_waits: Vec<(WaitKey, u64)>` | 端点:`wait --key=k` 且 `k` 不在表里 ⇒ 铸一个新**强 generation** 并记 `(k → g)`;`clear` 按 §11.4 的选择器删条目 | 它是**管道事件序列的函数**,同样可重放;它**住在 leaf 上**,所以 leaf 一消失它跟着消失(§10.6 第 4 条) |
| **事件级**(A7) | 两边都有 | 既有 `bell_latched` / `TerminalNotification` | BEL / `once` / `OSC 9` / `OSC 777` / `OSC 99` / 回合结束 | **不铸号、不进本节这台机器**(红线 14);它的去处在 §11.6 / §11.7 |
| **episode 协调器** | `bt-app`,`LeafSession` | `AttentionLedger`(下) | 只有它铸 episode | 它是**唯一同时看得见两个 generation 的地方**,所以它是唯一能回答「这是不是同一次请求」的地方 |

```rust
struct AttentionLedger {
    episode:      Option<u64>,  // per-session 单调递增、永不复用
    ticket:       Option<u64>,  // 既有 `attention_ticket`,一个字不改
    acked_weak:   u64,          // ack 水位,0 = 从未
    acked_strong: u64,          // ack 水位,0 = 从未
    toasted:      bool,         // 本 episode 是否已投过 toast(§11.2),随 episode 生灭
}
```

**三个计数器,三本账,不许合并**(§10.2.1「两个计数器」那条纪律的扩写):`ticket` 决定**队列次序**(per-window)、`episode` 决定**这是不是同一次请求**(per-session)、两个 generation 决定**这是不是同一条凭据**(各自 per-session)。

#### 11.1.2 状态是派生量,不另存

```text
weak_gen   = status.attention_request.unwrap_or(0)                      // 0 = 弱层没在举
strong_gen = attention_waits.iter().map(|(_, g)| *g).max().unwrap_or(0) // 0 = 强层没在举

unanswered_weak   = weak_gen   > acked_weak
unanswered_strong = strong_gen > acked_strong

asking       = unanswered_weak || unanswered_strong        // ← 入队门的第一个参数
any_asserted = weak_gen != 0 || strong_gen != 0
grounds      = if unanswered_strong { AwaitingInput } else { Requested }
queued       = ticket.is_some()
```

四个状态**照旧是那四个**(§10.2.2 一个字不改),只是判据换成上面这五个派生量:

| 状态 | 判据 | 一句话 |
|---|---|---|
| **`Idle`** | `!any_asserted` | 两个源都没在举。**leaf 出生即此** |
| **`Requested(e)`** | `asking && !queued` | 有未答凭据,还没拿到号 |
| **`Queued(e,t)`** | `asking && queued` | 有未答凭据,持号 `t` |
| **`Acknowledged(e)`** | `any_asserted && !asking` | **还有凭据举着,但举着的那些你都已经答过了** |

**「水位」是这一节全部正确性的来源,而它只是 §10.2.1 那条单调性论证的双份**:generation 严格递增、永不复用,所以 `gen > acked` 这个比较**天然地**让「旧 ack 吞不住新 generation」,也天然地让「同一条凭据的重申吞不掉自己」。于是:

- **不需要在 `=no` / `clear` 时回头清 ack**(§10.2.1 原话)—— 少一条清理路径,就少一处能忘;
- **不需要 ack 记一个集合** —— 两个 `u64` 水位就够,因为「答」这个动作在语义上就是「把此刻桌面上摆着的都答了」;
- **不需要 `grounds` 锁存位** —— 它是 `unanswered_strong` 的即时函数,强层一撤它自己就降下来(§10.8.2 v3 就地改)。

#### 11.1.3 合成规则(四条,双源全部的语义都在这里)

1. **铸号边只有一条:`asking` 由假变真。** 从 `Idle` 起(两源都空 → 一源举起)、或从 `Acknowledged` 起(已答的凭据之外**出现一个更新的 generation**),都铸一个新 episode,并把 `toasted` 置 false。**这条边同时闭合了死角 1**(协调器在 app 层,两个源都能触发它,谁也不用伪造谁)**和窄复核反例①**(已答之后 hook 报来一个真的权限请求 ⇒ 它是新 generation ⇒ `asking` 重新为真 ⇒ **新 episode,能再入队**)。
2. **答一次,答的是此刻桌面上的全部**:`Answer` 把两条水位同时追平到当前两个 generation。所以 `Acknowledged` 对 `Settle` 仍是**不动点**(§10.2.4 第 1 条,一个字不改),bool 做不到的那件事仍由它做成。
3. **每个源的撤回只撤自己那一层**(`=no` 只清弱层,`clear` 只删强层的条目)。**窄复核反例②的正解**:强层撤了而弱层还举着 ⇒ `grounds` 降回 `Requested`、文案退回「需要注意」、**号不退、也没有第二次 toast**。
4. **`asking` 由真变假时,号必须走**(不变量 I3)。走法有两种,而它们写不同的 trace:因 `Answer` 而走 ⇒ `answer`;因**最后一个未答凭据被它的生产者撤回**而走 ⇒ `withdraw`。这就是红线 11 那个「泛化后的例外一」。

#### 11.1.4 全部边(4 态 × 9 事件的到达网格)

事件九种:`WeakYes`(`=yes` 上升沿)、`WeakNo`(`=no`)、`StrongWait{新 key}`、`StrongWait{旧 key}`、`StrongClear{sel}`(含 TTL / `SessionEnd`,§11.4)、`Settle{consumed}`、`Answer{by}`、`LeafGone`、`mark_seen`。

**转移规则只有一条**:先按事件改 generation / 水位,**再由 §11.1.2 的派生式重算状态**;铸号发生在且仅发生在 `asking` 由假变真的那一刻。下表是这条规则展开后的**到达态与副作用**。

| 事件 \ 从 | `Idle` | `Requested(e)` | `Queued(e,t)` | `Acknowledged(e)` |
|---|---|---|---|---|
| **`WeakYes`**(上升沿) | → `Requested(e′)`,**铸号** `mint … src=osc` | 不是上升沿(弱层已 `Some`)⇒ 幂等零 trace | 同左,**ticket 不重铸** | 若弱层此刻是 `None`(先前被 `=no` 清过)⇒ 铸新弱 gen ⇒ `asking` ↑ ⇒ → `Requested(e′)` **铸新 episode**;否则幂等零 trace |
| **`WeakNo`** | 幂等零 trace | 强层仍有未答 ⇒ 留 `Requested(e)`,`clear src=osc`;强层已答但仍举着 ⇒ → `Acknowledged(e)`,`clear`;两源皆空 ⇒ → `Idle`,`clear` + `drop` | 强层仍有未答 ⇒ 留 `Queued(e,t)`,`clear`(`grounds` 不变,仍 awaiting);否则 `asking` ↓ ⇒ **退号** `withdraw … ticket=t reason=program src=osc`,再按 `any_asserted` 落 `Acknowledged(e)` 或 `Idle` | 仍有凭据 ⇒ 留 `Acknowledged(e)`,`clear`;全空 ⇒ → `Idle`,`clear` + `drop` |
| **`StrongWait{新 key}`** | → `Requested(e′)`,**铸号** `mint … src=pipe grounds=awaiting` | 留 `Requested(e)`,`grounds` 升,`upgrade`(**不带 ticket**);**不 toast**(没入队) | 留 `Queued(e,t)`,`upgrade … ticket=t`;**toast 门求值 ⇒ 这里就是后到的强凭据补投那一次的地方**(§11.2);**ticket 不重铸、不重排** | → `Requested(e′)`,**铸新 episode**(新 gen > 水位),`grounds=awaiting` |
| **`StrongWait{旧 key}`** | 不可达(旧 key 意味着强层非空) | 幂等零 trace | 幂等零 trace | 幂等零 trace |
| **`StrongClear{sel}`** | 幂等零 trace | 与 `WeakNo` 对称(`src=pipe`) | 弱层仍有未答 ⇒ 留 `Queued(e,t)`,`clear src=pipe` + **`downgrade … ticket=t grounds=requested`**,**号不退、`toasted` 不清**;否则退号 `withdraw … src=pipe` | 与 `WeakNo` 对称 |
| **`Settle{consumed=false}`** | 不动点,零 trace | → `Queued(e,t)`,`admit … ticket=t episode=e grounds=<…>`;**toast 门求值** | 不动点,零 trace | 不动点,**不拿号**,零 trace |
| **`Settle{consumed=true}`** | 不动点,零 trace | 不动点,`refuse … reason=watched` | 不动点,零 trace(已在队列里,你正看着它不改变它的号) | 不动点,零 trace |
| **`Answer{by}`** | 无 episode 可确认,零 trace | 两水位追平 ⇒ → `Acknowledged(e)`,`answer …`(**不带 ticket**) | 两水位追平 ⇒ → `Acknowledged(e)`,`answer … ticket=t`,**退号** | 幂等(水位已追平),零 trace |
| **`LeafGone`** | 零 trace | `drop … episode=e reason=leaf-gone` | `expire … ticket=t episode=e reason=leaf-gone`;号消失,`next_ticket` **不回退** | `drop … reason=leaf-gone` |
| **`mark_seen`** | 不变 | 不变 | 不变 | 不变 |

**这张网格与 §10.2.3 那十七条边的关系**:e1 → 第 1 行第 1 格;e2/e3 → `Idle` 那一列的幂等格;e4 → `Settle{false}` 那一行(**但 toast 门已按 §11.2 搬走**);e5 → `refuse`;e6/e11/e13 → 三个「幂等零 trace」格(**e13 收窄为「同一 generation 的重申」**);e7/e10/e14 → `WeakNo` 那一行(**按 §11.1.5 分成 `clear` / `withdraw` / `drop` 三种行**);e8/e9 → `Answer` 那一行;e12/e15 → `Settle` 两行的不动点格;e16 → `LeafGone` 那一行;e17 → `mark_seen` 那一行。**新增的是 `StrongWait` / `StrongClear` 两整行** —— 它们正是 v2 没有的那半台机器。

**零 trace 的边是一张封闭名单**(接 §10.2.6 那条钉):两个源的幂等重申、`Settle` 对 `Queued`/`Acknowledged`/`Idle`、`Answer` 对 `Acknowledged`/`Idle`、`mark_seen`、`LeafGone` 对 `Idle`。**一个每秒重申 `yes` 或每秒重发同 key `wait` 的程序,一行 trace 都写不出来** —— 因为它一个决定都没改变。

#### 11.1.5 `BT_ATTENTION_TRACE` 的字段(替换 §10.2.6,闭合死角 4)

死角 4 说的是:`Requested(e)` 与 `Acknowledged(e)` **没有 ticket**,却被 e7/e14 描述成 `withdraw`,而 `withdraw` 行强制 `ticket=<t>`;`Idle` **没有 episode**,却被 e16 要求写 `expire`。**正解是按「有没有号」把动词分开**,产品语义一个字不变:

```text
mint      tab=<i> seat=<s> episode=<e> src=<osc|pipe> gen=<g> grounds=<requested|awaiting> prev=<e|->
admit     tab=<i> seat=<s> ticket=<t> episode=<e> grounds=<requested|awaiting> active=<0|1> focused=<0|1>
refuse    tab=<i> seat=<s> episode=<e> reason=watched active=1 focused=1
upgrade   tab=<i> seat=<s> [ticket=<t>] episode=<e> grounds=awaiting  src=pipe gen=<g>
downgrade tab=<i> seat=<s> [ticket=<t>] episode=<e> grounds=requested src=pipe reason=<clear|ttl|session-end|overflow>
toast     tab=<i> seat=<s> ticket=<t> episode=<e> reach=<nothing|flash|toast> why=<awaiting|turn-end>
look      tab=<i> seat=<s> ticket=<t> episode=<e> kept=1
answer    tab=<i> seat=<s> [ticket=<t>] episode=<e> by=<keyboard|ime|paste|files-row|mouse-button|mouse-wheel> weak=<w> strong=<g>
clear     tab=<i> seat=<s> episode=<e> src=<osc|pipe> gen=<g> reason=<program|hook|ttl|session-end|overflow>
withdraw  tab=<i> seat=<s> ticket=<t> episode=<e> reason=program src=<osc|pipe>
expire    tab=<i> seat=<s> ticket=<t> episode=<e> reason=leaf-gone
drop      tab=<i> seat=<s> episode=<e> reason=<leaf-gone|session-end>
claim     tab=<i> was=<C> now=<C>
```

三条字段合同(**它们让上面那张网格与这张 schema 逐格自洽**):

- **`ticket=` 只在真有号的时候出现。** `admit` / `look` / `withdraw` / `expire` / `toast` **必带**;`upgrade` / `downgrade` / `answer` **选带**(在 `Requested` 上发生时不带)。
- **`withdraw` 与 `expire` 只属于 `Queued`。** 无号态的销毁写 `drop`(带 episode、不带 ticket);单个源被撤但 episode 还活着写 `clear`。`Idle` 上的任何事件**一行都不写**。
- **`episode=` 在有 episode 的每一行都必须出现** —— 这是「带 episode 之后,每一行都要能回答『这是哪一次请求』」那条要求的执行面(§10.2.6 原话)。`by=` 的取值是 §11.3 的七值**去掉 `mouse-motion`**:那个值**永远不会出现在 `answer` 行上**,因为它结构上到不了这扇门。

#### 11.1.6 不变量与钉

| # | 不变量 | 它挡住什么 |
|---|---|---|
| **I1** | **只有「`asking` 由假变真」铸 episode**,`toasted` 随之置 false | 「用户答了但程序没撤」= `Acknowledged`,而 `Acknowledged` 对 `Settle` 是不动点(§10.2.4 第 1 条原样) |
| **I2** | **两条水位单调不减,两个 generation 单调递增、永不复用** | 旧 ack 吞不住新 generation(**双源各一条钉**);同一条凭据的重申吞不掉自己 |
| **I3** | **`ticket.is_some() ⇒ asking`** | 号不会活得比「有未答凭据」更久 —— 这正是 2026-08-21 那次「badge 活得比它报告的东西更久」的一般化,写成了一条能被断言的不等式 |
| **I4** | **状态不另存**:四态是 §11.1.2 五个派生量的纯函数 | 两个源不会各存一份互相打架的电平;单测可以直接构造字段组合去断言状态,不必按边走一遍 |
| **I5** | **事件级凭据不出现在本节任何一处**(红线 14) | A7 之后仍然没有第八个视觉断言,也没有「一声铃发一张号」的回潮 |

**A-core 的红形(片单里逐条跑)**:①§11.1.4 网格**每一格**一条单测(不可达格写成不可达断言);②窄复核反例①(弱 `yes` → 答 → 强 `wait` ⇒ **第二个 episode、能再入队**);③窄复核反例②(弱 + 强 → 强 `clear` ⇒ **文案降级、号不退、无第二次 toast**);④I3 的属性测试:随机事件序列跑一万步,`ticket.is_some() && !asking` 从不出现。

---

### 11.2 【闭合②】强凭据后到的那一次 toast

**病在哪**:§10.2.3 把 toast 钉在 e4(admit)这一条边上,而 §10.8.2 又要求强凭据后到时「不重发号」。于是**弱凭据先入队(按规则不 toast)⇒ 六秒后 `permission_prompt` 把 `grounds` 升成 `AwaitingInput` ⇒ 这一次真正的 waiting 永远没有 toast**。这是漏,不是重复。

**唯一的 toast 门(替换「e4 是唯一投递边沿」;红线 12 盯着它只有这一处)**:

```text
// 每次状态转移结束后求值一次 —— 它是一个电平的门,不是某一条边的副作用
if queued && grounds == AwaitingInput && !toasted {
    投一次(经 §11.7 的同一个 `desktop_reach` 三档);
    toasted = true;
    trace: toast … ticket=t episode=e reach=<…> why=awaiting
}
```

**它能且只能在两个地方发生**:

1. **admit 的那一刻**(§11.1.4 `Settle{false}` 行):这个 episode 入队时 `grounds` 已经是 `AwaitingInput` —— 就是 v2 的老路子,行为一个字不变;
2. **queued 中的首次强升级**(`StrongWait{新 key}` 行):**这就是窄复核要补的那条边**。ticket 不重铸、不重排(先到先服务不因证据变强而被打乱),只补那一次桌面打扰。

**「不重复」的三条来源**:①`toasted` 与 `episode` 同生共死,一个 episode 最多一次;②同一 episode 内 `grounds` 反复升降不清 `toasted`(§11.1.4 `StrongClear` 格已写死);③强层重复 key 是幂等边,连 trace 都不写。

**「不漏」的准确陈述**:**每个 episode 只要曾经同时满足「持号」与「`grounds==AwaitingInput`」,就恰好投一次**;反之,一生只有弱凭据的 episode、以及一生都被 `refuse=watched` 挡在队列外的 episode,**一次也不投** —— 这正是 §10.8.2 那张合成表要的,现在它有了可施工的门。

**两个刻意的「不投」,写出来免得被当成 bug**:

- **被 `refuse=watched` 的 episode 即使升到 awaiting 也不投**:你正在看着它。它入队的那一刻(你切走了)门自然重算,那时才投 —— **一次,不补前面那次**(红线 5「不排队不补发」)。
- **同一 episode 内第二次强升级不投**:弱层一直举着、两次权限请求之间你一次都没答 ⇒ `asking` 从未落下 ⇒ 仍是同一个 episode ⇒ 只有一次桌面打扰。**你若答了任何一次,`asking` 就落下过**,下一次强凭据会铸新 episode、拿新的 `toasted`,于是照投(§11.4.3 的走查逐步演示了这一点)。

---

### 11.3 【闭合③】鼠标三分:按下/抬起算,滚轮算,motion 不算

**病在哪**:§10.3.2 把「被转发的鼠标」写成一格,并把它挂在 `send_mouse_input_to` 上。实测复核(2026-08-25)证明那一格的**定性与位置都不准**,而不准的代价是一个**只在特定程序上、只在鼠标经过时**发作的退号 bug。

**两个 helper 各自承载什么(逐调用点闭合,行号 2026-08-25 实测)**:

| helper | 调用点 | 承载 | 结论 |
|---|---|---|---|
| `send_user_input`(`55459`) | `63061` | 键盘按键 | `Keyboard` —— **算** |
| 同上 | `60529` | **被转发的鼠标按键**(`UserInputKind::Mouse`) | `MouseButton` —— **算** |
| `send_mouse_input_to`(`55438`) | `62142` | SGR 滚轮(线序上是 `MouseProtocolEvent::Press`,button 64/65) | `MouseWheel` —— **算** |
| 同上 | `62161` | 备用屏滚轮转方向键 | `MouseWheel` —— **算** |
| 同上 | `56298` | **SGR mouse motion**(`MouseProtocolEvent::Motion`) | `MouseMotion` —— **不算** |

**裁决面**:A3(2026-08-25 用户裁)「出队门扩,鼠标**算**」原样成立;**终修单把它收窄到「只有按下/抬起算用户动作,motion 不算」**。滚轮留在「算」这一侧是**线序上的同一件事**:一次滚轮就是一次 `Press`(button 64/65,`main.rs:62134` 写着 `MouseProtocolEvent::Press`),**没有独立的抬起** —— 「按下/抬起」在字节层面本来就覆盖它。**motion 才是被拆出去的那个**:悬停扫过不是回答,一个开了 `?1002h`/`?1003h` 的程序不该因为指针路过就被退号。
**若用户的本意是连滚轮一起撤出**,改动是**一格**:把上表两行 `MouseWheel` 改成「不算」,`is_answer()` 多一个 false,施工面反而更小(见下)。

**施工点(这一条比定性更重要)**:

- **不许在两个 write helper 上统一消费。** 两个 helper 各自承载不止一种语义(上表),在它们身上做判断,等于把一个语义问题交给一个只看得见字节的函数 —— 与 §10.3.2 引的那句现成理由同型:「`send_user_input` is handed a byte string and cannot tell an answer from a paste that happens to end in a newline」。
- **施工点在产生语义的五个事件入口**:`63061`(键盘)、`60529`(鼠标按键)、`63248`(IME commit)、`63111`(paste)、`49768`(files 行),各自带着自己的 `UserInputKind` 调 `answer_attention(seat, by)`。
- **`56298` / `62142` / `62161` 三个入口的落法**:motion 那个入口**永不调** `answer_attention`;滚轮两个入口**在自己的事件入口上显式调**,而不是搭 motion 的便车 —— 因为它们共用同一个 helper,而**共用 helper 不等于共用语义**。这也是「若要把滚轮撤出 Answer 只改一格」的原因:撤出后那条 helper 整条不碰 `answer_attention`。
- **枚举上必须有 `is_answer()`**(§10.3.2 就地改已写):`MouseMotion` 为 false,其余为 true。将来新增一条输入路,它得在**枚举上**回答这个问题,而不是在某个 `if` 里被忘掉 —— 与 `returns_view_to_live` 同型、同位置、同理由。

**钉**:程序开 `?1003h`(任意 motion 上报),鼠标在该 pane 上**扫过一整行**,`BT_ATTENTION_TRACE` 里**没有 `answer` 行**;随后**点一下**,有 —— 一条真机门,写进片 A2(§11.9 的 ⑮)。

---

### 11.4 【闭合④】hook wait 的 clear 配对:每一条进,都有一条出

**病在哪**:§10.4.1 把 `wait` 的名单写对了,却没写 `clear` 的配平。后果是可指认的:**批准或拒绝一次权限请求并不会触发 `UserPromptSubmit`**,于是那条强断言永久留着;而用户按键只把 Folio 送进 `Acknowledged`,**撤不掉强层**;下一次 `PermissionRequest` 再来,若被当成同一条凭据,就被吞掉。

#### 11.4.1 那把有界关联键(两个动词之外唯一多出来的东西)

```text
folio attention wait  --key=<opaque>            # key 未见过 ⇒ 铸一个强 generation
folio attention clear [--key=<opaque> | --kind=<permission|elicitation|agent|quota> | --all]
```

- **key 从哪来**:hook **自己收到的 payload** 里的标识(权限请求的 tool_use / request 标识、elicitation 的 id、background session 的 id)。取不到就用该 `kind` 的固定兜底键。
- **key 是什么**:一个**有界的、不具寻址能力的关联键**。它**只能在本端点内**指认一条**自己发出的** wait;端点在创建时就绑死了一个 `LeafSession`(§10.6 第 1/4 条),所以 key **选不了 pane、跨不了 pane**。**红线 13** 盯着这一条。
- **合同**:≤ 64 字节,`[A-Za-z0-9._:-]`;每 pane 最多 **8** 条 outstanding,超限**丢最旧**并写 `clear … reason=overflow`;**key 不进 UI 文案、不进 trace**(它可能带着工具名或路径),trace 只记 `gen=<g>`。
- **它没有把动词变成三个**:`wait` / `clear` 仍然是**幂等**的两个 —— 同 key 重发 `wait` 零效果零 trace,`clear` 一条不存在的 key 也零效果零 trace(§10.6 第 3 条原样成立)。

#### 11.4.2 逐源的 clear 配对表(**每一种进 wait 的事件都在这张表里有出口**)

| `wait` 源 | key | 正常出口 | 兜底一 | 兜底二 | 最终兜底 |
|---|---|---|---|---|---|
| **`PermissionRequest`**(事件,0s) | payload 的 tool_use / request 标识;取不到 ⇒ `kind=permission` | **`PostToolUse`** 同 key ——「批准了、而且那个工具已经跑完」的回执(**开工前须逐字取回原文**,§10.4.1 就地记) | **`UserPromptSubmit`** ⇒ `--all`(你已经回话了,那就什么都不在等你) | **`Stop` / `StopFailure`** ⇒ `--all`(回合结束 ⇒ 这个 pane 没有任何东西在等你输入) | **TTL 600s**(与官方 `PermissionRequest` 的同步超时同值)⇒ `clear … reason=ttl` |
| `Notification` **`permission_prompt`**(~6s 兜底) | 同上(同 key ⇒ 对上面那条是**幂等重申**,零 trace) | 同上 | 同上 | 同上 | 同上 |
| **`Elicitation`**(事件,0s) | payload 的 elicitation id | **`ElicitationResult`** 同 key | `elicitation_complete` / `elicitation_response`(**没有 id 时** ⇒ `--kind=elicitation`) | `UserPromptSubmit` / `Stop` / `StopFailure` ⇒ `--all` | TTL 600s |
| `Notification` **`elicitation_dialog`** / **`elicitation_url_dialog`**(~6s 兜底) | 同上 | 同上 | 同上 | 同上 | 同上 |
| `Notification` **`agent_needs_input`** | payload 的 background session id | **`agent_completed`** 同 id | `UserPromptSubmit` ⇒ `--all` | `SessionEnd` ⇒ `--all` | TTL 600s |
| `Notification` **`quota_auto_resume_stale`** | `kind=quota` | **`quota_auto_resume_fired`**(自动继续了)/ **`quota_auto_resume_disabled`**(结束等待) | `UserPromptSubmit` ⇒ `--all` | `SessionEnd` ⇒ `--all` | TTL 600s |

**普适出口(与 key 无关,三条)**:`SessionEnd` ⇒ `--all` + `drop`;**leaf 消失** ⇒ 端点关闭、旧 capability 永久失效(§10.6 第 4 条,与 §11.1.4 的 `LeafGone` 同一时刻);**TTL** ⇒ 逐条到期。

**一句话把 §10.4.1 第三张名单救回来**:**「绝不映射为 `wait`」不等于「不进本体系」。** `Stop` / `StopFailure` / `PostToolUse` / `agent_completed` / `quota_auto_resume_fired` / `quota_auto_resume_disabled` 现在**都是 clear 源**,而它们**一条都不会**把 pane 送进 `Awaiting` —— 这两件事从来不矛盾,v2 只是没把后半句写出来。

#### 11.4.3 连续两次权限请求的走查(窄复核点名要能连续工作的那件事)

```text
t0  PermissionRequest(key=A)  wait  → 强 gen 1 > acked_strong 0 ⇒ asking↑
                                     mint     episode=7 src=pipe gen=1 grounds=awaiting prev=-
t1  settle{consumed=false}           admit    ticket=12 episode=7 grounds=awaiting active=0 focused=0
                                     toast 门:queued ∧ awaiting ∧ !toasted ⇒ 投一次
                                     toast    ticket=12 episode=7 reach=flash why=awaiting
t2  你按 `2` 拒绝                     两水位追平(weak=0 strong=1)⇒ asking↓ ⇒ 退号
                                     answer   ticket=12 episode=7 by=keyboard weak=0 strong=1
                                     → Acknowledged(7);强层的 A 还挂着,但 1 ≤ 1 ⇒ 不再 asking(**正确:你答过了**)
t3  拒绝没有 PostToolUse               什么都不发生(这正是 v2 卡住的那一格)
t4  Stop(回合结束)                   clear --all → clear episode=7 src=pipe gen=1 reason=hook
                                     两源皆空 ⇒ → Idle;drop episode=7 …
                                     并按 A8 投一次回合结束通知(§11.7,why=turn-end)
t5  PermissionRequest(key=B)  wait  → 强 gen 2 > acked_strong 1 ⇒ asking↑
                                     mint     episode=8 src=pipe gen=2 grounds=awaiting prev=7
t6  settle{consumed=false}           admit    ticket=13 episode=8;toast 再投一次(**新 episode,新 toasted**)
```

**注意 t5 不依赖 t4。** 就算 `Stop` 没来、A 那条 wait 还挂着,`gen 2 > acked_strong 1` 也让 `asking` 重新为真 —— **水位这一条本身就够了**。clear 表与 TTL 要的是**卫生**(outstanding 不无界增长、UI 不长期显示一条已经不存在的等待),**不是正确性的支柱**。这两件事要分清楚,因为它们的失效模式不同:**水位错了会吞请求,clear 缺了只会留脏账。**

---

### 11.5 【闭合⑤】「管道唯一路线」收窄,与它对依赖表的真实影响

**准确的一句(替换 v2 的「路线 A 是 Claude Code 场景下的唯一可行路」)**:

> **对 Claude Code 的强层,pane-local attention pipe 是本方案已定义的唯一生产路线;对通用层,`OSC 1337;RequestAttention` 仍是正门。**

**官方白名单证明的是一件很窄的事**:`terminalSequence` 这条**代发**通道不许带 OSC 1337 —— **被堵死的是「hook 代发 1337」,不是「1337」**。它**不证明**三件事:

1. **`CONOUT$` 直写失败。** 白名单一个字都没提 `CONOUT$`。片 A0 的①②是**因为产品选路而撤销**(官方给了一条它自己维护的、race-free 的通道,我们不再走自己造的直写 tty),**不是因为实验证否**(§10.4.2 第 1 条已就地改)。
2. **别的 IPC 不可行。** loopback socket、broker、继承 handle 都没被这条事实排除;本方案选命名管道,是**安全合同**(§10.6)与**生命周期**(端点随 leaf 生灭)两条理由挑的,不是被逼到只剩一条路。
3. **别家 CLI 不能自己发 1337。** 任何自己持有 tty 的程序照发不误 —— iTerm2 生态的现成脚本、构建工具、用户自己的脚本。**片 A1 服务的正是它们**,这也是 A1 不该被 A3 取代的理由(§10.4.2 末段原样成立)。

**成立范围写进方案**:当前上游版本 + 本方案既定的安全边界 + 本方案已选的路线。上游哪天把 OSC 1337 加进白名单(**路线 C 的诉求,记账不排期**),这句话自然失效 —— 而**A1 的解析器当天就能接住,一行不用改**。

**对依赖的正确影响(三条,与窄复核逐条对齐)**:

- **A3 必须在 A2 之前或与 A2 原子落地** —— **成立,不动**。没有 A3,BEL 撤出后 claude pane 的 awaiting 永远是零,队列是空的。
- **A3 不依赖 A1** —— **v2 的片表写错了**。A1 是通用程序的 OSC 弱源,A3 是 Claude hook 的 pipe 强源,**两个源并行喂同一个双源核心**。正解是先抽 **A-core**(§11.1),然后 **A1 ∥ A3**,A2 依赖 A-core + A3(两道用户门已裁)。若排期上仍想先把 A1 做完,那是**打包顺序,不是技术依赖**,片单里要这么写。
- **C3 继续依赖 A2 + A3 + C2** —— **成立,不动**;v3 只把它的 toast 判断下沉到 §11.2 那一处门(红线 12)。

---

### 11.6 【A7】`OSC 9` / `OSC 777` 进凭据体系:三级,不是三堆

**裁决(2026-08-25 用户裁)**:把 `OSC 9` 与 `OSC 777` 的**通知序列**收进凭据体系。**它改的是名分,不是行为的方向** —— 这两条今天已经在跑既有的 toast 通道(`main.rs:52596` 那个唯一投递点),但它们**在方案的语言里没有位置**:红线 9 只说「它们是消息、不合并」,于是每次讨论「什么算凭据」它们都得被单独拎出来说一遍。A7 给它们一级。

**三级凭据表(红线 9 的执行面)**:

| 级 | 名字 | 生产者 | 可撤? | 铸 episode? | 发号? | 够到桌面的路 |
|---|---|---|---|---|---|---|
| **事件级** | **`Announced`** | 裸 BEL、`RequestAttention=once`、**`OSC 9;<text>`**、**`OSC 777;notify`**、`OSC 99`(kitty)、**回合结束**(hook `Stop`/`StopFailure`) | **否**(一次性,没有「关」) | **否** | **否** | 既有 toast 通道 + **A8 的两条臂**(§11.7) |
| **弱** | **`Requested`** | `OSC 1337;RequestAttention=yes`(任何自己写 tty 的程序) | 是(`=no`) | **是** | **是** | **不配 toast**(§10.8.2) |
| **强** | **`AwaitingInput`** | `folio attention wait`(§10.4.1 白名单里的 hook) | 是(`clear`,§11.4) | **是** | **是** | **配 toast**(§11.2) |

**三条规矩**:

1. **严格弱于**:`Announced` **不能升 `grounds`、不能铸 episode、不能延长任何 episode 的寿命**。一条 `OSC 9` 不会把 pane 送进队列,也不会让点变成 `Awaiting`(红线 14)。它今天做的三件事一件不多一件不少:置 `bell_latched`(`once` 与裸 BEL 那条路)、走既有 toast 通道(它有正文)、按 A8 够到桌面。
2. **可以借正文,不能借状态**:一条带正文的 `Announced`(`OSC 9;<text>` / `777` / `99`)到达时,若该 leaf **此刻有 live episode 且 toast 门正开着**,那次 toast 的正文**用它的文字** —— **程序自己的话优先于我们编的话**。若那个 episode **已经 toast 过**,它**不新投**(红线 5)。它永远不改 `grounds`、不改号。
3. **`OSC 9;4` 不在这张表里。** 那是**进度环**通道(`bt-term/src/inline_image.rs:1294-1308` → `parse_progress`),§1.2 已立 —— 同一个 OSC 号下的两件事,别混。

**为什么这不是「把消息升成状态」**(红线 9 的原意仍在):三级表**没有让任何一级变成另一级**。它做的是**把体系的定义域补全** —— 以前的表只有「状态」这一栏,于是事件只能站在表外;现在事件有了自己那一行,而**那一行的「铸 episode」「发号」两格都是「否」**。**一个凭据在表里有名分,和它有权力,是两件事。**

**钉(A1/A3 各带一条)**:①一条 `OSC 777;notify` 进来 ⇒ 既有 toast 照投,而**队列长度不变、`StatusClaim` 不变成 `Awaiting`**;②一条 `OSC 9;<text>` 在一个已有 live episode 且**尚未 toast** 的 pane 上进来 ⇒ 那次 toast 的正文是它的文字,**且只有一次**。

---

### 11.7 【A8】回合结束够到桌面:无焦闪、最小化 toast

**裁决(2026-08-25 用户裁)**:**回合结束 = 无焦任务栏闪(默认开)+ 最小化 toast(设置行默认开可关)。**

**它挂在三档表的哪一格**:**不新造判据**,走 §10.7 那个 8 行真值表的**同一个** `desktop_reach`:

| 真值表行 | 情形 | 回合结束时 |
|---|---|---|
| 第 1 行(有焦 + 活动 tab) | 你正看着它 | **`Nothing`** —— 你看得见,不打扰 |
| 第 2/3/4 行 | 有焦但非活动 tab / 副屏可见但无焦 / 都不是 | **`Flash`** —— 任务栏闪。**第 2 行照 §10.7 那条诚实说明**:前台窗口上 `FlashWindowEx` 是 no-op,可见产物是窗内既有的那两枚记号(`Bell` / `Unread`) |
| 第 5–8 行(`window_is_hidden`) | 最小化 / cloaked | **`Toast`** |

**它一张号都不发**(红线 14):回合结束是**事件级凭据**(§11.6),`Stop` 在 §10.4.1 里**不映射为 `wait`** 这条一个字不动 —— **把回合结束升成「正在等你回答」正是 §2 花四段录音证伪的那件事**。A8 给它的是**够到桌面的一条腿**,不是队列里的一张号。

**事实源有两条,都归 `Announced`**:

- **装了 hook 的 Claude Code**:强层管道的 `Stop` / `StopFailure`(它同时是 §11.4 的 clear 源 —— 一个事件两个身份,互不干扰);
- **没装 hook、但开了 `preferredNotifChannel: terminal_bell`**:那一枚**裸 BEL**(§2 已把它钉死在回合末尾,而 A0.5 之后它才真的会在你走开时响)。

**去重(两条证据会同时到,而它们说的是同一件事)**:**同一 leaf 上,两次回合结束通知之间必须隔着一次 `UserPromptSubmit` 或一次用户动作。** 没隔着就不投第二次。
**为什么不用毫秒窗口**:一个「1.5 秒内算同一次」的常数会在慢机器上漏、在快机器上误合,而**「你有没有开始下一个回合」这件事我们本来就知道** —— 用知道的事实,不用猜的阈值。这与 §3.4 拒绝「输出安静 N 秒」是同一条纪律。

**设置行**:`turn_end_notification`,**默认 On**,位置在设置块通知那一节,与 A5 的「Claude Code bell」那一行并排。

- **On**:上表两条臂都走。
- **Off**:两条臂都不走(既不闪也不 toast)。**窗内记号不受它影响** —— 那是 `Bell` / `Unread` 两枚点自己的事;这一行管的是**够不够得到桌面**,与红线 4 的分工一致。
- 裁决原话把两条臂分开写(「闪默认开」「toast 默认开可关」),**本方案用一行同时管两条**:因为「回合结束要不要够到桌面」是一句话,拆成两个开关会让「闪着但不 toast」和「toast 但不闪」这两个没人要的组合合法化。**若用户要两个开关,这一条改起来是加一行、不动判据。**
- i18n 两条串。

**与队列那条道的分界(写进 C3 的片单)**:两条道**共用** `Notifier` 与 `NotificationRoute`(点击回原 pane,一行不改),**都不排队不补发**(红线 5),trace 上靠 **`why=<awaiting|turn-end>`** 分辨。**队列那条道**由 §11.2 那一处门管(`grounds==AwaitingInput`);**事件那条道**由本节管。**两条道永远不互相触发**:一次回合结束不会让号变多,一张号也不会被算成一次回合结束。

**文案**:标题走既有 `toast_title` 的三层链(carried → pane 名 → profile 名);正文 =「回合结束」/ "Turn finished"。若同一刻有 §11.6 第 2 条那样的带正文 `Announced`,**用程序自己的话**。

---

### 11.8 片单与依赖(v3,替换 §10.11 的表)

**生产片十个**(A0 是 spike,D 已出块;新增 **A-core**):

| 序 | 片 | 前置 | 相对 v2 的变更 | 门 |
|---|---|---|---|---|
| 0 | **A0**(spike) | 无 | 不变(三问缩成一问,只剩 ③′);撤销理由改成**产品选路**(§11.5) | §10.5 的正门 + 副门 + 三个补测点 |
| 1 | **A0.5** | A0 ③′ | 不变 | 真机四格:out / in / 后台出生 / 普通 shell 不出乱码 |
| 2 | **A-core**(**新**) | 无(**可与 A0/A0.5 并行**) | **v3 新片**:§11.1 的双源账本 —— 两个 generation、两条 ack 水位、派生态、§11.1.5 的 trace schema、§11.2 的 toast 门。**不碰 UI、不改入队门、不接任何生产者** | §11.1.6 的四条红形:网格每格一条单测、两个反例各一条、I3 的属性测试 |
| 3 | **A1** | A-core(**与 A3 并行**) | 前置从「无」改成 A-core;内容不变(parser + 弱层 generation,不落 UI、不命名 Awaiting;`1337;` 要先在共同前缀后分流) | 红形 + 五个钉(BEL/ST 两种收尾、跨 chunk 重组、超限、未知 value、`fireworks` 原样忽略)+ **A7 的两条钉**(§11.6) |
| 4 | **A3** | A-core + §10.6 安全合同(**与 A1 并行**,**不依赖 A1**) | **前置去掉 A1**(§11.5);hook 按 §10.4.1 白名单 + `async: true` + **§11.4 的 clear 配对表与关联键**;安装提示照 §7.1.6j;**开工前逐字取回 `PostToolUse` 原文** | 真 claude + 真 Folio 人工门:权限提问出现 ⇒ 队列长一位;**外加 §11.4.3 的连续两次权限请求走查** |
| 5 | **A2** | **A-core + A3**(A2/A3 两道用户门**已裁**;A1 为打包顺序不是技术依赖) | 一次做完 admit / ack / withdraw / leaf-gone(**按 §11.1.4 的网格,不是 §10.2.3 的十七边**);trace 长 episode / grounds / src(§11.1.5);**`UserInputKind` 拆七值 + `is_answer()`**(§11.3);同时改 `attention_ticket` 与 `mark_seen` 两处注释 | `BT_ATTENTION_TRACE` 端到端;截图那个场景先变红再变绿;**§11.9 的 ⑪⑫⑬⑮⑱** |
| 6 | **B1** | A2 + A3 | 不变 | 命中表断言,不是矩形断言 |
| 7 | **C1** | 无(**与 A 线并行**) | 不变 | 事实源的钉 |
| 8 | **C2** | C1 + §10.7 | 8 行真值表;`FlashWindowEx`;`notify.rs:93-101` 注释重写;`attention_is_consumed` 保持两参;**外加 A8 的两条臂与设置行**(§11.7) | 真值表 8 行 + **A8 的三格**(有焦 / 无焦 / 最小化) |
| 9 | **C3** | A-core + A2 + A3 + C2 | toast 判断**下沉到 §11.2 那一处门**(红线 12),C3 自己不再判;`why=` 两值 | 「持续 `yes` 不连发 toast」+ **「后到的强凭据补投恰好一次」**(§11.9 ⑬) |
| 10 | **B2** | 命令面板本体 | 不变(解耦,不阻塞主链) | — |
| — | ~~D~~ | — | 已拆出本方案;**A1 裁「做」** ⇒ `Attached` 成为 §7.1.5b 上一条独立扩条,不占本块工期 | — |

**关键路径**:

```text
A0 ──▶ A0.5

A-core ──┬──▶ A1 ──┐
         │         ├──▶ A2 ──▶ B1
         └──▶ A3 ──┘

C1 ──▶ C2

A-core + A2 + A3 + C2 ──▶ C3
B2 ──▶ 独立等命令面板
```

**这样排掉的三个中间坏版本**:①「假队列已拆、真生产者尚未装」(A2 先于 A3);②「持续 `yes` 每帧 toast」(C3 挂在电平而不是边沿上);③**「两个生产者各写各的电平」**(A1 与 A3 各自长一台半截状态机 —— A-core 先落地就是为了挡这一个)。

**并行性**:**A0/A0.5、A-core、C1 三条线开工时互不依赖**;A-core 落地后 A1 与 A3 再开两条。

---

### 11.9 验收门增补(接 §8 的八步与 §10.14 的 ⑧⑨⑩)

- **⑪ 双源合成**:弱层发 `RequestAttention=yes` ⇒ 徽章 +1(文案「需要注意」);在那个 pane 里按一个键回答 ⇒ 点灭;**随后 hook 报一次真的权限请求** ⇒ **徽章重新 +1、trace 里有第二条 `mint`(`prev=` 指着前一个 episode)**。这一步是 v2 单源模型必挂、v3 必过的那一格(窄复核反例①)。
- **⑫ 强撤弱留**:同一 pane 先弱 `yes` 再 hook `wait` ⇒ 文案「正在等你回答」、**一次** toast;随后 hook `clear`、弱层**不撤** ⇒ 文案退回「需要注意」、**号不退**、**没有第二次 toast**(窄复核反例②)。
- **⑬ 后到的强凭据补投一次**:弱 `yes` 先入队(**无 toast**)⇒ 六秒后 `permission_prompt` 到 ⇒ **恰好一次** toast,且 **ticket 号不变**(§11.2)。
- **⑭ 连续两次权限请求**:批准第一次(`PostToolUse` 到)、拒绝第二次 ⇒ **两个 episode、两张号、两次 toast**;拒绝那次即使没有 `PostToolUse`,`Stop` 或下一次 `wait` 的新 generation 也能让它继续工作(§11.4.3 逐行核对)。
- **⑮ motion 不退号**:让 pane 里的程序开 `?1003h`,把鼠标在它上面扫过一整行 ⇒ `BT_ATTENTION_TRACE` 里**没有 `answer` 行**;再点一下 ⇒ **有一行 `answer … by=mouse-button`**(§11.3)。
- **⑯ A8 三格**:回合结束时 —— 窗口有焦 + 活动 tab ⇒ **什么都不加**;窗口在副屏无焦 ⇒ **任务栏闪、不 toast**;窗口最小化 ⇒ **toast**;把 `turn_end_notification` 关掉重跑三格 ⇒ **三格都不够到桌面,窗内记号照旧**(§11.7)。
- **⑰ A7 不越权**:发一条 `OSC 777;notify` ⇒ 既有 toast 照投,而**队列长度不变、点不变成 `Awaiting`**;在一个已有 live episode 且尚未 toast 的 pane 上发一条 `OSC 9;<text>` ⇒ 那次 toast 用**它的正文**,且**只有一次**(§11.6)。
- **⑱ trace 字段合同**:跑完整个端到端之后扫一遍 trace —— **`withdraw` / `expire` 行必带 `ticket=`;`drop` / `clear` / `mint` 行必不带**;每一行都有 `episode=`;**没有一行 `by=mouse-motion`**(§11.1.5)。
