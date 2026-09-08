# Folio shell integration for bash.
#
# Emits the same two things `folio.ps1` emits, so that a bash pane is
# not a second-class pane: FinalTerm `OSC 133` A/B/C/D command regions, and
# `OSC 7` working-directory reports. See `docs/shell-integration.md`.
#
# It is loaded two ways and behaves the same either way:
#
#   * Folio starts a Git Bash or WSL profile with `--init-file <this>`,
#     in which case `BT_SHELL_INTEGRATION` names the startup chain this file is
#     responsible for putting back — see "the startup chain".
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
# **Which chain, though, is the pane's question and not this file's.** bash has
# two, and it reads exactly one of them:
#
#   * an interactive shell that is *not* a login shell reads `~/.bashrc`, and
#     nothing else — that is the single file `--init-file` displaces;
#   * an interactive *login* shell reads `/etc/profile` and then the **first**
#     of `~/.bash_profile`, `~/.bash_login`, `~/.profile` that exists, and does
#     not read `~/.bashrc` at all.
#
# So `BT_SHELL_INTEGRATION` carries the mode rather than a bare `1`: `login` for
# a profile whose own arguments asked for a login shell — which is what both
# `wsl.exe` and the Git Bash shortcut start — and `interactive` for one that did
# not. An older spelling of the flag is read as `login`, which is what it always
# meant. Getting the login chain wrong is not cosmetic on Git for Windows:
# `/etc/profile` is what puts `/mingw64/bin` on the path, so a bash that skipped
# it is a Git Bash that cannot find git; and getting the interactive one wrong
# costs the reader every alias, function and prompt they keep in `~/.bashrc`.
#
# The login flag itself cannot simply be passed alongside `--init-file`, and
# that is bash's rule rather than a choice: `--init-file` names the startup file
# of an interactive shell that is not a login shell, and a shell started with
# `-l` does not read it at all — measured, not assumed. So the flag is dropped
# at the spawn and its chain is emulated here. What that leaves undone is
# `shopt login_shell`, which stays off for a pane whose profile asked for a
# login shell; a startup file that branches on it takes the non-login branch.
# ---------------------------------------------------------------------------
if [ -n "${BT_SHELL_INTEGRATION-}" ]; then
    # Read into a local name and unset rather than left set: a nested shell
    # started from this one inherits the environment but reads its own startup
    # files normally, and must not be told that somebody else already ran them.
    __bt_mode=$BT_SHELL_INTEGRATION
    unset BT_SHELL_INTEGRATION
    case $__bt_mode in
        interactive)
            if [ -f "$HOME/.bashrc" ]; then
                . "$HOME/.bashrc"
            fi
            ;;
        *)
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
            ;;
    esac
    unset __bt_mode
fi

# ---------------------------------------------------------------------------
# Percent-encoding, without forking
#
# The result lands in `__bt_encoded` instead of being printed, because every
# `$(...)` is a subshell and a subshell on MSYS costs about 20ms — a cost this
# would otherwise pay on every prompt. The safe set is RFC 3986 unreserved plus
# sub-delims plus `:`, `@` and `/`, which is the same set `folio.ps1` and
# `folio.zsh` keep; the three scripts describe the same URIs.
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
#     `/home/user/src` is a real directory with no Windows spelling at all.
#     (`wslpath -w` would answer `\\wsl.localhost\<distro>\home\user`, a UNC
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
# act, which is the moment bash is about to read a line, and it is raised again
# by the mark: **one `C` per prompt**, whatever else runs in between.
# ---------------------------------------------------------------------------
__bt_between=1
__bt_ran=0
__bt_debug_adopted=''
__bt_previous_debug=''
__bt_a=$'\033]133;A\007'
__bt_b=$'\033]133;B\007'

__bt_preexec() {
    [ "$__bt_between" = 0 ] || return 0
    __bt_between=1
    __bt_ran=1
    printf '\033]133;C\007'
}

# A DEBUG trap already installed by something else is kept and run first, rather
# than replaced — and it is read at the **first prompt** rather than here.
#
# Here is the one place it cannot be read from. A file being read by `.` is a
# call frame, and bash does not put the DEBUG trap in effect inside a call frame
# unless `set -T` is on: `trap -p DEBUG` in there answers with nothing however
# many traps the session holds, so a reader who dot-sources this file out of
# their own `~/.bashrc` used to have their trap replaced by ours with neither
# half able to notice. `PROMPT_COMMAND` is executed at the top level, where the
# trap *is* in effect, and it is also the last moment before a command of the
# reader's can run — so nothing is missed by waiting for it, and a trap
# installed by a startup file this one sourced is seen as well.
#
# The spec arrives as an argument because `$(trap -p DEBUG)` has to be expanded
# out there, at the top level; the parsing happens in here, where `set --`
# writes this function's own positional parameters and leaves the shell's alone.
# `.  folio.bash` from a script that was passed arguments must not take them away.
__bt_adopt_debug_trap() {
    [ -z "$__bt_debug_adopted" ] || return 0
    __bt_debug_adopted=1
    # `trap -p` prints `trap -- 'command' DEBUG`, and the shell's own quoting is
    # what `set --` removes, so the command comes back as `$3`.
    eval "set -- $1" 2>/dev/null || set --
    case ${3-} in
        # Nothing there, or something that already calls us — a session that
        # re-entered this file would otherwise chain its own handler to itself.
        ''|*__bt_preexec*) trap '__bt_preexec' DEBUG ;;
        *)
            __bt_previous_debug=$3
            trap 'eval "$__bt_previous_debug"; __bt_preexec' DEBUG
            ;;
    esac
}

