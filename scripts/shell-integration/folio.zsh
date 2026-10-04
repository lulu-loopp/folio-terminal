# Folio shell integration for zsh.
#
# Emits what `folio.bash` and `folio.ps1` emit, so that a zsh pane is not a
# second-class pane: FinalTerm `OSC 133` A/B/C/D command regions, and `OSC 7`
# working-directory reports. See `docs/shell-integration.md`.
#
# **zsh has no `--init-file`.** bash's flag names the startup file of one
# interactive shell and touches nothing on disk; zsh has no such argument and
# refuses the flag outright, so the door zsh does have is `ZDOTDIR` — the
# directory zsh reads its startup files *from*. Folio points it at a directory
# holding this file under three names, and this file's whole first half is
# putting the reader's own startup files back:
#
#   * `$ZDOTDIR/.zshenv`   — read by every zsh there is, login or not;
#   * `$ZDOTDIR/.zprofile` — read by a login shell, before `.zshrc`;
#   * `$ZDOTDIR/.zshrc`    — read by an interactive shell.
#
# Each of the three sources the reader's file of the same name and nothing else,
# and `.zshrc` hands `ZDOTDIR` back at the end of it — so `.zlogin`, and every
# zsh started from this one, read the reader's own directory with no trace of
# this arrangement left in the environment.
#
# `BT_USER_ZDOTDIR` is how the spawn tells this file which directory that was:
# `ZDOTDIR` itself has already been taken by the time zsh reads a line of this,
# and a reader who keeps their files somewhere other than `$HOME` has said so in
# exactly that variable.
#
# It is loaded two ways and behaves the same either way. Under one of the three
# names above it is Folio's copy and owes the startup files; under any other name
# it is a copy you installed yourself, dot-sourced as the last relevant line of
# your own `~/.zshrc` — and then it owes nothing, because zsh has already read
# your files. Which it is, is the file's own name and nothing else.
#
# Nothing here is Folio-specific except the comments: `OSC 133` and `OSC 7` are
# the sequences Windows Terminal, VS Code and iTerm2 all read.

# zsh only. A hand-installed copy can reach another shell, and every parameter
# below is zsh's own.
[ -n "${ZSH_VERSION-}" ] || return 0

# Which file zsh is reading right now. `%x` is the file the code being executed
# came from, which is the one thing that tells the three names apart — they are
# the same bytes three times over.
__bt_zsh_file=${${(%):-%x}:t}
__bt_zsh_home=${${(%):-%x}:A:h}

# ---------------------------------------------------------------------------
# The reader's own startup files
#
# `ZDOTDIR` is set to the reader's own for the length of the `source` below, so
# that a startup file which reads it — or which sets it, which is how a reader
# moves their own files somewhere else — is answered with the truth rather than
# with this directory. Whatever it says afterwards is where the *next* file comes
# from, which is zsh's own rule and not a second one.
#
# The `case` is what keeps a hand-installed copy out of here: under any name but
# the three, this file was dot-sourced by a reader out of a startup file zsh has
# already found, and sourcing "the file of the same name" would be sourcing
# itself, forever.
# ---------------------------------------------------------------------------
case $__bt_zsh_file in
    .zshenv|.zprofile|.zshrc)
        if [ -z "${BT_ZDOTDIR+set}" ]; then
            BT_ZDOTDIR=${BT_USER_ZDOTDIR:-$HOME}
        fi
        ZDOTDIR=$BT_ZDOTDIR
        [[ -r $BT_ZDOTDIR/$__bt_zsh_file ]] && source "$BT_ZDOTDIR/$__bt_zsh_file"
        BT_ZDOTDIR=${ZDOTDIR:-$HOME}
        ZDOTDIR=$__bt_zsh_home
        export ZDOTDIR

        if [[ $__bt_zsh_file != .zshrc ]]; then
            unset __bt_zsh_file __bt_zsh_home
            return 0
        fi

        # `.zshrc` is the last file this directory owns, so this is where
        # `ZDOTDIR` goes back. A zsh started from this session reads the reader's
        # own files, and so does `.zlogin`, which zsh looks up after this file
        # has run.
        if [[ $BT_ZDOTDIR == "$HOME" && -z ${BT_USER_ZDOTDIR-} ]]; then
            unset ZDOTDIR
        else
            ZDOTDIR=$BT_ZDOTDIR
            export ZDOTDIR
        fi
        unset BT_ZDOTDIR BT_USER_ZDOTDIR
        ;;
esac
unset __bt_zsh_file __bt_zsh_home

# Interactive only: the markers describe a person typing, and there is nobody to
# type in a shell that was handed a script.
[[ -o interactive ]] || return 0

# Idempotent, and the guard sits here rather than at the top: a reader whose own
# `.zshrc` dot-sources this file has two copies of it in one session, and the
# hooks are the half that must not be installed twice — while the startup files
# above are Folio's copy's alone to put back, whichever copy ran first.
[ -z "${__bt_integration-}" ] || return 0
__bt_integration=1

# ---------------------------------------------------------------------------
# Percent-encoding, without forking
#
# The safe set is RFC 3986 unreserved plus sub-delims plus `:`, `@` and `/`,
# which is the set `folio.bash` and `folio.ps1` keep; the three scripts describe
# the same URIs. `no_multibyte` for the length of the function makes `${#text}`
# and `${text[index]}` count *bytes*, so a non-ASCII path is encoded UTF-8 byte
# by byte as RFC 3986 requires rather than character by character, which would
# encode nothing at all. `localoptions` is what limits the setting to this call.
#
# The hex pair is built from a table rather than with `printf`, because zsh gained
# `printf -v` late and a substitution would be a fork on every `cd`.
# ---------------------------------------------------------------------------
__bt_hex=(0 1 2 3 4 5 6 7 8 9 A B C D E F)

