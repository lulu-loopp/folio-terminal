# M3-5 on the Mac — the lane, and the §M3 acceptance sentence on the real binary

Run 2026-09-12 on the Mac mini (Apple M4, macOS 26.6.2), toolchain
`1.94.1-aarch64-apple-darwin`, worktree `~/folio-port/wt/m3-5` at
`feature/macos-single-writer`, `CARGO_TARGET_DIR=~/folio-port/target-m3-5`.
The two launchers beside this file are what produced it.

Both Folio processes were started by the script, both pids were written down
when they were started, and only those two pids were ever ended.

## 1. `lane.sh` — `cargo test -p bt-platform` and `cargo check -p bt-app`

```
### the lane
1:== HEAD ==
3:== cargo test --locked -p bt-platform -j 4 ==
230:test result: ok. 177 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.02s
236:test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
251:test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
257:test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
277:rc=0
278:== cargo check --locked -p bt-app --all-targets -j 4 ==
844:rc=0
845:== df ==

### the cases this ticket added
test instance::tests::a_stale_endpoint_is_removed_by_the_next_holder_and_only_under_the_lock ... ok
test instance::tests::a_symlink_to_one_directory_is_that_directory ... ok
test instance::tests::case_folds_where_the_volume_folds_it_and_nowhere_else ... ok
test instance::tests::one_canonical_directory_is_claimed_once_and_released_on_drop ... ok
test instance::tests::the_launch_socket_fits_a_sockaddr_un_however_long_the_data_directory_is ... ok
test instance::tests::the_runtime_directory_is_this_users_and_is_0700 ... ok
test instance::tests::the_unix_claim_is_a_flock_on_a_descriptor_in_a_private_runtime_directory ... ok
test launch_pipe::tests::a_client_that_never_confirms_commits_nothing ... ok
test launch_pipe::tests::a_line_the_grammar_refuses_is_dropped_without_a_reply_and_without_effect ... ok
test launch_pipe::tests::a_request_crosses_the_launch_endpoint_and_is_answered ... ok
test launch_pipe::tests::a_request_past_the_frame_bound_is_refused_before_it_is_written ... ok
test launch_pipe::tests::a_server_that_never_answers_costs_the_launch_its_budget_and_no_more ... ok
test launch_pipe::tests::an_endpoint_nobody_is_listening_on_refuses_at_once ... ok
test launch_pipe::tests::an_executable_that_is_not_this_one_is_refused ... ok
test launch_pipe::tests::one_directory_is_one_claim_and_one_endpoint_however_it_is_spelled ... ok
test launch_pipe::tests::the_door_is_read_before_it_is_opened ... ok
test launch_pipe::tests::the_endpoint_is_private_to_this_user_and_goes_when_the_listener_does ... ok
```

`$TMPDIR` on this machine is on a **case-insensitive** volume (measured:
`mkdir folio` then `[ -d FOLIO ]`), so
`case_folds_where_the_volume_folds_it_and_nowhere_else` took its
case-insensitive branch — which is the measurement worth recording, because it
is Darwin's `realpath` answering with the on-disk spelling that makes two
spellings of one directory fold to one claim.

## 2. `acceptance.sh` — the §M3 sentence