# The user's own `PROMPT_COMMAND`, kept and called rather than replaced.
#
# **Which shape it is decides how**, and there are two. The scalar every bash
# has had is one string, and it is chained by being run from inside this file's
# own hook. bash 5.1's array is a *list* of hooks run in order, and assigning a
# name over it writes element 0 and leaves the rest — so the hooks after the
# first would run twice, once from the chain and once as themselves, and the
# second run lands after this file has lowered `__bt_between`, which is exactly
# what a command start looks like. The mark that opens the region would then
# belong to a prompt hook and the line the reader typed would get none.
#
# So the array is never assigned over: this file's own two halves are prepended
# and appended to it, in the order the scalar path already runs them in. `D`
# closes the last command before anything the prompt prints, the reader's hooks
# run in the middle with the status they would have seen, and the directory and
# the prompt marks go out last, once whatever `cd`s has `cd`ed.
#
# `declare -p` is what answers the shape question — `declare -a PROMPT_COMMAND`,
# `declare -ax PROMPT_COMMAND`, `declare -- PROMPT_COMMAND` — read as the
# attribute letters between `declare ` and the name.
__bt_previous_prompt_command=''
__bt_prompt_command_shape=$(declare -p PROMPT_COMMAND 2>/dev/null)
__bt_prompt_command_shape=${__bt_prompt_command_shape#declare }
__bt_prompt_command_shape=${__bt_prompt_command_shape%% *}

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

# D closes the command that just ran, and carries its exit code. Emitted first
# so that nothing the prompt hooks print lands inside the region that command
# owns, and the status is handed on so that whatever runs next — the reader's
# scalar hook below, or the next element of their array — sees the status it
# would have seen if this file were not here.
__bt_prompt_begin() {
    local status=$1
    if [ "$__bt_ran" = 1 ]; then
        printf '\033]133;D;%s\007' "$status"
        __bt_ran=0
    fi
    return "$status"
}

# The directory and the prompt marks, last: a hook that `cd`s is reported from
# where it left the shell, not from where it found it. `__bt_between` is lowered
# here and nowhere else, so this half has to be the last thing that runs before
# bash reads a line — anything after it would be marked as the reader's command.
__bt_prompt_end() {
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

# The scalar shape's whole hook: the two halves with the reader's own string
# between them.
__bt_prompt() {
    local status=$1
    __bt_prompt_begin "$status"
    if [ -n "$__bt_previous_prompt_command" ]; then
        __bt_return "$status"
        eval "$__bt_previous_prompt_command"
    fi
    __bt_prompt_end
}

__bt_wrap_ps1

# The first thing every prompt runs, whichever shape it is: the exit status the
# command left, taken before anything else can overwrite it, and the one reading
# of the DEBUG trap. Both are spelled here as *text* rather than folded into a
# function, because both have to happen at the top level — `$?` is the shell's
# and `$(trap -p DEBUG)` answers only out here.
#
# The guard is in front of the substitution rather than inside the function it
# calls, and that is the whole reason it is written twice: `$( )` is a fork, a
# fork on MSYS costs about 20ms, and this line runs on every prompt. Guarded
# here, the fork happens on the first prompt of the session and never again.
__bt_lead='__bt_status=$?; [ -n "$__bt_debug_adopted" ] ||'
__bt_lead=$__bt_lead' __bt_adopt_debug_trap "$(trap -p DEBUG)"'

case $__bt_prompt_command_shape in
    -*a*)
        PROMPT_COMMAND=(
            "$__bt_lead"' ; __bt_prompt_begin "$__bt_status"'
            "${PROMPT_COMMAND[@]}"
            __bt_prompt_end
        )
        ;;
    *)
        __bt_previous_prompt_command=${PROMPT_COMMAND-}
        PROMPT_COMMAND="$__bt_lead"' ; __bt_prompt "$__bt_status"'
        ;;
esac
unset __bt_lead __bt_prompt_command_shape

# No `OSC 0`/`OSC 2` title is emitted, deliberately. A title set by the shell
# outranks the working directory in Folio's own name stack, so a pane
# that announced "Ubuntu-24.04" once at startup would be called that forever and
# would stop following `cd` — the opposite of what this file exists to enable.
# A program that means to name itself still can, and still wins.
