# Task 00b：M-1 工程基线小返工

## 结论

**go**。00b 的五项返工均已完成，且保持在 M-1 纯逻辑原型范围内：没有开始 spike 03～06，没有进入 M0，没有修改 `Term::resize` / `shrink_lines`，也没有为 `dpi_milli`、`font_rev`、`theme_rev` 制造虚假的语义消费者。

本报告取代 `00-baseline.md` 中“七项生命周期表”和“静态依赖检查已经守护 adapter”的旧结论；旧报告保留为首轮交付的审计记录。

## 逐项结果与反向证据

| 条目 | 改了什么 | 为什么 | 实际验证它会红的证据 |
|---|---|---|---|
| 00b.1 穷尽映射 | 12 个 `Flags` 和 29 个 `NamedColor` 均逐变体断言 capture、decode 和 round-trip；Indexed/RGB 另测 | 单个映射项写错必须独立可见，不能靠并集碰运气 | 将 `DIM → DIM` 实际改坏为 `DIM → HIDDEN`，定向测试 exit 1，报告 `HIDDEN != DIM`；将 `BrightBlue → 12` 改坏为 `→ 13`，定向测试 exit 1，报告 `Named(13) != Named(12)`；两处均已恢复 |
| 00b.2 生命周期判定 | 选择方案 **(a)**。`lifecycle.rs` 现在以 removal 的 screen/scope/cause/row-shape 事实驱动有序规则，输出带 payload 的 typed directive；resize 由 old/new dimensions 产生 `ResizePlan`；ED3、RIS/DECCOLM、alt enter/exit 由同一 classify 入口产生 directive | 原七项表只是 enum 的同义反复。新表会决定一行是 capture、discard-and-rebase 还是 ignore，改表会真实改变主链行为 | 将“primary/full/normal → CaptureAndRebase”实际改成 `Ignore`，G1 `g1_scroll_out_stages_finalizes_decorates_and_observes_tail_rewrite` exit 1（staging 0，期望 1）；已恢复 |
| 00b.3 adapter 只报事实 | `TerminalAdapter` 不再拥有 `TranscriptStore`，不再 capture、clear 或 invalidate；`AdapterEvent::RowsRemoved` 携带 cause、screen、scope、真实 live row 和稳定 cell 数据。转录写入、ED3、candidate invalidation、配额删除均由 session 串行执行 | adapter 只回答 alacritty 发生了什么；§3.1 判定和跨 transcript/document 事务必须在同一个 session owner | adapter 单测直接证明 full/local/DL/alt/resize/ED3/RIS/DECCOLM 的事实；G1 从 VT bytes 穿过 session，证明转录仍由主链写入。`rg` 确认 adapter 无 `TranscriptStore` 变更 API |
| 00b.4 删除幽灵错误 | 删除 `MissingLifecycleRule` 和 `LifecyclePayloadMismatch`；`classify(AdapterEvent) -> LifecycleDirective` 消费 event 并把 payload 放进 directive，因此 mismatch 在类型上写不出来；保留跨 crate 的 `MissingStagingSource` | 原两分支结构上不可到达，伪装成不会响的护栏 | workspace 搜索无两个旧变体；编译器穷尽匹配守护 AdapterEvent→directive；`MissingStagingSource` 仍有真实构造点 |
| 00b.5 CI 边界门 | 新增 `scripts/check-adapter-boundary.ps1` 和独立 `adapter-boundary` CI job，禁止 `adapter.rs` / `cell_capture.rs` 引入 `bt-doc`、`bt-detect`、`bt-viewport` | `bt-term` 的 crate 依赖会让错误 import 正常编译，必须另有静态架构门 | 本地实际植入 `use bt_doc::HistoryDocument as _AdapterBoundaryProbe;` 后脚本 exit 1 并打印文件与行号，移除后 exit 0；CI job 自己也先种同一违规行、要求脚本失败、finally 恢复，再运行绿色检查 |

## DESIGN §3.1 十四行落点

§3.1 的行并非都对应 adapter event。把“没有事件”的规则伪造成 enum 表项会重建 00b 要删除的空门。方案 (a) 因而采用“事实判定集中在 lifecycle，机制由规格指定的 owner 执行”：所有需要 BetterTerminal 作选择的 adapter 事实都经过 `classify` / `plan_resize`，而 staging 配额与 soft-wrap 完整性仍由唯一配额权威 `bt-transcript` 执行。

