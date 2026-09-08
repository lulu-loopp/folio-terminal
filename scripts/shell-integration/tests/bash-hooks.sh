#!/usr/bin/env bash
# What `folio.bash` puts on the wire, measured rather than remembered.
#
# Every case below starts a real `bash`, hands it a HOME of its own holding
# fixture startup files, feeds it a line the way a console session does, and
# reads the `OSC 7` and `OSC 133` sequences back out of the streams the shell
# writes them to. Nothing on this machine is read or written outside the
# temporary HOME each case builds: the reader's own `~/.bashrc`, `~/.profile`
# and `~/.bash_profile` are never edited and never read, and `/etc/profile` is
# only ever read — by the shell, on the cases that ask for a login chain.
#
#     bash scripts/shell-integration/tests/bash-hooks.sh
#
# The two load paths are both exercised, because they are two different jobs:
#
#   * `--rcfile <a fixture>` that dot-sources this script, with no
#     `BT_SHELL_INTEGRATION` — the *hand installed* copy, dot-sourced by a
#     reader out of their own `~/.bashrc`;
#   * `--rcfile <the script itself>` with `BT_SHELL_INTEGRATION` set — the
#     *launch* path, where this file is responsible for the startup chain the
#     flag displaced, and the value of the flag says which chain that is.

set -u

here=$(cd -- "$(dirname -- "$0")" && pwd)
script=${1:-$here/../folio.bash}
[ -f "$script" ] || { printf 'no such script: %s\n' "$script" >&2; exit 2; }
script=$(cd -- "$(dirname -- "$script")" && pwd)/$(basename -- "$script")

work=${TMPDIR:-/tmp}/folio-bash-hooks-$$
mkdir -p "$work" || exit 2
trap 'rm -rf "$work"' EXIT
transcript=$work/transcript

failures=0
checked=0

# One case's markers, in the order the shell wrote them: the payload of every
# `OSC 7` and `OSC 133`, one per line, and nothing else. `OSC 0` titles and the
# colours a prompt is drawn with are not under test and are dropped here. The
# directory a case is run from is not either, so `7;<uri>` is reported as `7`.
#
# The prompt's own A and B travel with `PS1`, which bash writes to stderr, while
# `printf` writes C, D and the directory to stdout — so the order the reader's
# terminal sees is the order of the two streams merged, which is what each case
# captures.
markers() {
    tr '\033' '\n' < "$transcript" |
        sed -n 's/^\]\(7;[^\a]*\)\a.*/\1/p;s/^\]\(133;[^\a]*\)\a.*/\1/p' |
        sed 's/^7;.*/7/'
}

# Run one bash with `home` as its whole world, `rcfile` as its startup file and
# the remaining arguments as the lines a reader types.
#
# `env -i` is deliberate: the case's environment is exactly what it names, so a
# variable this session happens to carry cannot decide what the case measures.
# `PATH` is the one thing carried over, because a shell with no `PATH` cannot
# run the `sed` and `grep` its own startup files call.
run_case() {
    local home=$1 rcfile=$2 marker=$3
    shift 3
    if [ -n "$marker" ]; then
        printf '%s\n' "$@" | env -i HOME="$home" TERM=dumb PATH="$PATH" \
            BT_SHELL_INTEGRATION="$marker" \
            bash --rcfile "$rcfile" -i > "$transcript" 2>&1
    else
        printf '%s\n' "$@" | env -i HOME="$home" TERM=dumb PATH="$PATH" \
            bash --rcfile "$rcfile" -i > "$transcript" 2>&1
    fi
}

expect() {
    local name=$1 want=$2 got=$3
    checked=$((checked + 1))
    if [ "$want" != "$got" ]; then
        failures=$((failures + 1))
        printf 'FAIL %s\n' "$name" >&2
        printf '    expected: %s\n' "$(printf '%s' "$want" | tr '\n' '|')" >&2
        printf '    actual:   %s\n' "$(printf '%s' "$got" | tr '\n' '|')" >&2
    fi
}

