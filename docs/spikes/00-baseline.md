# Task 00：M-1 工程基线清理

## 结论

**go**。任务 00 的代码、架构和门禁清理已完成；没有开始 spike 03～06，也没有进入 M0。

本结论保留两项任务书明确要求延至 M0 的欠条：`SPIKE_CELL_HEIGHT_SUBPIXELS` 要由真实字体度量注入，`SPIKE_DEFAULT_FROZEN_QUOTA` 要由内存实测或配置决定。两者均已用 `SPIKE_` 前缀显式标记，不构成本次偏离。

## 逐项结果

| 条目 | 结果 | 实施摘要 |
|---|---|---|
| 00.1 护栏 | PASS | 六个第一方 crate 均继承 workspace lint；构建工具链固定为 `1.94.1-x86_64-pc-windows-msvc`；锁定依赖后运行全 workspace Clippy；临时植入 `todo!()` 的反向探针按预期失败，随后删除 |
| 00.2 hygiene | PASS | G2/G3 都加入从非零 live row 开始的 fixture；`SourceLifecycle::transition` 改为 `Result`，session 传播非法转移和缺失 staging source；修正 staging generation 的错误注释；删除 `HistoryEntry.source` 和幽灵 `Tombstoned` 变体；`DualPlaneSession` 明确为每 session 串行 actor 核心 |
| 00.3 upstream 隔离 | PASS | `bt-transcript` 自有 `TerminalColor` 和 `CellFlags`；所有 alacritty 映射集中在 `cell_capture.rs`，显式逐项映射并有反向解码/round-trip 测试 |
| 00.4 样式端到端 | PASS | 从 SGR 红色 + bold、OSC 8、CJK 宽字符的 VT 字节一路断言到 frozen `StyleSpan`、颜色、flags 和 hyperlink |
| 00.5 概念拆分 | PASS | `bt-term` 拆为 `adapter/cell_capture/lifecycle/scheduling/session`；`lib.rs` 仅 re-export；`bt-doc` 拆为 `anchor/versions/document`；`HeightTree` 独立为 `height_tree.rs` |
| 00.6 policy 边界 | PASS | resize epoch 与装饰调度全部移到 session/scheduling；adapter 只依赖 alacritty、`bt-transcript` 和 cell capture，不 import `bt-doc`/`bt-detect`/`bt-viewport` |
| 00.7 recorder | PASS | 删除 4 字节滑窗，使用 `vte::Parser` 的结构化 CSI dispatch 识别 DSR 6；支持跨分包、同一 chunk 多个查询；DCS/APC 普通数据不冒充查询 |
| 00.8 门测试 | PASS | G1/G2/G3 搬到 `bt-term/tests/{lifecycle_matrix,multiview,anchor_protocol}.rs`；一场景一函数；只用公开 API，从 VT 字节进入主链 |
| 00.9 死规格 | PASS | generation 与 LayoutKey 均选择“接线”，没有删规格，详见下节 |
| 00.10 其余清理 | PASS | 删除未使用 serde/thiserror；CLI parse 改为带上下文的 `Result`；重构 `capture()` 消除重复查找与产品路径 unwrap/expect；`compare_anchors` 用 `Option` 消除 `unreachable!()`；公开尺寸/配额改 `NonZero*`；完成命名和 `SPIKE_` 欠条修正 |

`lifecycle.rs` 现在有一张可执行的七项 `LIFECYCLE_RULES` 表。adapter 只上报事实，session 根据表执行捕获、定稿、park/restore、ED3/配额删除和 candidate invalidation；缺表项或 payload 不匹配会返回 `SessionError`，不再静默继续。

## 00.2 tombstone 与 actor 身份判断

采用代码原有的真实数据模型：

- `HistoryEntry` 存在即代表 Frozen；
- entry 已移除且 id 存在 `tombstones` 即代表 Tombstoned；
- `SourceLifecycle` 只描述 staging source 的 `Live → Frozen`。

因此删除 `HistoryEntry.source` 字段和 `SourceLifecycle::Tombstoned`，而不是只删那次死赋值。这样没有“写 Tombstoned 后立即 remove”的仪式状态，也没有永远无法构造的幽灵分支。ED3 与配额淘汰仍共用 `delete_transaction`，并由集成门验证 tombstone、successor 降级和 live-origin 降级。

`DualPlaneSession` 选择扶正为正式的纯逻辑 actor 核心，而非 test-support harness。理由是它已经是 §1.3 所要求的单 session 串行所有者，并且承担跨 crate 事务、容量、generation、resize epoch 与 worker 结果接纳；把它降为测试辅助反而会在 M0 再造一套协议编排。它仍不包含 GUI 或真实 PTY 产品代码。

## 00.9 判断：两个字段族都保留并接线

### ContentAnchor generation

保留三变体的 generation，并给出真实消费者：

- History：投影时必须等于目标 `FrozenLine.source_generation`；
- Staging：必须等于 projection 当前 transcript source generation；
- Live：必须等于当前 grid generation，且 row 必须仍在可寻址范围内。

不匹配统一返回 `AnchorError::StaleGeneration`。session 在 scroll/capture、resize、primary restore 等改变网格世代的事务中同步迁移或刷新存活锚点。`stale_anchor_generations_are_rejected` 分别覆盖 History/Staging/Live；G3 另用非零行锚点覆盖 rebase。

