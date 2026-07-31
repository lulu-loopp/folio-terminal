# Claude Code `/rc` OSC 8 发射门诊断

日期：2026-07-31  
Claude Code：2.1.220（Windows native binary）  
结论：Claude Code 的 `/rc` OSC 8 发射门明确接受 `WT_SESSION`、`TERM_PROGRAM=iTerm.app` 和 `FORCE_HYPERLINK=1`；不接受 `WT_PROFILE_ID`、`TERM_PROGRAM=BetterTerminal` 或 `TERM_PROGRAM=WezTerm`（即使给 WezTerm 版本）。这不是 BetterTerminal 的 OSC 8 解析/渲染问题，因为阴性条件的 PTY 字节里根本没有链接序列。

## 方法

- 使用仓库现有 `bt-record`，底层为同一 `portable-pty`/ConPTY 基建。
- 构建命令：`CARGO_INCREMENTAL=0 cargo build --locked --offline -p bt-corpus --bin bt-record`；构建 shell 中移除了 `PSModulePath`。
- ConPTY 实现：`source=sidecar version=1.25.260710002-preview`，来自 `target/debug/conpty.dll`。
- 每个条件启动独立的 `powershell.exe -NoProfile -Command claude`，PTY 尺寸 `120x40`，采集 15 秒。
- 没有向 PTY 写入任何提问或其他用户输入。
- stdout 经二进制 stream copy 写入各条件的 `output.raw`。
- 每轮结束以 recorder PID 为根执行进程树终止；metadata 同时记录杀前后代树、`taskkill` 结果和清理后差分。
- 所有主矩阵条件共享 `TERM=xterm-256color`、`COLORTERM=truecolor`。执行环境最初还继承了 `NO_COLOR=1`，因此另做 k 条件将其真正移除；结果仍为阴性。

## 主矩阵

`OSC8` 是完整 OSC 8 open 的数量；`\x1b]8;;` 是题目指定的精确前缀计数（在这些样本中是每个链接对应的 close）。数量差异来自 TUI 重绘次数，门判定看是否大于 0。

| 条件 | 有效身份变量 | bytes | OSC8 open | `\x1b]8;;` | `/rc` 总出现 | 链接中的 `/rc` | `/rc` 是否链接 |
|---|---|---:|---:|---:|---:|---:|---|
| a | `TERM_PROGRAM=BetterTerminal`, version `0.0.0` | 19,945 | 0 | 0 | 12 | 0 | 否 |
| b | `WT_SESSION=<GUID>`, `WT_PROFILE_ID=<GUID>` | 20,509 | 1 | 1 | 12 | 1 | 是 |
| c | `TERM_PROGRAM=WezTerm` | 22,128 | 0 | 0 | 13 | 0 | 否 |
| d | `TERM_PROGRAM=iTerm.app`, version `3.5.0` | 22,310 | 3 | 3 | 13 | 3 | 是 |
| e | 四个身份变量全部不存在 | 21,521 | 0 | 0 | 13 | 0 | 否 |

全部 a–e 样本：

- 精确 `\x1b[4m` 计数均为 0。
- SGR 状态机解析出的 underline-on transition 均为 0。
- `/rc` 出现时 underline-active 均为 0。

因此 CC 没有在阴性条件用下划线 SGR 包住 `/rc` 作为回退。

## 追加正交条件

| 条件 | 目的 | OSC8 open | 链接中的 `/rc` | 判定 |
|---|---|---:|---:|---|
| f `WT_SESSION` only | 分离两个 WT 变量 | 2 | 2 | `WT_SESSION` 单独足够 |
| g `WT_PROFILE_ID` only | 分离两个 WT 变量 | 0 | 0 | `WT_PROFILE_ID` 不足 |
| h `iTerm.app`、无版本 | 分离 iTerm 版本 | 5 | 5 | iTerm 名称单独足够 |
| i `WezTerm` + version `20260731` | 排除缺版本假阴性 | 0 | 0 | 此 CC 版本不接受 WezTerm 身份 |
| j `BetterTerminal` + `FORCE_HYPERLINK=1` | 测试诚实能力 override | 2 | 2 | override 有效 |
| k `BetterTerminal`、`NO_COLOR` 不存在 | 对齐产品会移除 `NO_COLOR` 的行为 | 0 | 0 | `NO_COLOR` 不是现状阴性的原因 |

所有追加条件的 `\x1b[4m` 和 `/rc` underline-active 也都是 0。

## 字节证据

阳性（b，session id 已遮盖）：

```text
<ESC>]8;id=v1ocf8;https://claude.ai/code/session_<redacted><BEL>/rc<ESC>]8;;<BEL><ESC>[K<CR><ESC>[1B
```

阳性（j，仍声明 BetterTerminal，仅加能力 override）同样是：

```text
<ESC>]8;id=...;https://claude.ai/code/session_<redacted><BEL>/rc<ESC>]8;;<BEL>
```

阴性（a/k）的最终 `/rc` 重绘附近是：

```text
[Fable 5] │ BetterTerminal git:(main*) ... /rc<ESC>[K<CR><ESC>[1B  Context ...
```

且整份字节流中没有任何 OSC 8 open/close。

