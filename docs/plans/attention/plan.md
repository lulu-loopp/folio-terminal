# 通知与注意力块方案 v1(2026-08-25 起草,送 Codex 评审;不写产品代码)

**由头**:用户把这一块提为当前最重要的块之一,原话「目前 claude code 的通知提示还是存在问题的」,并给了一张实机截图(竖栏 Icons 态,三枚 tab 图标,**前两枚右上角带橙点,而第一枚正是活动 tab**)。

本方案做四件事:①把「还是存在问题」拆成**可指认、带代码证据**的缺陷清单;②用**真 claude 进程的真字节流**回答「Claude Code 到底发什么」这个到今天为止只有散文没有数据的问题;③给 `awaiting` 一个诚实的信号源;④把 P1-8 注意力队列与 §7.6 桌面通知这两条早就各自有一半的线接起来。

**上位法**:DESIGN §7.1.5b(会话状态分类学 + P1-8,用户裁决 2026-07-18,三轮勘误 + 2026-08-21 入队门修正)、§7.6(桌面通知)、UI-UX §8(「等你」这一态的诚实处理)、PROBLEM-LIST P1-2 / P1-4 / P1-8 / P3-4 / P3-10、HANDOFF-2026-08-21 §4 g(提示三档,用户明说要先专题讨论)与 §5 第 5 条(`awaiting` 真信号源)与 §7 第 7 条(备用屏呼吸关掉后 tab 什么都不显示)。**本方案不推翻其中任何一条裁决**;要用户拍板的一共四处(Q1/Q2/Q4/Q5),全部集中在 §9,各自带推荐。

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
- **粒度是 pane 不是窗口**,理由在 D10:一个三座位 tab 里只有一个 pane 握着键盘,另外两个 agent 应当知道自己没有被看着。判据 = `窗口有焦点 && 这个 leaf 是 focused_leaf && 它的 tab 是活动 tab`;这三个位今天全都现成(`window_focused` / `focused_leaf` / `active_tab`)。
- **边沿而非电平**:只在答案**变**的那一帧发一次,不每帧发 —— 与 `BT_ATTENTION_TRACE`「按决定写行」同一条纪律。
- **协议前置已经备好**:§7.1.5i 把 `1004` 撤出了提示符归零名单,理由原文就是「焦点上报一旦实现,在提示符上关掉 ConPTY 要的那个 1004 就成了活的错误」。**那一天就是这一片。**
- **要量的两件**(spike,与 A0 同批):① ConPTY 会不会把 `\e[I`/`\e[O` 吃掉翻成 `FOCUS_EVENT`,而 Claude Code 读的是 VT 流不是 `ReadConsoleInput` —— 若被吃掉,这一片当场作废,方案落到 §3.2;② 一个不认识这两串的程序收到它们会不会把它们当字打出来(理论上不会:只有 `?1004h` 的程序才会收到,而收到就等于它要了)。
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
- `SessionStatus` 长一位 `attention_requested: bool`,由 `RequestAttention=yes` 置、`=no` 清;**与 `bell_latched` 并排,互不覆盖**。`=once` 走既有 BEL 那条路(置 `bell_latched`),一个字不新写。
- 与备用屏的关系:**照 §7.1.5b 已有的不对称** —— 环与点是全屏程序**故意**要够到的通道,一律照收;呼吸的豁免不动。
- 与 `TerminalNotification`(OSC 9 / 777)的关系:**两件事,别合**。通知是一条**消息**,`bt-term/src/session.rs:712-718` 早就写明它「carries no claim」;`RequestAttention` 是一个**状态**。

### 3.3 谁来发:Claude Code 今天一条都不发,所以要给它一条能发的路