# The trace one command typed at one prompt is owed: the prompt that offered it,
# the region the command opened, and the prompt that closed it.
one_command_trace='7
133;A
133;B
133;C
133;D;0
7
133;A
133;B'

# ---------------------------------------------------------------------------
# R3-5 — an array-valued PROMPT_COMMAND (bash 5.1 and later)
#
# The reader's hooks run **once** per prompt, and the `133;C` that opens a
# command region belongs to the command the reader typed, not to a prompt hook
# that happened to run after the region was declared open.
# ---------------------------------------------------------------------------
case_array_prompt_command() {
    local home=$work/array
    rm -rf "$home"; mkdir -p "$home"
    cat > "$home/rc" <<EOF
PS1='\$ '
mark_one() { printf 'HOOK1\n'; }
mark_two() { printf 'HOOK2\n'; }
PROMPT_COMMAND=(mark_one mark_two)
. '$script'
EOF
    run_case "$home" "$home/rc" '' 'echo RUN'
    expect 'R3-5 an array PROMPT_COMMAND keeps one region per command' \
        "$one_command_trace" "$(markers)"
    expect 'R3-5 an array PROMPT_COMMAND runs each hook once per prompt' \
        '4' "$(grep -o 'HOOK[12]' "$transcript" | wc -l | tr -d ' ')"
}

# The scalar form has to keep behaving exactly as it did, because it is what
# every bash before 5.1 has and what most readers still write.
case_scalar_prompt_command() {
    local home=$work/scalar
    rm -rf "$home"; mkdir -p "$home"
    cat > "$home/rc" <<EOF
PS1='\$ '
mark_one() { printf 'HOOK1\n'; }
PROMPT_COMMAND='mark_one'
. '$script'
EOF
    run_case "$home" "$home/rc" '' 'echo RUN'
    expect 'R3-5 a scalar PROMPT_COMMAND is still chained and still marked once' \
        "$one_command_trace" "$(markers)"
    expect 'R3-5 a scalar PROMPT_COMMAND runs once per prompt' \
        '2' "$(grep -o 'HOOK1' "$transcript" | wc -l | tr -d ' ')"
}

