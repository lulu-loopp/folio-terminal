# Where the architecture pictures came from

Two tracked files besides this one, both **own** — drawn here — and both
pictures of `docs/ARCHITECTURE.md`, which is the text they answer to. Where a
picture and the file disagree, the file is right and the picture is the defect.

| File | | |
|---|---|---|
| `today.svg` | **own** | The window process, its seven lanes and every production thread by name, the processes around it, and the crates, as the code stands at `b6ca4329` (2026-09-23). Every thread and process in it is a site counted in `docs/ARCHITECTURE.md` §0.1. |
| `after-the-ruled-migration.svg` | **own** | The same frame after `docs/ARCHITECTURE.md` §5.4 steps 2–5, §4.1 and §12. It draws only what that file rules, with the version it rules; everything the file leaves open is marked *not ruled*. |

**How they were made, and how to change them.** Plain SVG 1.1, laid out on a
1600-unit-wide canvas, dark text on a light ground. A throwaway script computed
the coordinates once; it is not kept, and the SVG is the source — a change is
an edit to the file itself, in the same commit as the prose it follows (§0).
Nothing is fetched when a picture is opened: no fonts, scripts, stylesheets or
images, only the system font stacks named in each file.
