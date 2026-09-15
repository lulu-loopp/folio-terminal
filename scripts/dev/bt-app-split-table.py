# bt-app-split-table.py - regenerate the Step 3 row tables in
# `docs/plans/bt-app-split.md` from the measured graph.
#
# The first draft of that plan carried a hand-written row table whose modules
# could not all leave (R-BT-APP-SPLIT findings 2 and 15: five of sixteen rows
# lost every module they listed). The rows now come from here, and the plan says
# so, so that the next person to change them changes this file and re-runs it
# rather than editing a number in prose.
#
# Read-only. It reads one JSON file and prints Markdown; it never builds,
# checks or tests.
#
#   python scripts/dev/bt-app-graph.py            # writes target/bt-app-graph.json
#   python scripts/dev/bt-app-split-table.py      # prints the two tables
#
# Exit status 1 means the manifest below disagrees with the graph - a row names
# a module the graph says cannot leave, or a module the graph frees is in no
# row. That is the check; a silent pass is the point of retaining it.

import json
import sys
from pathlib import Path

GRAPH = Path("target/bt-app-graph.json")

# The variant to plan against. `root_prod` resolves items owned by `main.rs`
# into a `@root` node, so a module naming a root-owned type is correctly
# un-extractable. Planning against `regex_full` (the first inventory's
# convention) is what produced the wrong table.
VARIANT = "root_prod"

# The cut the plan's Step 1 makes: the three functions that leave `i18n`.
CUT = "i18n"

# ---------------------------------------------------------------------------
# The manifest. Each row is a proposed crate: (name, modules, note).
# Rows are in dependency order - a row may only name modules whose intra-app
# dependencies are in this row or an earlier one.
# ---------------------------------------------------------------------------

ROWS = [
    ("bt-i18n", ["i18n"],
     "the `Text` table and `Lang`; a separate commit from Step 1's cut"),
    ("bt-trace", ["trace", "glyph_trace", "preview_trace", "attention_trace"],
     "the generic trace core and the three sinks that need no app identity"),
    ("bt-web-nav", ["webnav", "favicon", "web_trace"],
     "URL grammar, site labels, the favicon store; needs bt-trace"),
    ("bt-shortcuts", ["shortcuts"],
     "the binding table and chord matcher; needs bt-i18n; retarget both scripts"),
    ("bt-anim", ["animation"],
     "GIF and animated-picture streaming, decoder budgets preserved"),
    ("bt-keys", ["input"],
     "key, button and modifier translation over bt-platform"),
    ("bt-pdf", ["pdf"],
     "the glance card's first page, over hayro"),
    ("bt-text-field", ["text_field"],
     "the single-line editor model"),
    ("bt-measure", ["linebreak", "hex_peek", "watch_clock"],
     "the three helpers that survive of the old preview-text and watch rows"),
    ("(into bt-platform)", ["wsl", "app_delegate_wire"],
     "existing platform bridges; a move, not a new crate"),
    ("bt-seed", ["seed", "recent_folders"],
     "the seed vault and the recent-folder list; needs bt-i18n"),
    ("bt-quake", ["quake"],
     "the summon window's policy; needs bt-shortcuts"),
    ("(into bt-persist or bt-i18n)", ["context_menu"],
     "293 lines; a placement, not a crate"),
]

# Modules that cannot leave, with the root-owned item that blocks each. Printed
# as the second table so a Step 3 ticket can see its own prerequisite.
BLOCKED_NOTE = {
    "marks": "root `StatusDot` (marks.rs:1703)",
    "icons": "via `marks`",
    "settling": "root `Motion`, `RevealTween`",
    "arrival": "root `Motion`, `cubic_bezier`; also `marks`",
    "card_trace": "root `TabId` (card_trace.rs:67)",
    "web_thumb": "root `LeafId` (web_thumb.rs:89)",
    "palette_index": "root `AppEvent` (palette_index.rs:403)",
    "preview_watch": "root `AppEvent`",
    "files_watch": "root `AppEvent`",
    "git_watch": "root `AppEvent`",
    "dir_news": "root `AppEvent`",
    "formula_tools": "root `ChromeSprite`, `Motion`; also `icons`, `marks`, `tooltip`",
    "menubar": "root `APP_NAME`; also `shortcuts`",
    "version": "root `APP_NAME`, plus `FOLIO_COMMIT` from the build script",
    "update": "via `version`",
    "hang_watch": "via `version`",
    "persist": "via `hang_watch`",
    "schemes": "via `persist`",
    "pins": "via `persist`",
    "diagnostics": "via `hang_watch`, `version`",
    "preview_viewport": "`use crate::*;` (preview_viewport.rs:2) and an inherent "
                        "`impl Runtime` (:994), which no other crate may define",
    "preview_wrap": "root `PreviewDocument`, `MarkdownBlockLayout`, `WrapMeasure`, "
                    "`MarkdownBlockIntrinsic`, `MarkdownCaretBlock`, `PageArt`",
    "table_block": "root `DocumentMath`, `MarkdownBlockLayout`, `MarkdownSink`, "
                   "`MarkdownStyle`; also `preview`, `seats`",
}

