# M-1 spike 状态

| Spike | 结论 | 状态 |
|---|---|---|
| 00 工程基线清理 | go | 完成；概念拆分、稳定样式类型、公开 API 门禁、generation/LayoutKey 接线；241 tests，clippy 0 warning |
| 00b 工程基线小返工 | go | 完成；穷尽映射、§3.1 事实判定、adapter 纯事实边界、幽灵错误删除、CI 依赖门；245 tests，clippy 0 warning |
| 01 语料录制与回放 | go-with-caveats | 完成；7 个真实 ConPTY corpus（276,066 bytes），含 226 KB Claude Code 交互语料 |
| 02 双平面最小原型 | go-with-caveats | 第二轮返工完成；G1/G2/G3 通过；224 tests（含 vendored alacritty 180 tests），clippy 0 warning |
| 03 数学渲染引擎 | go-with-caveats | 选择 A（MiTeX + Typst）；锁定 M3 前必修 9 条同构宏的 h/d adapter 提取，自动折行为人工权衡 caveat；ReX 保留为未充分评估备胎 |
| 04 Windows IME/CJK | go-with-caveats | 真人门已执行（三家 × 十项：微软拼音/微信/搜狗），报告 `04-ime-cjk-manual-results.md`。**M0 可从 winit IME 起步**，三条 caveat：① 微软拼音连续跟随需 system caret 补丁（照 Chromium，只治它一家）；② 搜狗不上报预编辑，终端只能拿最终 commit；③ 键盘层须按 `logical==Process` 过滤、`set_ime_cursor_area` 须节流。仍待 M0 裁决：bt-term 是否上 grapheme 聚类、Ambiguous Width 判宽/窄 |
| 05 延迟基建 | go-with-caveats | 完成；60/120/144 Hz 注入 cadence 的 event→submit 方法通过，候选门 p95 ≤5 ms / p99 ≤10 ms；缺光子基线 |
| 06 背压与取消 | go-with-caveats | 完成；容量/公平/取消/shutdown 契约通过；64 MiB 洪水下 ring 严格 ≤1 MiB，逻辑原型待真实 ConPTY/RSS 复测 |

Spike 02 的首次 PASS 误报、第一轮返工以及第二轮新 blocker 均在报告中保留审计记录。当前结论基于从 VT 字节输入到装饰/投影结果的端到端测试、Live anchor 批量原子 rebase、恢复原版 vte 语义，以及纳入一键测试的 vendored alacritty 上游测试。

**M-1 全部四个 spike 已有结论（2026-07-16）**：03 选 A（MiTeX + Typst）带 h/d 必修 + 折行权衡 caveat；04 真人 IME 门已跑完，go-with-caveats，M0 可从 winit 起步（三条 caveat）；05/06 go-with-caveats。**M-1 实质完成，下一步进 M0（地基：ConPTY + alacritty_terminal + wgpu 文字网格）。** M0 两个裁决项（grapheme 聚类、Ambiguous Width）按里程碑归属推到 M1 的正确性攻坚（M0 是"纯地基、无 P 项"，直接用 alacritty 网格原样起步）。