**路线 A(推荐)—— `FOLIO_PANE` + 每 pane 一条命名管道 + `folio attention` 动词。**
这是 P3-4「可编程 API」的最小切片,DESIGN 已经给它留好了位置与三条硬规矩。Folio 给每个 pane 注入 `FOLIO_PANE=<window>.<tab>.<seat>` 与 `FOLIO_SOCK=<pipe 名>`;`folio attention wait|clear` 往管道写一行。Claude Code 侧**不用改一行代码**,靠它自己的 hooks 接上:`Notification` → `wait`、`Stop` → `wait`、`UserPromptSubmit` → `clear`。
- 好处:与 tty 无关(hook 的 stdout 被 Claude Code 收走,这条路绕开了这个问题);地址是我们自己发的,不会被别的程序冒充;将来 P3-11 mailbox 与 P3-4 自动化共用同一条地基。
- 代价:要建那条地基(命名管道生命周期、ACL、pane 消失后的清理),并且要在安全模型上先答一个问题(§7 Q3)。

**路线 B(兜底)—— hook 直写 `CONOUT$`。**
hook 是 Claude Code 起的子进程,与 agent 共用同一个 ConPTY;Windows 上打开 `CONOUT$` 写字节即写进这条 pty。零新地基。
- **必须先 spike**(片 A0):hook 子进程是否真的继承那个伪控制台、`async: true` 的 hook 时序抖动有多大、以及一串 OSC 插进正在画全屏 UI 的程序中间会不会被它自己的重绘吞掉(OSC 不动光标不占格,理论上安全,**但这是要量的不是要论证的**)。
- 红门不过 ⇒ 路线 A 成为唯一。

**路线 C(记账,不排期)** —— 请上游把 `preferredNotifChannel` 扩一档 `terminal_attention` 直接发 1337。写进方案是为了让它在别人问起时有出处,不占本块的工期。

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
| 用户答 | **那个 seat 里的任意用户输入字节**(不再只是 Enter) | 「看一眼不解除阻塞」原样成立 —— 看不产生字节。而「我往它嘴里塞了字节」是「我回答了它」唯一诚实的反面,`1`/`y`/`Esc` 都算 |
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
| 三 | 窗口**不可见** | toast |

**「窗口可见」这个新事实源必须诚实,而 Windows 上能可靠回答的只有一半。**
- 可靠:`IsIconic`(最小化)、`DwmGetWindowAttribute(DWMWA_CLOAKED)`(切到了别的虚拟桌面 / 被 shell 隐藏)。
- **不可靠**:「被另一个窗口完全盖住」。winit 的 `WindowEvent::Occluded` 在 Windows 后端**不发**;自己算要枚举 z 序 + 求区域差,那是启发式,而且每帧都要算。
- **建议**:第三档的判据 = `IsIconic() || DWMWA_CLOAKED`,并把「被完全遮挡」**明说为不做**、归第二档。这样三档全部站在能回答的事实上,代价是「窗口被 IDE 全屏盖住时只闪任务栏不弹 toast」——而任务栏闪在那种情形下是看得见的。→ §7 Q4 留裁决位(用户可能要「被盖住也算不可见」)。

### 5.3 `reaches_the_desktop` 要解耦(HANDOFF 已记这一笔)

今天它是 `enabled && !attention_is_consumed(...)`,而 `attention_is_consumed` 只有两个位。改成一个**新函数** `desktop_reach(enabled, tab_is_active, window_is_focused, window_is_hidden) -> Reach{Nothing, Flash, Toast}`,三档各一条臂。
**红线**:`attention_is_consumed` **一个参数都不许加**。它管的是账本(谁看过了),`desktop_reach` 管的是桌面(够不够得到你),`44f9f2d` 的教训就是这两句话缠在一起会把因果说反。今天 `notify.rs:93-101` 那段「读的是同一个谓词的另一面」的论证在两档模型下是对的,**三档之后它不再对**:第二档「可见但无焦点」既不是「看过了」也不是「够得到桌面」,它是第三个答案。这段注释要跟着改,别留着。

### 5.4 队列喂 toast

