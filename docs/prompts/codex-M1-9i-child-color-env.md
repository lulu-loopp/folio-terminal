# M1.9i-A 任务书:向子进程声明颜色能力(COLORTERM / TERM)

档位 medium(纯工程接线,无定性难点)。基线 = 提交 `a3ac666`。
来源:用户实拍(2026-07-20)Claude Code 输出整屏单色。

## 背景与已证事实(协调者,勿重复推导)

用户报"颜色出问题了",Claude Code 输出全无色。协调者已用四张实拍探针
在当前 `dist` 构建上证明**我们终端的颜色渲染完全正确**:16 色、亮色、
`38;2` 24 位真彩、`38;5;N` 256 索引调色板(含 6×6×6 色立方与灰阶)全部
正确。**所以这不是渲染 bug** —— 是 Claude Code 自身的颜色能力探测把本终端
判成"不支持颜色"后降级为单色。

根因在我们这侧的缺口:`crates/bt-pty/src/lib.rs`

- `PtyCommand`(第 88-93 行)**没有 env 字段**;
- `PtySession::spawn`(第 271-283 行)用 `CommandBuilder::new` 加 `arg` /
  `cwd`,**从不调用 `.env(...)`**;
- 于是子进程原样继承父进程环境,我们**从未声明** `COLORTERM` / `TERM` /
  `TERM_PROGRAM` 中的任何一个。

正规终端(Windows Terminal、iTerm、VS Code 终端等)都会给子进程注入这类
标识,让 Node `supports-color`、Rust `anstream`、Python `rich` 等无条件开
真彩。我们没注入,任何一个探测器都可能把我们降级。

> 诚实边界:用户此前在本终端跑 Claude Code 是有色的,环境未变、变的是
> Claude Code 版本。因此**不能断言注入 env 就一定翻回这一具体个案**;但
> 声明颜色能力是标准且正确的缺口补齐,把我们从探测等式里彻底摘出。任务
> 目标是"正确地声明能力",不是"逆向某版本 Claude Code 的探测分支"。

## 任务

1. 给 `PtyCommand` 增加环境变量携带能力(如 `envs: Vec<(OsString,
   OsString)>` 加 builder 方法 `env(key, val)`),在 `spawn` 里遍历注入
   `CommandBuilder::env`。保持既有 `arg` / `cwd` 语义不变。
2. 默认给交互 shell(`PtyCommand::powershell()` 及 `spawn_default` 路径)
   声明:
   - `COLORTERM=truecolor`
   - `TERM=xterm-256color`
   论证取值:这两者是事实标准;`TERM` 在 Windows 原生程序里通常无害
   (多数 Win 程序不读它),但 Node/跨平台 CLI 会据此判色。若你判断
   设 `TERM` 在 Windows ConPTY 下有已知副作用(例如误导某些程序走
   Unix 分支),给出证据并只设 `COLORTERM`,在答复里说明理由。
   - 是否额外设 `TERM_PROGRAM` / `TERM_PROGRAM_VERSION` 表明本终端身份:
     可选;若设,值需真实(如 `BetterTerminal`),不得冒充其他终端。
3. **不要覆盖用户显式意图**:若父环境已存在 `NO_COLOR`,尊重之——不要
   为了"开色"而删除或压制用户设的 `NO_COLOR`。即我们只在**未被显式关闭**
   时声明支持能力。给出你对 `NO_COLOR` 处理的判断与实现。
4. 这些是**默认注入**,不得破坏调用方通过 `PtyCommand::env` 显式设定的
   同名变量(显式 > 默认)。

## 门禁

- 全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D
  warnings`、fmt;vendor/glyphon 零改。
- 新增单测:构造一个 `PtyCommand`,断言默认注入含 `COLORTERM=truecolor`
  且 `TERM=xterm-256color`(或按 §2 你的取舍);断言显式 `env` 覆盖默认;
  断言存在 `NO_COLOR` 时按 §3 的既定行为。
- 若可行,加一条**真起子进程**的验证:通过 ConPTY 起 `cmd /c set`(或
  PowerShell `Get-ChildItem Env:`)抓取输出,断言子进程环境里确有
  `COLORTERM`。若宿主 ConPTY 不稳(近期 CreateProcessAsUserW 1312),标
  `#[ignore]` 并在答复注明,不算失败。

## 交付

答复给出:改动 file:line、`TERM` 取舍依据、`NO_COLOR` 处理依据、新增测试
清单、门禁实跑数字(从输出抄写)。停下等审,不提交。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HUzTxek25ako1mX1oSdfZW