完整原始证据在各条件的 `output.raw`；转义后的 `/rc` 上下文、OSC 8 event offset 和 URI 在 `analysis.json`。报告没有公开记录真实 session URL。

## 门判定

对 Claude Code 2.1.220，本实验可实证的布尔门是：

```text
emit_osc8_for_rc =
    WT_SESSION is present
    OR TERM_PROGRAM == "iTerm.app"
    OR FORCE_HYPERLINK == "1"
```

这是对已测输入域的归纳，不声称枚举了二进制中所有可能的终端白名单值。已测 `TERM_PROGRAM` 值中，只有 `iTerm.app` 命中；`BetterTerminal` 和 `WezTerm` 均未命中。`TERM`、`COLORTERM`、ConPTY TTY 和 OSC 8 渲染能力本身不足以让 CC 发射 `/rc` 链接。

## 产品建议

1. 不设置假的 `WT_SESSION`，也不把 `TERM_PROGRAM` 改成 `iTerm.app`。这会误导 CC 及其他子程序。
2. 可立即文档化一个 Claude Code 专用 workaround：在 Claude Code 的配置环境里设置 `FORCE_HYPERLINK=1`。本实验已证明它在保留 `TERM_PROGRAM=BetterTerminal` 时有效；Claude Code changelog 也把它当成受支持的环境变量。
3. 不建议 BetterTerminal 默认给所有 PTY 子进程无条件注入 `FORCE_HYPERLINK=1`。`FORCE_*` 往往会绕过 stdout 是否仍为 TTY 的检查，shell 内部把命令重定向到文件时可能把 OSC 8 控制字节带进文件。若产品要提供，宜做显式用户选项或 Claude Code 应用级 profile，并允许显式 `FORCE_HYPERLINK=0` 覆盖。
4. BetterTerminal 的 renderer 对阴性样本无能为力：字节流只有 `/rc` 文本，没有目标 URL。通用 URL auto-linkifier 也无法从 `/rc` 推回私有 session URL。
5. 长期应向 Claude Code 上游请求：识别 `TERM_PROGRAM=BetterTerminal`，或提供/document 一个能力式而非品牌白名单式的 OSC 8 协商入口。

## 上游 issue 草稿

Title:

```text
[BUG] /rc OSC 8 hyperlink is suppressed in OSC 8-capable terminals unless terminal identity is allowlisted
```

Body:

```markdown
### Environment

- Claude Code 2.1.220 (Windows native binary)
- Windows, interactive ConPTY
- TERM=xterm-256color
- COLORTERM=truecolor
- TERM_PROGRAM=BetterTerminal
- The terminal implements and renders OSC 8 links correctly (verified independently)

### Reproduction

1. Start a fresh interactive Claude Code process in the OSC 8-capable terminal.
2. Wait at the idle prompt until Remote Control finishes connecting. Do not send a prompt.
3. Inspect the PTY output bytes around the `/rc` footer.

### Actual behavior

With `TERM_PROGRAM=BetterTerminal`, `/rc` is emitted as plain text. The entire capture contains no OSC 8 sequence.

Controlled fresh-process A/B captures:

| Environment delta | OSC 8 around `/rc` |
|---|---|
| `TERM_PROGRAM=BetterTerminal` | no |
| no terminal identity variables | no |
| `TERM_PROGRAM=WezTerm`, with or without a version | no |
| `WT_SESSION=<random GUID>` only | yes |
| `WT_PROFILE_ID=<random GUID>` only | no |
| `TERM_PROGRAM=iTerm.app` (version absent) | yes |
| `TERM_PROGRAM=BetterTerminal`, `FORCE_HYPERLINK=1` | yes |

Positive byte shape:

`ESC ] 8 ; id=... ; https://claude.ai/code/session_<redacted> BEL /rc ESC ] 8 ; ; BEL`

Negative byte shape:

`... /rc ESC [ K ...` with no OSC 8 anywhere in the stream.

There is also no SGR underline fallback around `/rc`.

### Expected behavior

An OSC 8-capable terminal should have a capability-based way to receive the `/rc` link without impersonating Windows Terminal or iTerm. Please either:

1. recognize `TERM_PROGRAM=BetterTerminal`, or preferably
2. document/use a capability-oriented mechanism for OSC 8 support rather than a closed terminal-brand allowlist.

`FORCE_HYPERLINK=1` works as a scoped workaround, but terminal-wide injection is undesirable because forced escapes may leak into redirected output.
```

Claude Code 的官方仓库是 `https://github.com/anthropics/claude-code`；仓库 README 指示可用 `/bug` 或 GitHub Issues 报告。

## 清理与仓库状态

- 11 个最终纳入报告的条件，其 metadata 中 `surviving_tree_pids_after_taskkill=[]`。
- 11 个条件中 `new_claude_or_node_after_cleanup=[]`。
- 最终检查没有本实验创建的 `bt-record.exe`、`OpenConsole.exe`、`claude.exe` 或 `node.exe`。
- 未改产品代码、未 commit。
- `git status --short` 仅显示既存的 `.tmp-a85-parent/` 和本实验目录 `.tmp-cc-hyperlink-probe/` 为未跟踪；前者未触碰。
