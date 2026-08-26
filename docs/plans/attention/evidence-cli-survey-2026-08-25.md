# 普查:各家 agent CLI 的注意力信号(2026-08-25)

`evidence-2026-08-25.md` 用四段录音把 **Claude Code** 一家钉死了。本文件把同一把尺子横过来量其余各家,
为的是 plan §3 的适配器层能建在真数据上而不是建在「大概都差不多」上。

**这份文件不写产品代码,也不改任何既有裁决。** 它只回答三个问题:每家**发什么**、每家**不发什么**、
每家有没有一个**能让我们接上去的配置面**。

> **取证纪律——默认值一律以程序为准,不以 docs 为准。**
>
> 一个开关默认开还是默认关,决定的是安装器要不要多做一件事,而本块已经**三次**撞上「装了却不亮」:
> opencode 的 `attention.enabled` 默认关、gemini 的 `enableNotifications` 默认关、
> copilot 的 `beep` 默认关。**第三次还多一层**:上游自己的文档表里那一格写的是 `true`,
> 而上游自己的程序写的是 `kn.beep===!0`、自带的 `copilot help config` 也写着 "defaults to `false`"
> (`evidence-copilot-cli-2026-08-26.md` §3.2)。
>
> **所以本文件与后续任何一份取证里,默认值只从两处拄:程序源码,或程序自带的 `help` 输出。**
> docs 的默认值列只能当线索,不能当证据—— 它已经错过一次,而错的方向恰好是「看上去不用管」。

## 0. 盘点:这台机器上到底装了哪几家

盘点面铺满了三处,不只是 `PATH`:

```powershell
foreach ($c in 'codex','opencode','kimi','pi','aider','gemini','gh','claude','amp','crush','goose','qwen','droid','copilot') {
  $g = Get-Command $c -ErrorAction SilentlyContinue
  if ($g) { "FOUND $c -> $($g.Source)" } else { "MISSING $c" }
}
npm ls -g --depth=0                      # 全局 npm 包
gh extension list                        # gh 扩展(旧的 gh copilot 走这条路)
wsl -d Ubuntu-24.04 -- bash -lc 'command -v codex opencode kimi pi aider gemini copilot claude'
wsl -d Ubuntu-22.04 -- bash -lc 'command -v codex opencode kimi pi aider gemini copilot claude'
```

结果**只有两家**:

| CLI | 位置 | 版本 |
|---|---|---|
| `claude` | `C:\Users\Weiyi\.local\bin\claude.exe` | 已由 `evidence-2026-08-25.md` 量完(v2.1.241) |
| `codex` | `C:\Users\Weiyi\.codex\packages\standalone\current\bin\codex.exe` | `codex-cli 0.144.4` |

`gh` 装了但**没装 copilot 扩展**(`gh extension list` 空)。全局 npm 只有 `@playwright/mcp` 与 `pnpm`。
没有 pipx。两个 WSL 发行版里一家都没有。`cursor.cmd` 是编辑器启动器,不是 agent CLI,不计。

**所以:本文件里 codex 一家是实测,其余各家一律标「未实测(文档取证)」,并逐条注明取证来源。**

## 1. 台子与可重跑命令(codex)

与 `evidence-2026-08-25.md` §0 同一套台子、同一个 `bt-record`、同一份 `conpty.dll`:

```
BT_RECORD conpty_source="source=sidecar version=1.25.260710002-preview dll=…\target\release\conpty.dll"
```

```sh
# 隔离:APPDATA 指向空目录;清掉会被子进程继承的一切 agent 标记与终端身份标记
unset CLAUDE_CODE_CHILD_SESSION CLAUDECODE CLAUDE_CODE_ENTRYPOINT CLAUDE_CODE_SESSION_ID \
      CLAUDE_CODE_MESSAGING_SOCKET CLAUDE_CODE_MESSAGING_TOKEN CLAUDE_CODE_BRIDGE_SESSION_ID \
      CLAUDE_CODE_EXECPATH CLAUDE_PID CLAUDE_EFFORT AI_AGENT CLAUDE_CODE_ALT_SCREEN_FULL_REPAINT
unset TERM_PROGRAM TERM_PROGRAM_VERSION WT_SESSION KITTY_WINDOW_ID TMUX   # ← 这一行是要点,见 §2.4
export APPDATA=<scratchpad>/attn-cli/appdata

cd /d/Developer/BetterTerminal        # codex 的 trust 名单里有这个目录,躲开信任对话框
target/release/bt-record.exe <scratchpad>/out/codex-a.btcr --size 120x30 \
    --input-plan docs/plans/attention/evidence/codex-plan-a.txt \
    -- C:/Users/Weiyi/.codex/packages/standalone/current/bin/codex.exe \
       -c check_for_update_on_startup=false \
       -c 'notify=["<scratchpad>/attn-cli/notify-probe.cmd"]' \
    > <scratchpad>/out/codex-a.raw 2> <scratchpad>/out/codex-a.err
```

四段录音换 `--input-plan` 与末尾的 codex 参数即可:

| 录音 | plan | 额外参数 |
|---|---|---|
| A | `codex-plan-a.txt` | 无 |
| B | `codex-plan-b.txt` | 无 |
| C | `codex-plan-c.txt` | `-a untrusted -s read-only` |
| D | `codex-plan-b.txt` | `-c tui.notification_method=osc9` |

**`CODEX_HOME` 用的是真的那份**(`C:\Users\Weiyi\.codex`)—— codex 必须登录才能跑一个真回合,
而把 `auth.json` 复制进隔离目录有 OAuth refresh token 轮换互踢的风险,那才是真的「动用户的登录态」。
折中是:**配置不改一个字节**,一切偏移都走 `-c` 临时覆盖(`check_for_update_on_startup`、`notify`、
`tui.notification_method`、`-a`、`-s`),`APPDATA` 仍然隔离。四段录音在用户的 `~/.codex/sessions`
下留了四条正常的会话记录,与用户自己跑一次 codex 没有区别。

**`notify` 被临时指向一个探针脚本**(`notify-probe.cmd`,把 argv 追加进 `notify.log`),
既取到了 payload,又避免了去戳用户真实配置里那个 computer-use 可执行文件。

**不 kill 非自起进程**:全程只结束自己 spawn 的那一棵树。判读脚本沿用
`docs/plans/attention/evidence/scan.py` 与 `seq.py`,一个字没改 —— 它们把「OSC 8 的 BEL 收尾」
与「裸 BEL」分开数,这一条在 codex 上同样是主证据(codex 每次改标题都用 BEL 收尾,不分开数会得到几百的假计数)。

**原始字节留在 scratchpad,不进仓**:`<scratchpad>/attn-cli/out/codex-{a,b,c,d}.{raw,btcr,err}`
与 `notify.log`。它们带着会话 id、thread id 与真实 cwd。**那个目录随会话消失**,所以逐字节的判读结论全抄在下面。
进仓的是 `codex-plan-{a,b,c}.txt` 与 `notify-probe.cmd` —— 上面那条命令配这四个文件可以原样重跑。

## 2. Codex CLI —— 实测四段

### 2.1 四段录音

| 录音 | 场景 | 焦点上报 | **裸 BEL** | OSC 9 | OSC 133 | OSC 9;4 | OSC 777 | OSC 1337 | 改题次数 |
|---|---|---|---|---|---|---|---|---|---|
| A | 干净启动,20s 注入 `reply with only the word ok`,回合跑完后静置 | 不发 | **0** | 0 | 0 | 0 | 0 | 0 | 128 |
| B | **同 A**,但注入 prompt 前先发一次 `ESC [ O`(焦点离开) | **发了一次 focus-out** | **1** | 0 | 0 | 0 | 0 | 0 | 211 |
| C | 同 B,外加 `-a untrusted`,prompt 换成一条必须批准的命令 | 发了一次 focus-out | **1** | 0 | 0 | 0 | 0 | 0 | 272 |
| D | 同 B,外加 `-c tui.notification_method=osc9` | 发了一次 focus-out | **0** | **1** | 0 | 0 | 0 | 0 | 222 |

四段的回合都真的跑成了:A/B/D 的转录里有 `ok`,C 停在了 `Would you like to run the following command?`。

### 2.2 主证据一:**焦点闸门在 codex 身上第二次成立**

A 与 B **prompt 一模一样、回合一样短**,唯一差别是 B 在 prompt 之前发了一次 `ESC [ O`。
**A 零铃,B 一铃。** 这是与 `evidence-2026-08-25.md` §3.1 **同型的单变量对照,换了一个生产者又成立了一次。**

B 那一枚裸 BEL 在偏移 `190510`(全文件 192317 字节):

```
…\x1b[0 q \x1b[?25h \x1b[21;3H \x1b[?2026l \x1b]0;BetterTerminal\x07 \x07 \x1b[?2026h…
```

签名是:**标题先落回没有 spinner 的空闲形 `OSC 0;BetterTerminal`,紧接着一声铃** —— 即「回合结束、输入框交回给你」。

而 codex **自己开了 `?1004h`**。录音 A 的私有模式序列:

```
      7  ?1004h     ← ConPTY 自己的
     15  ?9001h     ← ConPTY
     23  ?2004h     ← 应用的(括号粘贴)
     31  ?1004h     ← 应用的(焦点上报)
 111528  ?2004l / ?1004l / ?1004h / ?1004l / ?9001l   ← 退出时关得很干净
```

**这一条不是猜的,它有上游源码解释**:codex 的 `tui.notification_condition` 默认值是 `unfocused`,
`notify()` 前面压着一个 `should_emit_notification(condition, terminal_focused)`,
`terminal_focused` 是一个由 crossterm 焦点事件更新的 `AtomicBool`。
**Folio 一个焦点字节都不发**(DESIGN §7.1.5i 记的全仓 `\e[I`/`\e[O` 零命中),
于是那个 bool 停在初值上,codex 认定「你正看着呢」,于是**一声不吭**。