**只有真 `awaiting` 信号发的号配 toast。** BEL 不配 —— 它是事件,而事件已经有 `OSC 9` / `OSC 777` 那条正路。toast 文案 = `toast_title` 的三层链(carried → pane 名 → profile 名)+ 一句「正在等你回答」;点击走既有 `NotificationRoute`,**一行不改**。
**不排队、不补发**(`raise_notifications` 那段注释已经写死了理由,原样成立)。

### 5.5 「Claude Code bell」设置行:实测改了它的诊断,没改它的存留

HANDOFF §4 g 写的是写 `~/.claude/settings.json` 的 `preferredNotifChannel=terminal_bell`。**§2 把这一行的诊断改了**:它本身是对的、也确实生效了,**买不到东西的原因在我们这边**(不发焦点上报),而那一半由片 A0.5 免费还回来。
所以这一行**留着不动**,并**加**一条:装 hooks(§3.3 路线 A/B 的那三条)。两者不冲突 —— 铃管「你走开了」,hook 管「我在等你」。
纪律**原样保留**:On 之前存原值、Off 恢复不删、状态从文件读、默认 Off、**问那个程序自己而不是拼路径**(§7.1.6j 的教训,`1070414` 那次现场逼出来的)。→ §7 Q5。

---

## 6. 分片(每片一条线可施工)

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
9. **`RequestAttention` 是状态、`OSC 9`/`777` 是消息,两条通道不合并**(`bt-term/src/session.rs:712-718` 已立)。

---

## 8. 验收门

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

- **Q1(需用户裁)**:第八记号 `Attached` 做不做?做 = 多一个视觉断言但备用屏 tab 不再是一片空白;不做 = D4 完全押在 §3 的信号源上,不装 hook 的人永远看不到东西。**推荐:做。**
- **Q2(需用户裁,本方案的核心)**:BEL 撤出入队门吗?撤 = 截图上那枚活动 tab 的橙点当场消失、D1 三条路径消失,代价是队列在 §3 落地前是空的。**推荐:撤,并与「任意字节即回答」这条一起裁。**
- **Q3(问 Codex)**:路线 A 的命名管道,安全模型怎么写才不违反 P3-4 的三条硬规矩(尤其「永远不能从工作区/仓库读自动化」)?一个 pane 的管道名泄漏出去,别的进程能冒充这个 pane 说「我在等你」——这是不是一个真的威胁,还是「它已经在你的管子里了」?
- **Q4(需用户裁)**:第三档的判据只认 `IsIconic || CLOAKED`,把「被别的窗口完全盖住」归第二档 —— 接受吗?
- **Q5(需用户裁)**:「Claude Code bell」那一行**留着 + 加一条装 hooks** 吗?铃管「你走开了」(A0.5 之后才真的有用),hook 管「我在等你」。**推荐:两条都写,纪律(存原值/恢复不删/问程序自己)原样。**
- **Q6(问 Codex)**:`OSC 1337;RequestAttention` 是否还有比它更合适的既有序列?本方案挑它的理由是「yes/no 是一对、程序能撤」;若有更好的先例请指出来 —— **这一条明确不接受「自己发明一个 OSC」这个答案**。
- **Q7(问 Codex)**:D9(`mark_seen` 整片清 fleet 的铃)在 BEL 撤出队列之后还成不成立?本方案倾向「成立、不动」,但那条注释的论证前提里有「tab 的断言是整个 fleet 的」,而队列的断言是**每个 seat 各自的** —— 两句话在一个 tab 上同时为真,值得一双外面的眼睛。
- **Q8(问 Codex)**:片 A0.5(发焦点上报)的粒度定在 pane,判据 = `窗口有焦点 && 是 focused_leaf && 它的 tab 活动` —— 有没有第四个位该进去(例如聚焦模式下「上台」的那一席、或独 preview pane 抢走键盘的那一刻)?另外:一个 pane 从**不可见的 tab** 变成可见但仍不握键盘,该不该发 `\e[I`?本方案倾向**不发**(协议问的是键盘不是眼睛),但这句话值得一双外面的眼睛。
