# m1.8d-r2 返工单:收起后画面停滞,直到鼠标移动才出帧(xhigh,定性优先)

## 症状(用户实测,d3da0f0 构建,真实 Claude Code 会话,两个都是向下展开的区块)

1. 区块 A:点击展开、原地再点收起都正常;**收起后若不移动鼠标,下方内容不渲染**
   ——停滞不是 150ms,是无限期;鼠标一动立刻恢复。
2. 区块 B:展开后**原地点击无法收起**,必须先把鼠标移到其它行再点才行;
   收起后渲染正常。

差分:A 说明「PTY 重绘的最后状态没有被呈现,直到下一个输入事件」;B 的一种
统一解释是展开把总内容顶过视口底、Ink 底部锚定使整体上移(区块头其实移位了),
用户按**停滞的旧帧**瞄准自然点不中——鼠标一动,新帧呈现,瞄准就对了。
若属实,修好 A 的停滞,B 随之消失;但你必须实证,不许当作推论收案。

## 已知代码事实(m1.8d 引入,提交 d3da0f0)

- crates/bt-app/src/main.rs:470 publish_frame:PtyOutput 帧与
  pending(否则 last-presented)呈现等价 → skip。
- :547 drain_pty:整段 drain 后,`changed && synchronized_update_deadline().is_none()`
  才 publish;有开着的 DEC 2026 sync 就整段延迟(**包括同段里 sync 之前的
  普通内容与已完整闭合的前几个 sync 块的内容**)。
- :590 finish_synchronized_update_if_due:到期才结算,且只有
  finish_synchronized_update 返回 true 才 publish。
- :1473 about_to_wait:drain → finish_if_due → WaitUntil=各 deadline 最小值。

## 嫌疑清单(逐一实证或排除)

1. **「内容 + 尾随 BSU」整段延迟后被吞**:drain 处理了 [可见内容 A…, BSU](下一
   个 sync 块刚开头) → 整段延迟;150ms 到期结算时 sync 缓冲若为空/无更新,
   finish 返回 false → **A 永远不发布**。之后每次 drain 无新字节(changed=false)
   也不发布 → 永久停滞。鼠标移动之所以能救:找出鼠标路径上哪个非 PtyOutput
   的 publish(它不带 unchanged-skip 或带了但状态确实新)——那就是停滞状态的
   活证据。
2. **unchanged-skip 对 pending 的误判**:skip 比较基准是 pending-else-last;
   构造 pending 从未呈现、后续帧被误判等价的时序。
3. **vte 内部 sync 状态与 session.synchronized_update_deadline() 脱钩**:
   vte 自己的 150ms 结算发生在后续 advance 内部时,deadline() 的窗口语义是否
   与 drain 的判断一致;BSU/ESU 快速成对(真实流 81 对)下 deadline 反复
   开合的边界。
4. 鼠标转发路径本身:点击坐标映射用的是 session 当前网格还是已呈现帧的状态
   ——确认转发正确性与呈现停滞无耦合(B 症状的另一可能面)。

## 真实字节流参考(私有 corpus,不入仓库)

C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\ff11be2c-05c5-4a23-840f-a94405b44c35\scratchpad\cc-collapse.btcr
——真实 Claude Code:零 CSI S/T/IL/DL,CUP 223 + EL 3922,81 对 BSU/ESU。
用 target/release/bt-replay.exe 看序列统计;从中提取 BSU/ESU 与内容的真实
交错模式来构造回归输入(提取脚本/统计可以进仓库,corpus 本身不行)。

## 要求

- 先定性:用 BT_PERF_TRACE(defer=synchronized-update / skip=unchanged 行已有)
  或最小单测把停滞时序钉死,写清因果链(file:line+时序),再修。
- 修复必须是调度语义级:凡 drain 中存在**已闭合的可见更新**,不允许被尾随的
  未闭合 BSU 无限期扣押;150ms 兜底路径必须在「sync 缓冲为空」时也把此前
  被延迟的内容发布出去(或从根上改成:延迟只作用于未闭合 sync 块本身,
  已闭合部分照常发布——选哪种要论证与 WT 行为对齐)。
- 回归测试:① [内容, BSU] → 到期 → 必须发布;② [BSU 内容 ESU BSU] 同 drain
  → ESU 前内容必须发布;③ 快速 BSU/ESU 链(模拟真实 81 对交错)下无帧被
  永久扣押;④ A-B-A pending 等价误判的构造用例。全部进 bt-app 测试。
- 门禁:cargo test --workspace --locked、clippy --all-targets -D warnings、
  fmt --check;vendor 零改动;m1.8d 的 2026 批帧收益(中间态不呈现)不得回退。
- 工作树 design/*、docs/UI-UX.md、docs/prompts/codex-M1-8e-row-cache.md 脏项
  保持原样。结果写
  C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\ff11be2c-05c5-4a23-840f-a94405b44c35\scratchpad\m1-8d-r2-result.md
  (绝对路径),停下等审,不提交。
