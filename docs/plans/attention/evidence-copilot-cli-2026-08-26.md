# 取证:GitHub Copilot CLI 的注意力信号(2026-08-26)

`evidence-cli-survey-2026-08-25.md` §4.6 把 copilot CLI 归了类,但**一行原文都没有**,
所以 `crates/bt-app/src/attention_map.rs` 的模块头把它记成「the open account」:

> **copilot CLI is not here yet, and this is the open account.** … What its two remaining
> subtypes and its `agent_completed` / `shell_completed` clears say **verbatim** has not been
> fetched under §10.4's rule, and this file's own discipline is that a row is written from a
> quotation or not at all.

本文件把那笔账取回来。**规矩照旧:一行要么从引文来,要么不写。** 每条都带 URL、抓取日期与逐字原文;
查不到的写「未找到」,不推测。

## 0. 取证面与可重跑命令

**三个面,全部是官方的:**

| 面 | 位置 | 抓取日期 | 说明 |
|---|---|---|---|
| ① 官方文档(Markdown 源) | `github/docs` 仓 `main` 分支 `content/copilot/**` | 2026-08-26 | 渲染页是 `docs.github.com`,但**渲染会丢表格与代码块**,所以逐字引文一律取自 Markdown 源 |
| ② 官方发布的**程序本体** | npm `@github/copilot-win32-x64@1.0.80` 里的 `package/app.js` | 2026-08-26 | 9,173,451 字节,bundler 打包但**未混淆字符串**;这是 TUI 与会话层的真源码 |
| ③ 官方 changelog 与 issue | `github/copilot-cli` 仓 | 2026-08-26 | 该仓**不含源码**,只有 `README.md` / `changelog.md` / `install.sh` / `LICENSE.md` |

```sh
# ① 文档源(渲染页会丢表格,必须取 raw)
curl -sO https://raw.githubusercontent.com/github/docs/main/content/copilot/reference/hooks-reference.md
curl -sO https://raw.githubusercontent.com/github/docs/main/content/copilot/how-tos/copilot-cli/customize-copilot/use-hooks.md
curl -sO https://raw.githubusercontent.com/github/docs/main/content/copilot/reference/copilot-cli-reference/cli-config-dir-reference.md

# ② 程序本体:npm 主包只是 loader,真身在按平台分发的 optionalDependencies 里
curl -s https://registry.npmjs.org/@github/copilot | jq -r '.versions["1.0.80"].optionalDependencies'
curl -sLO https://registry.npmjs.org/@github/copilot-win32-x64/-/copilot-win32-x64-1.0.80.tgz  # 168 MB
tar -xzf copilot-win32-x64.tgz package/app.js                                                  # 9.2 MB

# ③ changelog
curl -sO https://raw.githubusercontent.com/github/copilot-cli/main/changelog.md
```

**版本**:`@github/copilot` 的 `dist-tags.latest` = `1.0.80`(`prerelease` = `1.0.81-12`);
`changelog.md` 首行是 `## 1.0.80 - 2026-08-14`。下文一切源码引文都出自这一版。

> **两个坑,留档。**
> 1. **`gh copilot` 与 `copilot` 不是一个东西**,§4.6 已经分过一次,这里不重复:本文件量的只有独立
>    `copilot` CLI(npm `@github/copilot`,仓 `github/copilot-cli`)。
> 2. **`app.js` 里的 ESC 写作 `\x1B` 大写 B**(232 处),小写 `\x1b` 只有 30 处。
>    只搜小写会得到「这个 TUI 一个转义序列都不发」的荒谬结论。

## 1. 家族:A 型两个子类型 —— 现在有原文了

### 1.1 hooks 机制:有,而且是本块见过最完整的一套

**URL**:`https://docs.github.com/en/copilot/reference/hooks-reference`
(源:`https://raw.githubusercontent.com/github/docs/main/content/copilot/reference/hooks-reference.md`)
**抓取**:2026-08-26

开篇一句就把性质定死了:

> Hooks are external commands that execute at specific lifecycle points during a session, enabling custom automation, security controls, and integrations.

**跑不跑外部命令:跑。** 命令 hook 的字段表逐字:

> ### Command hooks
>
> Command hooks run shell scripts and are supported on all hook types.

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "type": "command",
        "bash": "YOUR_BASH_COMMAND",
        "powershell": "YOUR_POWERSHELL_COMMAND",
        "cwd": "OPTIONAL/WORKING/DIRECTORY",
        "env": { "VAR": "VALUE" },
        "timeoutSec": 30
      }
    ]
  }
}
```

| 字段 | 原文 |
|---|---|
| `bash` | "Shell command for Unix." |
| `powershell` | "Shell command for Windows." |
| `command` | "Cross-platform fallback. Copied to both `bash` and `powershell` when those fields are absent; explicit `bash` or `powershell` entries take precedence on their respective platforms." |
| `timeoutSec` | "Timeout in seconds. Default: `30`." |
| `type` | "Hook type. Defaults to `\"command\"` when omitted." |

**`bash` / `powershell` 两栏并列,Windows 上走 `powershell`** —— 这一条对 Folio 的安装器是可施工事实:
模板里两栏都要写,不能只写 `command`。

### 1.2 配置文件路径:六处,逐字

