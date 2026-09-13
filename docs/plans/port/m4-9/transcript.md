# M4-9 on the Mac — the six Service cases, and the §M4 acceptance sentence on the real binary

Run 2026-09-12/13 on the Mac mini (Apple M4, macOS 26.6.2), toolchain
`1.94.1-aarch64-apple-darwin`, worktree `~/folio-port/wt/m4-9` at
`feature/macos-services` `23ec90a9`, `CARGO_TARGET_DIR=~/folio-port/target-m4-9`.
The two launchers beside this file are what produced it.

Every process was started by the script, every pid was written down when it was
started, and the only process ended was the one Folio the acceptance script
launched. Nothing was matched by name or by title. The pasteboard used
throughout is `+[NSPasteboard pasteboardWithUniqueName]`; the general pasteboard
was never written. No TCC prompt appeared — `NSPerformService` needs none, which
is what X-4 measured.

## 1. `m4-9-door.sh` — the lane

```
== cargo test -p bt-platform -j 4 ==
test result: ok. 213 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.02s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
rc=0
== cargo check -p bt-app --all-targets -j 4 ==   rc=0
== cargo build -p bt-app -j 4 ==                 rc=0
```

Every `.app` test target printed its own skip line out of `target/debug/deps`,
`macos_services` among them — the gate this file's header describes.

## 2. Registration, and it is LaunchServices' own answer

```
--- pbs -dump_pboard, the rows naming this bundle ---
        NSBundleIdentifier = "io.github.lulu-loopp.folio.m4-9";
        NSBundlePath = "/Users/weiyishi/folio-port/wt/m4-9/out/FolioServicesM49.app";
        NSMenuItem =         {
            default = "Open in Folio M4-9";
--- lsregister -dump, rows naming this bundle: 8
```

## 3. The six cases

```
[      0ms] pid=51503 bundle=…/out/FolioServicesM49.app row="Open in Folio M4-9"
[     33ms] the Services provider was registered
[     33ms] PASS NSApp.servicesProvider answers openInFolio:userData:error:
[    122ms] ready
[    159ms] STEP TheColdDelivery
[    160ms] EVENT openInFolio:userData:error: ["…/fixtures/cold folder/"]
[    160ms] PASS a cold Service reaches the application it started
[    160ms] PASS nothing crossed the door before the application said it was up
[    160ms] STEP APlainFolder
[    204ms] EVENT openInFolio:userData:error: ["…/fixtures/plain/"]
[    204ms] PASS a folder, warm
[    204ms] STEP ASpaceAndCjk
[    240ms] EVENT openInFolio:userData:error: ["…/fixtures/中文 folder/"]
[    240ms] PASS a folder with a space and a CJK character
[    242ms] STEP ThreeAtOnce
[    343ms] EVENT openInFolio:userData:error: ["…/fixtures/one/", "…/fixtures/中文 folder/", "…/fixtures/three three/"]
[    343ms] PASS a multi-selection of three, one event, in order
[    343ms] STEP AFile
[    385ms] EVENT openInFolio:userData:error: ["…/fixtures/中文 folder/notes 中文.md"]
[    386ms] PASS a file is delivered as itself, for the landing to read as its folder
[    386ms] TALLY services=5 crossed_before_ready=0 drained_before_resumed=0 failures=0
```

**The cold case really was cold**: the application did not exist when the
sender ran, LaunchServices started it, and the delivery is the one the sender
made. The multi-selection arrived as **one** event with the three paths in the
order the sender wrote them.

Both bundles were then `lsregister -u`'d and deleted; `pbs -dump_pboard` and
`lsregister -dump` name neither (`0` rows each).

## 4. `m4-9-acceptance.sh` — §M4 acceptance ⑤ on `debug/folio`

The binary is the one `cargo build -p bt-app` produced above, inside a bundle
with its own identifier and an isolated `HOME` through a `CFBundleExecutable`
wrapper (§13.31 ⑧(d)). One Service **cold** on a folder whose name carries a
space and a CJK character, one **warm** on a plain folder while it ran.

```
SENDER NSPerformService("Open in Folio M4-9 App") = true     # …/fixtures/中文 folder
folio pid=51689 after 0s
SENDER NSPerformService("Open in Folio M4-9 App") = true     # …/fixtures/plain
--- the shells this application started, and where each one stands ---
child 51707: …/out-acc/home
child 51716: …/out-acc/fixtures/\xe4\xb8\xad\xe6\x96\x87 folder
child 51745: …/out-acc/fixtures/plain
--- the pty dump ---
pty.dump.2: ]7;file:///…/fixtures/%E4%B8%AD%E6%96%87%20folder
            weiyishi@WeiyideMac-mini …文 folder %
pty.dump.3: ]7;file:///…/out-acc/fixtures/plain
```

Three panes: the tab the launch itself opened, standing in the isolated `HOME`,
and **one tab per Service, each standing in the folder that Service named** —
read off the shell's own working directory and again off the `OSC 7` the shell
wrote into the pane. That is the acceptance sentence.

**The warm Service went to the running process**, which is §13.36 ⑦ made by the
product rather than by a case: no second `folio` was started, so there was
nothing for the launch socket to hand over.

The Folio was ended by the pid the script recorded when it started it; nothing
else was signalled. Both bundles were unregistered and deleted, and the
`Library` directories a bundle identifier reaches were removed.
