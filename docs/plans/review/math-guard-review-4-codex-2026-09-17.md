# Formula pipeline review, round 4

Base: `20b002825c62a34620c41420878cbf0686fa2dac` (2026-09-17).
Status: review in progress; no verdict yet.
Safety: product code read-only; no dangerous construction will be executed.

## Findings

None recorded yet.

## Completed attack items

- Read both original briefs and the round-4 addendum; confirmed a clean worktree at the requested base.
- Items 1-3 (parser/converter): inspected all parser functions and `arg_match.rs`. Six cycles return through `content` (`parser.rs:394`): groups/list; commands/arguments; commands/argument groups/list; environments/body or arguments; left-right/list; attachments. Each opens a node; single-token arguments terminate without recursion (`parser.rs:729`). `text`, `single_char`, `clause_lr`, and conditional-body scanning are iterative.
- Suppression consumes at `parser.rs:398`; argument loops consume or return, even when groups are suppressed. Live/suppressed frames balance (`depth.rs:175`, `:187`); independent preorder rejects excess height. No new non-progress path found. Converter recursion funnels through `converter.rs:321` before descent; sibling walks iterate, with no reparsing or macro expansion there. Ordinary panics are caught at `crates/bt-math/src/lib.rs:73`; stack overflow is not.
- Item 8: emission/asset audit found no new executable-source escape. Math words become spaced characters (`converter.rs:375`); punctuation is escaped (`:408`); code-mode arguments use content blocks (`:904`); raw Typst/image/label sites refuse. Text quotes and backslashes cannot close a string because text arguments are markup, not interpolated string literals. Spec aliases are fixed names/calls. `sys.inputs.source` receives converted text (`crates/bt-math/src/lib.rs:524`), then the template's sole `eval` consumes it in math delimiters (`:219`). Asset color/length parsers consume data; no additional `eval`, file resolver, plugin, or argument-derived code found.