> Hooks are loaded from the following sources in order (policy, then user, then project, then plugins) and combined. When the same event appears in multiple sources, all hook entries from all sources are run.
>
> * **Policy-level hook files** — JSON files in the platform-appropriate policy directory, loaded in alphabetical order. Policy hooks are machine-wide and load before all other hooks. They cannot be disabled by `disableAllHooks` and are available regardless of folder trust state.
> * **Repository-level hook files** — `.github/hooks/*.json` in the repository root.
> * **User-level hook files** — `*.json` files in the user-level hooks directory. By default this is `~/.copilot/hooks/` on macOS and Linux, or `%USERPROFILE%\.copilot\hooks\` on Windows. If `COPILOT_HOME` is set, it is `$COPILOT_HOME/hooks/`.
> * **Inline `hooks` block in repository settings** — the `hooks` field at the top level of `.github/copilot/settings.json` (Git committed) or `.github/copilot/settings.local.json` (typically gitignored and user specific) in the repository. Cross-tool `.claude/settings.json` and `.claude/settings.local.json` files in the repository are also read.
> * **Inline `hooks` block in user-level config** — the `hooks` field at the top level of `~/.copilot/settings.json`.
> * **Hooks contributed by installed plugins** — declared by each plugin in its own `hooks.json` (or under `hooks/hooks.json`) inside the plugin's installation directory.

**Folio 只写 `~/.copilot/hooks/` 这一处。** plan §10.4.3 / §10.6 第 5 条那条纪律
(「只写用户级配置,绝不从工作区/仓库发现或加载 hook」)在 copilot 上一字不改地成立,
而且**这一家的仓内攻击面比别家宽**:`.github/hooks/*.json`、`.github/copilot/settings.json`、
`.github/copilot/settings.local.json`,外加**它还读别人家的** `.claude/settings.json` 与
`.claude/settings.local.json`。

### 1.3 事件表:13 个,逐字

> | Event | Fires when | Output processed | Cloud agent |

| 事件(原文拼写) | Fires when(原文) | Output processed(原文) |
|---|---|---|
| `agentStop` | "The main agent finishes a turn." | "Yes — can block and force continuation." |
| `errorOccurred` | "An error occurs during execution." | "No" |
| `notification` | "Fires asynchronously when the CLI emits a system notification (shell completion, agent completion or idle, permission prompts, elicitation dialogs). Fire-and-forget: never blocks the session. Supports a `matcher` regex pattern (the value of the `matcher` field) on `notification_type`." | "Optional — can inject `additionalContext` into the session." |
| `permissionRequest` | "Fires before the permission service runs (rules engine, session approvals, auto-allow/auto-deny, and user prompting). …" | "Yes — can allow or deny programmatically." |
| `postToolUse` | "After each tool completes successfully." | "Yes — can modify the tool result or inject additional context for the model." |
| `postToolUseFailure` | "After a tool completes with a failure." | "Yes — can provide recovery guidance via `additionalContext` (exit code `2` for command hooks)." |
| `preCompact` | "Context compaction is about to begin (manual or automatic). …" | "No — notification only." |
| `preToolUse` | "Before each tool executes." | "Yes — can allow, deny, or modify." |
| `sessionEnd` | "The session terminates." | "No" |
| `sessionStart` | "A new or resumed session begins." | "Optional — can inject `additionalContext` into the session." |
| `subagentStart` | "A subagent is spawned (before it runs). …" | "Optional — cannot block creation, but `additionalContext` is prepended to the subagent's prompt." |
| `subagentStop` | "A subagent completes." | "Yes — can block and force continuation." |
| `userPromptSubmitted` | "The user submits a prompt." | "Optional—`modifiedPrompt` is honored only by SDK programmatic hooks." |
| `userPromptTransformed` | "Fires after the runtime transforms a submitted prompt into its model-facing content… System notifications never trigger it." | "Yes — can rewrite the model-facing content." |

**两套拼写并存**,文档逐字:

> * **camelCase format** — Configure the event name in camelCase (for example, `sessionStart`). Fields use camelCase.
> * **{VS Code} compatible format** — Configure the event name in PascalCase (for example, `SessionStart`). Fields use snake_case to match the {VS Code} {Copilot} extension format.

小节标题里成对给出的别名:`sessionStart` / `SessionStart`、`sessionEnd` / `SessionEnd`、
`userPromptSubmitted` / `UserPromptSubmit`、`preToolUse` / `PreToolUse`、`postToolUse` / `PostToolUse`、
`postToolUseFailure` / `PostToolUseFailure`、`agentStop` / `Stop`、`subagentStop` / `SubagentStop`、
`errorOccurred` / `ErrorOccurred`、`preCompact` / `PreCompact`;
`userPromptTransformed`、`subagentStart`、`notification` **无 PascalCase 别名**。
`permissionRequest` 的 PascalCase 形是 `PermissionRequest`(见 §1.5)。

### 1.4 `notification` hook:payload 逐字

> ## `notification` hook
>
> > [!NOTE]
> > **{Copilot CLI} only.** The `notification` hook does not fire under {Copilot cloud agent}.
>
> The `notification` hook fires asynchronously when the CLI emits a system notification. These hooks are fire-and-forget: they never block the session, and any errors are logged and skipped.
>
> **Input:**
>
> ```typescript
> {
>     sessionId: string;
>     timestamp: number;
>     cwd: string;
>     hook_event_name: "Notification";
>     message: string;           // Human-readable notification text
>     title?: string;            // Short title (e.g., "Permission needed", "Shell completed")
>     notification_type: string; // One of the types listed below
> }
> ```

**六个子类型,逐字:**

> | Type | When it fires |
> |------|---------------|
> | `shell_completed` | A background (async) shell command finishes |
> | `shell_detached_completed` | A detached shell session completes |
> | `agent_completed` | A background subagent finishes (completed or failed) |
> | `agent_idle` | A background agent finishes a turn and enters idle state (waiting for `write_agent`) |
> | `permission_prompt` | The agent requests permission to execute a tool |
> | `elicitation_dialog` | The agent requests additional information from the user |

输出与 matcher:

> **Output:**
>
> ```typescript
> {
>     additionalContext?: string; // Injected into the session as a user message
> }
> ```
>
> **Matcher:** Optional regex on `notification_type`. The regex pattern is the value of the `matcher` field, anchored as `^(?:PATTERN)$`. Omit `matcher` to receive all notification types.

**源码侧的对照(`app.js` 逐字)** —— 三处 `fireNotificationHook` 的调用点,把上面的表钉在实现上:

```js
async fireNotificationHook(e,n,r){let o=await this.nativeHookProcessor?.event("notification",
  {message:e,notificationType:n,...r!==void 0&&{title:r}},this.agentId??this.sessionId);
  typeof o?.additionalContext=="string"&&o.additionalContext&&this.send({prompt:o.additionalContext,prepend:!…
```

```js
notifyPermissionPrompt(e){this.fireNotificationHook(e,"permission_prompt","Permission needed")
  .catch(n=>{E.debug(`Failed to fire permission prompt notification hook: ${String(n)}`)})}
```

```js
requestElicitation:q.toolConfig.enableRequestElicitation?Ge=>(
  this.fireNotificationHook(Ge.message??"Information requested","elicitation_dialog","Information requested")
  .catch(Fe=>{E.debug(`Failed to fire elicitation notification hook: ${String(Fe)}`)}),
  this.pendingRequests.requestElicitation(Ge)):void 0
```

```js
L=j=>(this.fireNotificationHook(h.sessionDescribePermissionRequest(JSON.stringify(j)),
  "permission_prompt","Permission needed")
  .catch(V=>{E.debug(`Failed to fire permission prompt notification hook: ${String(V)}`)}),
  this.requestPermissionWithHooks(j))
```

**这四段是本次取证的主证据**:`permission_prompt` 的触发点**紧挨着 `requestPermissionWithHooks(j)`**
—— 也就是「把批准框摆到用户面前」那一步本身;`elicitation_dialog` 的触发点紧挨着
`pendingRequests.requestElicitation(Ge)`。**它们不是「刚才那件事完了」,是「现在停下等你」。**

### 1.5 `permissionRequest` hook:**语义对口,但不能当 wait 行**

> The `permissionRequest` hook fires before the permission service runs—before rule checks, session approvals, auto-allow/auto-deny, and user prompting. If hooks return `behavior: "allow"` or `"deny"`, that decision short-circuits the normal permission flow. Returning nothing falls through to normal permission handling.

> All configured `permissionRequest` hooks run for each request (except `read` and `hook` permission kinds, which short-circuit before hooks). Hook outputs are merged with later hook outputs overriding earlier ones.

> | Field | Values | Description |
> |-------|--------|-------------|
> | `behavior` | `"allow"`, `"deny"` | Whether to approve or deny the tool call. |
> | `message` | string | Reason fed back to the LLM when denying. |
> | `interrupt` | boolean | When `true` combined with `"deny"`, stops the agent entirely. |

**两条各自独立的取消资格理由:**

1. **它在用户被问之前就开火,而且对根本不会问到用户的请求也开火。** 原文是
   "fires before the permission service runs—before rule checks, session approvals, auto-allow/auto-deny,
   **and user prompting**"。一条被规则自动放行的 `view` 也会点亮它。
   把它记成 wait 就是把「程序想干点什么」当成「程序在等你」—— plan §2.3 花了四段录音否掉的正是这一类。
2. **它是同步决定门**,输出直接决定 allow/deny。plan §10.4.3 那条「信号 hook 不能把决定门拖住」
   的施工约束在这里是硬约束:
   > For command hooks, exit code `2` is treated as a deny; stdout JSON (if any) is merged with `{"behavior":"deny"}`, and stderr is ignored.
   > **Exception: `preToolUse` command hooks are fail-closed on exit `2` and on non-timeout errors**…
   一个把 `folio attention wait` 挂上去的用户,如果那条命令崩了,**会把工具调用拒掉**。

**所以 copilot 的 primary wait 层走 `notification`,不走 `permissionRequest`。**
这与 Claude Code 正相反(那边 `PermissionRequest` 是 primary、`Notification.permission_prompt` 是六秒兜底)。
**原因是文档写死的:copilot 的 `notification` "never blocks the session",而它的 `permissionRequest` 会。**

`permissionRequest` **没有 input payload 小节** —— 文档的「Hook event input payloads」一章里
`sessionStart` / `sessionEnd` / `userPromptSubmitted` / `userPromptTransformed` / `preToolUse` /
`postToolUse` / `postToolUseFailure` / `agentStop` / `subagentStart` / `subagentStop` / `errorOccurred` /
`preCompact` **各有一节,唯独 `permissionRequest` 没有**。能确认的字段只有正文里点名的两个:
`toolName`(matcher 打在它上面)与 `toolInput`(`requestSandboxBypass: true` 在它里面)。
**其余字段:未找到。**

### 1.6 clear 侧:两条的原文

> ### `userPromptSubmitted` / `UserPromptSubmit`
>
> **camelCase input:**
>
> ```typescript
> {
>     sessionId: string;
>     timestamp: number;
>     cwd: string;
>     prompt: string;
> }
> ```
>
> **{VS Code} compatible input:**
>
> ```typescript
> {
>     hook_event_name: "UserPromptSubmit";
>     session_id: string;
>     timestamp: string;      // ISO 8601 timestamp
>     cwd: string;
>     prompt: string;
> }
> ```

> ### `agentStop` / `Stop`
>
> **camelCase input:**
>
> ```typescript
> {
>     sessionId: string;
>     timestamp: number;
>     cwd: string;
>     transcriptPath: string;
>     stopReason: "end_turn";
>     stop_hook_active: boolean; // true when this turn was already forced to continue by a prior "block" decision from this hook
> }
> ```

`sessionEnd` 的触发时机原文是 "The session terminates.";它的 `reason` 取值在 cloud agent 那一栏里被点了名:

> `reason` is typically `"complete"`, `"error"`, or `"timeout"`; `"abort"` and `"user_exit"` are not expected because there is no user.

**注意这句话说的是 cloud agent。** CLI 侧 `reason` 的完整取值表:**未找到**。

### 1.7 `agent_idle`:§12.3 那笔挂账,可以结了

plan §12.3 / `attention_map.rs` 模块头把 `agent_idle` 撤下来时写的条件是:

> 要收它,须先逐字取证它说的是「在等你输入」而不是「安静了 N 秒」

**逐字取回来了,而且答案是第三种,两边都不是:**

> | `agent_idle` | A background agent finishes a turn and enters idle state (waiting for `write_agent`) |

`write_agent` 是**父会话对子 agent 说话的工具**,不是用户的键盘。源码侧的三处对照:

```js
case"agent_idle":return `Agent ${t.agentId} idle`;
```

```js
async sendBackgroundAgentIdleNotification(e,n){let{id:r,agentType:o,description:s}=e,
  a={type:"agent_idle",agentId:r,agentType:o,description:s},
  l=`Agent "${r}" (${o}) has finished processing and is now idle.`,…
```

```js
case"agent_idle":return{text:`Background agent "${t.description??t.agentId}" (${t.agentType}) completed.`};
```

**结论:`agent_idle` 是「一个后台子 agent 干完了」,不是「有人在等你」。撤销成立,而且撤得比原来的理由更硬**
—— 原来的理由是「与 Claude Code 的 `idle_prompt` 同名同型」(类比),现在是原文(直证)。
**这笔挂账可以从 §12.5 的三件里划掉一件。**

同一条判据下 `agent_completed` / `shell_completed` / `shell_detached_completed` 也全部是完成态:

> | `agent_completed` | A background subagent finishes (completed or failed) |
> | `shell_completed` | A background (async) shell command finishes |
> | `shell_detached_completed` | A detached shell session completes |

## 2. 终端里发什么:逐字节

### 2.1 全表(`app.js` 1.0.80,9,173,451 字节)

| 字面量 | 命中 | 结论 |
|---|---|---|
| `\x07`(转义写法) | 49 | **发**,见 §2.2 |
| `` | 21 | 同上,多为正则字符类 |
| 裸 `0x07` 字节 | 0 | bundler 一律写转义形 |
| ESC(`\x1B`/`\x1b`/`\033`) | 300 | — |
| `ESC ] 9 ; 4` | **2** | **发**,见 §2.3 |
| `ESC ] 9 ;`(OSC 9 通知) | 2 | **两次都是上面那两条 `9;4`**,没有 OSC 9 文本通知 |
| `ESC ] 777` | **0** | **不发** |
| `ESC ] 1337` | **0** | **不发** |
| `RequestAttention` | **0** | **不发** |
| `ESC ] 99 ;` | **0** | **不发** |
| `ESC ] 133 ;` | **0** | **不发** |
| `ESC ] 0 ;` | 1 | 标题,见 §2.4 |
| `ESC ] 8 ;` | 15 | 超链接 |
| `?1004` | 3 | **开焦点上报**,见 §2.5 |
| `?1049` | 6 | **住备用屏**,见 §2.5 |
| `MessageBeep` | **0** | 见 §2.2 的裁决 |
| `PlaySound` | 1 | **是 highlight.js 的 LSL 语法关键字表**,与发声无关 |
| `Action Required` | **0** | 标题里没有等待态字面量 |

### 2.2 裸 BEL:**它写的是真的 `0x07`,不是 Win32 声音 API**

序列表与写出口,逐字:

```js
ao={ALT_SCREEN_ON:"\x1B[?1049h",ALT_SCREEN_OFF:"\x1B[?1049l",BPASTE_ON:"\x1B[?2004h",
BPASTE_OFF:"\x1B[?2004l",FOCUS_ON:"\x1B[?1004h",FOCUS_OFF:"\x1B[?1004l",…
MODIFY_OTHER_KEYS_OFF:"\x1B[>4;0m",BEEP:"\x07",TITLE_PUSH:"\x1B[22;0t",TITLE_POP:"\x1B[23;0t",
PROGRESS_INDETERMINATE:"\x1B]9;4;3;0\x07",PROGRESS_OFF:"\x1B]9;4;0;0\x07",…}
```

```js
beep(){this._suspended||this.writeOut(ao.BEEP,this.stdout)}
```

> **§4.6 那条「Windows 上走 `MessageBeep()`/`PlaySound()`,标为存疑」的猜测,可以撤了。**
> `app.js` 里 `MessageBeep` **零命中**,`PlaySound` 唯一那次在一张语法高亮关键字表里。
> 铃就是往 `stdout` 写一个 `0x07`。那条存疑的来源是 issue #1458 的**报告人自己的猜测**,
> 见 §3.2 —— 而那是 `v0.0.410-1`(2026-02),距今半年、隔着 60 个版本。

**但铃说的是什么,才是本块要的那句话。** 铃只有一个调用点:

```js
function rk(t,e=!0,n=!0){let r=useRef(null),o=useRef(!1),
  s=useCallback(()=>{e&&Bi().beep()},[e]);
  return useEffect(()=>{ … t.on("user.message",l=>{ … }) … },[t]),
  useEffect(()=>{if(!t||!e)return;let a=t.on("session.idle",()=>{
    if(r.current!==null&&clearTimeout(r.current),!n&&o.current){r.current=null;return}
    r.current=setTimeout(()=>{r.current=null,s()},100)});…},[t,e,n,s]),
  useEffect(()=>{ … t.on("assistant.turn_start",()=>{r.current!==null&&(clearTimeout(r.current),r.current=null)});…},[t]),s}
```

而它返回的那个 `s`,在装配处被**和桌面通知拼成一个复合回调**,再交给六个「等你」队列:

```js
let l$=rk(ee,kn.beep===!0,kn.beepOnSchedule!==!1);
let uj=Kan(ee,kn.notifications===!0,{sessionTitle:By}),
    gB=useCallback(P=>{l$(),uj(P)},[l$,uj]);
… =sln(gB,UI,x2n),Mj=qRn(ee,Rd,kn.beep===!0,gB),YA=NRn(ee,Rd,kn.beep===!0,gB),
Kp=hln(kn.beep===!0,gB),Uy=lln(kn.beep===!0,gB),…
```

**所以同一声铃有两个出处:`session.idle`(回合结束,100ms 去抖,被 `assistant.turn_start` 撤销)
与六个批准/追问队列(真在等你)。** 它们在 tty 上是**同一个字节**,没有任何区分位。
这与维护者在 issue #2456 的回话对得上(§3.2)。

> **裁决:BEL 在 copilot 上是 `Bell` 那一档,不能升 `Awaiting`。**
> 与 aider(plan §11.10.3 里 B′ 型那一格)同型同因:**事件不是状态**,而且这一家更糟 ——
> aider 的铃至少只说一件事,copilot 的铃说两件事而不说是哪件。

### 2.3 OSC 9;4:**真发,但被一张终端白名单挡在 Folio 外面**

```js
PROGRESS_INDETERMINATE:"\x1B]9;4;3;0\x07", PROGRESS_OFF:"\x1B]9;4;0;0\x07"
```

```js
setProgress(e){!this.acceptsOsc()||!kDe()||(this.state.progress=e,!this._suspended&&this.writeOut(ao.PROGRESS_INDETERMINATE,this.stdout))}
clearProgress(){!(this.state.progress==="indeterminate")||!this.acceptsOsc()||!kDe()||(this.state.progress="off",!this._suspended&&this.writeOut(ao.PROGRESS_OFF,this.stdout))}
acceptsOsc(){return this.stdout.isTTY===!0&&process.env.TERM!=="dumb"}
```

`kDe()` 就是那张白名单:

```js
Zzn=new Set(["ghostty","kitty","rio","foot","alacritty","iterm2","wezterm","windows-terminal","vscode","vscode-insiders","cursor","windsurf"])
```

```js
function kDe(){let t=w6();return t==="apple-terminal"?!1:t!==null&&Zzn.has(t)}
```

终端身份怎么定的(env 优先,拿不到再发 XTVERSION 探询):

```js
function Pwt(){let t=process.env.TERM_PROGRAM,e=process.env.TERM,n=(process.env.VSCODE_GIT_ASKPASS_MAIN||"").toLowerCase();
return process.env.CURSOR_TRACE_ID||n.includes("cursor")?"cursor":n.includes("windsurf")?"windsurf":
n.includes("code")?(n.includes("insiders")?"vscode-insiders":"vscode"):
t==="vscode"||process.env.VSCODE_GIT_IPC_HANDLE?"vscode":process.env.TMUX?null:
process.env.WT_SESSION?"windows-terminal":t==="Apple_Terminal"?"apple-terminal":
t==="iTerm.app"?"iterm2":t==="WezTerm"?"wezterm":t==="ghostty"?"ghostty":t==="rio"?"rio":
e==="xterm-kitty"?"kitty":e==="xterm-ghostty"||e==="ghostty"?"ghostty":…
```

配套文档原文(`cli-config-dir-reference.md` 的用户设置表):

> | `terminalProgress` | `boolean` | `true` | Emit OSC 9;4 terminal progress indicators while the agent is working. Supported terminals include Windows Terminal, iTerm2, Ghostty, and ConEmu. |

程序自带的 `copilot help config` 文本(从 `app.js` 逐字取出):

> `terminalProgress`: whether to emit terminal progress indicators (OSC 9;4) while the agent is working; defaults to `true`.
>   - Shows an indeterminate progress indicator in the terminal title bar or tab on supported emulators (Windows Terminal, iTerm2, Ghostty, ConEmu)
>   - Set to `false` to suppress these escape sequences on terminals that render them as visible artifacts

> **这是 codex 那张 `notification_method = auto` 白名单在第二家身上原样重演**
> (`evidence-cli-survey-2026-08-25.md` §2.4)。**Folio 不在 `Zzn` 里**,而 `Zzn` 是一个闭集合 ——
> 靠答 XTVERSION 也进不去,因为解析器 `Bwt` 只认那十几个已知前缀。
> **后果:copilot 在 Folio 里一个 `9;4` 都不会发。** plan §11.10.1 那句
> 「pi 与 copilot CLI 都真发 `9;4`,进度环这条通道**有源**」**要就地补一个限定**:
> copilot 那一源**今天对 Folio 是关的**,要开得走 §11.10.5 的上游诉求账。

### 2.4 标题:`OSC 0`,身份不带忙闲

```js
_wt=t=>`\x1B]0;${t}\x07`
```

```js
setTitle(e){if(!this.acceptsOsc())return;let n=zzn(e);if(this.state.title=n,this._suspended)return;
let r="";this.state.titlePushed||(r+=ao.TITLE_PUSH,this.state.titlePushed=!0),r+=_wt(n),this.writeOut(r,this.stdout)}
```

```js
function i0n(t,e=!0,n){useEffect(()=>{if(!e)return;let r=Bi();return()=>{r.restoreTitle()}},[e]),
useEffect(()=>{if(!e)return;let r=Bi();
  t?r.setTitle(n?`${n} - ${t} - GitHub Copilot`:`${t} - GitHub Copilot`)
   :r.setTitle(n?`${n} - GitHub Copilot`:"GitHub Copilot")},[t,e,n])}
```

自带 help 文本逐字:

> `updateTerminalTitle`: whether to update the terminal tab/window title with current intent; defaults to `true`.
>   - Shows what the agent is currently working on in the terminal title bar
>   - Only works on supported terminal emulators (xterm, iTerm, Windows Terminal, etc.)
>   - Can also be disabled via the `COPILOT_DISABLE_TERMINAL_TITLE` environment variable

```js
i0n(G2, kn.updateTerminalTitle!==!1 && Ts.env.COPILOT_DISABLE_TERMINAL_TITLE!=="1"
     && Ts.env.COPILOT_DISABLE_TERMINAL_TITLE?.toLowerCase()!=="true", By)
```

**`Action Required` 在整份 bundle 里零命中。** 标题是「在做什么 - 会话名 - GitHub Copilot」,
**没有 codex 的三态机,也没有 gemini-cli 的 `✋`。C 型(标题嗅探)在这一家上无对象可嗅。**

### 2.5 焦点上报与备用屏

```js
FOCUS_ON:"\x1B[?1004h", FOCUS_OFF:"\x1B[?1004l"
enableFocusReporting(){this.state.focusReporting||(this.state.focusReporting=!0,this.queue.push(ao.FOCUS_ON))}
```

启动序列(逐字):

```js
De.enterAltScreen(),De.enableRawMode(),De.enableBracketedPaste(),De.enableFocusReporting(),
Qe||De.enableMouse(Na()?"any-event":"button"),De.hideCursor(),…
```

**两条事实:**

1. **copilot 住备用屏**(`enterAltScreen()`,`?1049h`)。这与 Claude Code 同、与 codex 反。
   plan §11.10.4 那道开放题 B1(第八记号 `Attached` 的判据是 `alternate_screen == true`)
   **在 copilot 上是亮的** —— 也就是说那个记号覆盖的是「住备用屏的那一半」,copilot 落在亮的那半边。
   **这不改判据,只是给 B1 多一个数据点。**
2. **copilot 开 `?1004h`,而且拿它当门 —— 但门开在哪一侧,与 codex 相反。**

焦点门的全文:

```js
function Kan(t,e=!0,n={}){let r=useRef(null),o=useRef(n);o.current=n;let s=useRef(null),a=useRef(null);
useEffect(()=>{let c=Bi(),d=()=>{s.current=!0},u=()=>{s.current=!1};
  return c.on("focus",d),c.on("blur",u),()=>{c.off("focus",d),c.off("blur",u)}},[]);
let l=useCallback(c=>{e&&s.current!==!0&&WWe(tzr(c,o.current))},[e]);
…
let c=t.on("session.idle",()=>{ … r.current=setTimeout(()=>{r.current=null,
  s.current!==!0&&WWe(nzr(o.current,a.current))},100)});…}
```

**`s.current` 的初值是 `null`,门写的是 `s.current !== true`。**
Folio 一个焦点字节都不发 ⇒ 它永远停在 `null` ⇒ **门恒开**。

> **这一条把 `evidence-cli-survey-2026-08-25.md` §5 的第 3 条横向事实补了一个反例。**
> 前三家(Claude Code / codex / opencode)的焦点门初值都等价于「你正看着呢」,于是 Folio 不发焦点上报就挨闷棍;
> **copilot 的初值等价于「不知道,那就通知」。** 片 A0.5(发焦点上报)对这一家**不是修复,是收紧** ——
> 装了之后 Folio 里的 copilot 反而会在前台时闭嘴。
> **这不改片 A0.5 的裁决**(它对三家是修复,而且发焦点上报本来就是终端该做的事),
> 但它是一条**必须记下来的副作用**:片 A0.5 落地后要复测 copilot,别把一条今天通着的路修断。
>
> 需要说清成色:**这道门管的是 OS 桌面 toast,不是 tty**(见 §2.6),所以 Folio 今天既收不到那条 toast,
> 也不因它闭嘴 —— 副作用落在**用户的桌面通知**上,不落在 Folio 的账本上。**铃(§2.2)没有焦点门。**

### 2.6 桌面通知:走 OS toast,**一个字节都不上 tty**

```js
function WWe(t){if(process.env.COPILOT_DISABLE_DESKTOP_NOTIFICATIONS==="1")return;
let e=Date.now(),n=jan.get(t.kind),r=t.kind==="attention"?Hqr:Gqr;
if(n!==void 0&&e-n<r)return;let o=Vqr();
if(o)try{o.showDesktopNotification(Jqr(t))&&jan.set(t.kind,e)}catch{}}
var Yqr="Agent finished",Zqr="Needs your attention",Xqr="Copilot";
function tzr(t,e){let n=SAe(t?.activityLabel)??Zqr;return{kind:"attention",summary:Qan(e),body:Van(n,SAe(t?.body))}}
function nzr(t,e){return{kind:"idle",summary:Qan(t),body:Van(Yqr,ezr(e))}}
```

自带 help 文本逐字:

> `notifications`: whether to show OS notifications when user attention is required and when the agent finishes; defaults to `false`.
>   - Set to `true` to enable desktop toasts. Toasts are suppressed while the terminal window already has focus.
>   - Clicking a toast attempts to focus the terminal window that raised it where supported; multi-tab terminals (e.g. Windows Terminal) can't switch to the specific tab.

**这里有一个对本块极其重要的事实:copilot 内部把两件事分得清清楚楚 ——
`kind:"attention"`(默认文案 `"Needs your attention"`)与 `kind:"idle"`(文案 `"Agent finished"`)。**
`attention` 那一路的 `activityLabel` 七个取值,全部逐字:

| `activityLabel`(原文) | 触发处(源码逐字) |
|---|---|
| `"Needs permission"` | `t({activityLabel:"Needs permission",body:szr(N.request)})` — 权限请求队列 |
| `"Needs permission"` | `t({activityLabel:"Needs permission",body:azr(N.request)})` — hook 门控权限队列 |
| `"Needs path access"` | `t({activityLabel:"Needs path access",body:`${N.kind} ${izr(N.paths)}`})` |
| `"Needs URL access"` | `t({activityLabel:"Needs URL access",body:`${N.intention}: ${ozr(N.url)}`})` |
| `"Needs input"` | `t({activityLabel:"Needs input",body:N.request.question})` — `ask_user` |
| `"Needs information"` | `t({activityLabel:"Needs information",body:N.request.message})` — elicitation |
| `"Needs approval"` | 三处:`"Auto mode switch"` / `"Session limit reached"` / `` `MCP sampling from ${A.serverName}` `` |

> **这是 §3 那问「有没有『等待批准』这一状态的可观测信号」的正面答案:有,而且是一等公民。**
> **但它出不了这台机器的桌面** —— `showDesktopNotification` 是 OS toast,不是 OSC。
> Folio 只能通过 §1.4 的 hook 拿到其中两个(`permission_prompt` / `elicitation_dialog`);
> **另外五个(路径、URL、`ask_user`、auto-mode、MCP sampling)今天没有任何一条通往 tty 或 hook 的路。**
> 这一条进 §5 的上游诉求账。

### 2.7 从头到尾**不发**的

`OSC 777`、`OSC 1337`(含 `RequestAttention`)、`OSC 99`、`OSC 133` —— 四条在 `app.js` 里**逐条零命中**。
`OSC 9` 的两次命中**全部是 `9;4` 进度**,**没有 OSC 9 文本通知**。

**与 `evidence-cli-survey-2026-08-25.md` §5 的两条横向事实一致,并把样本从八家变成八家实证一家更硬:**
`OSC 1337;RequestAttention` **仍然一家都不发**;`OSC 133` **仍然一家都不发**。

## 3. 官方 changelog 与 issue:时间线与两处纠错

### 3.1 changelog 里与本块相关的每一行(逐字,带版本)

| 版本 | 日期 | 原文 |
|---|---|---|
| `0.0.407` | 2026-02-11 | "Terminal title works on all TTY terminals, not just select few" |
| `0.0.411` | 2026-02-17 | "Terminal bell rings once when agent finishes, not on every tool completion" |
| `1.0.18` | 2026-04-04 | "Add notification hook event that fires asynchronously on shell completion, permission prompts, elicitation dialogs, and agent completion" |
| `1.0.23` | 2026-04-10 | "Shell output with BEL characters no longer causes repeated terminal beeping" |
| `1.0.26` | 2026-04-14 | "Permission prompt notification hook only fires when a prompt is actually shown to the user" |
| `1.0.28` | 2026-04-16 | "Support COPILOT_DISABLE_TERMINAL_TITLE environment variable to opt out of terminal title updates" |
| `1.0.51` | 2026-05-20 | "Add `terminalProgress` setting to enable or disable OSC 9;4 terminal progress indicators" |
| `1.0.55` | 2026-05-28 | "Terminal bell no longer sounds on turn completion unless explicitly enabled via config" |
| `1.0.60` | 2026-06-05 | "Session completion signal (terminal beep, autopilot continuation) now waits for background shell commands to finish" |
| `1.0.61` | 2026-06-09 | "Add `beepOnSchedule` setting to disable completion beeps for scheduled `/every` and `/after` runs" |
| `1.0.66` | 2026-06-30 | "Show desktop notifications on macOS from the CLI" |
| `1.0.66` | 2026-06-30 | "Add desktop notifications for attention prompts and idle sessions" |
| `1.0.70` | 2026-07-09 | "Fix a crash on Windows triggered by desktop toast notifications" |

**`1.0.26` 那一行值得单独看:** 它修的正是 issue #2586
(`https://github.com/github/copilot-cli/issues/2586`,开于 2026-04-08,已 closed/completed,报的是 `1.0.21`):

> The `Notification` hook with `notification_type: "permission_prompt"` fires for every tool execution in interactive mode, even when the tool has already been approved for the session.
>
> `Notification(permission_prompt)` should only fire when the user is actually presented with a permission prompt — i.e., when the tool has not yet been approved and user input is required.

**所以「`permission_prompt` = 用户真的被问了」这条语义,是 `1.0.26`(2026-04-14)之后才成立的。**
Folio 的适配器**要么记下最低版本 `1.0.26`,要么接受早于它的版本会误报**。
**这一条建议写进模板的版本门,不建议默默接受。**

### 3.2 `beep`:一处存疑撤销,一处**文档与程序对不上**

**存疑撤销。** `evidence-cli-survey-2026-08-25.md` §4.6 的那条「Windows 上走 Win32 `MessageBeep()`」
出自 issue #1458(`https://github.com/github/copilot-cli/issues/1458`,2026-02-13,已 closed):

> The sound plays SystemHand (Windows Foreground.wav) and falls back to the .Default beep (Windows Background.wav). This is not a terminal bell (BEL/\x07) — the bellStyle: "none" Windows Terminal setting and AudibleBell=0 console registry key have no effect. **The CLI appears to be calling the Win32 MessageBeep() or PlaySound() API directly.**

**这是报告人的推测**(原文就写着 "appears to"),版本是 `v0.0.410-1`。§2.2 的源码搜证否掉了它在 `1.0.80` 上的成立。
另有反证:issue #3573(`https://github.com/github/copilot-cli/issues/3573`,2026-05-29,Linux/Alacritty)

> In version 1.0.55, the bell character doesn't seem to be sent (I don't get the urgent window hint anymore) even when having `"bell": true` in the settings.

维护者回:

> The setting for enabling the bell is `beep` _not_ `bell` 😄 Could you try changing your settings file to `"beep": true` instead?

报告人回:"It does work with `beep` indeed." —— **`beep: true` 打开后,Alacritty 收到了 urgent window hint。
那是 tty 上的真 BEL。**

**铃到底说什么,维护者在 issue #2456**(`https://github.com/github/copilot-cli/issues/2456`,已 closed)**说了:**

> Setting `beep` to `false` disables all terminal bell notifications, including permission prompts, plan approvals, and agent completion.

—— 与 §2.2 的源码完全吻合:**一声铃,三种以上出处,tty 上不可区分。**

**文档与程序对不上(逐字对照,两边都抄下来):**

| 来源 | 原文 |
|---|---|
| `docs.github.com`(`cli-config-dir-reference.md` 用户设置表) | `\| `beep` \| `boolean` \| **`true`** \| Play an audible beep when attention is required. \|` |
| `docs.github.com`(同表) | `\| `beepOnSchedule` \| `boolean` \| `true` \| Play an audible beep when a scheduled `/every` or `/after` run finishes. \|` |
| **程序自带的 `copilot help config`(1.0.80)** | "`beep`: whether to beep when user attention is required; defaults to **`false`**." |
| 程序自带(同处) | "`beepOnSchedule`: whether to beep when a scheduled `/every` (or `/after`) run finishes; defaults to `true`. - Only applies when `beep` is enabled…" |
| **源码装配处** | `let l$=rk(ee,kn.beep===!0,kn.beepOnSchedule!==!1);` |

`kn.beep===!0` 是**严格等于 `true`**:没写就是关的。**程序侧与它自带的 help 一致,文档表那一格是错的。**
changelog `1.0.55`「no longer sounds on turn completion **unless explicitly enabled via config**」也站在程序这一边。

> **对 Folio 的意义:B 型(把开关打开)在这一家上是必须的,不是可选的。**
> 而且**别信 docs 的默认值列** —— 以程序自带的 `copilot help config` 为准。
> 这条教训与 §4.6 记的 opencode `attention.enabled` 默认关、gemini `enableNotifications` 默认关同型,
> 是**第三次**撞上「装了却不亮」。

### 3.3 一条仍开着的上游诉求(别人已经提过)

issue #1651(`https://github.com/github/copilot-cli/issues/1651`,"Add hook for permission prompt",已 closed):

> Add a similar hook to Copilot CLI. This will allow creating integrations that notify user when Copilot is asking for users permission to run some tool etc.

**这正是 Folio 要的那件事,而且它已经被实现了**(`1.0.18` 加 `notification`,`1.0.26` 修准语义)。
记在这里是为了说明:**这条路是上游认可的用法,不是我们钻的空子。**

## 4. 可直接写成 `attention_map` 行的候选

**入选门槛(与 `attention_map.rs` 模块头同一条):一行要么从上面某段引文来,要么不写。**
`family` 常量按现有命名法取 `"copilot"`(`CLAUDE_CODE` = `"claude-code"`、`CODEX` = `"codex"`、`PI` = `"pi"`)。

### 4.1 `ROWS` —— wait

| 事件名(`event`) | lane / 落点 | via / 依据原文 |
|---|---|---|
| `Notification.permission_prompt` | `WaitKind::Permission`,`MappedAction::Wait { tier: Tier::Primary }`,`id: IdSource::None` | hook,`notification` 事件。"`permission_prompt` \| The agent requests permission to execute a tool";且 "never blocks the session"。源码:`notifyPermissionPrompt(e){this.fireNotificationHook(e,"permission_prompt","Permission needed")…}` 与 `L=j=>(this.fireNotificationHook(…,"permission_prompt","Permission needed")…,this.requestPermissionWithHooks(j))` |
| `Notification.elicitation_dialog` | `WaitKind::Elicitation`,`MappedAction::Wait { tier: Tier::Primary }`,`id: IdSource::None` | hook,`notification` 事件。"`elicitation_dialog` \| The agent requests additional information from the user"。源码:`this.fireNotificationHook(Ge.message??"Information requested","elicitation_dialog","Information requested"),this.pendingRequests.requestElicitation(Ge)` |

**两行都是 `Tier::Primary`,而且这一家只有一层** —— 没有「零延迟事件 + 六秒兜底」那种双层,
所以 `Primary` 是「只有它在那儿」而不是「它赢了兜底」(与 `Notification.agent_needs_input` 那一行同型)。

**`id: IdSource::None`**:`notification` 的 payload 逐字是
`{sessionId, timestamp, cwd, hook_event_name, message, title?, notification_type}` ——
**没有 request id**,`permissionRequest` 的 payload 小节文档里根本不存在。
按 §12.1.6 的规矩,**没有逐字取回标识符就走 level 路**,这不是缺口,是答案。

### 4.2 `ROWS` —— clear

| 事件名(`event`) | lane / 落点 | via / 依据原文 |
|---|---|---|
| `userPromptSubmitted` | `ClearClass::Boundary`,`ClearScope::All`,`ClearReason::Hook`,`begins_turn: true` | "The user submits a prompt." payload 含 `prompt: string` |
| `agentStop` | `ClearClass::Boundary`,`ClearScope::All`,`ClearReason::Hook`,`begins_turn: false` | "The main agent finishes a turn." payload 含 `stopReason: "end_turn"` |
| `sessionEnd` | `ClearClass::Boundary`,`ClearScope::All`,`ClearReason::Hook`,`begins_turn: false` | "The session terminates." |

### 4.3 `TURN_END` —— announcement,**不是 wait**

| 事件名 | via | 依据原文 |
|---|---|---|
| `agentStop` | `Via::Stop` | "The main agent finishes a turn." |

`Stop`/`agentStop` 同时进 `ROWS`(作 clear)与 `TURN_END`(作 announcement),
与 Claude Code 的 `Stop` 完全同型 —— 模块头已经写明这不矛盾。

### 4.4 `OSC_ROWS` —— **零行**

**不列任何 OSC 行,三条理由各有原文:**

1. **裸 BEL 不可分辨**:同一个 `0x07` 既是 `session.idle`(回合结束)又是六个「等你」队列
   (§2.2 的 `rk` 与 `gB` 两段源码,加维护者在 #2456 的原话)。**它已经在 `Via::Bel` 那一档里,不需要新行。**
2. **OSC 9;4 在 Folio 里根本不会发**:`Zzn` 白名单闭集合(§2.3)。
3. **OSC 777 / 1337 / 99 / 133 / OSC 9 文本通知全部零命中**(§2.7)。

### 4.5 **被逐字证据挡在门外的候选**(列出来,是为了下次别再有人提)

| 候选 | 为什么不写 |
|---|---|
| `permissionRequest` / `PermissionRequest` 当 wait | 原文:"fires before the permission service runs—before rule checks, session approvals, auto-allow/auto-deny, **and user prompting**"。它对**根本不会问到用户**的请求也开火。且它是同步决定门,exit 2 = deny(§1.5) |
| `Notification.agent_idle` 当 wait | 原文:"A background agent finishes a turn and enters idle state (waiting for `write_agent`)"。等的是父会话的工具调用,不是用户。**§12.3 的挂账就此结清**(§1.7) |
| `Notification.agent_completed` / `shell_completed` / `shell_detached_completed` 当 wait | 三条原文全部是完成态(§1.7 末) |
| `errorOccurred` 当 clear | 原文只有 "An error occurs during execution.",**说不出这个错误终结了哪一个等待**。与 `StopFailure`("Runs instead of Stop when the turn ends due to an API error")不同型 —— 那一条明说了回合结束 |
| C 型标题嗅探 | `Action Required` 在 bundle 里零命中;标题是 `${intent} - ${session} - GitHub Copilot`,无忙闲无等待(§2.4) |
| 桌面 toast 的七个 `activityLabel` | 是 OS toast,不上 tty 也不进 hook(§2.6)。**七个里只有两个有 hook 出口**,其余五个进 §5 |

### 4.6 装机清单(B 型:让它亮起来)

**不是适配器代码,是安装器与文档要做的事**,每条都有原文:

1. **hook 模板写到 `~/.copilot/hooks/folio.json`**(或 `$COPILOT_HOME/hooks/`),`version: 1`,
   `bash` 与 `powershell` 两栏都写(§1.1、§1.2)。**仓内 `.github/hooks/` 与 `.github/copilot/settings*.json` 一律不写不读。**
2. **`notification` hook 不设 `matcher` 会收到全部六种子类型**("Omit `matcher` to receive all notification types"),
   而我们只要两种 ⇒ **模板必须带 matcher**,建议 `^(?:permission_prompt|elicitation_dialog)$`
   (锚定规则原文:"anchored as `^(?:PATTERN)$`")。
3. **`timeoutSec`**:默认 30 秒;`notification` 是 fire-and-forget,超时只被 "logged and skipped"。
   写一个小值即可,**不要沿用 30**。
4. **版本门 `>= 1.0.26`**(§3.1)。
5. **`beep`**:程序默认 `false`(§3.2)。要不要提示用户开,是一道**产品题**,不是取证题 ——
   但**别照 docs 的表当成默认开的**。
6. **`disableAllHooks`**:用户设置里有 "Disable all hooks (both repository-level and user-level)."
   安装器装完之后如果这个是 `true`,**装了也不亮**,要检出来告诉用户。

## 5. 上游诉求账(不占本块工期)

沿用 `evidence-cli-survey-2026-08-25.md` §7 的格式。**copilot CLI 三条:**

1. **把 `terminalProgress` 的白名单换成能力探询。** `Zzn` 是闭集合(§2.3),
   与 codex 的 `notification_method = auto` 白名单同病:**所有新终端都拿到最差的那一档**。
   这一条与已经记在案的 codex 那条可以合并成同一份诉求文本。
2. **给「等你」那七个 `activityLabel` 一条 tty 或 hook 出口。** 今天 `Needs path access` /
   `Needs URL access` / `Needs input`(`ask_user`)/ `Needs approval`(auto-mode、session limit、MCP sampling)
   **只进 OS toast**(§2.6)。程序内部已经把它们和 `idle` 分得清清楚楚了,只差一个出口。
3. **修 `cli-config-dir-reference.md` 里 `beep` 的默认值**(表里写 `true`,程序自带 help 与源码都是 `false`,§3.2)。
   这条是文档 bug,提 PR 即可。

## 6. 与既有文件的三处出入(交用户复核)

**都不是本文件自己改的,是取证结果与已写下的话对不上,列在这里等裁。**

1. **plan §11.10.1**:「pi 的核心 `TERMINAL_PROGRESS_ACTIVE_SEQUENCE` 与 copilot CLI 的 `terminalProgress`
   **都真发 `9;4`**,进度环这条通道**有源**」。
   **属实,但要补限定**:copilot 那一源**对 Folio 恒关**,因为 Folio 不在 `Zzn` 白名单里(§2.3)。
   建议改成「pi 真发;copilot 会发但被终端白名单挡住,今天 Folio 收不到」。
2. **`evidence-cli-survey-2026-08-25.md` §4.6 的存疑标记**:「Windows 上它走的是 Win32 `MessageBeep()`/`PlaySound()`
   而不是真的往 tty 写 BEL 字节 …… 标为存疑,装上之后必须实测」。
   **可以撤了**:`MessageBeep` 零命中,`beep(){…writeOut(ao.BEEP,…)}` 写的是 `"\x07"`(§2.2、§3.2)。
   那条存疑的来源是报告人的推测,版本 `v0.0.410-1`。
3. **`evidence-cli-survey-2026-08-25.md` §5 第 3 条**:「`?1004` 焦点上报…… **三家把它当闸门**」。
   **copilot 是第四家开 `?1004h` 的,但它的门朝反方向开**(初值 `null`,`s.current !== true` 才通知,§2.5)。
   片 A0.5 落地后**要复测 copilot**,别把一条今天通着的桌面通知路修断。
   —— 这一条是**副作用提示**,不是对片 A0.5 的反对。

## 7. 未找到(明写,不推测)

- **`permissionRequest` hook 的 input payload 字段表**:文档「Hook event input payloads」一章里没有这一节;
  正文只点了 `toolName` 与 `toolInput` 两个字段名。其余字段未找到。
- **CLI 侧 `sessionEnd` 的 `reason` 完整取值**:文档只在 cloud agent 那一栏给了
  `"complete"` / `"error"` / `"timeout"` / `"abort"` / `"user_exit"`,**并明说那是 cloud agent 的情形**。
  CLI 侧的取值表未找到。
- **`notification` payload 里有没有任何请求级标识符**:逐字 payload 是
  `{sessionId, timestamp, cwd, hook_event_name, message, title?, notification_type}` —— **没有**。
- **`title` 字段在 `permission_prompt` / `elicitation_dialog` 上的确切取值是否恒定**:
  源码里两处硬编码为 `"Permission needed"` 与 `"Information requested"`,
  文档只说 `title?: string; // Short title (e.g., "Permission needed", "Shell completed")` —— 是举例,不是枚举。
  **不建议拿 `title` 做分流**,`notification_type` 才是。
- **实测**:本文件**没有一段录音**。本机没装 copilot(`evidence-cli-survey-2026-08-25.md` §0 的盘点,
  `gh extension list` 空、全局 npm 只有 `@playwright/mcp` 与 `pnpm`、两个 WSL 里也没有)。
  上面每一条都是文档 + 官方发布二进制里的源码,**不是运行时观测**。
  §2.5 的焦点门方向、§2.2 的铃双出处,都值得在真装上之后各录一段确认。
