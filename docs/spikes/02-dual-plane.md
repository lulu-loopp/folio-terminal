# Spike 02：双平面最小原型（第二轮返工版）

## 结论

**go-with-caveats**。

首次交付把孤立的零件级单测误报为 G1/G2/G3 全通过；第一轮审核据此判定不通过。第一轮返工接通端到端主链并修复原有 blocker，但第二轮审核又发现两个真实缺陷：对 `vte` 的修补破坏了未终止 APC/DCS 后的 ESC 重同步，以及存活的非零行 Live anchor 未在滚动/缩高后 rebase。本轮先将报告恢复为 no-go，加入可复现失败测试，再完成修复。

目前 G1/G2/G3 均由字节输入驱动的集成测试覆盖，两个第二轮 blocker 已关闭，vendored 上游测试也已纳入工作区的一键测试。仍保留 caveat：resize 移出行计算与上游 `Grid::shrink_lines` 内部公式耦合；生产配额参数、异步 worker 和更完整 staging preview 仍需后续里程碑验证。本阶段不得据此自动进入 M0。

## 第二轮 blocker 修复

### NEW-B1：恢复上游 vte 的 ESC 重同步语义

- 完全删除 `vendor/vte` 及根 workspace 中对 `vte` 的 path patch，使用 crates.io 原版 `vte 0.15.0`。
- ED3 不再依赖原始字节嗅探，也不修改 parser 状态机；它只由 `alacritty_terminal::Handler::clear_screen(ClearMode::Saved)` 的结构化 hook 上报。
- APC/DCS 中的普通载荷 `[3J` 不触发 ED3；若出现 ESC，上游 parser 按其既有语义退出/重同步，后续合法 `ESC[3J` 正常触发 ED3。

回归测试：`unterminated_apc_and_dcs_can_resynchronize_on_escape`、`ed3_is_only_emitted_by_the_vt_clear_history_action`。

### NEW-B2：批量原子迁移并 rebase Live anchors

vendor 每次滚动或缩高用一个 `RowsRemoved(Vec<RemovedLiveRow>)` 事件上报全部被移除行。`HistoryDocument::capture_rows_transaction` 在同一事务中：

- 将有实际捕获内容的被移除行从 Live 迁到 Staging；
- 将被移除但未捕获的空白行降级到 live origin；
- 将仍存活的 primary Live anchor 按其前方被移除行数向上 rebase；
- 保持 alternate screen 命名空间隔离。

`ViewportProjection` 保存当前 live row 数，`anchor_y` 对越界 Live anchor 返回 `LiveOutOfBounds`，不再生成越界像素坐标。

回归测试覆盖 row 1 滚动后 `1→0`、缩高后 row 3 `→1`、被移除 row 迁到 History，以及越界拒绝：`live_anchor_rebases_after_scroll_and_height_shrink`、`capture_batch_migrates_removed_rows_and_rebases_survivors`、`live_anchor_outside_grid_is_rejected`。

## 第一轮 blocker 与 major 的收敛状态

- **DL/IL 误捕获**：`scroll_up_relative` 接收显式 capture intent；正常全屏上滚传 `true`，DL/IL 传 `false`，并同时要求 primary、全屏 scroll region、row 0 origin。
- **宽高同时缩小丢历史 / 启发式对齐**：删除 `infer_removed_top`；vendor 在宽度 reflow 前复制真正被顶部移出的 cells，并携带原始 `live_row`。
- **不可信输入伪造 ED3**：删除 adapter 的 `ends_with` 字节嗅探；ED3、RIS、DECCOLM、primary park/restore 均由 VT 语义处理点上报。
- **六 crate 未接线**：`bt-term::DualPlaneSession` 串接 parser、transcript、document、detector、worker queue 和 viewport projection；测试从喂 VT 字节开始，不手工构造主链中间态。
- **转录层无配额**：`TranscriptStore` 是 staging 与 frozen 配额的唯一权威；超额淘汰走与 ED3 相同的删除、tombstone、锚点降级、选区清理与任务取消事务。
- **协议类型重复**：`SourceLifecycle`、`DetectionRevision`、`LayoutKey`、`ViewGeneration`、`VersionStamp` 只在 `bt-doc` 定义一次，其他 crate 直接导入。
- **redetect 语义**：重新检测会重建携带新 revision 的 `DecorationIntent`，再令各 projection 消费新 revision；不是只清布局缓存。
- **分包契约**：parser 接收完整 slice，session 仅按 256 KiB quantum 切分；proptest 随机分包比较 grid、document 与尺寸。
- **锚点投影**：History/Staging/Live 均可映射到每视口 i64 定点像素；普通行、live 行统一使用构造时传入的 cell height；artifact 高度进入高度树。
- **缓存复用**：projection refresh 复用 `(span, source_gen, detection_rev, LayoutKey)` 缓存；仅 artifact 变化的 span 失效，其他 span 命中保持。
- **生命周期清理**：删除了先写入 `Tombstoned` 再立即 remove 的仪式性状态跳转；tombstone 由 transcript 删除记录作为权威事实。

