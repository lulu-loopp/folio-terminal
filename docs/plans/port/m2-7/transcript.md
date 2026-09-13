# M2-7 — the reading-surface acceptance sweep, as it was run

Five runs on the Mac mini, 2026-09-13, against the debug `folio` of
`feature/macos-reading-sweep`. The scripts beside this file are the ones that
were run; the findings, the numbers and the hand procedure are DESIGN §13.40.
This is the record of what happened in what order, so that the next agent can
re-run it and so that the three defects can be read in the evidence that found
them rather than only in the sentences about them.

## The venue

Apple M4, macOS 26.6.2, one 4K panel presented at 1920×1080 points, backing
scale 2. Lane `m2-7`, `CARGO_TARGET_DIR=~/folio-port/target-m2-7`,
`nice -n 10 -j 4`. The application is `FolioM27.app`, a throwaway bundle with
the identifier `io.github.lulu-loopp.folio.m2-7-acc` and an isolated `HOME`
through a `CFBundleExecutable` wrapper script (§13.31 ⑧(d)). Every window opened
was `100,100 1280×800` points, `2560×1600` physical.

## The runs

| run | what it was for | what came out |
|---|---|---|
| 1 | the first pass, fixtures outside the isolated home | every row but the typing one driven; the `~` rule not exercised, because the fixture folder was not under the reader's home; the `.png` press swallowed by the first-run card (`route=none`) |
| 2 | fixtures moved under the isolated home | `~/pages` on the files column's foot and `~ › pages` on the rail — **and `/Users/…/pages` on the terminal pane's head**, which is finding two. Every point came back `NO-LABEL`: the script piped a frame into a python whose standard input was the here-document carrying the program |
| 3 | the pipe fixed; the pane-head fix in the build | all three surfaces say `~/pages`; the first-run card dismissed by pressing its own button; the `.png` row press routes (`taken=1 at=press-routed … FilesRow { index: 4 }`) but two four-second-apart singles are two first presses and open nothing; the integral still printed as source at 14 s, at 30 s and after a press, with the math worker parked in `recv` |
| 4 | a real double click; the `math` station in `BT_PREVIEW_TRACE` | the pair opens the picture (`open_preview_image … leave=opened`); the station says `math formulas=1 drawn=0 asked=1 worker=1` at 111 ms and `math answered set=1` at 1684 ms, and no page is ever laid out again — finding three |
| 5 | the formula fix | `math answered set=1` at 1558.027 ms and `math formulas=1 drawn=1 asked=0` at 1558.142 ms: the page is resolved again 115 µs later and the integral is on the glass |

## The order the evidence arrived in

1. **`cargo build -p bt-app` and `cargo test -p bt-platform`** through
   `m2-7-door.sh`. Both green; the platform tests are the watch contracts the
   two `FSEvents` rows lean on.
2. **`m2-7-acceptance.sh`**, which builds its own fixtures, writes the seeded
   `session.json`, signs and registers the bundle, and then walks §M2's line in
   three launches: the `.md`, the `.png` and the formula. It ends by
   unregistering the bundle, removing the identifier's three library folders and
   printing `df`.
3. **`cargo test -p bt-math --test acceptance_display_integral -- --nocapture`**
   on both machines, which is the cross-platform reference §13.40 ⑥ reads the
   Mac's photograph against.

## Reading the artifacts

Everything a run writes lands under `~/folio-port/wt/m2-7/out-acc/`:

* `shots/` — `screencapture -x -o -l<windowid>`, so pixel (0,0) is the window's
  own corner and there is no shadow in the frame;
* `dump/chrome.dump` — `BT_CHROME_DUMP`. The only place a *string* the window is
  showing can be read without reading pixels, and the file the click points are
  computed from. `label  [x0, y0, x1, y1] #rrggbb "text"`, appended per frame;
* `dump/preview.trace` — `BT_PREVIEW_TRACE`, including the `math` station this
  ticket added;
* `dump/mouse.trace` — `BT_MOUSE_TRACE`, which is what says a press arrived and
  where it went;
* `dump/pty.dump` — `BT_PTY_DUMP`, the pane's own bytes.

## Rules kept

Every pid the sweep started was recorded and only those were ended — `kill
"$PID"` on the pid `ps` reported for this bundle's own executable path, never a
name and never a pattern. No key was injected. Nothing was written to the general
pasteboard. `~/folio-port/Folio.app` and `Folio-next.app` were not opened, the
production daemon was not touched, and nothing outside `~/folio-port` was written
except the three library folders the bundle identifier reaches, which are removed
at the end of every run.
