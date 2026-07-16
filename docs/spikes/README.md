# M-1 spike 状态

| Spike | 结论 | 状态 |
|---|---|---|
| 00 工程基线清理 | go | 完成；概念拆分、稳定样式类型、公开 API 门禁、generation/LayoutKey 接线；241 tests，clippy 0 warning |
| 00b 工程基线小返工 | go | 完成；穷尽映射、§3.1 事实判定、adapter 纯事实边界、幽灵错误删除、CI 依赖门；245 tests，clippy 0 warning |
| 01 语料录制与回放 | go-with-caveats | 完成；7 个真实 ConPTY corpus（276,066 bytes），含 226 KB Claude Code 交互语料 |
| 02 双平面最小原型 | go-with-caveats | 第二轮返工完成；G1/G2/G3 通过；224 tests（含 vendored alacritty 180 tests），clippy 0 warning |
| 03 数学渲染引擎 | no-go | 三路均未满足选择门；A 最接近但 9 条有效宏缺 h/d 且无折行，B 的 Windows SVG 链构建失败，C 不是原生 SVG |
| 04 Windows IME/CJK | no-go（证据不足） | 04b 返工完成；真实 IME 人工门待执行；bt-term 与 grapheme 候选仅 15/22 匹配，Ambiguous Width 7/9 待裁决 |
| 05 延迟基建 | go-with-caveats | 完成；60/120/144 Hz 注入 cadence 的 event→submit 方法通过，候选门 p95 ≤5 ms / p99 ≤10 ms；缺光子基线 |
| 06 背压与取消 | go-with-caveats | 完成；容量/公平/取消/shutdown 契约通过；64 MiB 洪水下 ring 严格 ≤1 MiB，逻辑原型待真实 ConPTY/RSS 复测 |

Spike 02 的首次 PASS 误报、第一轮返工以及第二轮新 blocker 均在报告中保留审计记录。当前结论基于从 VT 字节输入到装饰/投影结果的端到端测试、Live anchor 批量原子 rebase、恢复原版 vte 语义，以及纳入一键测试的 vendored alacritty 上游测试。

本阶段 M-1 spike 03/05/06 已交付；03 的 no-go 需要人工裁决 M3 降级或后续 h/d 调查，04 的 no-go 仍等待真人 IME 证据。没有进入 M0。
