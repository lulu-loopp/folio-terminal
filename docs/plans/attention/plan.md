# 通知与注意力块方案 v1(2026-08-25 起草,送 Codex 评审;不写产品代码)

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
9. **`RequestAttention` 的 `yes/no` 这一臂是状态、`once` 是事件;`OSC 9`/`777` 是消息,三者不合并**(`bt-term/src/session.rs:712-718` 已立)。
   **v1 原文记作**「`RequestAttention` 是状态」—— 那句话把 `once` 也裹了进去,而 `once` 按定义是一次性的,它走既有 `bell_latched` 那条路(评审 Q6,折入 §10.8)。
10. **焦点上报不是用户输入。** 片 A0.5 写出去的 `\e[I`/`\e[O` 必须走终端自答通道(`take_pty_writes` 那条路,`main.rs:22560`/`22582`),**绝不许经过 `send_user_input` / `send_mouse_input_to`**;否则 A0.5 发的 `CSI O` 会被 A2 的出队门当成「用户回答了」而退号(评审 Q8,折入 §10.10)。
11. **`attention_ticket` 的「nothing here takes a place away」有且只有两个具名例外**,都在 A2 里长出来:程序自撤(`=no`)与 leaf 消失。**用户回答那扇门仍在函数外**(`answer_attention`)。加第三个例外前必须回来改这条红线(§10.2)。

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
| e4 | `Requested(e)` | `Settle{consumed=false}` | `Queued(e,t)` | `admit … ticket=t episode=e`;**C3 toast 的唯一投递边沿(M8)** |
| e5 | `Requested(e)` | `Settle{consumed=true}` | `Requested(e)` | `refuse … reason=watched` |
| e6 | `Requested(e)` | `Yes` | `Requested(e)` | **幂等重申**:不铸新号、不重发 toast |
| e7 | `Requested(e)` | `No` | `Idle` | `withdraw … reason=program` |
| e8 | `Requested(e)` | `Answer` | `Acknowledged(e)` | 你抢在入队前答了 |
| e9 | `Queued(e,t)` | `Answer` | `Acknowledged(e)` | `answer … ticket=t`;**退号** |
| e10 | `Queued(e,t)` | `No` | `Idle` | `withdraw … reason=program`;**退号** |
| e11 | `Queued(e,t)` | `Yes` | `Queued(e,t)` | 幂等;**ticket 不重铸**(先到先服务;`attention_ticket` 的 `is_none()` 守卫原样) |
| e12 | `Queued(e,t)` | `Settle{*}` | `Queued(e,t)` | 无 |
| e13 | `Acknowledged(e)` | `Yes` | `Acknowledged(e)` | **本状态机存在的理由**:程序持续 yes 也不能再入队 |
| e14 | `Acknowledged(e)` | `No` | `Idle` | **「下一次能再入队」的唯一开关** |
| e15 | `Acknowledged(e)` | `Settle{*}` / `Answer` | `Acknowledged(e)` | 不拿号;幂等 |
| e16 | 任意 | `LeafGone` | — | `expire … reason=leaf-gone`;号消失,`next_ticket` **不回退** |
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
answer   tab=<i> seat=<s> ticket=<t> episode=<e> by=<keyboard|ime|paste|files-row|mouse>
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

| # | 位置 | 是什么 | 算不算 `Answer` |
|---|---|---|---|
| 1 | `main.rs:55478` `send_user_input(…, UserInputKind::Keyboard)` | 键盘按键 | **算** |
| 2 | `main.rs:63248` `Ime::Commit` | **IME 提交**(不是 preedit) | **算** —— 提交出来的就是你打的字 |
| 3 | `main.rs:63111` `paste_from_clipboard_into` | 剪贴板粘贴(`Ctrl+V` / `Shift+Insert` / 终端菜单粘贴) | **算** |
| 4 | `main.rs:49768` files 行插入路径 | 从文件树把一条路径插进 shell | **算** —— 一次用户手势,且落进的是具名的那一席 |
| 5 | `main.rs:55456` `send_mouse_input_to` | **被转发的鼠标**(点击/滚轮进了追踪程序) | **Q2b 的一个子格,需用户裁。推荐:算。** 正方:`send_mouse_input_to` 的 doc 自己写着「a wheel forwarded into a program running there is as much a claim on it as a keystroke」;反方:滚轮翻页不是回答 |
| 6 | `main.rs:22561` `take_pty_writes()`(读到输出之后) | **终端协议自答**(DA / DSR / 颜色查询 / **焦点上报**) | **绝不算** |
| 7 | `main.rs:22583` `take_pty_writes()`(空转时再排一次) | 同上 | **绝不算** |
| 8 | `main.rs:54149` PSReadLine resize 锚点修复 | **我们自己发的自愈输入** | **绝不算** —— 是终端为修自己发的,不是你 |

**IME preedit(`Ime::Preedit`)不算**:它一个字节都不进 pty(只更新 `window.preedit`),连门都碰不到。写出来是为了让「IME 算」不被读成「组字中途就算」。

