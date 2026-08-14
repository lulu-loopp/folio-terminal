# Folio shell integration for bash.
#
# Emits the same two things `folio.ps1` emits, so that a bash pane is
# not a second-class pane: FinalTerm `OSC 133` A/B/C/D command regions, and
# `OSC 7` working-directory reports. See `docs/shell-integration.md`.
#
# It is loaded two ways and behaves the same either way:
#
#   * Folio starts a Git Bash or WSL profile with `--init-file <this>`,
#     in which case `BT_SHELL_INTEGRATION` is set and this file is responsible
#     for the startup files `--init-file` displaced — see "the startup chain".
#   * You dot-source it yourself, as the last relevant line of `~/.bashrc`,
#     exactly as the PowerShell script is dot-sourced into `$PROFILE`. Then
#     `BT_SHELL_INTEGRATION` is unset and nothing is sourced on your behalf.
#
# Nothing here is Folio-specific except the comments: `OSC 133` and
# `OSC 7` are the sequences Windows Terminal, VS Code and iTerm2 all read.

# Bash only, interactive only. `--init-file` is read by neither a non-interactive
# shell nor another shell family, but a hand-installed copy can reach both.
[ -n "${BASH_VERSION-}" ] || return 0
case $- in
    *i*) ;;
    *) return 0 ;;
esac

# Idempotent, and the guard is set **before** the startup chain below rather than
# after the hooks are installed. A user's own `~/.bashrc` may well dot-source
# this file, and that file is one of the ones the chain sources: without the
# guard first, sourcing it would re-enter here and run the chain again, forever.
[ -z "${__bt_integration-}" ] || return 0
__bt_integration=1

# ---------------------------------------------------------------------------
# The startup chain
#
# `bash --init-file <file>` replaces `~/.bashrc` with <file> — that is the whole
# of its documented effect, and it is why this hook can be installed for one
# session without editing anything that belongs to the user. What it costs is
# that the file it replaced no longer runs, and for a *login* shell (which is
# what both `wsl.exe` and the Git Bash shortcut start) `--init-file` is not read
# at all unless the login flag is dropped. So Folio drops it and this
# file puts the chain back, in bash's own documented order.
#
# The order below is `man bash`'s for an interactive login shell: `/etc/profile`,
# then the **first** of `~/.bash_profile`, `~/.bash_login`, `~/.profile` that
# exists. Getting it wrong is not cosmetic on Git for Windows — `/etc/profile` is
# what puts `/mingw64/bin` on the path, so a bash that skipped it is a Git Bash
# that cannot find git.
# ---------------------------------------------------------------------------
if [ -n "${BT_SHELL_INTEGRATION-}" ]; then
    # Unset rather than left set: a nested shell started from this one inherits
    # the environment but reads its own startup files normally, and must not be
    # told that somebody else already ran them.
    unset BT_SHELL_INTEGRATION
    if [ -f /etc/profile ]; then
        . /etc/profile
    fi
    if [ -f "$HOME/.bash_profile" ]; then
        . "$HOME/.bash_profile"
    elif [ -f "$HOME/.bash_login" ]; then
        . "$HOME/.bash_login"
    elif [ -f "$HOME/.profile" ]; then
        . "$HOME/.profile"
    fi
fi

# ---------------------------------------------------------------------------
# Percent-encoding, without forking
#
# The result lands in `__bt_encoded` instead of being printed, because every
# `$(...)` is a subshell and a subshell on MSYS costs about 20ms — a cost this
# would otherwise pay on every prompt. The safe set is RFC 3986 unreserved plus
# sub-delims plus `:`, `@` and `/`, which is the same set `folio.ps1`
# keeps; the two scripts describe the same URIs.
#
# `LC_ALL=C` for the duration makes `${#text}` and `${text:i:1}` count *bytes*,
# so a non-ASCII path is encoded UTF-8 byte by byte as RFC 3986 requires rather
# than character by character, which would encode nothing at all.
# ---------------------------------------------------------------------------
__bt_encode() {
    local text=$1 out='' index char
    local LC_ALL=C
    for (( index = 0; index < ${#text}; index++ )); do
        char=${text:index:1}
        case $char in
            [A-Za-z0-9]) out+=$char ;;
            '-'|'_'|'.'|'~'|'/'|':'|'@') out+=$char ;;
            '!'|'$'|'&'|"'"|'('|')'|'*'|'+'|','|';'|'=') out+=$char ;;
            *) printf -v char '%%%02X' "'$char"; out+=$char ;;
        esac
    done
    __bt_encoded=$out
}

