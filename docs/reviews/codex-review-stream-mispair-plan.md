我先按你给的优先级读取第三轮审因、交接约束和检测/会话实现，同时检查仓库内的协作说明与相关测试。评审会把“检查点是否可证稳定”“红门是否可判定”和 hold 三套语义分开落到可执行不变量，不只复述现有六条建议。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$files = @('"'AGENTS.md','docs\\AGENTS.md','crates\\AGENTS.md','crates\\bt-detect\\AGENTS.md','cra`
docs\reviews\stream-mispair-3rd-audit.md	18880
docs\HANDOFF-formula-rendering.md	20068
crates\bt-detect\src\lib.rs	130536
crates\bt-term\src\session.rs	386243

### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\reviews\\stream-mispair-3rd-audit.md'"`
# Independent audit (3rd round): "流式块错位滞留" — compress-rewrite.vt

_Read-only 审因. Worktree HEAD `ca3b062` (last product commit `3875209`), based on main.
Capture `.tmp-repaint-capture/compress-rewrite.vt` (+`.chunks`), 265 490 bytes, 104×26,
one initial RESIZE marker only (104×26 @0.45s — no zoom, no manual resize). All findings
reproduced with throwaway probes that were reverted; worktree tree clean._

## TL;DR

- **The root is the SAME failure class the two prior audits named — one unbalanced `
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\HANDOFF-formula-rendering.md'"`
# Handoff:公式渲染(M1.9 线)— 下个 session 从这里接手

_最后更新:2026-07-24,HEAD `33fb866`_

## 当前状态:**M1.9 公式线全部收线,用户真机确认「CC 没什么问题了」**

已提交链(main 分支):
- `bac8d5a` M1.9k — 检测器重写(多行 `$$`/`\[` 渲染,长会话不熄火,两条红线)
- `20846b1` M1.9m — 呈现模型(块高=净高+对称 padding,竖直居中,底锚)
- `efd2587` M1.9o — 消灭滚/点/双击闪回(矮块;DEC 2026 重绘识别 + oracle 工具)
- `b9faf31` M1.9t — 多行大块跨 CC 内部窗格滚动保持渲染(identity/placement 分层
  + 事务级分段映射 + occlusion)+ 数学环境 `\\` 还原(带开关)
- `951c3fc` M1.9u — occluded 尾行不露源码 + Jump 芯片不烤进渲染
  (`reject_claude_code_jump_chip_overlay`
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "neutral|checkpoint|stale|hold|artifact|resync|bridge|occlusion|row.?0|formula|math|delim`
crates/bt-detect/src/lib.rs:7:    DecorationLifecycle, DetectionRevision, GridGeneration, GridPoint, LayoutKey, MathMode,
crates/bt-detect/src/lib.rs:12:pub const MAX_MATH_SOURCE_BYTES: usize = 8 * 1024;
crates/bt-detect/src/lib.rs:17:    /// inside a LaTeX math environment. Set this to `false` once Claude Code emits the original
crates/bt-detect/src/lib.rs:20:    /// Reject a display-math candidate containing Claude Code's exact scroll-review overlay text.
crates/bt-detect/src/lib.rs:35:pub enu
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "struct DetectionContext|impl DetectionContext|is_neutral|scan_math_blocks_impl|frozen_de`
crates/bt-detect/src/lib.rs:82:pub struct PlaceholderArtifact {
crates/bt-detect/src/lib.rs:96:    pub artifact: PlaceholderArtifact,
crates/bt-detect/src/lib.rs:138:pub struct DetectionContext {
crates/bt-detect/src/lib.rs:144:impl DetectionContext {
crates/bt-detect/src/lib.rs:155:    pub fn is_neutral(&self) -> bool {
crates/bt-detect/src/lib.rs:245:    pub artifact: Option<PlaceholderArtifact>,
crates/bt-detect/src/lib.rs:248:    pub stale_artifact: Option<StaleArtifact>,
crates/bt-detect/sr
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "(Get-Content -LiteralPath 'docs\\HANDOFF-formula-rendering.md').Count; (Get-Content -LiteralPath '`
232
278
3446
9573

### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n -S \"M1\\.9p|红线|stale|hold|resize|zoom|重印|reprint|neutral|checkpoint|bridge|occlusion|row-0|`
docs/HANDOFF-formula-rendering.md:8:- `bac8d5a` M1.9k — 检测器重写(多行 `$$`/`\[` 渲染,长会话不熄火,两条红线)
docs/HANDOFF-formula-rendering.md:12:  + 事务级分段映射 + occlusion)+ 数学环境 `\\` 还原(带开关)
docs/HANDOFF-formula-rendering.md:13:- `951c3fc` M1.9u — occluded 尾行不露源码 + Jump 芯片不烤进渲染
docs/HANDOFF-formula-rendering.md:14:  (`reject_claude_code_jump_chip_overlay` 开关;红门 `scripts/dev/check-occlusion-leak.py` 30→0)
docs/HANDOFF-formula-rendering.md:21:- `241e74b` M1.9v — **溢出可达 + occlusion 逐 cell 清 + 粘滞本地回看**:
docs/HANDOFF-f
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$targets = @('"'docs\\HANDOFF-formula-rendering.md','docs\\reviews\\stream-mispair-3rd-audit.md','`
docs\HANDOFF-formula-rendering.md:232
docs\reviews\stream-mispair-3rd-audit.md:278
crates\bt-detect\src\lib.rs:3446
crates\bt-term\src\session.rs:9573

### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\HANDOFF-formula-rendering.md'"`
# Handoff:公式渲染(M1.9 线)— 下个 session 从这里接手

_最后更新:2026-07-24,HEAD `33fb866`_

## 当前状态:**M1.9 公式线全部收线,用户真机确认「CC 没什么问题了」**

已提交链(main 分支):
- `bac8d5a` M1.9k — 检测器重写(多行 `$$`/`\[` 渲染,长会话不熄火,两条红线)
- `20846b1` M1.9m — 呈现模型(块高=净高+对称 padding,竖直居中,底锚)
- `efd2587` M1.9o — 消灭滚/点/双击闪回(矮块;DEC 2026 重绘识别 + oracle 工具)
- `b9faf31` M1.9t — 多行大块跨 CC 内部窗格滚动保持渲染(identity/placement 分层
  + 事务级分段映射 + occlusion)+ 数学环境 `\\` 还原(带开关)
- `951c3fc` M1.9u — occluded 尾行不露源码 + Jump 芯片不烤进渲染
  (`reject_claude_code_jump_chip_overlay`
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$c=Get-Content -LiteralPath $p; $ranges=@(@(1,120),@(10`
--- 1-120 ---
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    error::Error,
    fmt,
    hash::{DefaultHasher, Hash, Hasher},
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_detect::{
    DecorationRecord, DelimiterKind, DetectionContext, DetectionInput, DetectionOptions,
    DetectionTask, LiveDetectionInput, LiveDetectionSource, LiveDetectionTask,
    MAX_MATH_SOURCE_BYTES, MathCellSegment, MathSou
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$c=Get-Content -LiteralPath $p; $ranges=@(@(1,230),@(740,`
--- 1-230 ---
//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use bt_doc::{DecorationIntent, HistoryDocument};
pub use bt_doc::{
    DecorationLifecycle, DetectionRevision, GridGeneration, GridPoint, LayoutKey, MathMode,
    SUBPIXELS_PER_PX, ScreenId, SourceLifecycle, VersionStamp, ViewGeneration,
};
use bt_transcript::{SourceGeneration, TranscriptId};

pub const MAX_MATH_SOURCE_BYTE
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "frozen_detection_context|advance_detection_context|update_detection_context|DetectionCon`
crates/bt-term/src/session.rs:12:    DecorationRecord, DelimiterKind, DetectionContext, DetectionInput, DetectionOptions,
crates/bt-term/src/session.rs:15:    StaleArtifact, advance_detection_context, detect_math_blocks_with_options,
crates/bt-term/src/session.rs:278:    initial_context: DetectionContext,
crates/bt-term/src/session.rs:449:    alternate_detection_context: DetectionContext,
crates/bt-term/src/session.rs:482:    frozen_detection_context: DetectionContext,
crates/bt-term/src/session
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$c=Get-Content -LiteralPath $p; $c[680..1105]'`
    multi_letter_words >= 2 || (multi_letter_words >= 1 && words >= 3)
}

fn is_cjk_prose_char(character: char) -> bool {
    matches!(character,
        '\u{3000}'..='\u{303f}'      // CJK punctuation
        | '\u{3400}'..='\u{4dbf}'    // CJK extension A
        | '\u{4e00}'..='\u{9fff}'    // CJK unified ideographs
        | '\u{f900}'..='\u{faff}'    // compatibility ideographs
        | '\u{ff00}'..='\u{ffef}'    // fullwidth forms
    )
}

type DisplayDelimiter = DelimiterKin
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$c=Get-Content -LiteralPath $p; $ranges=@(@(3310,3445),`
--- 3310-3445 ---
        );
        let restored = self.terminal.begin_resize_transaction();
        debug_assert_eq!(restored, vendor_candidate_rows);
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::VendorRestore { rows: restored },
        );
        Ok(())
    }

    fn trace_resize_event(&mut self, observed_at: Instant, kind: ResizeTraceKind) {
        let Some(started) = self.resize_trace_started else {
            return;
        };
  
初读已经暴露一个关键分歧：现有 `DetectionContext::is_neutral()` 只是“按此前累计扫描得到 closed”，不是独立证明；一旦前缀先丢了一个 `$$`，后续 context 的 closed/open 会整体翻相。因此“找最近 `is_neutral` 行”本身不能当可信墙。我会继续核对 M1.9p 的不可判定论证、冻结上下文的生成/失效路径，以及三类 hold 的实际退出条件，再给出是否应改成“带依赖证据的认证检查点”。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\reviews\\M1.9p-scrollback-symmetric-ambiguity-nogo.md'"`
我先按任务书做证据审阅：完整读取 M1.9p、M1.9 审因、仓库约束，并核对 `efd2587` 的变更；确认门禁、红线和交付文件后再动代码。不会提交。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.3.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$files = @('"'AGENTS.md','docs/prompts/codex-M1-9p-scrollback-detection-coverage.md','docs/reviews`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been termin
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\reviews\\live-norender-audit.md'"`
# Independent audit: "吃输出" + "完整 $$ 块间歇性不渲染,zoom 才出" (pure-stream Codex)

_Read-only 审因. HEAD `d0dda35` (worktree, based on main HEAD). Main capture
`.tmp-repaint-capture/live-norender.vt` (+`.chunks`), 186 711 bytes, 104×26, 2 390 chunks,
one initial RESIZE marker only (104×26 @0.5s — no zoom, no manual resize). All findings
reproduced with throwaway probes that were reverted; worktree tree clean._

## TL;DR

- **Both symptoms are one root: a detector `$$`-pairing desync in the scanned history,
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "stale_artifact|offscreen_decorations|primary_repaint_snapshot|primary_repaint_in_progres`
crates/bt-term/src/session.rs:282:    stale_artifact: Option<StaleArtifact>,
crates/bt-term/src/session.rs:454:    offscreen_decorations: VecDeque<LiveDecorationRecord>,
crates/bt-term/src/session.rs:462:    primary_repaint_in_progress: bool,
crates/bt-term/src/session.rs:465:    /// `observe_live_damage` does not invalidate a decorated row: the proven raster keeps rendering
crates/bt-term/src/session.rs:467:    /// and its source flashing through. `finish_primary_repaint` reprojects the snapsho
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\reviews\\M1.9p-scrollback-symmetric-ambiguity-nogo.md'"`
我先按任务书做证据审阅：完整读取 M1.9p、M1.9 审因、仓库约束，并核对 `efd2587` 的变更；确认门禁、红线和交付文件后再动代码。不会提交。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.3.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$files = @('"'AGENTS.md','docs/prompts/codex-M1-9p-scrollback-detection-coverage.md','docs/reviews`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been termin
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'docs\\reviews\\live-norender-audit.md'"`
# Independent audit: "吃输出" + "完整 $$ 块间歇性不渲染,zoom 才出" (pure-stream Codex)

_Read-only 审因. HEAD `d0dda35` (worktree, based on main HEAD). Main capture
`.tmp-repaint-capture/live-norender.vt` (+`.chunks`), 186 711 bytes, 104×26, 2 390 chunks,
one initial RESIZE marker only (104×26 @0.5s — no zoom, no manual resize). All findings
reproduced with throwaway probes that were reverted; worktree tree clean._

## TL;DR

- **Both symptoms are one root: a detector `$$`-pairing desync in the scanned history,
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$c=Get-Content -LiteralPath $p; $ranges=@(@(700,1035),@`
--- 700-1035 ---

    pub fn live_detection_count(&self) -> u64 {
        self.live_detection_count
    }

    pub fn frozen_detection_count(&self) -> u64 {
        self.frozen_detection_count
    }

    pub fn live_invalidation_count(&self) -> u64 {
        self.live_invalidation_count
    }

    pub fn register_live_anchor(
        &mut self,
        screen: ScreenId,
        point: GridPoint,
        bias: Bias,
    ) -> AnchorId {
        self.document.register_anchor(Con
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\session.rs'; "'$c=Get-Content -LiteralPath $p; $c[1487..1810]'`
    fn finish_primary_repaint(&mut self, snapshot: AlternateRepaintSnapshot) {
        let dirty = std::mem::take(&mut self.primary_repaint_dirty);
        if self.live_screen != ScreenId::Primary || self.terminal.modes().alternate_screen {
            return;
        }
        if !dirty && self.offscreen_decorations.is_empty() {
            // The reprint changed no row under any resident record and nothing is waiting off-band:
            // every record is already correctly placed, so 
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$c=Get-Content -LiteralPath $p; $c[1420..1585]; $p2='"'cr`

fn commonmark_indented_code(text: &str) -> bool {
    let mut columns = 0usize;
    for character in text.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += 4 - columns % 4,
            _ => break,
        }
        if columns >= 4 {
            return true;
        }
    }
    false
}

fn commonmark_fence_marker(text: &str) -> Option<(char, usize)> {
    if commonmark_indented_code(text) {
        return None;
    }
    let t
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "resolve_live_detection_tasks|live_logical_lines|live_grid_boundary_index|live_occurrence`
1499:    let logical = live_logical_lines(inputs);
1500:    let boundary = live_grid_boundary_index(&logical, inputs);
1524:    let grid_logical = live_logical_lines(&grid_inputs);
1611:    let logical = live_logical_lines(&task.inputs);
1617:    let live_grid_boundary = live_grid_boundary_index(&logical, &task.inputs);
1636:pub fn resolve_live_detection_tasks(tasks: &mut [LiveDetectionTask]) {
1643:    let logical = live_logical_lines(&inputs);
1645:    let live_grid_boundary = live_grid_bounda
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n -S \"compress-rewrite|stream-mispair|isolation_gap|FormulaFlashOracle|source_plane|parity|re`
docs\HANDOFF-formula-rendering.md:62:- **trace_blocks 多行暴露**已补:oracle 现打 `source_plane=...`(整屏逐行文本),trace_blocks 用它把
docs\HANDOFF-formula-rendering.md:65:  `FormulaFlashOracle`(逐 occurrence + occlusion)权威、判 0**,cc-topbot/cc-scrollout 的那 1 是工具误报,
docs\HANDOFF-formula-rendering.md:221:  才算真桥,否则废弃、网格重新配对。**oracle 新红门 `isolation_gap`**(孤立可证但缺席检测,
docs\HANDOFF-formula-rendering.md:223:  **`3875209` 收敛性修正(3da6d64 首版回归,审修 stream-mispair.vt)**:废弃判定曾不收敛——
docs\HANDOFF-formula-rendering.md:227:  ②散文判定改按空白
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-ChildItem -LiteralPath '.tmp-repaint-capture' -Force -ErrorAction SilentlyContinue | Select-Ob`

Name                             Length
----                             ------
cc-large.vt                      977426
cc-large.vt.chunks                 9461
cc-scrollout.vt                  762966
cc-scrollout.vt.chunks             5831
cc-stuck.vt                      747525
cc-stuck.vt.chunks                 6359
cc-topbot.vt                     529856
cc-topbot.vt.chunks                5804
claude-repaint.vt                812908
claude-repaint.vt.chunks           6724
codex-
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'git status --short; git log -8 --oneline'`
?? docs/reviews/stream-mispair-3rd-audit.md
ca3b062 docs: handoff — the boundary-resync convergence fix and prose tokenizer correction recorded under the parity entry
3875209 opus: the boundary resync only abandons a frozen opener when the grid reading actually re-pairs, and hyphenated math is no longer mistaken for prose
7717842 docs: handoff — zoom position jump and the odd-parity history poison are closed; isolation_gap red gate documented
3da6d64 opus: an odd-parity $$ residue in frozen hist
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-term\\src\\bin\\bt-repaint-oracle.rs'; "'$c=Get-Content -LiteralPath $p; $c[540..`
    }

    if env::var_os("BT_PROBE_STAGED").is_some() {
        for staged in oracle.session.transcript().staged_rows() {
            let text = staged
                .row
                .cells
                .iter()
                .filter(|cell| !cell.wide_spacer)
                .map(|cell| cell.text.as_str())
                .collect::<String>();
            eprintln!(
                "STAGED[{}] continues={} |{}",
                staged.id.0,
                staged.row.con
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'crates\\bt-detect\\src\\lib.rs'; "'$c=Get-Content -LiteralPath $p; $c[2980..3115]; $p2='"'cr`
        assert!(!resolve_live_detection_task(&mut task));
    }

    fn boundary_inputs(rows: &[(LiveDetectionSource, &str)]) -> Arc<[LiveDetectionInput]> {
        rows.iter()
            .map(|(source, text)| LiveDetectionInput {
                source: *source,
                text: (*text).to_owned(),
                continues: false,
                cell_boundaries: scalar_boundaries(text),
            })
            .collect()
    }

    /// A determinant identity line `=ad-b
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "rg -n -S \"const LIVE_FENCE_HISTORY_CONTEXT_LINES|fn live_detection_context\\(|fn live_initial_det`
53:const LIVE_FENCE_HISTORY_CONTEXT_LINES: usize = 1_024;
1160:    fn live_detection_context(&self) -> Arc<[LiveDetectionInput]> {
1200:    fn live_initial_detection_context(&self, inputs: &[LiveDetectionInput]) -> DetectionContext {
1488:    fn finish_primary_repaint(&mut self, snapshot: AlternateRepaintSnapshot) {
1657:    fn primary_resize_preservation_active(&self) -> bool {
1708:    fn restore_offscreen_decorations(&mut self) {
3784:    fn schedule_detection(&mut self, id: TranscriptId) {
3
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "ISOLATION_GAP|flash_detected|live_detection_isolation_gap" crates/bt-term/src/bin/bt-rep`
249:        let flash_detected = self.flash_oracle.flash_detected();
250:        let isolation_gap = self.session.live_detection_isolation_gap();
265:            flash_detected,
575:    let final_isolation_gap = oracle.session.live_detection_isolation_gap();
577:        "ISOLATION_GAP final={final_isolation_gap} max={}",
581:    if oracle.flash_oracle.flash_detected() {

### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command 'rg -n -S "pub struct DetectionContext|pub fn is_neutral|pub fn advance_detection_context|fn scan_m`
138:pub struct DetectionContext {
155:    pub fn is_neutral(&self) -> bool {
699:pub fn advance_detection_context(context: &mut DetectionContext, id: TranscriptId, text: &str) {
807:fn scan_math_blocks_impl<'a>(
860:        // Frozen→live `$$` boundary resync. A Dollars opening whose opener lies in the frozen
1085:fn valid_display_body(body: &str, render_source: &str, options: DetectionOptions) -> bool {
1494:pub fn live_detection_isolation_gap(
1636:pub fn resolve_live_detection_tasks(tasks: &m
目前判断倾向“需修正”，不是推倒重来：真正可收敛的不是裸 `is_neutral`，而是“带来源世系和依赖区间的已认证 token 归属/检查点”。我再用现有真实录制跑一次当前 oracle，确认第三轮报告描述的最终状态及门禁未接退出码，然后开始写评审。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$env:BT_PROBE_INPUT='"'.tmp-repaint-capture/compress-rewrite.vt'; "'$env:BT_PROBE_CHUNKS='"'.tmp-r`

frame=0 elapsed_us=41690 event=pty state=Empty rendered=[] source_rows=[] occluded=[] flash=false detections=0 
invalidations=0 isolation_gap=0 source_plane="\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n"
frame=1 elapsed_us=41887 event=pty state=Empty rendered=[] source_rows=[] occluded=[] flash=false detections=0 
invalidations=0 isolation_gap=0 source_plane="\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n"
frame=2 elapsed_us=4328569 event=resize state=Empty rendered=[] source_row
报告初稿已写完。自检时我补出一个必须明确的门禁分层：真实录制本身就含一个上游孤儿 `$$`，若把“源字节完整性”与“检测器是否正确隔离损伤”混成同一条零容忍门，修复后这份固定录制也永远不可能转绿。报告会把它拆成 source-integrity 信号与 release-blocking containment gate，并要求已知隔离损伤按录制精确标注，新的孤儿仍红。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'docs\\reviews\\codex-review-stream-mispair-plan.md'; "'$c=Get-Content -LiteralPath $p; $c[0.`
# BetterTerminal Codex 流式公式错配修复方案独立评审

_评审基线：HEAD `ca3b062`；重点复核 `stream-mispair-3rd-audit.md`、公式线 handoff、`bt-detect` 扫描器与 `bt-term` 的 live/hold 路径。未修改产品代码。_

## 总体判决

**判决：需修正后可行，不必另起，但不能按原样只做 ⑥→①→③。**

第三轮审因对故障类别的判断成立：当前 live 检测每轮把最多 1024 条 frozen history 与 grid 拼成一个流（`crates/bt-term/src/session.rs:53,1160`），再用单一 `opening` 相位重扫（`crates/bt-detect/src/lib.rs:807`）。任意一处单 `$$` 损伤都能改变整个后缀的解释。现有 seam resync（`lib.rs:860`）只覆盖其中一种相位，不能让这个模型对无界改写触发点收敛。

但方案中的 ① 还缺一个决定性限定：**现有 `DetectionContex
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "git status --short; rg -n \"[ \\t]+"'$" docs/reviews/codex-review-stream-mispair-plan.md'`
?? docs/reviews/codex-review-stream-mispair-plan.md
?? docs/reviews/stream-mispair-3rd-audit.md

### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'docs\\reviews\\codex-review-stream-mispair-plan.md'; \"lines="'$((Get-Content -LiteralPath $`
lines=287 bytes=22087
5:## 总体判决
25:## 1. “中性检查点”必须是什么
27:### 1.1 现有 `is_neutral` 为什么不够
39:### 1.2 建议定义：`CertifiedNeutralCheckpoint`
54:### 1.3 “已证完整块的闭符之后”是否足够
67:### 1.4 改写与失效
77:## 2. ①能否围住全部触发点；替代架构判断
79:### 2.1 ①不能单独围住全部触发点
92:### 2.2 推荐架构：ownership ledger，而非仅“增量 toggle”
104:## 3. ⑥红门是否可判定
106:### 3.1 原句对任意终态屏幕不可完全判定
118:### 3.2 可判定且不泥潭的版本
155:## 4. ③与 stale-artifact/hold 的交互
177:## 5. 六方向逐条采纳意见与依赖
188:## 6. 红线风险审查
190:### M1.9p：不猜屏外前缀
196:### 保持须精确源码
200:### 不 reconcile 重装
204:### 绝不把散文排成公
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'docs\\reviews\\codex-review-stream-mispair-plan.md'; "'$c=Get-Content -LiteralPath $p; $c[10`

实现上可先用有界 Vec/BTreeMap 与反向依赖索引，不必立即上 segment tree。若性能后来需要，再把每行的有限状态 transfer function 做可组合树；正确性边界仍由认证 ownership 决定，而不是由数据结构决定。

## 3. ⑥红门是否可判定

### 3.1 原句对任意终态屏幕不可完全判定

“每个结构 `$$` 必须归属于检出或合法拒绝块，奇偶失衡即红”只有在**结构 token、合法拒绝、观察区域是否封口**都形式化后才可判定。任意终态屏幕中的奇数 `$$` 可能是：

- 正在流式输出、合法未闭合；
- opener 已在 row 0 上方且由既有 occurrence 拥有；
- 用户故意展示的破损 LaTeX；
- CommonMark/code/prose 中的非结构文本；
- 真正的单符损伤。

仅凭最终可见字节无法区分全部情形。把“完成 prompt 在下方”做成产品启发式也有误判风险；它最多可作为特定录制的 seal 元数据。

### 3.2 可判定且不泥潭的版本

让 authori
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.4.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command '$p='"'docs\\reviews\\codex-review-stream-mispair-plan.md'; "'$matches = Select-String -LiteralPath`
no trailing whitespace
?? docs/reviews/codex-review-stream-mispair-plan.md
?? docs/reviews/stream-mispair-3rd-audit.md

评审完成，报告已写入：

[codex-review-stream-mispair-plan.md](D:/Developer/BetterTerminal/docs/reviews/codex-review-stream-mispair-plan.md)

总体判决：**需修正后可行，不必另起。**

核心结论：

- 不能直接把 `DetectionContext::is_neutral()` 当可信检查点；它会继承上游错误相位。
- 应改成带来源世系、依赖区间和 token 归属的“认证检查点”。
- 推荐顺序：**⑥真值账本 → ①＋④＋证据驱动的② → ③ → ⑤**。
- ④必须与①同批，否则无法可靠处理 row-0 opener 剪切及其后新公式。
- hold 应继续维持精确源码的观感，但以 `HeldUnbacked` 独立报告检测死亡。
- 红门需拆分 source-integrity 与 detector-containment；已知坏输入可精确隔离，新增孤儿仍必须红。
- 任意强制 closed、synthetic opener 或模糊源码重锚都会触碰 M1.9p 等红线。

报告包含六方向逐条意见、实施切分及 13 项验收判据。未修改产品代码；现有第三轮审因文件未动。