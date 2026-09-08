# Changes Folio makes to `portable-pty`

`src/` here is the MIT-licensed `portable-pty 0.9.0` source with a small number
of deliberate Windows changes. `vendor/conpty/README.md` describes the original
loader patch and why the package is vendored at all; this file is the running
list, so that a reader comparing these files with the published crate can see at
once what is ours and why.

Every entry names the review row it answers where there is one
(`docs/plans/review/adversarial-review-2026-09-08.md`).

## The ConPTY loader (`src/win/psuedocon.rs`)

The original patch, described in full in `vendor/conpty/README.md`: the sidecar
`conpty.dll` is loaded only by an absolute path beside the running executable
and only with its paired `OpenConsole.exe` present, the packaged `Conpty*` ABI
is called rather than the compatibility spellings, `ConptyReleasePseudoConsole`
is called after the child is attached, `ConptyClearPseudoConsole` is bound and
exposed as `win::conpty::clear_host_buffer`, and the selected implementation is
reported through `ConPtySource`.

## A job object around each child (`src/win/mod.rs`, `src/win/psuedocon.rs`) — R2-6

The child is created with `CREATE_SUSPENDED`, put in an unnamed job object
carrying `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and then resumed; the job handle
is held by the `WinChild`, so closing the pane ends everything the pane started
rather than leaving grandchildren running with nothing to show them. Suspended
first because a child that is already running can spawn before it is assigned,
and that grandchild would be outside the job. A job that cannot be created or
joined is logged and not an error: the pane is then exactly as good as it was
before this existed. `OpenConsole.exe` is created by the pseudoconsole
implementation rather than by this call, so it is never in the job.

## A registry string is terminated before Win32 reads it (`src/cmdbuilder.rs`) — R2-2

`reg_value_to_string` passed `winreg`'s value bytes straight to
`ExpandEnvironmentStringsW`, which reads until a NUL. The registry does not
guarantee one — its own documentation warns that a string value "may not have
been stored with the proper terminating null characters" — so the call read past
the allocation, twice for every `REG_EXPAND_SZ` value, on every pane spawn. The
bytes now go through `wide_terminated`, which drops a trailing odd byte, ends the
string at an embedded NUL and appends the terminator. Both `ExpandEnvironmentStringsW`
results are checked rather than ignored.

## The registry fills the process environment rather than replacing it (`src/cmdbuilder.rs`) — R2-22

`get_base_env` began with `std::env::vars_os()` and then wrote both
`Environment` registry keys over it, so a child never saw the environment the
window itself was standing in — most visibly its `PATH`. The registry sweep now
builds its own map (keeping upstream's system/user `PATH` merge within it) and
`fill_the_gaps` folds it in with `or_insert`: a name this process already has
keeps its value, and a name it does not have still arrives, which is what reading
the registry was for.

## An environment block refuses a name it cannot spell (`src/cmdbuilder.rs`) — R2-22

`environment_block` wrote every `NAME=VALUE` pair unchecked. A block ends a name
at its first `=` and an entry at its first NUL, so an entry named `A=B` set `A`
to `B=` plus the value — a caller's row silently overwriting a variable it did
not name — and a name carrying a NUL ended the block early and took every entry
after it away from the child. `a_block_can_carry` is the grammar written down,
and an entry that fails it is left out.

## An interpreter's own command line (`src/cmdbuilder.rs`) — R2-8

`CommandBuilder::set_interpreter_line` adds a tail that `cmdline` writes into the
command line verbatim, after `argv[0]` and the switches, which are still quoted
the ordinary way. `cmd.exe` after `/c` parses the rest of the line by its own
rules rather than as argv, and argv quoting cannot express them; the caller that
knows it is addressing an interpreter writes that line (`bt_pty`'s
`through_the_interpreter`) and says so here.

## Three items made public for the terminal's tests (`src/cmdbuilder.rs`)

`wide_terminated`, `a_block_can_carry`, `CommandBuilder::environment_block` and
`CommandBuilder::cmdline`. This package is a fork and not a workspace member, so
`cargo test --workspace` never reaches its own test module; the rules above are
pinned in `crates/bt-pty`, which it does reach.

## The manifest

`futures` and `smol` remain declared as dev-dependencies of a package whose
`examples/` were never vendored; `winapi` gains the `jobapi2`,
`processthreadsapi`, `winbase` and `winnt` features the job object needs.
