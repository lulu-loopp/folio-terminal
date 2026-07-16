# Spike 05：延迟测量基建

日期：2026-07-16
结论：**go-with-caveats**

## 结论

事件到 wgpu 提交边界可以稳定、可重复地测量，而且窗口/事件来源已隔离成可替换接口，不会把 winit 固化进 M0 的 bench API。在持续的 2,000 Hz 输出 generation 洪水、single-slot latest 合并和竞争绘帧下，60/120/144 Hz 三组各 240 个样本全部完成；`event → Queue::submit return` 的最差组为 144 Hz：p50 0.885 ms、p95 3.525 ms、p99 7.276 ms。

结论保留 caveat：本机没有光敏电阻或高速相机，所以没有得到“按键到实际上屏”的光子基线；自动注入从 `EventLoopProxy::send_event` 前开始，不包含物理键盘、Windows 输入栈或 TSF。当前显示器是 120 Hz，因而 144 Hz 是**事件注入/提交 cadence**，不是把显示器切到了 144 Hz。

## 交付物

- 独立 workspace：`spikes/05-latency/`
- 可复用核心：`src/lib.rs`
- winit + wgpu 适配器/bench：`src/bin/latency-bench.rs`
- 原始结果：`docs/spikes/artifacts/05-latency.json`
- 运行：`cargo run --manifest-path spikes/05-latency/Cargo.toml --release --locked --bin latency-bench -- <output.json>`
- 测试：`cargo test --manifest-path spikes/05-latency/Cargo.toml --locked`

独立 workspace 使用 wgpu 30.0.0、winit 0.30.13 和 DX12，不修改根 workspace。

## 测量定义

时间点严格定义为：

1. `t0`：注入线程调用 `EventLoopProxy::send_event` **之前**；
2. 窗口线程收到事件，获取 surface texture，编码一次 clear pass；
3. `t1`：`wgpu::Queue::submit` 返回之后；
4. `t2`：`wgpu::Queue::present` 调用返回之后。

主指标是 `t1 - t0`。它包含用户事件排队/分派、surface acquire、clear 编码和 queue submit；排除物理键盘/TSF、GPU 完成、DWM 合成、扫描输出和像素发光。`t2 - t0` 只是 CPU 侧 present enqueue 边界，**不是**光子时间。

公共库只定义 `EventSource`、`PresentationBoundary`、`EchoEvent`、`SubmitReceipt` 和统计逻辑，没有 winit/wgpu 类型。若 Spike 04 的真人门最终要求直接 TSF，替换事件适配器即可，测量定义不变。

## 洪水模型与一次失败的方法学

洪水 producer 以 2,000 Hz 推进 output generation，但窗口事件队列中至多保留一个 `Flood` 事件；窗口线程读取最新 generation，执行确定性的 parser-like CPU 工作，并每四次请求一个可合并的竞争绘帧。这对应 `DESIGN.md §1.3` 的 latest-wins/coalescing，而不是把每段输出都无限排队。

首轮原型曾把每个洪水 tick 都送入无界 `EventLoopProxy`，并用 Fifo 在 120 Hz 显示器上提交超过刷新率的帧。结果 144 Hz 组 p50 达 483 ms。它测到的是探针自己制造的 backlog，不能作为产品阈值。修正后：洪水改为 single-slot latest；CPU submit 基线使用 Immediate present mode，避免较低的物理刷新率把 surface acquire 阻塞混进 submit 指标。Fifo/实际刷新与光子延迟必须作为另一组端到端测量，不能冒充本指标。

## 支撑数据

硬件/环境：NVIDIA GeForce RTX 5070 Ti Laptop GPU，DX12，驱动 32.0.15.9184；`DISPLAY1` 当前 120 Hz；窗口 1280×720 logical backing，scale 2.0；Immediate present，maximum frame latency 1。共处理 6,610 个合并后的洪水事件。

### `event → Queue::submit return`

