# Spike 06：背压与取消验证

日期：2026-07-16
结论：**go-with-caveats**

## 结论

`DESIGN.md §1.3` 的容量、公平性、取消与关停契约可以用纯逻辑管线实现，而且本次压力探针没有发现需要改动规格的阻碍。64 MiB 洪水下，PTY 字节环峰值严格停在 1 MiB，writer 实际阻塞 252 次；512 个交互事件全部被处理，延迟 p50/p95/max 为 49/555/990 µs。worker 队列峰值 64，576 次同 span 更新都原位替换，满队列的新 span 被拒绝并在 idle 后重试；构造的过期负载中取消率为 75%。

结论保留 caveat：这是确定性的管线/调度原型，不是真实 ConPTY、真实 VT parser 或 OS 线程优先级测量；内存结论是协议控制的缓冲字节上界，不是完整进程 RSS 上界。

## 交付物

- 独立 workspace：`spikes/06-backpressure/`
- 可复用库：`src/lib.rs`
- bench：`cargo run --manifest-path spikes/06-backpressure/Cargo.toml --release --locked --bin backpressure-bench -- <output.json>`
- 原始结果：`docs/spikes/artifacts/06-backpressure.json`
- 一键测试：`cargo test --manifest-path spikes/06-backpressure/Cargo.toml --locked`

该 workspace 用自己的 `[workspace]` 和 lockfile，不修改根 workspace；`vendor/alacritty_terminal` 仍留在根 workspace members。

## 契约与证据

| 规格契约 | 原型行为 | 结果 |
|---|---|---|
| per-session PTY ring = 1 MiB，满时阻塞 writer | byte-counted `ByteRing` + condvar；超容量单 chunk 直接拒绝 | 峰值 1,048,576 B；252 次阻塞；64 MiB 无丢失 |
| 单次 parse quantum = 256 KiB | 每次 dequeue 最多 262,144 B | 67,108,864 B 全部消费 |
| Term/UI 快照为 single-slot latest | 新快照覆盖旧快照 | 最大占用 1；509 次覆盖 |
| worker 每 session 64；同 span 替换 | 64 个 slot；按 `SpanId` 原位替换 | 峰值 64；576 次替换 |
| 满队列：拒绝 + retry-on-idle；actor 不阻塞 | 新 span 只登记 retry marker，slot 释放后重试 | 1 次拒绝；重试后 marker = 0 |
| generation 过期取消 | worker 开始工作前比较 generation | 48 取消 / 16 完成，构造取消率 75% |
| 不可见会话至少 1/8 | WRR 权重 visible:invisible = 7:1 | 700:100，不可见份额 12.5% |
| render 并发每 session ≤2、全局 ≤8 | 两层 admission limiter | 实测峰值 2 / 8 |
| parser threads = clamp(physical-2, 1, N) | 纯函数推导 | 1 核→1，2 核→1，12 核/8 session→8 |
| shutdown/cancel 独立于拥塞数据面 | `close()` 唤醒阻塞 writer，不向满队列塞 sentinel | 281 µs 释放，无死锁 |
| 洪水中输入不得饿死 | actor 每轮先 drain input，再消费一个 parse quantum | 512/512；p95 555 µs，max 990 µs |

时间数据用于证明探针确实走过拥塞路径，不提议产品延迟阈值。线程定时器、CPU 频率与本机调度都会影响这些微秒值。

## 测试与反向验证

`cargo test` 共 7 个测试，覆盖：精确配置常量、洪水内存上界与输入活性、worker 替换/拒绝/重试、公平份额与两级 render limiter、阻塞 writer 的 shutdown、1–2 核降级、单 chunk 超容量拒绝。`cargo clippy --all-targets -- -D warnings` 通过。

为确认门会失败，曾把 `WORKER_QUEUE_TASKS` 从 64 故意改为 63，只运行 `design_capacity_constants_are_exact`：测试以 exit code 101 失败，明确报告 `left: 63, right: 64`。随后恢复 64 并重新跑绿。该断言覆盖构建配置本身，不会因为测试也引用同一个错误常量而恒真。

## 遗留风险

1. `ByteRing` 目前模拟字节所有权和阻塞语义，没有连接 `portable-pty`；Windows pipe reader 的实际阻塞/退出行为仍需在 M0 管线落地时复测。
2. 输入活性测试使用独立有界控制通道和人工 parse 开销，证明 actor 调度策略，不包含 Windows scheduler 优先级反转或真实 VT escape 序列成本。
3. worker 取消发生在任务开工边界；第三方渲染库内部能否被抢占属于 Spike 03 的进程隔离问题。
4. 逻辑缓冲内存有界不等于总 RSS 有界；真实 parser、快照对象和渲染 artifact 的预算仍须在 M0/M3 持续测量。
5. WRR 原型证明 1/8 dispatch 份额；生产实现仍需定义会话加入/离开、空队列和多不可见会话时的记账细节，但不能降低规格保底。

## 偏离申请

无。原型没有重复验证双平面已经覆盖的 parser 分包不变性；这里只验证 `§1.3` 的队列、背压、取消与公平性层。