**两家不同的 agent、两套不同的实现,踩的是 Folio 侧同一个洞。** §7.1.5i 那条「发焦点上报」
(plan 片 A0.5)因此从「Claude Code 的修法」升格为**通用修法**:它一次修好两家,而且很可能修好所有
把 `?1004` 当门的家(§4 里 opencode 与 pi 也开 `?1004`)。

### 2.3 主证据二:**codex 的标题里有一句「Action Required」**

录音 C 是四段里唯一一段真的停在「等你回答」上的。它的改题序列在末尾出现了 A/B/D 都没有的两个字面量:

```
[ . ] Action Required | BetterTerminal      x40
[ ! ] Action Required | BetterTerminal      x40
```

两者以约 473 字节一次的节奏**交替**闪烁,直到 `ESC` 撤销批准框为止,之后立刻回到 `⠴ BetterTerminal` → `BetterTerminal`。

于是 codex 的标题是一台**三态机**,不是 Claude Code 那种零语义转轮:

| 态 | 标题形状 |
|---|---|
| 空闲 | `BetterTerminal`(裸项目名,无前缀) |
| 忙 | `<盲文 spinner> BetterTerminal` |
| **等你** | `[ . ] Action Required \| BetterTerminal` ⇄ `[ ! ] Action Required \| BetterTerminal` |
| 退出 | `OSC 0;`(清空) |

那一枚裸 BEL 在偏移 `166612`,**紧挨着 Action Required 标题的前一个字节**:

```
…\x1b[?25h \x1b[21;3H \x1b[?2026l \x07 \x1b]0;[ . ] Action Re…
```

同一段屏上的文字是 `Would you like to run the following command?` —— 铃与标题指的是同一件事。

**但这一条要说清它的成色**:它满足 plan §3.1 的判据①(是程序主动说的)、②(有开有关)、③(三个主都在),
**唯独不是一条协议** —— 它是一个可被 `tui.terminal_title` 重配、可被上游改文案、没有版本号的**字符串**。
按 plan §3.4 的裁决,读它属于「读屏猜」那一档的近亲。本文件把它记为**事实**,并在 §6 里把它放进
「兜底档,明标最低可信度」而不是主通道。

### 2.4 主证据三:**codex 会发 OSC 9,只是不认得 Folio**

录音 D 只改了一个参数 `-c tui.notification_method=osc9`,那一枚裸 BEL 就换成了

```
OSC 9;ok
```

—— 一条**带正文的桌面通知**,正文就是本回合助手的最后一句话(`ok`)。裸 BEL 归零。

原因在上游的 `auto` 判定里:`notification_method` 默认 `auto`,而 `auto` 只对
**Ghostty / iTerm2 / Kitty / Warp / WezTerm** 这五家选 OSC 9,其余(Apple Terminal、Alacritty、
GNOME Terminal、Konsole、VS Code、Windows Terminal、VTE、unknown、dumb)一律降级成裸 BEL。
**Folio 不在那张表上,于是拿到的永远是最没有信息量的那一种。**

这条实测把一件事变成可施工的:`crates/bt-term/src/inline_image.rs` 早就把 `OSC 9` 解析成
`TerminalNotification`(§1006-1017 那张表),**收的一侧一行都不用写**;缺的只是让对面知道我们收得住。

### 2.5 codex **不住备用屏**

四段录音的 `?1049` 命中数**全部是 0**。codex 0.144.4 在 ConPTY 上跑的是**内联模式**,
一直写在主屏与 scrollback 里(`tui.alternate_screen` 默认 `auto`)。

这与 Claude Code 正相反(它 `?1049h` 在第 86 字节,整场不退)。后果落在 Folio 的两处既有规则上:

- §7.1.5b 那条「备用屏程序豁免呼吸」对 codex pane **不生效** —— 它不在备用屏里。
- plan §3.5 提的第八记号 `Attached`(判据是 `alternate_screen == true`)**对 codex pane 恒为假**。
  也就是说那个记号填的是「claude 型」的洞,不是「所有 agent CLI」的洞。**这一条建议交用户复核。**

### 2.6 codex 的配置面:两套,`notify` 与 `hooks`

**① `notify`(旧路,turn-complete 专用)。** `~/.codex/config.toml` 的 `notify = ["程序", "参数"...]`,
codex 在 argv 末尾**追加一个 JSON 字符串**后 fire-and-forget 地 spawn 它。探针实测到的原文(录音 A):

```json
{"type":"agent-turn-complete","thread-id":"01a037af-…","turn-id":"01a037af-…",
 "cwd":"D:\\Developer\\BetterTerminal","client":"codex-tui",
 "input-messages":["reply with only the word ok"],"last-assistant-message":"ok"}
```

**`type` 今天只有 `agent-turn-complete` 一个值。** 而**录音 C 一条 notify 都没有产生** ——
批准框弹出来的那一刻,这条通道是哑的。

> **所以 `notify` 恰好发不出本块要的那句话。** 它说的是「我说完了」,而 plan §2.3 已经用四段录音
> 证伪过「说完了 = 在等你」。把 `notify` 映射成 `wait` 与 plan §10.4.1 里被否掉的 `Stop → wait` 同型同错。

