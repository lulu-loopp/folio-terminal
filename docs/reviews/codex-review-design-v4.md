## 1. 缺口闭合情况

| 第三轮项目 | 判定 | 结论 |
|---|---|---|
| scrollback 所有权矛盾 | **closed** | `alacritty=0`、转录层唯一所有者已明确。 |
| thaw 状态路径 | **closed** | 删除 thaw 后状态机内部一致。 |
| FreezeCandidate 位置 | **closed** | staging 已明确归属与用途。 |
| soft-wrap / resize 生命周期 | **open** | staging 中已移出的片段不再位于 alacritty grid；宽度 reflow 无法由 stock alacritty 与仍在 grid 的尾部联合完成。还缺稳定 candidate ID、完整 cell/source-map、重新分片规则，以及超过 4K 的超限策略。 |
| ContentAnchor | **open** | affinity、删除降级、worker generation 已闭合；但所谓“全序”没有纳入 `ScreenId`，staging 又没有可寻址的 anchor variant，且它在文档序上应位于 History 与当前 Live 之间，而不是含混的“Live 序尾段”。 |
| 双状态机 / DetectionRevision | **closed** | 单向状态机及四版本失效边界已成立。 |
| 分屏 / PTY owner、overlay | **closed** | 第三轮结论不变。 |
| 背压 | **open** | actor 串行和数字容量已补；但 worker 队列满时阻塞、丢弃还是合并未规定，“可见性加权”也没有最低份额，256 KiB quantum 无法约束非字节型渲染任务。 |
| BlockSource | **closed** | 快照、显式刷新、搜索及网络路径策略已一致。 |
| WebView2 | **closed** | 第三轮要求的协议、消息桥及导航边界已补齐。 |
| G1/G3 | **open** | 尚未覆盖高度变矮的实际淘汰分支、staging 超限、staging↔live 跨界 reflow、ConPTY resize 后全屏寻址；G3 仍缺 staging/多 ScreenId 锚点验证。 |

“删除 thaw”目前有一个新的 **BLOCKER**：它本身可以成立，但不能同时宣称为 tmux 语义并用冻结历史填充变高后的活动视口。

- tmux 变高会把历史行重新拉入可寻址 screen；并非保持冻结后仅作视觉填充。[tmux `screen_resize_y`](https://raw.githubusercontent.com/tmux/tmux/master/screen.c)
- alacritty 在无 history 时变高是增加底部空行；变矮则先考虑光标和底部空间，不保证固定从顶部移出。[alacritty resize 实现](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/grid/resize.rs)
- ConPTY 的尺寸必须对应实际显示缓冲，并会向 CUI 程序报告该完整尺寸；不能把顶部若干行永久留给不可寻址的冻结历史。[Microsoft `ResizePseudoConsole`](https://learn.microsoft.com/zh-cn/windows/console/resizepseudoconsole)

因此必须二选一：

1. 保留严格无 thaw：变高只增加 live 空行，不用历史填充，承认这不是 tmux 语义。
2. 保留 tmux 式历史填充：历史行必须成为可寻址 live/copy-on-write 行，本质上恢复 thaw 或等价的所有权迁移。

## 2. 最终结论

**否。v3.1 尚不能作为确定性的 M-1 实施规格，也不能承诺“三门通过后进入 M0”。**

阻塞点集中在 resize-grow 语义、staging 跨界 reflow/超限，以及 staging/多屏锚点身份。修正上述语义并补齐 G1/G3 oracle 后，其余第三轮问题基本已经闭合。