# The status a command left has to survive both halves of the split: `D` carries
# it, and the reader's own hook is handed it as though this file were not here.
case_exit_status_through_the_chain() {
    local home=$work/status
    rm -rf "$home"; mkdir -p "$home"
    cat > "$home/rc" <<EOF
PS1='\$ '
saw() { printf 'SAW[%s]
' "\$?"; }
PROMPT_COMMAND=(saw)
. '$script'
EOF
    run_case "$home" "$home/rc" '' 'sh -c "exit 7"' 'true'
    expect 'R3-5 the D mark carries the status of the command it closes'         '133;D;7
133;D;0' "$(markers | grep '^133;D')"
    expect "R3-5 the reader's own hook is handed that status too"         'SAW[7],SAW[0],'         "$(grep -a -o 'SAW\[[0-9]*\]' "$transcript" | tail -n +2 | tr '
' ',')"
}

# ---------------------------------------------------------------------------
# R3-8 — the startup chain this file is responsible for
#
# `--init-file` displaces exactly one file, and which one depends on the mode
# the pane asked for. An interactive shell that is not a login shell reads
# `~/.bashrc` and nothing else; a login shell reads `/etc/profile` and then the
# first of `~/.bash_profile`, `~/.bash_login`, `~/.profile` — and not
# `~/.bashrc`.
# ---------------------------------------------------------------------------
chain_home() {
    local home=$1
    rm -rf "$home"; mkdir -p "$home"
    printf 'printf "BASHRC\\n"\n' > "$home/.bashrc"
    printf 'printf "BASH_PROFILE\\n"\n' > "$home/.bash_profile"
    printf 'printf "BASH_LOGIN\\n"\n' > "$home/.bash_login"
    printf 'printf "PROFILE\\n"\n' > "$home/.profile"
}

chain_read() {
    grep -E '^(BASHRC|BASH_PROFILE|BASH_LOGIN|PROFILE)$' "$transcript" | tr '\n' ','
}

case_chain_interactive() {
    local home=$work/chain-interactive
    chain_home "$home"
    run_case "$home" "$script" 'interactive' 'exit'
    expect 'R3-8 an interactive shell that is not a login shell reads ~/.bashrc alone' \
        'BASHRC,' "$(chain_read)"
}

case_chain_login() {
    local home=$work/chain-login
    chain_home "$home"
    run_case "$home" "$script" 'login' 'exit'
    expect 'R3-8 a login shell reads the first of the profile files and not ~/.bashrc' \
        'BASH_PROFILE,' "$(chain_read)"
}

case_chain_login_falls_through() {
    local home=$work/chain-login-tail
    chain_home "$home"
    rm -f "$home/.bash_profile" "$home/.bash_login"
    run_case "$home" "$script" 'login' 'exit'
    expect 'R3-8 a login shell with no ~/.bash_profile falls to ~/.profile' \
        'PROFILE,' "$(chain_read)"
}

# ---------------------------------------------------------------------------
# R3-17 — reading the DEBUG trap that was already there
#
# The reader's own trap is kept and run, and the shell keeps its own positional
# parameters: a file that is dot-sourced may not take a script's arguments away.
# ---------------------------------------------------------------------------
case_previous_debug_trap() {
    local home=$work/debug-trap
    rm -rf "$home"; mkdir -p "$home"
    cat > "$home/rc" <<EOF
PS1='\$ '
theirs() { printf 'THEIRS\n'; }
trap 'theirs' DEBUG
set -- alpha beta gamma
. '$script'
printf 'ARGV[%s]\n' "\$*"
EOF
    run_case "$home" "$home/rc" '' 'echo RUN'
    # After the file has been read, not before it: the trap ran on its own until
    # this file installed one, so the only evidence that it was *kept* is a run
    # from after the install — the `ARGV` line the fixture prints as its last act.
    local kept
    kept=$(awk '/^ARGV\[/ { seen = 1; next } seen && /THEIRS/ { n++ }
        END { print (n > 0 ? "kept" : "lost") }' "$transcript")
    expect 'R3-17 the trap that was already installed still runs' 'kept' "$kept"
    expect 'R3-17 chaining the trap does not cost the command mark' \
        '1' "$(markers | grep -c '^133;C$')"
}

# The same, in a session that has `set -T` on — the one setting that makes bash
# put the DEBUG trap in effect inside a call frame, and therefore the one that
# reaches the second half of this row: the read did find the trap there, and it
# recovered the command by writing the shell's own positional parameters.
case_previous_debug_trap_functrace() {
    local home=$work/debug-trap-functrace
    rm -rf "$home"; mkdir -p "$home"
    cat > "$home/rc" <<EOF
PS1='\$ '
set -T
theirs() { printf 'THEIRS\n'; }
trap 'theirs' DEBUG
set -- alpha beta gamma
. '$script'
printf 'ARGV[%s]\n' "\$*"
EOF
    run_case "$home" "$home/rc" '' 'echo RUN'
    expect 'R3-17 the shell keeps its own positional parameters' \
        'ARGV[alpha beta gamma]' \
        "$(grep -a -m1 -o 'ARGV\[[^]]*\]' "$transcript")"
    local kept
    kept=$(awk '/^ARGV\[/ { seen = 1; next } seen && /THEIRS/ { n++ }
        END { print (n > 0 ? "kept" : "lost") }' "$transcript")
    expect 'R3-17 the trap is kept under functrace too' 'kept' "$kept"
}

case_array_prompt_command
case_scalar_prompt_command
case_exit_status_through_the_chain
case_chain_interactive
case_chain_login
case_chain_login_falls_through
case_previous_debug_trap
case_previous_debug_trap_functrace

if [ "$failures" -gt 0 ]; then
    printf '%s of %s bash-hook checks disagree with folio.bash.\n' "$failures" "$checked" >&2
    exit 1
fi
printf '%s bash-hook checks agree.\n' "$checked"