**② `hooks`(新路,有一个正对口的事件)。** 从本机 `codex.exe` 里直接读出的 hook schema 标题共 **10 个事件**:

```
session-start   user-prompt-submit   pre-tool-use   post-tool-use   permission-request
pre-compact     post-compact         subagent-start subagent-stop   stop
```

其中 **`permission-request`** 是唯一语义对口的那一个。它的 input schema(逐字从二进制里取出)是:

```json
{"hook_event_name": {"const": "PermissionRequest"},
 "session_id": "string", "turn_id": "string", "cwd": "string",
 "tool_name": "string", "tool_input": true, "model": "string",
 "permission_mode": {"enum":["default","acceptEdits","plan","dontAsk","bypassPermissions"]},
 "transcript_path": "string|null"}
```

**要点三条:**

- `permission-request` hook 是**同步的决定门**(output schema 里有 `decision.behavior: allow|deny`),
  与 Claude Code 的 `PermissionRequest` 同构。plan §10.4.3 那条「信号 hook 不能把决定门拖住」
  的施工约束**原样适用**。
- 配置位置:`~/.codex/hooks.json`、`~/.codex/config.toml` 的 `[hooks]` 内联表,以及仓内
  `.codex/hooks.json` / `.codex/config.toml`。**仓内那两条是攻击面** —— plan §10.4.3 已裁「Folio 的安装器
  只写用户级配置,绝不从工作区发现或加载 hook」,这条纪律在 codex 上一字不改地成立。
- codex 对非托管 hook 有**按哈希记账的信任门**(`/hooks` 命令查看与授信;
  `--dangerously-bypass-hook-trust` 绕过)。**Folio 的安装器要考虑「装完之后用户还得去授信一次」这一步**,
  不能装完就当接上了。

**③ 收铃/发铃的配置键**(上游 schema 逐字):`tui.notifications`(`bool | string[]`,默认 `true`;
数组形式可按类型过滤,允许值 `"agent-turn-complete"` / `"approval-requested"` / `"plan-mode-prompt"`)、
`tui.notification_method`(`auto|osc9|bel`,默认 `auto`)、`tui.notification_condition`
(`unfocused|always`,默认 `unfocused`)、`tui.terminal_title`(item id 数组,默认 `["activity","project"]`)、
`tui.alternate_screen`(`auto|always|never`)。

> **`tui.notification_condition = always` 是一条今天就能用的旁路。** 它绕开焦点闸门,
> 让 codex 在 Folio 里无条件发铃。它**不该替代**片 A0.5(发焦点上报)—— 那才是正解,
> 而且 `always` 会让用户在自己正盯着的 pane 上也挨铃。记在这里是因为它可以当片 A0.5 的**验证对照组**:
> 打开它如果铃就来了,焦点闸门这条因果就又多一次确认。

### 2.7 codex 从头到尾**不发**的

`OSC 133`、`OSC 9;4`、`OSC 777`、`OSC 1337`(含 `RequestAttention`)、`OSC 99` —— 四段录音全部零命中,
并且**本机二进制里也搜不到这些字面量**(`\x1b]133;`、`\x1b]9;4`、`\x1b]777`、`\x1b]1337`、`RequestAttention`
逐条 0 命中;`\x1b]9;` 命中 2 次即 OSC 9 本体与它的 tmux DCS 透传变体,`\x1b]0;` 命中 1 次即标题)。

推论与 Claude Code 那边一致:**进度环通道对 codex 同样是空的**;它发的 OSC 只有 `0`(标题)与
(在被认出来的终端上)`9`(通知)。

## 3. Claude Code —— 不重测,引用

见 `evidence-2026-08-25.md` 全文。为横向表的完整性抄结论:发 `OSC 0`(零语义转轮)与 `OSC 8`;
不发 133 / 9;4 / 777 / 1337;自己开 `?1004h` 并把它当闸门;整场住备用屏;
hooks 的 `terminalSequence` 白名单**逐字点名拒收 OSC 1337**(plan §10.4.2)。

## 4. 未实测各家(纯文档/源码取证)

**以下六家本机一家都没装,下面每一条都是文档或公开源码取证,不是实测。** 逐家注明取证面。

### 4.1 opencode(未实测)

取证面:`opencode.ai/docs/`、`github.com/anomalyco/opencode`(仓库已从 `sst/opencode` 迁移)、
其渲染库 `github.com/anomalyco/opentui`。

