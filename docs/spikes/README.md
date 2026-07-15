# M-1 spike 状态

| Spike | 结论 | 状态 |
|---|---|---|
| 00 工程基线清理 | go | 完成；概念拆分、稳定样式类型、公开 API 门禁、generation/LayoutKey 接线；241 tests，clippy 0 warning |
| 01 语料录制与回放 | go-with-caveats | 完成；7 个真实 ConPTY corpus（276,066 bytes），含 226 KB Claude Code 交互语料 |
| 02 双平面最小原型 | go-with-caveats | 第二轮返工完成；G1/G2/G3 通过；224 tests（含 vendored alacritty 180 tests），clippy 0 warning |
| 03 数学渲染引擎 | 未执行 | 不在本会话范围 |
| 04 Windows IME/CJK | 未执行 | 不在本会话范围 |
| 05 延迟基建 | 未执行 | 不在本会话范围 |
| 06 背压与取消 | 未执行 | 不在本会话范围 |

Spike 02 的首次 PASS 误报、第一轮返工以及第二轮新 blocker 均在报告中保留审计记录。当前结论基于从 VT 字节输入到装饰/投影结果的端到端测试、Live anchor 批量原子 rebase、恢复原版 vte 语义，以及纳入一键测试的 vendored alacritty 上游测试。

本阶段没有进入 M0，也没有开始任务 3～6。