# ---------------------------------------------------------------------------
# Which spelling of "here" this shell reports
#
# A shell reports the directory in the namespace it actually stands in, because
# that is the only one it can state truthfully and the pane it is drawn in knows
# which one that is (`profiles::PathNamespace`).
#
#   * Under MSYS — Git Bash — the process's working directory is a Win32
#     directory that bash merely *spells* `/d/Developer`. `pwd -W` is the MSYS
#     builtin that gives the Win32 spelling, and it is the true one: it is what
#     `CreateProcess` was handed, what Explorer opens, and what every other pane
#     in the window speaks.
#   * Everywhere else — WSL, or a Linux box over ssh — `$PWD` is the answer, and
#     `/home/weiyi/src` is a real directory with no Windows spelling at all.
#     (`wslpath -w` would answer `\\wsl.localhost\<distro>\home\weiyi`, a UNC
#     whose authority the receiving end is obliged to reject as a remote share.)
#
# The style is decided once, here, by asking rather than by sniffing the OS.
# ---------------------------------------------------------------------------
if pwd -W >/dev/null 2>&1; then
    __bt_pwd_style=windows
else
    __bt_pwd_style=posix
fi

__bt_cwd_seen=''
__bt_cwd_uri=''

# `pwd -W` is a builtin but capturing it needs a subshell, so the answer is
# remembered against the `$PWD` it was computed from: the fork happens when you
# `cd`, not on every prompt. On the POSIX side there is no fork at all.
__bt_refresh_cwd() {
    local place
    if [ "$__bt_pwd_style" = windows ]; then
        place=$(pwd -W 2>/dev/null) || place=''
        [ -n "$place" ] && place="/${place//\\//}"
    else
        place=$PWD
    fi
    if [ -z "$place" ]; then
        # An empty report **retracts** the previous directory rather than
        # leaving a stale one to answer for a place the shell has left — the
        # same rule `folio.ps1` follows off a non-filesystem provider.
        __bt_cwd_uri=''
        return
    fi
    __bt_encode "$place"
    __bt_cwd_uri="file://$__bt_encoded"
}

# ---------------------------------------------------------------------------
# The command region
#
# `__bt_between` is 0 only while the shell is waiting for you to type. The DEBUG
# trap fires before every simple command — including every command inside the
# prompt hook itself — so the flag is what tells the *first* command of a line
# from all the machinery around it. The prompt hook lowers it as its very last
# act, which is the moment bash is about to read a line.
# ---------------------------------------------------------------------------
__bt_between=1
__bt_ran=0
__bt_a=$'\033]133;A\007'
__bt_b=$'\033]133;B\007'

__bt_preexec() {
    [ "$__bt_between" = 0 ] || return 0
    __bt_between=1
    __bt_ran=1
    printf '\033]133;C\007'
}

# A DEBUG trap already installed by something else is kept and run first, rather
# than replaced. `trap -p` prints `trap -- 'command' DEBUG`, so `set --` over it
# recovers the command as `$3`.
__bt_previous_debug=''
if [ -n "$(trap -p DEBUG)" ]; then
    eval "set -- $(trap -p DEBUG)"
    __bt_previous_debug=$3
fi
if [ -n "$__bt_previous_debug" ]; then
    trap 'eval "$__bt_previous_debug"; __bt_preexec' DEBUG
else
    trap '__bt_preexec' DEBUG
fi

# The user's own `PROMPT_COMMAND`, kept and called rather than replaced. Joined
# with `;` so that bash 5.1's array form and the older scalar form both survive
# as one string.
__bt_previous_prompt_command=$(IFS=';'; printf '%s' "${PROMPT_COMMAND[*]}")

__bt_return() { return "$1"; }

# `\[` and `\]` fence the markers as non-printing, without which bash measures
# the prompt as several characters wider than it draws and every redraw of a
# recalled line lands in the wrong column.
__bt_wrap_ps1() {
    case $PS1 in
        *"$__bt_a"*) ;;
        *) PS1="\[$__bt_a\]$PS1\[$__bt_b\]" ;;
    esac
}

__bt_prompt() {
    local status=$?
    # D closes the command that just ran, and carries its exit code. Emitted
    # first so that nothing the prompt hooks print lands inside the region that
    # command owns.
    if [ "$__bt_ran" = 1 ]; then
        printf '\033]133;D;%s\007' "$status"
        __bt_ran=0
    fi
    # Then the user's hook, with the status it would have seen if this file were
    # not here, and before the directory is read — a hook that `cd`s is reported
    # from where it left the shell, not from where it found it.
    if [ -n "$__bt_previous_prompt_command" ]; then
        __bt_return "$status"
        eval "$__bt_previous_prompt_command"
    fi
    if [ "$PWD" != "$__bt_cwd_seen" ]; then
        __bt_cwd_seen=$PWD
        __bt_refresh_cwd
    fi
    printf '\033]7;%s\007' "$__bt_cwd_uri"
    # Re-applied every prompt because a theme that rebuilds `PS1` from scratch in
    # its own `PROMPT_COMMAND` — starship, powerline, and most prompt kits — would
    # otherwise drop A and B after the first line, and the region markers would
    # simply stop arriving with nothing to show why.
    __bt_wrap_ps1
    __bt_between=0
}

__bt_wrap_ps1
PROMPT_COMMAND=__bt_prompt

# No `OSC 0`/`OSC 2` title is emitted, deliberately. A title set by the shell
# outranks the working directory in Folio's own name stack, so a pane
# that announced "Ubuntu-24.04" once at startup would be called that forever and
# would stop following `cd` — the opposite of what this file exists to enable.
# A program that means to name itself still can, and still wins.
