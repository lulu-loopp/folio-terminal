# M4-7 on the Mac — the lane, and the attention endpoint on the real binary

Run 2026-09-12 on the Mac mini (Apple M4, macOS 26.6.2), toolchain
`1.94.1-aarch64-apple-darwin`, worktree `~/folio-port/wt/m4-7` at
`feature/macos-attention-socket`, `CARGO_TARGET_DIR=~/folio-port/target-m4-7`.
The two launchers beside this file are what produced it, run against the
branch's final commit.

Both Folio processes were started by the script, both pids were written down
when they were started, and only those two pids were ever ended. Nothing here
wrote the pasteboard, injected a key, or went near the owner's own `~/.claude`,
`~/.codex` or `~/.copilot` — the hook script this ran is a file under the
worktree's own `.probe/`.

## 1. `lane.sh`

```
== cargo test --locked -p bt-platform -j 4 ==
test result: ok. 225 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
rc=0
== cargo check --locked -p bt-app --all-targets -j 4 ==
rc=0
== cargo build --locked -p bt-app -j 4 ==
rc=0
-rwxr-xr-x  1 weiyishi  staff  234741912 /Users/weiyishi/folio-port/target-m4-7/debug/folio
```

The nine cases this ticket added, all of which need a real socket and a real
`$TMPDIR` and none of which can run on the Windows workstation:

```
test attention_pipe::tests::a_line_crosses_the_endpoint_and_the_same_value_arrives ... ok
test attention_pipe::tests::a_message_past_the_frame_bound_is_refused_before_it_is_written ... ok
test attention_pipe::tests::a_peer_of_another_user_is_not_this_endpoints_caller ... ok
test attention_pipe::tests::a_stale_socket_is_cleared_by_the_next_holder_of_the_claim ... ok
test attention_pipe::tests::every_client_that_attaches_is_accounted_for ... ok
test attention_pipe::tests::one_directory_is_one_doorbell_however_it_is_spelled ... ok
test attention_pipe::tests::only_this_users_runtime_directory_can_be_addressed ... ok
test attention_pipe::tests::the_door_is_read_before_a_capability_is_written_to_it ... ok
test attention_pipe::tests::the_endpoint_is_private_to_this_user_and_goes_when_the_listener_does ... ok
```

**Three passes, and what each of them found.** The first went red on two of this
ticket's own pins, and both were the pin rather than the arm. `every_attention_door_keeps_its_signature_on_every_machine`
compared a signature rustfmt had wrapped against one it had left on a line and
called a trailing comma a difference; `the_unix_arm_says_which_principal_it_is_naming`
looked for "wider principal than the Windows door" in a header that spells it
`*wider* principal…`. Both are now read out of the file's prose with
its comment markers and line breaks folded out, so what they hold is the
sentence rather than where the eightieth column fell. The second pass was green
on every pin and red on **`http::tests::the_releases_list_comes_back_as_json`
and `a_body_longer_than_the_cap_is_an_error`** — M4-10's two NSURLSession cases,
both answering "The request timed out", neither of them anything this ticket
touches. The third pass, run with nothing else changed, is the **225 passed, 0
failed** above. The flake is recorded rather than quietly re-run away: those two
cases reach the network.

## 2. `acceptance.sh` — a hook posts and the pane is raised

### The venue fact the boundary rests on

```
TMPDIR=/var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/
DARWIN_USER_TEMP_DIR=/var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/
RUNTIME=/var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501
```

**`$TMPDIR` is per user and per boot, not per session** — measured rather than
assumed, and it is the measurement §13.37 ① rests on. This is an `ssh` session
and it is handed the *same* `/var/folders/yh/…/T/` the console session has, so
the private runtime directory adds nothing at all to the question of whether a
second login of this user is inside the door. It is not a session boundary and
the module does not claim it is one.

### One data directory, three files, one digest

```
srw-------  1 weiyishi  staff  0  30f97d9a9d6858a3.attn.sock
-rw-------  1 weiyishi  staff  0  30f97d9a9d6858a3.lock
srw-------  1 weiyishi  staff  0  30f97d9a9d6858a3.sock
drwx------ weiyishi /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501
```

The claim, the launch door and the doorbell, all built out of one folding of one
data directory's canonical path (§13.28 ②, with one more name in it). The
doorbell is `srw-------` inside a `drwx------` of this user's, and its whole path
is **85 bytes** — `sun_path` is 104 including the terminator, and the doorbell is
the longer of the two socket names.

