我会按指定三份文档做只读终审：先还原一轮/二轮提出的精确要求，再逐条映射当前方案，并额外检查这些修订是否造成内部矛盾或遗漏。不会改文件。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppData\\Local\\Temp\\claude\\D--Developer-Better`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppDa
首次读取被 Windows 沙箱的进程启动错误拦住了（不是文档内容问题）。我会改用当前会话可用的只读资源接口继续；若宿主仍拒绝访问，我会明确报告而不臆测结论。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -NoProfile -Command "Get-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppData\\Local\\Temp\\claude\\D--Devel`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppDa
终端读取连续两次都因同一宿主登录会话错误失败。我将尝试浏览器的本地只读打开作为兜底；其技能说明文件也受同一文件读取故障影响，因此只能采用最小范围的只读导航，不做任何交互或改动。
三处走样和四处残留的对应文本都已找到；七项本身已按二轮要求补齐。现在只剩一处新增措辞需要判定其是否构成实质矛盾：OSC 8/link 段写的是“裁掉的 cell 仍须保留 anchor”，这与“每帧只物化可见窗口 + overscan”可能正面冲突。我正在按上下文区分它是笔误还是方案要求。
终审结论：二轮指出的三处走样与四处残留均已正确落实；但新引入一处 OSC 8/link 措辞矛盾，因此最终 verdict 为：**不通过**。

| 核对项 | 结论 | 依据 |
|---|---|---|
| anchor 身份复用 | 通过 | [第 16 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:16>) 明确“复用既有 anchor 身份，只新增横向投影，不重建体系”。 |
| OSC 8/link 全名单及行列分轴 | 通过 | [第 50–51 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:50>) 已覆盖协议点、光标、IME/preedit、选择/搜索、OSC 8/link、InlineImage/OSC 133，并明确列走横轴转换、行走既有纵向映射。 |
| 帧时间预算 | 通过 | [第 26–32 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:26>) 已设为硬门：连续横滚中位数 ≤3ms、临时分配有上限、首次建索引有时间上限。 |
| 第 3 行措辞 | 通过 | [第 3 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:3>) 已区分 v1 四项修订和二轮七项修订，不再与正文状态冲突。 |
| row/col 类型 | 通过 | [第 51 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:51>) 已明确行列使用不同映射，类型错误消除。 |
| DESIGN 现在时时机 | 通过 | [第 65 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:65>) 现在只记录裁决；各能力实现并验收后才改为现在时。 |
| DECAWM 丢失边界 | 通过 | [第 42 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:42>) 已说明台阶二仅把边界抬至 `W_virtual`，超过它仍丢，历史丢失不可恢复。 |

新引入问题只有一处：

- [第 51 行](</C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/8c51127a-6f8c-4b17-b04c-caef6a7f83fe/scratchpad/horizontal-scroll-plan.md:51>) 写成“**裁掉的 cell** 仍须保留……anchor”。字面要求给已经裁除的 cell 保留 anchor，与第 28 行“每帧只物化视口 + overscan”冲突。正确对象应是“**裁切后保留下来的 cell**”或“**投影出的可见 cell**”。

最终 verdict：**不通过**。七个指定修订点已经全部落实，但需先消除上述新矛盾；这是单处措辞修正，无需重新审代码或调整总体方案。