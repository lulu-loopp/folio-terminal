# `--remove-explorer-menu` — implementer's report, 2026-09-20

Branch `feat/remove-explorer-menu-flag`. Nothing was run: the flag, the Settings switch, the deployment API and the
registry were left untouched. All of this is read off the code.

## The On/Off paths today, before judging anything

**On** (a press on the Settings row): `apply_explorer_place` (main.rs:53724) reads `job_in_flight` and the cached
`state`, then `request_explorer_package` (main.rs:53792) → `request` (explorer_menu.rs:1112) → `run_request` (:1139),
a thread and `msix::register`; then `context_menu::apply(true)` (context_menu.rs:153) → `install_context_menu` inside
`changing_explorer_menu`. The first-run card spends the same thing through `first_run::applications` →
`ExplorerShape::place` → `place_when_on`. **Off** is those two calls with `false`: `run_request`'s else branch
re-reads the machine (`read_state`, :518), asks `removal_for` (:739) for the name and calls `msix::remove`;
`apply(false)` → `remove_context_menu`. **Who owns "is it ours":** `classify` (:596) — the registration's external
path against `current_exe`'s folder through `same_path` (:631), then whether the `folio.exe` in that folder is this
very file — and `bt_platform::explorer_reassert_wanted` (lib.rs:2415), the one rule for "a live stranger's", already
shared by `reassert_wanted` (:696) and `context_menu_reassert_wanted` (lib.rs:2433). **H1 partly:** Off has the calls
and the name but not the ownership question, because a press names *this* Folio by hand, and the flag adds only that
question. **H2 true**, and now bounded. **H3 true**, and already built.

## The decision — `package_removal` (explorer_menu.rs:827), `classic_removal` (context_menu.rs:217)

Both pure, both table-tested, both consuming the owners above; there is no second reading of "is it ours".

| registration | the `folio.exe` it names | answer |
| --- | --- | --- |
| none, or a platform with no first page | — | nothing to remove |
| not probed yet, Windows refused the query, or `current_exe` unreadable | — | unanswerable; remove nothing |
| package `Current` (this folder, this file) | — | remove |
| package `Elsewhere` | gone, or on disk and this very file | remove |
| package `Elsewhere` | on disk and not this file | **leave** (B1) |
| classic: exactly what this build would write | — | remove |
| classic `Stale`, incl. a verb key with no `command` (R2-26) | each named exe gone or ours; or none named | remove |
| classic `Stale` | one named exe on disk and not ours | **leave** (B1) |

Exit `0` on every row but a removal the machine refused, which exits non-zero with Windows' own sentence on stderr.
Bounded by `REMOVAL_TIMEOUT` (60 s, explorer_menu.rs:870): both reads and both removals sit on one thread behind one
`recv_timeout`, so there is one number and one place it is enforced.

## How the one line reaches a console, and the two UNKNOWNs

`adopt_parent_console` is `main`'s first line (main.rs:123292); on Windows (lib.rs:9763) it replaces **only null**
standard handles — a caller's redirection is left alone — via `AttachConsole(ATTACH_PARENT_PROCESS)` and `CONOUT$`.
The write is `write_to_console` (lib.rs:10162): a `stdout` that is not a console handle gets UTF-8 through
`WriteFile`; otherwise it attaches the parent's console and writes UTF-16 to `CONOUT$`, answering `false` only when
there is nowhere at all. The flag ignores that `false` deliberately — `report_at_the_front_door` (main.rs:123231)
turns it into a message box, and an uninstall hook must never raise one.
**① `--version` through a GUI shim.** The binary is `#![windows_subsystem = "windows"]` (main.rs:17), so a shell does
not wait for it and anything printed lands after the prompt returns; and `ATTACH_PARENT_PROCESS` needs a parent that
still exists and holds a console, which an exited shim is not. Either explains "prints nothing", and telling them
apart needs a run I may not make. Certain from the code is the half this flag depends on: a **redirected or piped**
stdout is taken by `write_to_console`'s first branch and never touches a console, so a package manager that captures
its hook's output gets the line. **② The card's Explorer row starts Off** — `first_run.rs:460`, `on: false`, beside
`RowKind::Update`'s `on: true` at :451.

## Wrong in Observed, and what is unverified

Nothing in Observed is wrong. One addition: since R4-13 `classify` compares more than the folder, so a registration
serving *this* folder for a differently named binary already reads `Elsewhere`, and the flag leaves it — which is
right. **No part of `bt-app` has been compiled**: local builds of it were forbidden and `cargo fmt` is all that ran,
so every type, import, bound and match arm added to `cli.rs`, `explorer_menu.rs`, `context_menu.rs`, `i18n.rs` and
`main.rs` is unchecked. Five repository check scripts were run and pass (`check-doc-words`, `check-machine-paths`,
`check-portable-core`, `check-adapter-boundary`, `check-screenshots`). Until CI: compilation, the two table tests,
the usage-block shape test (its line count moved 9 → 10), and the start-up-order test with its needles. Until the
owner's machine: that the flag removes this copy's two registrations and leaves the second copy's, that the line
reaches a package manager's log, and that a refused removal exits non-zero.