**施工点在哪(M4 的另一半)**:

- **不许下沉到 `write_pty_input`。** 那个函数只看得见一个字节串和一句 `&'static str`,分不出上面八行 —— 与 Enter 门 doc 里那句现成的理由完全同型:「`send_user_input` is handed a byte string and cannot tell an answer from a paste that happens to end in a newline」(`main.rs:63056`)。
- **正解**:把既有的 `UserInputKind`(`main.rs:10687`,今天只有 `Keyboard | Mouse`)**扩成来源枚举**,并让四条真用户路都经过它:`Keyboard` / `Ime` / `Paste` / `FilesRow` / `Mouse`。`answer_attention(seat, by: UserInputKind)` 由每条路自己调,`by` 直接进 trace(§10.2.6)。
- **协议自答那三条(#6/#7/#8)根本不经过 `UserInputKind`**,所以它们不是「要记得排除的例外」,而是**结构上够不到那扇门** —— 这正是红线 10 想要的形状,也是片 A0.5 的焦点上报能安全存在的原因。
- **`answer_attention` 的取号语义不变**:仍是「一次动作确认当前 episode 并取走号」,只是入口从一个变成四(或五)个,且每个入口都带来源。

---

### 10.4 【必改 M5】hook 白名单 —— 只有真正在等输入的才映射为 waiting

**证据来源**:`https://code.claude.com/docs/en/hooks.md` 原文,2026-08-25 逐字取回(291 536 字节),**不经转述**。文档里 `## Hook events` 下共 **31** 个事件。下表的每一句「官方原文」都是从那份 markdown 里抄的。

#### 10.4.1 三张名单

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

**绝不映射为 `wait`(名单是封闭的:**其余 24 个事件与 5 个 notification type 一律不映射**):**

| 来源 | 官方原文定义 | 为什么不 |
|---|---|---|
| **`Stop`** | "Runs when the main Claude Code agent has finished responding." | **这正是本方案 §2 花四段录音证伪的那件事。** v1 §3.3 路线 A 写的 `Stop → wait` 是把「回合结束」升回 `Awaiting`,与本方案对 BEL 的纠正同型同错。**v1 原文记作**「`Notification` → `wait`、`Stop` → `wait`、`UserPromptSubmit` → `clear`」—— 第二条必删,第一条必须细分到 type |
| `StopFailure` | "Runs instead of Stop when the turn ends due to an API error." | 结束不是等待 |
| `SubagentStop` | 子 agent 结束 | 同上 |
| `Notification` `idle_prompt` | "Claude finished responding about 60 seconds ago and you haven't typed since" | **另裁,见 §10.13。** 它是上游版的「输出安静 N 秒」启发式 —— 而 v1 §3.4 明确不做这一档 |
| `Notification` `auth_success` | "Authentication completes" | 不等输入 |
| `Notification` `agent_completed` | "A background session finishes or fails." | 不等输入 |
| `Notification` `quota_auto_resume_fired` / `quota_auto_resume_disabled` | 自动继续了 / 结束等待 | 不等输入 |

**评审列的可疑拼写全部核对无误**:`permission_prompt`、`elicitation_dialog`、`idle_prompt`、`auth_success`、`elicitation_complete`、`elicitation_response` 六个都在原文里、无一拼错;原文另有评审没列的 `elicitation_url_dialog`、`agent_needs_input`、`agent_completed`、`quota_auto_resume_fired`、`quota_auto_resume_stale`、`quota_auto_resume_disabled` 六个,共 12 个 type。

#### 10.4.2 核对时撞见的一条事实:**`terminalSequence` 明确拒绝 OSC 1337**

这一条评审没看到,而它**推翻本方案的一条选路**。官方原文两处:

> `terminalSequence` … A terminal escape sequence for Claude Code to emit on your behalf … **Restricted to OSC `0`/`1`/`2`/`9`/`99`/`777` and BEL.** If the value contains anything outside the allowlist, the field is ignored. Use this instead of writing to `/dev/tty`, which is unavailable to hooks

> Hooks run without a controlling terminal, so writing escape sequences directly to `/dev/tty` fails. Instead, return the escape sequence in the `terminalSequence` field and Claude Code emits it for you through its own terminal write path. **This is race-free, works inside tmux and GNU screen, and works on Windows where there is no `/dev/tty`.**

> Anything outside the allowlist, including CSI cursor and color sequences, OSC palette sequences, OSC 8 hyperlinks, OSC 52 clipboard writes, **and OSC 1337**, is rejected and the field is ignored.

**三条后果,逐条落到片单上:**

1. **v1 片 A0 的问题①②当场消失。** v1 要量的是「hook 子进程写不写得进 pty」与「`async` hook 的时序抖动多大」;上游已经把这条路做好了,而且**明说是 race-free 的、在 Windows 上工作**。**A0 从三问缩成一问**,只剩改正后的 ③′(§10.5)。v1 路线 B「hook 直写 `CONOUT$`」**作废** —— 它现在是一条自己造的、官方已经提供了替代品的风险路径。它的位置由**路线 B′(`terminalSequence`)** 顶替。

2. **但 B′ 发不出 `RequestAttention`。** 官方白名单逐字点名 OSC 1337 是被拒的。于是**在 Claude Code 这个生产者身上,正文 §3.2 挑的那条通道无论 hook 怎么写都走不通**。B′ 能发的与本块有关的只有:`OSC 777;notify`(消息,红线 9 已判它不是状态)、`OSC 99`(kitty 通知,同上)、裸 BEL(事件)、`OSC 9;4`(进度,实测无源)、标题(转轮,§2.3 已证伪)。**一条都不满足判据②③。**

3. **所以路线 A 不再是「推荐」,它是 Claude Code 场景下的唯一可行路。** hook 是普通子进程,能执行任意命令、继承 pane 的环境变量,`folio attention wait` 完全不受 `terminalSequence` 白名单约束。这与评审「A3 必须前置于 A2」的重排指向同一处,并给了它一个更硬的理由:**没有 A3,claude pane 上的 awaiting 永远是零,把 BEL 撤出去就只剩一个空队列。**

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

**没有替代通道可换。** §10.4.2 刚证明 `terminalSequence` 发不出 1337,路线 B 作废;路线 A 是 Claude Code 场景下唯一可行路。所以正确的做法不是换路,是**把合同写清楚并把能力封死**。

**路线 A 的安全合同(六条,施工时逐条对照):**

1. **`FOLIO_PANE` 只供诊断,绝不作为授权或路由依据。** 服务端在**创建端点时**就把它绑定到一个 `LeafSession`;消息里**不接受** window/tab/seat 字段,也不允许调用者改目标。
2. **每 pane 一个不可预测的端点名 / 至少 128-bit capability**;`PIPE_REJECT_REMOTE_CLIENTS`、`FILE_FLAG_FIRST_PIPE_INSTANCE`;**显式 DACL 只给当前 logon SID**(及确有需要的 SYSTEM),**不用默认描述符**。
3. **只准两个幂等动词 `wait` / `clear`**,限制单帧长度、连接数与速率。即使被同用户进程偷到,**最大影响也只是给这一 pane 制造/撤销一条注意力声明** —— 不能打字、不能开 pane、不能读转录、不能寻址别的 pane。(这一条与 §10.2 的幂等边 e6/e11/e13 互相加强:重复的 `wait` 连一行 trace 都写不出来。)
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

#### 10.8.2 合成规则(强吃弱)

| 弱层在请求 | 强层在请求 | 号的 `grounds` | 文案 | 配 toast? |
|---|---|---|---|---|
| 否 | 否 | 不发号 | — | — |
| **是** | 否 | `Requested` | 「需要注意」/ "Attention requested" | **不配** |
| 否 | **是** | `AwaitingInput` | 「正在等你回答」/ "Waiting for you" | **配** |
| **是** | **是** | `AwaitingInput` | 「正在等你回答」 | **配** |

- **`grounds` 只升不降,在一个 episode 内**:一个 pane 先发 1337 `yes`(弱)、六秒后 hook 又报了 `permission_prompt`(强),是**同一次请求**被证实了 —— 升级 `grounds`,**不铸新 episode、不重发号、不重新排队**(先到先服务不能因为证据变强而被打乱)。反过来不降:强层撤了但弱层还举着,`=no` 之前它仍是同一次请求,只是**没有第二次 toast**。
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

**必须先裁、否则片不能开工的:**

| 题 | 内容 | 挡住哪片 | 推荐 |
|---|---|---|---|
| **Q2a** | **BEL 撤出入队门吗?**(复议 2026-08-21 裁决) | **A2** | **撤** |
| **Q2b** | **出队门从 Enter 扩到「带来源的用户动作」吗?**(复议 2026-07-18 裁决)含子格:**被转发的鼠标算不算**(§10.3.2 第 5 行) | **A2** | **扩**;鼠标**算**。**若 Q2a 被否,Q2b 必须同否** |
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

正文 §8 那八步原样有效,加三步:

- **⑧ 同窗非活动 tab**:窗口保持前台,在 tab 3 的 agent 上触发一次请求 ⇒ **窗内徽章 +1、tab 3 上点亮,不弹 toast**(§10.7 第 2 行)。
- **⑨ 已答但程序没撤**:回答权限提问之后**不要**让 agent 撤;让它继续举着 `wait` ⇒ 徽章**不许**在下一帧重新 +1(§10.2 边 e13)。这一步是 bool 版本必挂、状态机版本必过的那一格。
- **⑩ 同窗内换键盘 owner**:窗口焦点不动,点一下文件树 ⇒ `BT_PTY_DUMP` 里该 pane 收到**恰好一次** `\e[O`;点回终端 ⇒ **恰好一次** `\e[I`(§10.10 边沿要求 1)。

**外加**:关掉 Windows 动画(reduced-motion)重跑 ②③,Bell 与 Awaiting 必须仍然分得出(红线 3,原样)。