| §3.1 事件 | 可执行落点 | 门测试 |
|---|---|---|
| 全屏正常上滚移出行 | `LIFECYCLE_RULES`: primary + full + normal → capture；session 写 staging 并 rebase | `g1_scroll_out_stages_finalizes_decorates_and_observes_tail_rewrite` |
| soft-wrap 部分移出 | lifecycle 允许 capture；`TranscriptStore::capture` 按 `continues` 保持 mutable staging，session 的 `sync_staging_tail` 接尾部改写 | 同上 |
| DECSTBM IL/DL/上滚 | vendor 上报 screen/scope/cause；规则 catch-all → ignore | `g1_local_scroll_region_never_enters_history` + adapter local/DL facts |
| 用户向上滚动 | 无 terminal removal fact，只由 viewport 状态改变 | G2 独立 scroll anchor |
| resize 变宽/变窄 | `plan_resize`: width change → 强制定稿；随后 adapter reflow，冻结源不改 | `g1_width_resize_forces_a_cross_boundary_logical_line_split`、`g1_width_reflow_never_rewrites_frozen_source` |
| resize 变高 | `plan_resize` 不强制定稿；vendor 无 removal fact，不回填历史 | `g1_resize_grow_makes_the_entire_new_grid_addressable` |
| resize 变矮 | vendor 的既有 resize hook 给出实际 removed set；规则按 blank/nonblank → discard/capture，并都 rebase | `g1_resize_shrink_captures_exactly_nonblank_rows_removed_from_the_top` |
| resize 抖动 | `plan_resize` 触发 `ResizeEpoch` cooldown，session 在冷却内不调度装饰 | `g1_resize_jitter_does_not_duplicate_captured_rows` |
| ED3 | `classify(ClearHistory)` → clear history + staging；session 走共享 deletion pipeline | `g1_ed3_deletes_history_and_records_tombstones`、G3 ED3 anchor 降级 |
| 配额淘汰 | `TranscriptStore` 是唯一配额权威；session 取得 eviction ids 后调用与 ED3 相同的 `delete_history` 管线 | `g1_frozen_quota_evicts_through_the_document_pipeline`、G3 successor 降级 |
| 进入 alt screen | `classify(PrimaryParked)` → park；alt removal facts仍被规则 ignore | `g1_alternate_screen_parks_detection_and_restores_fresh_work`、`g1_alternate_screen_never_enters_primary_history` |
| 退出 alt screen | `classify(PrimaryRestored)` → generation 更新与任务恢复/作废 | 同上 |
| RIS / DECCOLM | structured vendor fact → `InvalidateStaging`；冻结历史不删 | `g1_ris_and_deccolm_invalidate_candidates_but_keep_frozen_history` |
| 无换行最后一行 | 没有 removal fact，因此进不了 capture policy；只保留 live grid | `g1_unterminated_last_line_remains_live` |

## adapter 事件变大与事务顺序

事件携带 cells 是正确的兼容层边界，不是策略泄漏。vendor hook 在改变 grid 前克隆即将移除的 cell；adapter 只把 upstream cell 转成稳定 `CapturedRow`；session 随后在自己的串行调用栈内依次更新 transcript、document、staging source map、decoration scheduler 和 tombstone。外部在 `feed` / `resize` 返回前无法观察中间状态，因此原子可见性没有因搬迁而削弱，反而消除了“adapter 先改 transcript、session 事后补 document”的双 owner 顺序。

为完整上报 IL/DL/局部/alternate facts，vendor seam 增加 screen/scope/cause，并让 `scroll_up_relative` 在 grid 变更前上报有效移除行。超大 DL 只上报 region 内实际行数，避免不可信 CSI 参数造成越界。相对 crates.io 0.26.0，vendor 仍只有 `src/term/mod.rs` 一个文件不同，当前总补丁为 `+152/-4`；本轮没有改 `Term::resize` 或 `shrink_lines`。

## 验证结果

- `cargo test --workspace --locked`：**245 passed，0 failed，0 ignored**（vendored alacritty 180，第一方 65）。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：**PASS，0 warning**。
- `cargo fmt --check`：已按要求实际运行；它只因 vendored alacritty 的 upstream 格式与本 workspace rustfmt 不同而失败。Task 00 已明确 vendor 不做第一方重排；CI 使用六个第一方 package 的定向命令，该命令 **PASS**。
- `cargo test --workspace --locked -- --ignored --list`：**0 tests, 0 benchmarks**。
- adapter boundary：正常树 **PASS**；实际植入违规 import 的反向探针 **FAIL as expected**，exit 1。
- vendor workspace metadata：`alacritty_terminal 0.26.0` 仍是 workspace member；上游 180 项仍在一键测试内。

## 遗留项（按任务书原样带入 M0，本轮未做）

1. 接入真实 cell height 时，`dpi_milli` 必须真正参与计算；`font_rev` / `theme_rev` 也只能在真实语义接入后扩展，不能用假消费者粉饰。
2. G2 的单个 64 行测试和 G3 的四 boundary 循环继续拆分。
3. `bt-transcript` 两个必为 Some 的 `if let`、临时 Vec 的 `shrink_to_fit()`、`sync_staging_tail` 的必为 Some 分支和 `visible_row(0)` 解释性注释仍登记为小残留。
4. Task 00 已登记的真实字体度量与 frozen quota 实测欠条继续保留。

## 偏离申请

无。方案 (a) 的边界按 DESIGN §1.2/§3.1 实现；没有修改 DESIGN 决策。