| 注入 cadence | n | min | p50 | p95 | p99 | max |
|---:|---:|---:|---:|---:|---:|---:|
| 60 Hz | 240 | 0.249 ms | 0.775 ms | 2.545 ms | 4.637 ms | 7.443 ms |
| 120 Hz | 240 | 0.248 ms | 1.209 ms | 3.210 ms | 4.721 ms | 6.358 ms |
| 144 Hz | 240 | 0.259 ms | 0.885 ms | 3.525 ms | 7.276 ms | 9.366 ms |

### `event → present() call return`

| 注入 cadence | n | p50 | p95 | p99 | max |
|---:|---:|---:|---:|---:|---:|
| 60 Hz | 240 | 1.652 ms | 3.398 ms | 5.304 ms | 8.497 ms |
| 120 Hz | 240 | 2.039 ms | 4.072 ms | 5.388 ms | 7.618 ms |
| 144 Hz | 240 | 1.892 ms | 4.268 ms | 8.629 ms | 10.988 ms |

## 建议写入 DESIGN §2 的阈值

建议先落一条**软件回归门**，不要把它称为上屏 SLA：

> 在 latest-wins 输出洪水模型下，按 60/120/144 Hz 分组测量，从事件进入应用边界到 `Queue::submit` 返回；每组预热 ≥30、有效样本 ≥240。参考 Windows/DX12 机器每组均须满足 p95 ≤5 ms、p99 ≤10 ms。必须同时记录 GPU、驱动、present mode、窗口尺寸、显示器当前刷新率与洪水模型。

该候选线来自本机最差值 3.525/7.276 ms，留有约 1.4×/1.37× 余量。它适合发现 M0 后的严重回归，不是跨硬件承诺；在至少一台 iGPU、1–2 核降级机和光子基线完成前，不建议更紧。

光子侧阈值暂不提数值。没有真实样本时指定数字会制造一扇永远不会红的门。

## 光子基线

本会话无光敏电阻/微控制器或 ≥1,000 fps 摄像设备，**未测量** submit 到实际上屏偏差。

可执行替代方案：

- 1,000 fps 相机同时拍摄硬件按键/LED 与目标屏幕 patch；时间量化约 ±1 ms，另有曝光窗口误差；
- 光敏电阻 + 微控制器，把硬件输入开关与屏幕传感器接到同一时钟；校准后仪器误差目标约 ±0.1 ms，但仍需按屏幕扫描位置分层。

两种方案都应在 60/120/144 Hz 的**真实显示模式**分别采样，报告中分开 `event→submit`、`submit→photon` 和总延迟。

## 测试与反向验证

`cargo test` 5 passed，`cargo clippy --all-targets -- -D warnings` 通过。测试覆盖：事件源/提交端可替换、时间戳反转拒绝、nearest-rank 分位数、三种目标 cadence 精确配置，以及慢分布的门禁失败。

反向验证 `a_deliberately_slow_distribution_fails_the_gate` 注入 50 ms 尾部样本，在 p95 10 ms / p99 20 ms 的测试门下明确返回 false；若统计代码漏掉尾部或门没有执行，该测试会红。另一个更重要的动态反证是首轮 uncoalesced/Fifo 方法产生的 483 ms p50，它直接促成了测量模型修正。

## 遗留风险

1. 缺光子基线；不能回答 DWM/扫描输出/面板响应带来的偏差。
2. 自动注入不经过物理键盘、Raw Input 或 TSF；M0 应提供对应 `EventSource` 适配器并保留同一 recorder。
3. Immediate 模式只用于隔离 CPU submit 成本；生产 present mode 确定后，要另跑 Fifo/Mailbox 与实际刷新率组合。
4. 目前只在 RTX 5070 Ti Laptop + DX12 上测量；iGPU、远程桌面/Parsec、混合显卡复制路径均未覆盖。
5. parser-like 工作是确定性模拟，不代表完整 VT parser + shaping + detection 成本；工具应在 M0 逐步替换边界实现，而不是改指标定义。

## 偏离申请

无。由于没有 144 Hz 物理显示模式证据，本报告没有把 144 Hz 注入 cadence 冒充 144 Hz 光子测量；缺失项如实保留为 caveat。