__bt_encode() {
    setopt localoptions no_multibyte
    local text=$1 out='' char
    local -i index code
    for (( index = 1; index <= ${#text}; index++ )); do
        char=${text[index]}
        case $char in
            [A-Za-z0-9]) out+=$char ;;
            '-'|'_'|'.'|'~'|'/'|':'|'@') out+=$char ;;
            '!'|'$'|'&'|"'"|'('|')'|'*'|'+'|','|';'|'=') out+=$char ;;
            *)
                code=$(( #char ))
                out+="%${__bt_hex[code / 16 + 1]}${__bt_hex[code % 16 + 1]}"
                ;;
        esac
    done
    __bt_encoded=$out
}

# ---------------------------------------------------------------------------
# Which spelling of "here" this shell reports
#
# The same question `folio.bash` asks, asked the same way: a shell reports the
# directory in the namespace it actually stands in, and it finds that out by
# asking rather than by sniffing the operating system.
#
#   * Under MSYS2 — a zsh on Windows — the process's working directory is a
#     Win32 directory that zsh merely *spells* `/d/Developer`, and `cygpath` is
#     the program that gives the Win32 spelling. It is the true one: it is what
#     `CreateProcess` was handed and what every other pane in the window speaks.
#   * Everywhere else — WSL, or a Linux box over ssh — `$PWD` is the answer, and
#     `/home/user/src` is a real directory with no Windows spelling at all.
# ---------------------------------------------------------------------------
if (( $+commands[cygpath] )); then
    __bt_pwd_style=windows
else
    __bt_pwd_style=posix
fi

__bt_cwd_seen=''
__bt_cwd_uri=''

# The answer is remembered against the `$PWD` it was computed from, so the fork
# `cygpath` costs happens when you `cd` rather than on every prompt. On the POSIX
# side there is no fork at all.
__bt_refresh_cwd() {
    local place
    if [[ $__bt_pwd_style == windows ]]; then
        place=$(cygpath -m -- "$PWD" 2>/dev/null) || place=''
        [[ -n $place ]] && place="/$place"
    else
        place=$PWD
    fi
    if [[ -z $place ]]; then
        # An empty report **retracts** the previous directory rather than leaving
        # a stale one to answer for a place the shell has left — the rule the
        # other two scripts follow.
        __bt_cwd_uri=''
        return
    fi
    __bt_encode "$place"
    __bt_cwd_uri="file://$__bt_encoded"
}

# ---------------------------------------------------------------------------
# The command region
#
# zsh has the two hooks bash has to build out of a DEBUG trap: `preexec` runs
# once, after a line has been read and before it runs, and `precmd` runs once
# before each prompt. So there is no flag here telling the reader's command from
# the machinery around it — the shell has already told us.
# ---------------------------------------------------------------------------
__bt_ran=0
__bt_a=$'\e]133;A\a'
__bt_b=$'\e]133;B\a'
__bt_c=$'\e]133;C\a'

__bt_preexec() {
    __bt_ran=1
    printf '%s' "$__bt_c"
}

# `%{` and `%}` fence the markers as non-printing, without which zsh measures the
# prompt as several characters wider than it draws and every redraw of a recalled
# line lands in the wrong column.
__bt_wrap_prompt() {
    case $PS1 in
        *"$__bt_a"*) ;;
        *) PS1="%{$__bt_a%}$PS1%{$__bt_b%}" ;;
    esac
}

# `__bt_left` and not `status`: in zsh `status` *is* `$?`, a special parameter,
# and a local of that name is a shell error rather than a variable.
__bt_precmd() {
    local __bt_left=$?
    # D closes the command that just ran, and carries its exit code.
    if (( __bt_ran )); then
        printf '%s' $'\e]133;D;'"$__bt_left"$'\a'
        __bt_ran=0
    fi
    # Quoted, because an unquoted right-hand side of `[[ ]]` is a *pattern* in
    # zsh and a directory may be called `[draft]`.
    if [[ $PWD != "$__bt_cwd_seen" ]]; then
        __bt_cwd_seen=$PWD
        __bt_refresh_cwd
    fi
    printf '%s' $'\e]7;'"$__bt_cwd_uri"$'\a'
    # Re-applied every prompt because a theme that rebuilds `PS1` from scratch in
    # its own `precmd` — starship, powerlevel10k, and most prompt kits — would
    # otherwise drop A and B after the first line, and the region markers would
    # simply stop arriving with nothing to show why.
    __bt_wrap_prompt
}

# `add-zsh-hook` and not an assignment to `precmd_functions`: it is zsh's own way
# of adding a hook to whatever is already there, and a reader's prompt kit is
# already in that list.
autoload -Uz add-zsh-hook
add-zsh-hook precmd __bt_precmd
add-zsh-hook preexec __bt_preexec
__bt_wrap_prompt

# No `OSC 0`/`OSC 2` title is emitted, deliberately — the reasoning is
# `folio.bash`'s: a title set by the shell outranks the working directory in
# Folio's own name stack, so a pane that announced its distribution once at
# startup would be called that forever and would stop following `cd`.