### LayoutKey 四字段

保留 `width_cells / dpi_milli / font_rev / theme_rev`。`LayoutKey` 是布局缓存键和 worker `VersionStamp` 的统一组成部分，`DualPlaneSession::set_layout_key` 提供真实更新入口并驱动 artifact/layout 失效。`every_layout_key_field_has_an_independent_cache_identity` 每次从同一 base key 出发，只改变一个字段，逐项证明各字段独立产生 cache miss。

## “修复前会红”的证据

| 变化 | 修复前失败证据 |
|---|---|
| lint 继承/真实执行 | 临时在 `bt-doc` 植入 `todo!()` 后，`cargo clippy -p bt-doc --locked -- -D warnings` 以 exit 1 失败，错误为 `clippy::todo`；移除后全 workspace Clippy 通过 |
| 工具链可复现 | 仅写版本号会继承本机 GNU default host，本机链接阶段因缺 `dlltool.exe` 失败；固定完整 MSVC host 后 `cargo test --workspace --locked` 可运行 |
| transition 不再静默 | `illegal_source_transition_is_observable` 要求 `Frozen → Frozen` 返回具体 `InvalidSourceTransition`；旧 bool API 无法满足该接口，编译即失败 |
| alacritty 内存布局隔离 | `style_flags_round_trip_without_upstream_bits_crossing_the_boundary`、`every_upstream_color_family_round_trips_through_stable_types` 要求稳定类型与反向解码；旧裸 `u16/u32` 模型无这些类型/decoder，编译失败 |
| 样式接缝 | `g1_style_color_and_osc8_metadata_survive_the_real_capture_pipeline` 从 VT bytes 断到稳定样式类型；旧接口无法表达该断言，编译失败。它是新增回归护栏，不声称发现了既有样式运行时错误 |
| recorder 嗅探 | `cursor_query_is_recognized_across_arbitrary_chunks` 把 query 后接普通输出，旧 `ends_with` 滑窗返回 0；`every_cursor_query_in_one_output_chunk_gets_a_response` 还会让旧实现漏掉同 chunk 的多个 query |
| anchor generation 消费 | `stale_anchor_generations_are_rejected` 要求三个变体返回 `StaleGeneration`；旧投影不读 generation，会给出像素坐标或其他非 stale 结果 |
| 配额降级跨 crate 接缝 | `g3_quota_eviction_degrades_to_the_next_surviving_history_entry` 从字节生成历史并让配额淘汰首行，要求 anchor 指向 successor；旧 gate 依赖手工中间态，不能通过公开 API 编译/执行这条主链 |
| 非零尺寸边界 | `zero_resize_dimension_is_rejected_at_the_corpus_boundary` 要求 `ZeroDimension`；旧 replay callback 接受裸 `u16`，会把 0 传入下游 |

模块拆分、依赖删除、撒谎注释修正、tombstone 模型简化、命名调整属于保持行为的结构/语义清理，不伪造“修复前运行时测试会红”。它们由全 workspace 编译测试、Clippy、公开 API gate 和静态依赖检查共同守护。

## 三道门现状

| 门 | 文件 | 结论 | 关键覆盖 |
|---|---|---|---|
| G1 | `tests/lifecycle_matrix.rs` | PASS | scroll→staging→rewrite→finalize→decorate、全尺寸 grow、宽高 shrink oracle、reflow、resize jitter、跨界 split、staging/frozen quota、ED3、alt、DECSTBM、RIS/DECCOLM、最后一行、真实样式 |
| G2 | `tests/multiview.rs` | PASS | 同一字节转录的 4/8 cell 投影；高度树、selection、scroll anchor、cache 独立；live anchor 从 row 1 开始 |
| G3 | `tests/anchor_protocol.rs` | PASS | Live→Staging→History 两步事务；非零行 survivor rebase；ED3/live-origin 与配额/successor 降级；primary/alt 隔离；四版本 stale worker 丢弃；redetect intent rebuild |

## 验证数据

- `cargo test --workspace --locked`：**241 passed，0 failed，0 ignored**（最终复跑数据；vendor 180，第一方 61）。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：**PASS，0 warning**。
- 六个第一方 crate 的定向 `cargo fmt ... -- --check`：**PASS**；vendor 未被第一方格式化策略改写。
- `cargo metadata --no-deps --locked --format-version 1`：`vendor/alacritty_terminal` 仍是 workspace member，版本仍精确为 `0.26.0`。
- 第一方产品代码无 `todo!()` / `unimplemented!()`；测试框架无 ignored case。

## 遗留风险与 M0 欠条

1. `SPIKE_CELL_HEIGHT_SUBPIXELS` 仍是 18 px 的 spike 假值；M0 接真实字体度量后必须注入 session。
2. `SPIKE_DEFAULT_FROZEN_QUOTA` 仍是 100,000；M0 必须以实测内存或配置替代。
3. vendor manifest 自带 resolver 声明，Cargo 会提示“非根包 resolver 被忽略”；根 workspace 已是 resolver 3，提示不影响解析或测试。未修改 vendor manifest，避免无关补丁面。

## 偏离申请

无。generation、LayoutKey、生命周期表、并发 quantum、转录配额与投影模型均按 DESIGN.md 接线；任务书明确延至 M0 的两项参数只做欠条标记，没有自行改变规格。
