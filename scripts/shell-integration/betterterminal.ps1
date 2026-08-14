# Transitional shim. The product was named Folio on 2026-08-13 and this script
# moved to `folio.ps1` beside it; everything that was here is there now.
#
# It exists because the line that dot-sources it lives in a file that belongs to
# the user — their `$PROFILE` — and a rename that silently stopped their prompt
# from reporting `OSC 7` would be this project breaking something it wrote into
# somebody else's home directory. Point `$PROFILE` at `folio.ps1` and this file
# can be deleted; until then it is one line and costs one file open.
#
# `$PSScriptRoot` is the directory of *this* script even when it is dot-sourced,
# so a checkout anywhere resolves its own sibling and no path is baked in.
. (Join-Path $PSScriptRoot 'folio.ps1')