- **不发裸 BEL**:没有 `bell` 配置键;`#36472` 是一条仍开着的「加 BEL」特性请求,今天只能靠用户自写插件。
- **发 OSC 9 / OSC 777 / OSC 99**:三条都在 OpenTUI 的 Zig 核心里(`packages/native/src/ansi.zig`),
  由 `renderer.triggerNotification(message, title)` 按**终端身份**选协议:
  Kitty/foot → OSC 99,Ghostty/WezTerm/Warp/VTE/GNOME Terminal/**Windows Terminal** → OSC 777,
  iTerm/Apple Terminal/ConEmu → OSC 9。可用 `OPENTUI_NOTIFICATION_PROTOCOL` 覆盖。
- **开 `?1004h`**,并且**明确拿它当闸门**:`ansi.zig` 里有 `focusSet = "\x1b[?1004h"` 与 DECRQM 探询
  `"\x1b[?1004$p"`;文档原话是「non-subagent events request desktop notifications only when the terminal is blurred」。
  ——**第三家把焦点当门的。**
- **配置面**:`tui.json` 的 `attention` 块,键为 `enabled`(默认 `false`)、`notifications`(默认 `true`)、
  `sound`、`volume`、`sound_pack`、`sounds`(按事件覆盖:`default` / `question` / `permission` / `error` /
  `done` / `subagent_done`)。**注意 `enabled` 默认是关的。**
- **插件面**:`opencode.json` 的 `plugin` 数组 + `api.event.on(...)`,事件含 `question.asked`、
  `permission.asked`、`session.status`、`session.error` —— **`question.asked` / `permission.asked` 语义对口。**
- 不发 133 / 1337。标题走 OSC 0 带会话身份;**标题是否随忙闲翻转没查实,标为存疑。**

### 4.2 Kimi CLI(未实测)

取证面:`MoonshotAI/kimi-cli`(正在并入后继 `MoonshotAI/kimi-code`)、`moonshotai.github.io/kimi-cli/en/`。

- **本体几乎什么都不发**:只有 `src/kimi_cli/utils/proctitle.py` 里一次 `\033]0;{title}\007`,
  且只在启动时被 `init_process_name("Kimi Code")` 调一次 —— **标题是个静态字符串,没有忙闲语义**。
- 不发 BEL,不发 9 / 9;4 / 99 / 777 / 133 / 1337;**仓里搜不到 `?1004`**。
- **但 hooks 是它最强的一面**:`~/.kimi/config.toml` 的 `[[hooks]]` 数组(字段 `event` / `command` /
  `matcher` / `timeout`),**13 个事件**,含 `PreToolUse` / `PostToolUse` / `UserPromptSubmit` / `Stop` /
  `SessionStart` / `SessionEnd` / `SubagentStart` / `SubagentStop` / `PreCompact` / `PostCompact` /
  `PostToolUseFailure` / `StopFailure` / **`Notification`**(带 `sink` / `notification_type` / `title` /
  `body` / `severity`)。hook 收 stdin JSON,退出码 0 放行 / 2 拦截。
- 文档里给桌面提醒的示例 hook 直接 shell 出去调 `osascript` —— **它把「够到用户」整件事外包给 hook**,
  这对我们反而是好消息:一条 `folio attention wait` 就能接上。

### 4.3 pi(earendil-works/pi,未实测)

取证面:`github.com/earendil-works/pi`(npm scope `@earendil-works/pi-coding-agent` / `pi-tui`)。
**先确认了身份**:不是树莓派工具链,也不是 Inflection 的 Pi 聊天机器人。

- **发 OSC 9;4 —— 这是六家里唯一一个真发进度的。** `packages/tui/src/terminal.ts`:
  `TERMINAL_PROGRESS_ACTIVE_SEQUENCE = "\x1b]9;4;3\x07"`(不定式进度,每 1000ms 重发保活)与
  `TERMINAL_PROGRESS_CLEAR_SEQUENCE = "\x1b]9;4;0\x07"`,在回合首尾由 `interactive-mode.ts` 调用。
  **plan §2.3「进度环通道在 agent 场景下无源」这句话,在 pi 身上被证伪了。**
- **发 OSC 777 / OSC 99**:在随仓发的示例扩展 `examples/extensions/notify.ts` 里
  (「sends a native terminal notification when Pi agent is done and waiting for input」),
  挂在 `agent_settled` 事件上,按 `WT_SESSION` / `KITTY_WINDOW_ID` 选协议,Windows Terminal 上退化成
  PowerShell toast。**注意它是示例扩展不是内建默认。**
- **开 `?1004h`**(`tui-alt-screen.ts`,与鼠标追踪打包在一起,并有显式的 `FOCUS_IN`/`FOCUS_OUT` 处理)。
  **是否全程常开没查实,标为存疑。**
- **扩展面很厚**:`~/.pi/agent/extensions/*.ts` 或 `.pi/extensions/*.ts`,`pi.on(event, handler)`,
  事件表里对本块最有用的是 **`agent_settled`**(「fires only once a run fully settles」)与
  `tool_call`(可拦截)。
- 标题:`${APP_TITLE} - ${sessionName} - ${cwdBasename}`,**身份不是忙闲**;忙闲由 OSC 9;4 那条单独走。
- 不发 133 / 1337。

### 4.4 aider(未实测)

取证面:`aider.chat/docs/usage/notifications.html`、`aider.chat/docs/config/options.html`。

- **裸 BEL 是它的默认机制**,由 `--notifications` 开关(yaml `notifications`,env `AIDER_NOTIFICATIONS`,
  **默认 `False`**)。文档原话:「Enable/disable terminal bell notifications when LLM responses are ready」。
- `--notifications-command "<cmd>"`(yaml `notifications_command`,env `AIDER_NOTIFICATIONS_COMMAND`)
  可以把铃整个换成任意外部命令 —— **这是一条现成的、零适配的接入点**。
- 不发任何 OSC(9 / 9;4 / 777 / 1337 / 133 均未见于文档);`?1004` 未见;标题未见。
- **注意它说的是「responses are ready」= 回合结束**,不是「在等你」。同 §2.6① 的问题。

### 4.5 gemini-cli(未实测)

取证面:`geminicli.com/docs/cli/notifications/`、`geminicli.com/docs/reference/configuration/`、
`google-gemini/gemini-cli` 的 `docs/cli/settings.md`。

- **首选 OSC 9,兜底裸 BEL**,由两个键控制:`general.enableNotifications`(bool,**默认 `false`**)
  与 `general.notificationMethod`(`auto | osc9 | osc777 | bell`,默认 `auto`)。
  文档原话:「The CLI uses the OSC 9 terminal escape sequence to trigger system notifications」。
  **`osc777` 是可选值之一。**
- **标题是六家里语义最重的**:`ui.dynamicWindowTitle`(bool,**默认 `true`**)——
  「Update the terminal window title with current status icons(**Ready: ◇, Action Required: ✋, Working: ✦**)」;
  另有 `ui.showStatusInTitle`(默认 `false`)把模型的思考写进标题。
  **「Action Required: ✋」是一个字面上的等待态标记,与 §2.3 codex 的 `Action Required` 同型。**
- 不发 9;4 / 133 / 1337;`?1004` 未见于文档。
- **没有 hooks/事件回调面** —— 只有上面那两三个配置键。**这是六家里适配最难的一家。**

### 4.6 GitHub Copilot CLI(未实测,**两个东西别混**)

**(a) 旧的 `gh copilot` 扩展**(`github/gh-copilot`):只有 `suggest` / `explain` 两条一次性命令,
**没有持续会话、没有回合循环**,因此本块的一切信号对它都不适用。本机也没装。

**(b) 新的独立 `copilot` CLI**(npm `@github/copilot`,`github/copilot-cli`)——
取证面:`docs.github.com/en/copilot/reference/hooks-reference`、`.../copilot-cli/customize-copilot/use-hooks`、
`.../cli-config-dir-reference`、仓库 `changelog.md`。

- **`beep`**(bool,默认 `true`)——「Play an audible beep when attention is required」;
  另有 `beepOnSchedule`(默认 `true`)。**注意:多条 issue(#1458 / #3573 / #3748)报告 Windows 上它走的是
  Win32 `MessageBeep()`/`PlaySound()` 而不是真的往 tty 写 BEL 字节 —— 如果属实,这一路 Folio 一个字节都收不到。
  标为存疑,装上之后必须实测。**
- **发 OSC 9;4**:`terminalProgress` 设置(changelog v1.0.51:「Add `terminalProgress` setting to enable
  or disable OSC 9;4 terminal progress indicators」)。**第二家真发进度的。**
- **hooks 面是六家里最完整的**:`~/.copilot/hooks/`(或 `$COPILOT_HOME/hooks/`)与仓内 `.github/hooks/<name>.json`,
  事件含 `sessionStart` / `sessionEnd` / `userPromptSubmitted` / `userPromptTransformed` / `preToolUse` /
  `postToolUse` / `postToolUseFailure` / `preCompact` / `agentStop` / `subagentStart` / `subagentStop` /
  `errorOccurred` / **`notification`**。
  **`notification` 是直接对口的那一个**,子类型有 `shell_completed`、`shell_detached_completed`、
  `agent_completed`、**`agent_idle`**、**`permission_prompt`**、**`elicitation_dialog`**,
  且**异步 fire-and-forget、绝不阻塞会话** —— 与 plan §10.4.3 的施工约束天然相容。
  后三个正是「真在等你」的那三种,与 plan §10.4.1 的白名单可以逐条对上。
- 不发 OSC 9(toast)/ 777 / 1337 / 133;`?1004` 未见。标题带会话身份(changelog v1.0.66),**不带忙闲**。

## 5. 横向表

「发」= 有实测或源码/文档证据说它会发;「—」= 未见证据;「?」= 存疑,见对应小节。

| | Claude Code | **codex** | opencode | Kimi CLI | pi | aider | gemini-cli | copilot CLI |
|---|---|---|---|---|---|---|---|---|
| **实测?** | ✅ 四段 | ✅ **四段** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| 裸 BEL | 发(焦点闸门后) | **发(焦点闸门后)** | — | — | — | 发(默认关) | 兜底 | `beep`(Windows 上 ?) |
| OSC 9 | — | **发(仅认得的终端)** | 发 | — | 发(示例扩展) | — | **首选** | — |
| OSC 9;4 进度 | — | **—** | — | — | **发(核心)** | — | — | **发** |
| OSC 777 | — | **—** | 发 | — | 发(示例扩展) | — | 可选值 | — |
| OSC 99 | — | **—** | 发 | — | 发(示例扩展) | — | — | — |
| OSC 133 | — | **—** | — | — | — | — | — | — |
| **OSC 1337 / RequestAttention** | **—**(且被白名单拒收) | **—** | — | — | — | — | — | — |
| 开 `?1004` 焦点上报 | **开,且当闸门** | **开,且当闸门** | **开,且当闸门** | — | 开(范围?) | — | — | — |
| 住备用屏 | **是** | **否(内联)** | ? | ? | ? | ? | ? | ? |
| 标题带忙闲 | 转轮,**零语义** | **三态,含 `Action Required`** | 身份(?) | 静态 | 身份 | — | **三态,含 `✋`** | 身份 |
| hooks/事件面 | 31 事件,含 `PermissionRequest` | **10 事件,含 `permission-request`** | 插件 `api.event.on` | 13 事件,含 `Notification` | 扩展 `pi.on`,含 `agent_settled` | 仅 `--notifications-command` | **无** | 13 事件,含 `notification` |
| 外部通知程序 | — | **`notify`(仅 turn-complete)** | — | — | — | `notifications_command` | — | — |

**三条横向事实,每一条都改了 plan 里的一句话:**

1. **`OSC 1337;RequestAttention` 一家都不发。** plan §3.2 把它挑作首选通道时的理由是「不发明序列」,
   这条理由仍然成立;但本普查证明**它今天没有任何一个 agent CLI 生产者**。
   §3.2 那段「它的用户不是 Claude Code,是任何自己写 tty 的程序」的定位**必须原样保留并加重** ——
   片 A1 的价值在通用性,不在这八家。
2. **`OSC 133` 一家都不发。** 八家全零。agent CLI 场景下命令边界这条路是彻底空的。
3. **`?1004` 焦点上报是八家里被用得最一致的一条**,而且**三家把它当闸门**(Claude Code / codex / opencode)。
   **片 A0.5 因此是本块投入产出比最高的一片,而且它是唯一一片「修一次,修好至少三家」的。**

## 6. 适配器层建议表

**先说这一列量的是什么。** Folio 收的一侧早就铺好了:`crates/bt-term/src/inline_image.rs` §1006-1017
已经解析 `OSC 9`(含 ConEmu 的 `9;4` 进度)、`OSC 777;notify`、`OSC 133`、`OSC 1337;File=`。
**所以「通用序列覆盖度」量的是对面发不发,不是我们收不收。**

「够到 awaiting」= 有没有一条能表达 plan §3.1 判据①②③的通道。

| 家 | 通用序列覆盖度(对我们有用的部分) | 够到 `awaiting`? | 需不需要适配器 | 适配器形态 |
|---|---|---|---|---|
| **Claude Code** | 只有 `OSC 0`(零语义)+ `OSC 8`;铃被焦点闸死 | ❌ 通道全无 | **要** | **A 型(hook → 命名管道 → `folio attention`)**。plan §10.4.1 的三张名单即成品,`PermissionRequest`/`Elicitation` 走 0 延迟边。**这是唯一可行路**(§10.4.2 已证)。 |
| **codex** | `OSC 0`(**三态,含 `Action Required`**);`OSC 9` 只在被认出的终端上;铃被焦点闸死 | ⚠️ 有两条半 | **要,但比 Claude Code 轻** | **A 型为主**:`hooks.permission-request` → `folio attention wait`,`user-prompt-submit` → `clear`。**注意信任门**(§2.6②)。<br>**外加一条零成本的 B 型**:让 codex 认出 Folio,`OSC 9` 就自己来了(§2.4)。<br>**`notify` 不用**——它只说 turn-complete(§2.6①)。 |
| **opencode** | `OSC 9/777/99` 三选一,**按终端身份挑**;`?1004` 当闸门 | ✅ **事件级够到**(`question.asked`/`permission.asked`) | **要,轻** | **A 型**:一个 `plugin` 包,`api.event.on('permission.asked'\|'question.asked')` → `wait`,`session.status`(idle)→ `clear`。**外加 B 型**:`attention.enabled` 默认是关的,安装器要提示开。 |
| **Kimi CLI** | 本体几乎零(只有一个静态标题) | ✅ **hooks 够到** | **要** | **A 型**:`~/.kimi/config.toml` 的 `[[hooks]] event = "Notification"` → `folio attention`,按 `notification_type` 分流;`UserPromptSubmit` → `clear`。它本来就把「够到用户」外包给 hook,接得最顺。 |
| **pi** | **`OSC 9;4` 进度是真的**(核心、带保活);通知在示例扩展里 | ✅ **事件级够到**(`agent_settled`) | **要,轻** | **A 型**:一个 `~/.pi/agent/extensions/folio.ts`,`pi.on('agent_settled')` → `wait`,`pi.on('input')` → `clear`。<br>**外加零适配红利**:它的 `OSC 9;4` 今天就能直接点亮 Folio 的进度环 —— **plan §7.1.5b 的「进度」一态在 pi 身上有源。** |
| **aider** | 只有裸 BEL(默认还是关的) | ❌ 只有事件不是状态 | **要,最轻** | **B′ 型(一行配置)**:`notifications_command = "folio attention wait"`。**但它的语义是 turn-complete,不是 awaiting** —— 按 plan §4 B2 的出队门处置,**只能进 `Bell` 那一档,不能升 `Awaiting`。** |
| **gemini-cli** | `OSC 9` 首选 / `osc777` 可选 / BEL 兜底;**标题三态含 `✋`** | ❌ **没有任何事件面** | **要,而且只能做次等的** | **无 A 型可做**。只有两条:①**B 型**——`general.enableNotifications = true` + `notificationMethod = "osc9"`,拿到一条 `TerminalNotification`(消息,不是状态);②**C 型(标题嗅探)**——读 `ui.dynamicWindowTitle` 的 `✋`。**②按 plan §3.4 属最低可信度档,建议记账不做。** 真正的出路是上游诉求(§7)。 |
| **copilot CLI** | `OSC 9;4` 进度;`beep`(Windows 上是否真写 tty **存疑**) | ✅ **hooks 够到,而且最干净** | **要,轻** | **A 型**:`~/.copilot/hooks/folio.json`,`notification` 事件按子类型分流 —— `permission_prompt` / `elicitation_dialog` / `agent_idle` → `wait`,`agent_completed` / `shell_completed` → 不映射。**它天生异步不阻塞**,与 §10.4.3 相容。**仓内 `.github/hooks/` 一律不加载**(§10.4.3 纪律)。 |

### 6.1 适配器只有三种形态,不是八种

上表收敛下来只有三型,**这就是适配器层该长的样子**:

- **A 型「事件 → 管道」**:装一份用户级 hook/插件配置,里面调 `folio attention wait|clear`。
  八家里有 **六家**能走(Claude Code / codex / opencode / Kimi / pi / copilot)。
  **共用的只有 `folio attention` 这一个动词与它背后的管道**;每家不同的只有**一份配置模板 + 一张事件名映射表**。
  → **适配器 = 数据(模板 + 映射表),不是代码。** 这是本普查最重要的一条设计结论。
- **B 型「让对面认出我们 / 把开关打开」**:codex 的 `notification_method`、opencode 的 `attention.enabled`、
  gemini 的 `enableNotifications`、aider 的 `notifications`。这一型**不写适配器,写安装器与文档**。
  值得单独记一笔:**至少三家的通知默认是关的或默认降级的**,「装了 Folio 就该亮」这件事有一半输在这里。
- **C 型「标题嗅探」**:codex 的 `Action Required` 与 gemini 的 `✋`。**建议记账不做**(plan §3.4),
  除非用户裁定给 gemini-cli 这种「没有事件面」的家开一个明标最低可信度的例外。

### 6.2 两条与 plan 冲突、需要用户裁的

1. **plan §3.5 的第八记号 `Attached` 判据是 `alternate_screen == true`,而 codex 不住备用屏**(§2.5)。
   于是那个记号在 codex pane 上恒不亮 —— 它填的是「claude 型」的洞,不是「agent pane」的洞。
   **要么承认它只覆盖一半,要么换判据。**
2. **plan §2.3 说「进度环通道在 agent 场景下无源」,pi 与 copilot CLI 都真发 `OSC 9;4`**(§4.3 / §4.6)。
   那句话该改成「**在 Claude Code 与 codex 上无源**」。进度环不是死的,是这两家不发。

## 7. 上游诉求账(不占本块工期)

- **Claude Code**:把 OSC 1337 加进 hooks `terminalSequence` 白名单(plan §10.4.2 已记,不重复)。
- **codex**:`tui.notification_method` 的 `auto` 判定表里加 Folio;或者更好 ——
  **改成按 `OSC 9` 的能力探询而不是按终端名白名单**。今天这张白名单让所有新终端都拿到最差的那一档。
- **gemini-cli**:它是八家里唯一**完全没有事件面**的。诉求是给它一个 hook/extension 面,
  哪怕只有一个 `notification` 事件。

## 8. 本次取证踩的两个坑(留档,下次别再踩)

1. **`bt-record` 的 input-plan 第一次注入不能是裸 `Enter`。** codex 启动时可能先弹一个
   「Update available!」的选择框,`› 1. Update now` 是**默认高亮项**,而 plan-0 的第一条指令是
   `/quit` + `\r` —— 那个 `\r` 落在选择框上,**当场触发了一次 codex 自更新**
   (0.144.4 → 0.149.1),PowerShell 5.1 的缓冲式下载爬到 ~4KB/s、20 分钟没走完,只好把自起的那棵树
   (installer + codex + bt-record)结束掉。**结束后 `codex --version` 仍是 `0.144.4`,安装完好** ——
   那个 installer 是下完再换,半路停不留残骸。
   **正解写进 §1 的命令里了:`-c check_for_update_on_startup=false`。**
   更一般的教训:**给一个没见过的 TUI 录音,第一段必须是「只看不按」的哑录**,先拿到启动形状再写按键计划。
2. **终端身份的环境变量必须一起清。** `TERM_PROGRAM` / `WT_SESSION` / `KITTY_WINDOW_ID` / `TMUX`
   会让 codex(以及 opencode、pi)走完全不同的通知分支。不清它们,量到的是「这台机器上恰好继承了什么」,
   不是「Folio 里会发生什么」。§1 的 `unset` 里那一行不是装饰。