## 三道门

| 门 | 结论 | 端到端证据 |
|---|---|---|
| G1 生命周期回放矩阵 | **PASS** | scroll→staging→tail rewrite→finalize→detect/decorate；宽/高 resize 内容 oracle；变高不回填；跨界 resize split；staging/frozen 配额；ED3；alt park/restore；DECSTBM；RIS/DECCOLM；无换行尾行 |
| G2 双视口投影 | **PASS** | 同一字节驱动转录在 4-cell/10-cell 两视口独立布局；高度树、布局缓存、scroll anchor、selection 独立；refresh 缓存复用及局部 artifact 失效 |
| G3 锚点与四版本 | **PASS** | Live→Staging→History 两步事务；滚动/resize 后存活 Live rebase；越界拒绝；ED3/配额降级；stale generation 丢弃；四版本失效边界；redetect revision 进入 intent |

全尺寸可寻址测试断言实际网格行内容，不只比较 dims：`4×2 → 4×4` 后分别寻址四行写入并验证 `A/B/C/D`。

## Vendor 补丁面

与本机 crates.io 原版逐文件、逐字节比较：

| crate | 文件 | 增删 | 用途 |
|---|---|---:|---|
| `alacritty_terminal 0.26.0` | `src/term/mod.rs` | **+109 / -4** | 精确移出行、capture intent、ED3/RIS/DECCOLM/alt 结构化事件 |

除该文件外 vendored `alacritty_terminal` 与 crates.io 原版一致。`vendor/vte` 不存在，`vte` 没有本地补丁。`vendor/alacritty_terminal` 是 workspace member，因此上游单元、reference 与 doctest 会随 `cargo test --workspace` 一起运行。

## 支撑数据

- `cargo test --workspace --offline`：**224 passed，0 failed，0 ignored**。
  - vendored alacritty：134 unit + 45 reference + 1 doctest = **180**。
  - BetterTerminal 六 crate：**44**。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：**0 warning**。
- `alacritty_history_size()` 始终为 0；永久历史只存在于 `TranscriptStore`。
- vendor 逐文件比对只有 `src/term/mod.rs` 不同，统计为 +109/-4。

## 遗留风险

1. resize 捕获在 `Term::resize` 中镜像上游 `Grid::shrink_lines` 的移出行公式。行为已由宽高同缩、光标位置、空白裁剪和内容 oracle 测试锁定，但升级 alacritty 时必须逐版本审查该公式；这是实现耦合风险，不是已知语义偏离。
2. frozen 默认配额 100,000 是 spike 参数；唯一配额所有权和淘汰事务已验证，生产默认值仍需以后结合内存数据确定。
3. staging live-tail snapshot 当前只服务协议验证；更完整的跨 fragment preview/search API 不在本阶段范围，且不得改变冻结规则。
4. worker 仍是假的固定高度 worker。真实进程隔离、取消耗时和全局公平调度属于后续独立任务，不在本次返工范围。

## 偏离申请

无待裁决的语义偏离。第二轮审核指出的 `vte` 实质偏离已经完全撤销；parser、redetect、生命周期与 ContentAnchor 行为均按 DESIGN.md 实现。resize 公式耦合作为遗留实现风险披露，不申请改变规格。
