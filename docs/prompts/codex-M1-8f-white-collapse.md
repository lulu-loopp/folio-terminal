# m1.8f 返工单:向上展开→收起后白屏/错位(WT 对照已判我方,xhigh 定性优先)

## 症状(用户实测,ce4bf88 构建,真实 Claude Code 会话)

向上展开的区块(展开时内容经滚出进转录)收起后:
1. 原区域**变白**,鼠标一动(motion 报告激发 Claude Code 重绘)才恢复;
2. 恢复后**视口不回原位**,整体显示了更下方的内容;
3. 用户白屏截图里有 Claude Code 的 "Jump to bottom" 芯片——收起发生时
   **BT 视口大概率处于 Anchored(上滚)状态**,不是 Bottom。

## 判决依据(为什么是我们的)

- **WT 对照阴性**:同一 Claude Code、同一操作,WT 收起正常。字节它发了,
  我们画丢了。(另一症状"原地点击不生效需先动鼠标"WT 也复现,已结案归
  Claude Code,不在本单。)
- BT_PERF_TRACE 现场(scratchpad/bt-perf-repro.log):白屏期间呈现管线活着
  ——spinner 每跳一帧都正常 present(frame 169-171,rows_reshaped=1-2),
  大面积重绘帧(139-164,22-29 行)也都落地。**不是 m1.8d-r2 那类扣押**。
- **合成探针阴性**(收窄了嫌疑面):BT_PROBE_INPUT 一次性喂
  「60 行滚出 → CUP+EL 重写 12 行 + 逐行擦除下方」合成完全正确
  (scratchpad/collapse-probe.vt + collapse-probe.png)。
  单 drain、Bottom 视口、无 sync 包裹的路径是干净的。

## 因此嫌疑=真实场景多出的三个变量(逐一实证或排除)

1. **Anchored 视口的合成路径**:收起重绘落在 live 屏,视口锚定在转录
   逻辑行;冻结区/live 区拼接在「live 内容被 CUP+EL 原地改写+此前有滚出」
   时是否产生空洞或错位(白=合成视口的洞,不是网格的空)。重点审
   crates/bt-viewport 的 Anchored 组合与 content_rows_below。
2. **多 drain 交错**:重绘按几十个 BSU/ESU 块分批到达(真实统计:81 对,
   51 个零字节 ESU BSU 紧邻),每个已闭合块单独发布一帧;这些中间帧在
   Anchored 视口下的投影是否把「擦除阶段」定格进用户所见,而后续块的
   重写行落在视口窗口之外(用户看到的是洞+"更下方的内容")。
3. **150ms 超时结算插中段**:timeout commit 把半个块提交时,与 1/2 的
   交互。

## 复现与回归要求

- 用 lifecycle 矩阵造多 drain 复现:{滚出 N 行} × {CUP+EL 收起重绘,
  拆成 k 个 BSU/ESU 块分批 feed} × {视口 Bottom | Anchored(先上滚 m 行)}
  × {是否夹入 timeout 结算}。断言:每个已发布帧的合成视口**无空洞**
  (被擦除的行如果在视口内必须来自真实网格状态)、行位与直接一次性 feed
  的终态一致、Anchored 锚不因 live 原地改写而漂移。
- 真实字节流参考(私有,不入仓库):scratchpad/cc-collapse.btcr,
  bt-replay 可看交错统计;需要更贴真的输入模式就从它提取时序特征
  (特征可入仓库,内容不行)。
- 修复原则:合成层修语义,不做"补画/猜测内容"的化妆;若定性推翻上述
  嫌疑清单、指向别处(如 renderer 局部重绘矩形),如实改道并给证据。

## 边界

- vendor 零改动优先;确需 vendor 侧修改须单独说明理由与上游断言零改。
- m1.8d/r2 的批帧与无变化跳过语义不得回退(既有六个回归必须继续绿)。
- 门禁:cargo test --workspace --locked、clippy --all-targets -D warnings、
  fmt --check。
- 工作树可能有 m1.8e(行缓存)已落地的改动,保持不动,只加不改;若冲突
  停下报告。
- 结果写 C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\ff11be2c-05c5-4a23-840f-a94405b44c35\scratchpad\m1-8f-result.md
  (绝对路径),含因果链 file:line、复现矩阵结果、门禁数字。停下等审,
  不提交。