### What the pane was told, and how it got out of the pane

`FOLIO_PANE=1.1`, `FOLIO_ATTENTION_PIPE=/var/folders/yh/…/T/folio-501/30f97d9a9d6858a3.attn.sock`,
and a 33-byte `FOLIO_ATTENTION` (present, never printed). It left the pane
through the isolated `$HOME`'s own shell startup files, which is the whole
reason this run uses an isolated `HOME`: no key injection, no pasteboard, and
nothing read out of the owner's session.

### The hook, and the line it produced

```
#!/bin/sh
'/Users/weiyishi/folio-port/target-m4-7/debug/folio' attention claude-code:PermissionRequest
```

Single-quoted, no `&`, no endpoint in the line — this is exactly what
`attention_hooks::command_for_on(…, HostPlatform::MacOs)` renders, and the
address travels in the environment the pane's shell handed its child, as it
always has.

```
hook-rc=0
# BT_ATTENTION_TRACE_V1 elapsed_ms event field=value…
11081.040 mint  tab=0 seat=SeatId(1) episode=1 src=pipe gen=1 grounds=awaiting prev=-
11091.462 admit tab=0 seat=SeatId(1) ticket=0 episode=1 grounds=awaiting active=1 focused=0
11093.410 toast tab=0 seat=SeatId(1) why=awaiting ticket=0 episode=1 reach=marks
11093.526 claim tab=0 episode=1 was=Silent now=Awaiting
```

**That is the acceptance sentence.** `src=pipe` is the endpoint; `mint` is the
line having crossed it, parsed, and named the pane whose capability it carried;
`admit` is the queue giving that pane a place; `toast … reach=marks` and
`claim … now=Awaiting` are the pane actually being raised. Every link from a
shell command to a mark on a tab, over a Unix socket, on the first machine this
code has ever run on.

### The two refusals

```
== a capability that names no pane is delivered and refused ==
forged-hook-rc=0   mint-lines-before=1 mint-lines-after=1

== a name outside this user's runtime directory never reaches a socket ==
off-grammar-hook-rc=1
```

The first is the division of labour working: the transport delivered a
well-formed line (the verb exited 0) and the *grammar* refused it, because a
capability that names no live pane names nothing. The second is `names_an_endpoint`
— `/tmp/folio.sock` is not in this user's runtime directory, so the verb exits
non-zero without a socket ever being touched.

### The stale doorbell, and the next holder clearing it

```
== end the one pid this script started ==
54928 Terminated: 15 …
P1 has ended
== the name it left behind ==
srw-------  1 weiyishi  staff  0  Sep 12 23:04  30f97d9a9d6858a3.attn.sock
srw-------  1 weiyishi  staff  0  Sep 12 23:04  30f97d9a9d6858a3.sock
```

**Both names are still standing after the process ended**, which is §13.28 ⑦'s
point made by the product rather than by a case: `bt-app` parks both endpoints
in `OnceLock` statics and a static is never dropped, so an orderly quit leaves
the names behind exactly as a crash does. The cleanup under the lock is the
answer and the `Drop` is not.

```
P2=55017 … RN
srw-------  1 weiyishi  staff  0  Sep 12 23:05  30f97d9a9d6858a3.attn.sock
srw-------  1 weiyishi  staff  0  Sep 12 23:05  30f97d9a9d6858a3.sock
second-run-hook-rc=0
14446.915 mint   tab=0 seat=SeatId(1) episode=1 src=pipe gen=1 grounds=awaiting prev=-
14447.358 refuse tab=0 seat=SeatId(1) episode=1 reason=watched active=1 focused=1
```

**Both names carry the second run's minute, not the first's** — 23:05 where a
moment earlier they said 23:04 — which is the unlink and the rebind visible in
the directory listing itself. The second Folio took the claim, cleared both
stale names under it, bound over them, and **answered a hook**, which is the
only thing that could have made the verb exit 0: a name with nobody behind it
refuses a connection rather than accepting one. The second run's `refuse …
reason=watched` is the ledger's own correct answer for a pane the reader is
already looking at, and is not a transport fact.

## 3. Housekeeping

`~/folio-port/target-m4-7` and `~/folio-port/wt/m4-7` were removed when the
ticket's Mac work finished. Nothing outside `~/folio-port` and `~/.rustup` was
written.