# Files that are entirely `#[cfg(test)]`. They are not production surface and
# are never crate candidates; they belong to the module that declares them.
TEST_ONLY = ["preview_typing", "source_pin", "focus_thumb_restore_tests",
             "preview_viewport_tests"]


def main() -> int:
    if not GRAPH.exists():
        print(f"{GRAPH} is not there - run scripts/dev/bt-app-graph.py first",
              file=sys.stderr)
        return 2
    graph = json.loads(GRAPH.read_text(encoding="utf-8"))

    stats = {}
    for path, value in graph["stats"].items():
        node = path.split("/")[0].removesuffix(".rs")
        row = stats.setdefault(node, {"lines": 0, "test": 0})
        row["lines"] += value["lines"]
        row["test"] += value["test"]

    cut = graph["cuts"][VARIANT][CUT]
    free = set(cut["free"])

    listed = [m for _, mods, _ in ROWS for m in mods]
    problems = []
    for module in listed:
        if module not in free:
            problems.append(f"row names `{module}`, which the graph does not free")
    if len(listed) != len(set(listed)):
        problems.append("a module is named by two rows")
    for module in sorted(free - set(listed)):
        problems.append(f"the graph frees `{module}`, which no row names")

    print(f"### What leaves with no root item moved "
          f"(`{VARIANT}`, after the `{CUT}` cut)")
    print()
    print("| # | Crate | Modules | Lines | Production | What it is |")
    print("| ---: | --- | --- | ---: | ---: | --- |")
    total_lines = total_prod = 0
    for index, (name, modules, note) in enumerate(ROWS, 1):
        lines = sum(stats[m]["lines"] for m in modules)
        prod = sum(stats[m]["lines"] - stats[m]["test"] for m in modules)
        total_lines += lines
        total_prod += prod
        mods = ", ".join(f"`{m}`" for m in modules)
        print(f"| {index} | **`{name}`** | {mods} | {lines:,} | {prod:,} | {note} |")
    print(f"| | **Total** | **{len(listed)} modules** "
          f"| **{total_lines:,}** | **{total_prod:,}** | |")
    print()
    # The graph weights a node at newline count + 1, which is what `wc -l`
    # reports for a file with no trailing newline and what the first inventory
    # counted. The row sums above are newline counts, so the two differ by
    # exactly one per module. Printed rather than reconciled, because a
    # silently adjusted number is how the first table stopped being checkable.
    print(f"Graph total for the same set: {cut['free_count']} modules / "
          f"{cut['free_lines']:,} lines - {cut['free_lines'] - total_lines} more "
          f"than the rows, being one per module for the graph's +1 convention.")
    print()

    print("### What needs a named root item moved first")
    print()
    print("| Module | Lines | Blocked by |")
    print("| --- | ---: | --- |")
    for module, why in BLOCKED_NOTE.items():
        if module in free:
            problems.append(f"`{module}` is listed as blocked but the graph frees it")
        lines = stats.get(module, {"lines": 0})["lines"]
        print(f"| `{module}` | {lines:,} | {why} |")
    blocked_total = sum(stats[m]["lines"] for m in BLOCKED_NOTE if m in stats)
    print(f"| **Total** | **{blocked_total:,}** | |")
    print()

    print("### Test-only files, which are not production surface at all")
    print()
    for module in TEST_ONLY:
        row = stats.get(module)
        if row is None:
            problems.append(f"`{module}` is not in the graph")
            continue
        if row["test"] != row["lines"]:
            problems.append(f"`{module}` is not entirely cfg(test) "
                            f"({row['test']} of {row['lines']})")
        print(f"- `{module}`: {row['lines']:,} lines, all `#[cfg(test)]`")
    print()

    if problems:
        print("MANIFEST DISAGREES WITH THE GRAPH:", file=sys.stderr)
        for problem in problems:
            print("  " + problem, file=sys.stderr)
        return 1
    print("manifest agrees with the graph")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