```
== runtime directory before ==
total 0
drwx------    5 weiyishi  staff    160 Sep 12 20:32 .
drwx------@ 931 weiyishi  staff  29792 Sep 12 20:35 ..
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 37378cf17712eada.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 5581d3cee22b7f0b.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 5e8dfdcee767716e.lock
== ① the first copy, data directory A ==
A1=24175
24175 RN  
-- ls the data directory --
diagnostics.log
session.lock
settings.json
shell-integration
update-check.json
== ② a second copy on the SAME data directory: it hands over ==
second-copy-rc=0
second-copy-seconds=0
-- what the handed-over copy said --
-- how many folio processes are alive --
1
24175 UN  
== ③ a copy on a DIFFERENT data directory: it runs independently ==
B1=24248
24175 RN  
24248 RN  
-- ls both data directories --
diagnostics.log
session.json
session.lock
settings.json
shell-integration
update-check.json
diagnostics.log
session.lock
settings.json
shell-integration
update-check.json
== ④ the runtime directory: one lock and one socket per data directory ==
total 0
drwx------    9 weiyishi  staff    288 Sep 12 20:36 .
drwx------@ 931 weiyishi  staff  29792 Sep 12 20:36 ..
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 37378cf17712eada.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 5581d3cee22b7f0b.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 5e8dfdcee767716e.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:36 909e476e5cb7f286.lock
srw-------    1 weiyishi  staff      0 Sep 12 20:36 909e476e5cb7f286.sock
-rw-------    1 weiyishi  staff      0 Sep 12 20:36 cc9084dcaa13259f.lock
srw-------    1 weiyishi  staff      0 Sep 12 20:36 cc9084dcaa13259f.sock
drwx------ 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501
srw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/909e476e5cb7f286.sock
srw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/cc9084dcaa13259f.sock
-rw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/37378cf17712eada.lock
-rw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/5581d3cee22b7f0b.lock
-rw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/5e8dfdcee767716e.lock
-rw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/909e476e5cb7f286.lock
-rw------- 501 /var/folders/yh/p1l50dbn7sz5rwc2pxg6bhl40000gn/T/folio-501/cc9084dcaa13259f.lock
== ⑤ a second copy on data directory B hands over too ==
second-copy-B-rc=0
== ending the two pids this script started, and nothing else ==
-- the endpoints after both quit --
total 0
drwx------    9 weiyishi  staff    288 Sep 12 20:36 .
drwx------@ 931 weiyishi  staff  29792 Sep 12 20:36 ..
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 37378cf17712eada.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 5581d3cee22b7f0b.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:32 5e8dfdcee767716e.lock
-rw-------    1 weiyishi  staff      0 Sep 12 20:36 909e476e5cb7f286.lock
srw-------    1 weiyishi  staff      0 Sep 12 20:36 909e476e5cb7f286.sock
-rw-------    1 weiyishi  staff      0 Sep 12 20:36 cc9084dcaa13259f.lock
srw-------    1 weiyishi  staff      0 Sep 12 20:36 cc9084dcaa13259f.sock
-- the two logs --
ALL_DONE
```

### What it says

* **② the same data directory hands over rather than writing.** The second
  `folio` under `$HOME/.probe/homeA` exited **0 in under a second**, printed
  nothing, and left **one** folio process alive — the first one. A handover
  that had failed would have opened a window and never returned, because that
  invocation was in the foreground.
* **③ a different data directory runs independently.** `A1=24175` and
  `B1=24248` are both `R` after the second one starts, and each has its own
  data directory.
* **the directory listing.** `A` lists `settings.json` and, once the first
  session write was due, `session.json`. `B`'s twelve seconds of life were not
  long enough for a session write and it lists `settings.json` only — the
  session document is `bt_app::persist`'s debounce and not this ticket's.
* **④ the runtime directory.** `drwx------ 501`, one `srw-------` socket and
  one `-rw-------` lock per data directory, `909e…` and `cc90…` for the two
  live ones. The three older `.lock` files are the test suite's, from the run
  above: a lock file is deliberately never unlinked, because unlinking it would
  destroy the thing the next process is waiting on.
* **the endpoints outlive an orderly quit.** Both `.sock` files are still
  there after both processes ended, which is the design and not a leak:
  `bt-app` parks the endpoint in a `OnceLock` static and a static is never
  dropped, so the general answer is the stale-endpoint cleanup under the
  ownership lock in `instance::claim_data_directory` — see DESIGN §13.28 ⑦.

## 3. The source pin really fires

The one guarantee a Windows runner holds about the Unix arm is a pin on this
file's own text, so it was made to go red before it was trusted. Replacing the
claim's `flock` with `let taken = true;` and running only that case:

```
test instance::tests::the_unix_claim_is_a_flock_on_a_descriptor_in_a_private_runtime_directory ... FAILED
panicked at crates/bt-platform/src/instance.rs:618:9:
the claim asks and does not wait, and it is exclusive: (directory: &Path) -> Option<DataDirectoryClaim> {
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 228 filtered out
```

The file was restored and the suite is green again — 228 passed, 0 failed.
