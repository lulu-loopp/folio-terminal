我会先按你指定的顺序读三份优先文件，再追 `scan_math_blocks_impl`、`advance_detection_context`、`required_start` 的所有调用与相关回放测试；这轮只写架构复核报告，不改检测实现。重点会把“可判定证据、状态机权威归属、单调收敛、两类正当 Suppressed”逐项落到代码语义。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$files = @('"'crates\\bt-detect\\src\\lib.rs','crates\\bt-term\\src\\session.rs','docs\\HANDOFF-fo`
FILE crates\bt-detect\src\lib.rs
//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

mod ledger;
pub use ledger::{
    ContainmentVerdict, LedgerEntry, LegitimateRejection, OrphanKind, OwnershipLedger,
    SourceIntegrityAnnotation, StructuralDelimiterKind, TokenFate,
};
use ledger::{OwnershipRecorder, source_line_of, structural_kind};

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use bt_doc::{DecorationIntent, HistoryDocument};
pu
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg --files -g 'AGENTS.md' -g '"'!target'"' .; git status --short; rg -n \"scan_math_blocks_impl|ad`
crates/bt-detect/src/lib.rs:154:    pub fn required_start(&self, candidate: TranscriptId) -> Option<TranscriptId> {
crates/bt-detect/src/lib.rs:706:pub fn advance_detection_context(context: &mut DetectionContext, id: TranscriptId, text: &str) {
crates/bt-detect/src/lib.rs:800:    scan_math_blocks_impl(lines, initial_context, options, None, None, None)
crates/bt-detect/src/lib.rs:803:/// `live_grid_boundary` is the logical index of the first live-grid line when this scan spans a
crates/bt-det
初步定位与背景一致：frozen-only API 确实把 `live_grid_boundary=None` 传给同一权威扫描器，而 session 在每条定稿行先用无 body 视野的 `DetectionContext` 推进相位，再据 `required_start` 构造候选窗口。下一步我在核对空体 `[$$,$$]`、合法 body、ambiguous-prefix 和现有 phantom 单测的精确语义，避免方案把“看似合理”写成不可证明的规则。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$ranges=@(@(620,1030),@(1031,1255),@(1580,1925),@(2180,22`
RANGE 620-1030
  620:             .chars()
  621:             .any(|character| ('\u{0370}'..='\u{03ff}').contains(&character))
  622: }
  623: 
  624: fn inline_group(runs: Vec<InlineMathRun>) -> Option<MathSpan> {
  625:     let first = runs.first()?;
  626:     let last = runs.last()?;
  627:     Some(MathSpan {
  628:         byte_start: first.byte_start,
  629:         byte_end: last.byte_end,
  630:         original_source: runs
  631:             .iter()
  632:             .ma
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$c=Get-Content -LiteralPath $p; foreach($r in @(@(950,110`
RANGE 950-1105
  950:             };
  951:             if abandon_stale_dollars {
  952:                 opening = None;
  953:                 if let Some(rec) = recorder.as_deref_mut() {
  954:                     rec.abandon_pending(LegitimateRejection::PhantomOpenerAbandoned);
  955:                 }
  956:             }
  957:         }
  958:         if opening
  959:             .as_ref()
  960:             .is_some_and(|active| active.delimiter == DisplayDelimiter::Dollars)
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$x=Get-Content -LiteralPath $p; for($i=3840;$i -le 3895;$`
 3840:     #[test]
 3841:     fn rejects_nested_empty_unclosed_and_adjacent_empty_blocks() {
 3842:         assert!(
 3843:             detect_block_math("$$outer $$ inner$$").is_empty(),
 3844:             "nested delimiter"
 3845:         );
 3846:         assert!(
 3847:             detect_block_math("$$$$").is_empty(),
 3848:             "empty single-line block"
 3849:         );
 3850:         assert!(
 3851:             detect_math_blocks([(TranscriptId(1), "$$"), (TranscriptId
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$x=Get-Content -LiteralPath $p; for($i=4140;$i -le 4235`
 4140:     fn schedule_retry_artifacts(&mut self) {
 4141:         if self.primary_parked || !self.resize_epoch.decorations_allowed() {
 4142:             return;
 4143:         }
 4144:         for id in self.scheduler.retry_sources(WORKER_QUEUE_CAP) {
 4145:             self.schedule_scan(id);
 4146:         }
 4147:     }
 4148: 
 4149:     fn schedule_scan(&mut self, candidate_id: TranscriptId) {
 4150:         let detection_options = self.detection_options();
 4151:         let S
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$x=Get-Content -LiteralPath $p; foreach($r in @(@(2130,`
RANGE 2130-2220
 2130:                     SessionMathTask::Live(mut task) => {
 2131:                         if resolve_live_detection_task(&mut task) {
 2132:                             let artifact = live_placeholder(&task);
 2133:                             let occupied = self.occupied_live_band_rows(task.start.row);
 2134:                             size_resolved_live_task_band(
 2135:                                 &mut task,
 2136:                                 math_presenta
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg --files | rg \"scroll-strand|frozen-phantom|codex-review-frozen|\\.vt"'$|'"\\.chunks"'$"; Get-C`
scripts\dev\width-probe-input.vt
scripts\dev\glyph-probe-input.vt

934a39f (HEAD -> main) opus: the exonerated diagnostics return after the revert took them down with the guilty fix, plus a bt-math pin for the integral's lower limit
77f7b36 opus: a dropped completion is now accounted and re-armed once at source quiescence, fixing the stuck-Pending block without the reschedule storm
ac5c12f Revert "opus: a dropped worker completion reopens its stranded Pending record instead of freezing the bloc
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n \"suppressed|FROZEN\\[|final|FINAL|DECOR|NotDetected|frozen\" .tmp-repaint-capture/scroll-st`
27760:  FROZEN id=147 state=suppressed reason=- src="$$"
27761:  FROZEN id=149 state=suppressed reason=- src="$$"
27762:  FROZEN id=151 state=suppressed reason=- src="$$"
27763:  FROZEN id=153 state=suppressed reason=- src="$$"
27764:  FROZEN id=156 state=suppressed reason=- src="$$"
27765:  FROZEN id=158 state=suppressed reason=- src="$$"
27766:  FROZEN id=162 state=suppressed reason=- src="$$"
27769:  FROZEN id=171 state=suppressed reason=- src="$$"
27772:DECOR_TRACE frame=1782 epoch_u
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'Get-Content -LiteralPath .tmp-repaint-capture/scroll-strand.decor.log | Select-Object -Last 90'`
  FROZEN id=87 state=none reason=- src="$$"
  FROZEN id=89 state=ready reason=- src="$$"
  FROZEN id=91 state=none reason=- src="$$"
  FROZEN id=93 state=ready reason=- src="$$"
  FROZEN id=95 state=none reason=- src="$$"
  FROZEN id=97 state=ready reason=- src="\begin{pmatrix}"
  FROZEN id=100 state=none reason=- src="\end{pmatrix}"
  FROZEN id=101 state=suppressed reason=- src="$$"
  FROZEN id=103 state=suppressed reason=- src="$$"
  FROZEN id=104 state=ready reason=- src="\begin{alig
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n \"BT_PROBE_(FROZEN|DOCDUMP|DECOR|INPUT|CHUNKS|COLUMNS|ROWS)|DECOR_TRACE|FROZEN\\[\" crates/b`
docs/HANDOFF-formula-rendering.md:86:- oracle 跑法:`BT_PROBE_INPUT=<vt> BT_PROBE_CHUNKS=<chunks> BT_PROBE_COLUMNS=106
docs/HANDOFF-formula-rendering.md:87:  BT_PROBE_ROWS=33 cargo run --locked --offline -p bt-term --bin bt-repaint-oracle`
docs/HANDOFF-formula-rendering.md:120:- oracle 新探针:`BT_PROBE_DOCDUMP`(重建完整 scrollback+模拟 app 调度报每页渲染/失败)、
docs/HANDOFF-formula-rendering.md:121:  `BT_PROBE_FROZEN`/`BT_PROBE_STYLES`(canonical frozen 行+样式)。录制:`.tmp-repaint-capture/
docs/HANDOFF-formula-renderi
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -LiteralPath .tmp-repaint-capture/scroll-strand.vt.chunks -TotalCount 12; rg -n \"scro`
# BT_PTY_DUMP_CHUNKS_V1 sequence elapsed_us bytes
0 54068 4
1 54822 19
# RESIZE 104 26 451433
2 5976860 51
3 6387468 4
4 6387881 4
5 6388418 7
6 6388534 4
7 6388646 4
8 6399520 37
9 42998129134 6

现有语义已钉死一个关键点：空体不是合法公式——`valid_display_body` 明确要求非空，测试也拒绝 `[$$,$$]` 和 `$$$$`。因此 B 不需要为“合法空体”保留例外；真正的两难是：无 body 的 checkpoint 不能区分“孤儿闭符”与“合法开符”，而不是空体兼容问题。我会再用 `scroll-strand.vt` 的终态 canonical frozen 文本核对 16 个结构性 Suppressed 的实际拓扑。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$env:BT_PROBE_INPUT='"'.tmp-repaint-capture/scroll-strand.vt'; "'$env:BT_PROBE_CHUNKS='"'.tmp-repa`

FROZEN[1] state=none reason=- |
FROZEN[2] state=none reason=- |│ Run powershell -ExecutionPolicy Bypass -c '$env:CODEX_NON_INTERACTIVE=1; irm          
               │
FROZEN[3] state=none reason=- |│ https://chatgpt.com/codex/install.ps1 | iex' to update.                               
               │
FROZEN[4] state=none reason=- |│                                                                                       
               │
FROZEN[5] state=none reason=- |│ See full releas
回放正在跑（当前主要耗时在离线编译/重建），没有改变仓库。到目前为止的设计倾向已经很明确：不能把 abandon 判定塞进现有奇偶机；扫描器必须成为唯一能基于 body + 前向合法块作裁决的权威，checkpoint 只能消费扫描器产出的、带来源版本的已认证相位。直接让每个候选各自“本地纠错”会导致同一历史前缀出现多种相位，反而破坏收敛。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$files=@('"'docs\\reviews\\stream-mispair-3rd-audit.md','docs\\reviews\\codex-review-stream-mispai`
FILE docs\reviews\stream-mispair-3rd-audit.md
# Independent audit (3rd round): "流式块错位滞留" — compress-rewrite.vt

_Read-only 审因. Worktree HEAD `ca3b062` (last product commit `3875209`), based on main.
Capture `.tmp-repaint-capture/compress-rewrite.vt` (+`.chunks`), 265 490 bytes, 104×26,
one initial RESIZE marker only (104×26 @0.45s — no zoom, no manual resize). All findings
reproduced with throwaway probes that were reverted; worktree tree clean._

## TL;DR

- **The root is the SAME failure clas
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n "认证|certif|checkpoint|检查点|裁决|required_start|advance_detection_context|单调|收敛|扫描器" docs/review`
1:我先按你给的优先级读取第三轮审因、交接约束和检测/会话实现，同时检查仓库内的协作说明与相关测试。评审会把“检查点是否可证稳定”“红门是否可判定”和 hold 三套语义分开落到可执行不变量，不只复述现有六条建议。
34:### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "neutral|checkpoint|stale|hold|artifact|resync|bridge|occlusion|row.?0|formula|math|delim`
55:### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n -S \"M1\\.9p|红线|stale|hold|resize|zoom|重印|reprint|
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'docs\\reviews\\codex-review-stream-mispair-plan.md'; "'$x=Get-Content -LiteralPath $p; for($`
  348: _评审基线：HEAD `ca3b062`；重点复核 `stream-mispair-3rd-audit.md`、公式线 handoff、`bt-detect` 扫描器与 `bt-term` 的 live/hold 路径。未修改产品代码。_
  349: 
  350: ## 总体判决
  351: 
  352: **判决：需修正后可行，不必另起，但不能按原样只做 ⑥→①→③。**
  353: 
  354: 第三轮审因对故障类别的判断成立：当前 live 检测每轮把最多 1024 条 frozen history 与 grid 拼成一个流（`crates/bt-term/src/session.rs:53,1160`），再用单一 `opening` 相位重扫（`crates/bt-detect/src/lib.rs:807`）。任意一处单 `$$` 损伤都能改变整个后缀的解释。现有 seam resync（`lib.rs:860`）只覆盖其中一种相位，不能让这个模型对无界改写触发点收敛。
  355: 
  356: 但方案中的 ① 还缺一个决定性限定
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n \"frozen_detection_contexts|frozen_detection_context|live_initial_detection_context|live_det`
517:    frozen_detection_context: DetectionContext,
518:    frozen_detection_contexts: BTreeMap<TranscriptId, DetectionContext>,
635:            frozen_detection_context: DetectionContext::default(),
636:            frozen_detection_contexts: BTreeMap::new(),
1214:        let inputs = self.live_detection_context();
1215:        let initial_context = self.live_initial_detection_context(&inputs);
1278:    fn live_detection_context(&self) -> Arc<[LiveDetectionInput]> {
1318:    fn live_initi
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$x=Get-Content -LiteralPath $p; for($i=1260;$i -le 1365`
 1260:             };
 1261:             new_tasks.push(task);
 1262:         }
 1263:         resolve_live_detection_tasks(&mut new_tasks);
 1264:         let scheduled = new_tasks.len();
 1265:         for task in new_tasks {
 1266:             self.enqueue_live_task(task);
 1267:         }
 1268:         self.live_detection_count = self.live_detection_count.saturating_add(scheduled as u64);
 1269:         if scheduled != 0 && std::env::var_os("BT_PERF_TRACE").is_some() {
 1270:     
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -LiteralPath crates\\bt-detect\\src\\ledger.rs -Raw; rg -n \"SourceIntegrityAnnotation`
//! Token-ownership ledger and the split source-integrity / detector-containment red gates.
//!
//! Batch ⑥ of the formula-detection rearchitecture (Codex plan review §2.2 ownership ledger, §3.2
//! decidable red gate; 3rd-round audit `stream-mispair-3rd-audit.md` 补法). The ledger is pure
//! instrumentation: it records, for every *structural* display delimiter the authoritative scanner
//! toggles on (`$$`, `\[`, `\]`, `\begin{env}`, `\end{env}`), which fate that same scanner already
//! a
回放终态也核实了：18 条正好是 16 条结构性 `$$` 候选，加两条必须保留的合法源码——`id=55 \end{pmatrix}`（真缺方向性 opener）与 `id=70 › 请用$$包裹`（非结构散文）。这也排除了“统一把 Suppressed 重开再扫”的方向：修复必须只改变有证据的 16 条，不能按状态批量处理。现在开始落报告。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "git diff --check; git status --short; (Get-Content -LiteralPath docs\\reviews\\codex-review-frozen`
?? docs/reviews/codex-review-frozen-phantom-abandon.md
526
4:冻结候选调度，以及 `.tmp-repaint-capture/scroll-strand.vt`（104×26）终态。本文只作
7:## 结论先行
9:**方案判决：需修正后可行，不另起。**
19:1. **A 采用带世系的冻结证据门**：复用 live 的 body-invalid + forward-valid 收敛谓词，但在
20:   frozen 侧额外要求一个精确、可失效的 `FrozenResyncWitness`。仅凭“附近已有 environment
24:   context。不是简单地在一次扫描后回填现有 `required_start`。
34:## 已核实的当前事实
36:### 1. frozen 确实永远绕过现有守卫
46:### 2. frozen 候选窗口先被无 body 的奇偶相位决定
52:- `schedule_scan` 用 `candidate_context.required_start(candidate_id)
报告已写入并通过 `git diff --check`；工作树只新增这一份评审文档。最后校验时我特别保留了一个“停线条件”：若实现无法为 16 条结构记录重建精确世系 witness，验收必须红，不能为了让回放变绿而退化成 frozen 全文启发式。
评审报告已写入：[codex-review-frozen-phantom-abandon.md](/D:/Developer/BetterTerminal/docs/reviews/codex-review-frozen-phantom-abandon.md)

核心判决：**需修正后可行，不另起。**

- A：不能直接在 frozen 全文套用“旧 body 非法 + 前向块合法”；缺少 live seam 时仍有对称 `$$` 方向歧义。必须增加基于精确 token 世系的 `FrozenResyncWitness`。
- B：不把规则塞进无 body 视野的奇偶机。scanner 成为权威，并以 proof index、repair frontier、proof epoch 驱动候选窗口；现有 context 仅作廉价 hint。
- 空体块当前明确非法，`[$$,$$]` 不需要兼容。
- 收敛依靠 certified frontier 单向推进，同一 source revision 不允许 abandon↔re-pair；仅新 delimiter/source/detector 事件可推进。
- 已核实回放终态：16 条结构性 `$$` 应恢复；`id=55 \end{pmatrix}` 与 `id=70 › 请用$$包裹` 必须继续 Suppressed/源码。
- 报告包含实施四阶段、代码共享形态、M1.9p/精确源码/不 reconcile/alt 不动风险，以及完整验收清单。

仅新增该报告，未修改产品代码；`git diff --check` 通过。