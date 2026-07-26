// Throwaway ground-truth simulator for the scroll-strand frozen scheduling.
// Replicates session.rs schedule_detection/schedule_scan using only public bt-detect APIs.
use bt_detect::{
    DetectionContext, DetectionOptions, advance_detection_context,
    detect_frozen_math_blocks_in_context_with_options, scan_math_blocks_in_context_with_options,
};
use bt_transcript::TranscriptId;
use std::collections::BTreeMap;

include!("strand_lines.rs");

fn may_contain(text: &str) -> bool {
    let t = text.trim_start_matches([' ', '\u{2022}', '\u{25e6}', '\u{25aa}', '\u{25cf}']);
    let t = t.trim_start();
    t.starts_with("$$")
        || t.starts_with("\\[")
        || t.starts_with("\\]")
        || t.starts_with("\\begin{")
        || t.starts_with("\\end{")
}

#[test]
fn strand_ground_truth() {
    let lines: Vec<(TranscriptId, &str, &str)> = STRAND
        .iter()
        .map(|(id, state, text)| (TranscriptId((*id).into()), *state, *text))
        .collect();

    // Build the running frozen context and per-id snapshot exactly like schedule_detection.
    let mut running = DetectionContext::default();
    let mut contexts: BTreeMap<TranscriptId, DetectionContext> = BTreeMap::new();
    for (id, _state, text) in &lines {
        contexts.insert(*id, running.clone());
        advance_detection_context(&mut running, *id, text);
    }

    let opts = DetectionOptions::default();
    let text_of: BTreeMap<TranscriptId, &str> = lines.iter().map(|(id, _, t)| (*id, *t)).collect();

    // For every candidate, replicate schedule_scan: required_start -> window -> scan.
    println!("=== per-candidate frozen scan ===");
    for (id, state, text) in &lines {
        if !may_contain(text) {
            continue;
        }
        let cand_ctx = &contexts[id];
        let required_start = cand_ctx.required_start(*id);
        let (initial_context, window_start) = match required_start {
            None => {
                println!("id={} state={} FENCE(no scan) text={:?}", id.0, state, text);
                continue;
            }
            Some(start) => (
                contexts
                    .get(&start)
                    .cloned()
                    .unwrap_or_else(|| cand_ctx.clone()),
                start,
            ),
        };
        // window = entries [window_start ..= id]
        let window: Vec<(TranscriptId, &str)> = text_of
            .range(window_start..=*id)
            .map(|(wid, t)| (*wid, *t))
            .collect();
        let scan = scan_math_blocks_in_context_with_options(
            window.iter().map(|(wid, t)| (*wid, *t)),
            initial_context,
            opts,
        );
        let ends_here = scan.blocks.iter().find(|b| b.end == *id);
        let verdict = if ends_here.is_some() {
            "RENDER"
        } else {
            "suppress"
        };
        println!(
            "id={} obs={} req_start={} verdict={} block={:?}",
            id.0,
            state,
            window_start.0,
            verdict,
            ends_here.map(|b| (
                b.start.0,
                b.end.0,
                b.span.original_source.replace('\n', "\\n")
            ))
        );
    }

    // Whole-history FROZEN RESYNC scan from a Known prefix: the authoritative detection-layer
    // result the session scheduling must reproduce. This is the scroll-strand red gate at the
    // detection layer.
    let all: Vec<(TranscriptId, &str)> = lines.iter().map(|(id, _, t)| (*id, *t)).collect();
    let frozen = detect_frozen_math_blocks_in_context_with_options(
        all.iter().copied(),
        DetectionContext::default(),
        opts,
    );
    let ends: std::collections::BTreeSet<u64> = frozen.iter().map(|b| b.end.0).collect();
    let starts: std::collections::BTreeSet<u64> = frozen.iter().map(|b| b.start.0).collect();

    // The 7 blocks (14 delimiters) that the reflow poison stranded and the resync must recover.
    // Each closer id below is currently `suppressed` in the capture; each must become a detected
    // block after the resync.
    for closer in [111u64, 131, 139, 147, 151, 156, 162] {
        assert!(
            ends.contains(&closer),
            "frozen resync must recover the block closing at id={closer}"
        );
    }
    // The four legitimately-non-rendered rows must NEVER be fabricated into a block: id=55
    // (\end{pmatrix} orphan, \begin eaten), id=70 (prose containing $$), id=101 (pmatrix orphan
    // closer, opener eaten — abandoned, not wrapped), id=171 (lone trailing $$ streaming tail).
    for orphan in [55u64, 70, 101, 171] {
        assert!(
            !ends.contains(&orphan) && !starts.contains(&orphan),
            "id={orphan} must stay source (never anchor a block)"
        );
    }

    // The recovery is real: the naive whole-history scan (no resync) strands these blocks.
    let base = scan_math_blocks_in_context_with_options(
        all.iter().copied(),
        DetectionContext::default(),
        opts,
    );
    let base_ends: std::collections::BTreeSet<u64> = base.blocks.iter().map(|b| b.end.0).collect();
    assert!(
        [111u64, 131, 139, 147, 151, 156, 162]
            .iter()
            .any(|id| !base_ends.contains(id)),
        "the baseline scan must reproduce the strand this fix recovers"
    );
    println!(
        "frozen_block_count={} baseline_block_count={}",
        frozen.len(),
        base.blocks.len()
    );
}
