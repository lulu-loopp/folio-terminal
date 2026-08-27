# Spike 01：语料录制与回放基建

## 结论

**go-with-caveats**。

已完成 Windows ConPTY 原始输出录制、单调微秒时间戳、resize 事件、退出码、二进制精确读写，以及 recorded / fixed / pattern 三种分包回放。回放目标是真实 `alacritty_terminal`（scrollback=0），所有语料和属性测试可由同一个 `cargo test` 入口执行。

caveat 是 recorder 仍不是完整终端前端：当前自动回答 DSR 6n，并可按时间计划注入 raw input；尚未实现通用 DA/DECRQSS/OSC 查询响应器。该限制不影响本批语料。

## 实现

- workspace 只建立本阶段需要的 `bt-term`、`bt-transcript`、`bt-doc`、`bt-viewport`、`bt-detect`、`bt-corpus` 六个 crate；没有 GUI、wgpu 或 M0 产品代码。
- `bt-record OUTPUT.btcr [--size COLSxROWS] [--resize MS:COLSxROWS] [--stdin FILE] [--input-plan FILE] -- COMMAND ...` 通过 `portable-pty 0.9` / ConPTY 运行任意命令。
- `--input-plan` 每行使用 `MS:HEX`，可在交互程序运行期间按时发送原始输入；用于本次 Claude Code TUI 录制。
- `BTCRP001` 按事件记录 `at_micros: u64` 与 raw output bytes、`Resize { cols, rows }`、exit code，不做 UTF-8 转码。
- `bt-replay CORPUS.btcr [CHUNK_SIZE]` 回放到 `bt-term`；库 API 另支持任意非零 chunk pattern。
- parser 每次接收完整 slice；session actor 只在 256 KiB quantum 处切分，不再逐字节调用 parser。

## 首批语料

共 **7 个 `.btcr`，276,066 bytes**：

| 文件 | bytes | 来源 / 覆盖 |
|---|---:|---|
| `pwsh-daily.btcr` | 427 | PowerShell cwd、目录、日期、普通输出（2026-08-27 已脱敏） |
| `cargo-build-flood.btcr` | 15,525 | 本 workspace 的 verbose cargo build 洪水（2026-08-27 已脱敏） |
| `editor-alt-screen.btcr` | 526 | 本机无 vim，使用明确标注的 alt-screen 编辑/退出脚本替代，含 `?1049h/l` |
| `tui-redraw.btcr` | 8,568 | 本机无 htop/top，使用明确标注的全屏与底部状态区持续重绘脚本替代 |
| `claude-code-session.btcr` | **226,262** | Claude Code 2.1.210 真实交互 TUI：alt-screen、输入、思考动画、Glob 工具活动、两次 resize、正常退出（2026-08-27 已脱敏） |
| `shell-dollars.btcr` | 3,629 | `$PATH`、awk `$1/$2`、80 行 `$$` / 美元符号密集输出 |
| `resize-sequence.btcr` | 21,129 | 全屏重绘期间四次 resize，最终 90×22 |

`claude-code-session.btcr` 已替换首次审核指出的 209-byte `claude -p` 固定输出。新语料由 `claude` 交互程序直接录制，包含可断言的 `Claude Code v2.1.210`、用户消息 `Now revise steps 4 and 9`、`Searching for 2 patterns`、alt-screen 控制序列和 2 个 resize marker。定时计划的第一条输入在 TUI 完成初始化前发送，未被应用接受；第二条输入被接受并触发真实模型/工具活动。这一事实不影响它作为持续重绘交互语料的覆盖价值。

## 脱敏（2026-08-27）

录制当时没有做任何环境替换。这些是真实机器上真实程序的原始 ConPTY 字节流，因此 `claude-code-session.btcr`、
`cargo-build-flood.btcr`、`pwsh-daily.btcr` 三份把录制账户的名字、邮箱地址、组织标签、Windows 家目录和仓库
父目录一起写进了仓库。2026-08-27 发布就绪复核（`docs/plans/release/readiness-review-codex-2026-08-27.md` 错 1）
点出这一点后，三份录制已就地脱敏；另外四份逐字节扫描后不含任何身份，未改动。

替换是**等长**的：BTCR 的每个 output payload 前有 `u32` 长度前缀，录制里又带着 ConPTY 烘死的换行
（CR LF + CUP 回末列 + 重画该格字符，会把一个名字或路径劈成两段），所以替换必须既保住字节数又保住列对齐。
实际改动是三份文件里共 4,705 个单字节替换（`cargo-build-flood` 664、`claude-code-session` 4,032、
`pwsh-daily` 9），全部是可打印 ASCII 换可打印 ASCII，文件长度、事件数、时间戳、长度前缀与全部控制序列
一字未动；回放得到的 live grid 与原始录制逐行相同，只是那几处字母不同。

替换的类别与逐份清单记在 `corpus/PROVENANCE.md`（不写原值）。回归门是
`crates/bt-corpus/tests/corpus_privacy.rs::no_recording_carries_a_person`：它扫描 `corpus/` 下**全部**
`.btcr`，既扫原始字节，也扫把 ConPTY 换行还原后的逻辑字节流，任何身份重新出现都判红。

## 支撑数据

- 格式 round-trip 测试逐字节相等，包含 ANSI、非 UTF-8 边界、resize 和 exit。
- 6 个代表性录制文件（含新 Claude 语料）分别以 recorded 与 `[1, 7, 2, 31, 3]` 循环分包回放，最终 live grid、frozen transcript、尺寸相等。
- proptest 随机生成分包边界，比较整 slice 与任意 chunk 后的 grid 和 document。
- Claude 语料专门断言版本标识、交互消息、工具活动、alt-screen 和 2 个 resize marker，避免再次退化为短固定输出。
- `cargo test --workspace --offline` 当前总计 **224 passed**（BetterTerminal 44 + vendored alacritty 180）；`cargo clippy --workspace --all-targets --offline -- -D warnings` 为 **0 warning**。

## 遗留风险

1. recorder 只自动响应本批程序需要的 DSR；更复杂的 DA/DECRQSS/OSC 查询需扩展终端响应回路。
2. input bytes 当前不写入 BTCR 文件；“精确回放”范围是 PTY output、时间戳与 resize。若未来要重放交互因果链，应给格式新增版本化 Input event，不能悄悄改变 v1 语义。
3. vim 和 htop/top 替代品已明确标注，仍需在具备这些二进制的 dogfood/CI 机器补录真实程序。
4. 语料绑定当前 Windows、ConPTY、PowerShell、Claude Code 与 parser 行为；依赖升级后应重录一组并做差异审计。

## 偏离申请

无。任务允许在缺少 Claude Code 时使用持续重绘 TUI；本机实际存在 Claude Code，已补录真实交互会话。vim/htop 缺失时采用的替代语料也按任务要求明确注明。
