# Transitional shim. The product was named Folio on 2026-08-13 and this script
# moved to `folio.bash` beside it; everything that was here is there now.
#
# It exists because the line that sources it lives in a file that belongs to the
# user — their `~/.bashrc` — and because a `--init-file` argument recorded in a
# shortcut or a launcher still names this path. A rename that silently stopped a
# bash pane from reporting `OSC 7` would be this project breaking something it
# asked somebody to write. Point the source line at `folio.bash` and this file
# can be deleted; until then it is one line.
#
# `${BASH_SOURCE[0]}` is this file's own path whether it was sourced or handed
# over as an init file, so the sibling resolves from wherever the checkout is.
. "$(dirname "${BASH_SOURCE[0]}")/folio.bash"
