# Card restore replay

Recorded by the prior card-anchor audit's controlled PowerShell ConPTY child, `repaint.ps1`, on 2026-09-14. The child prints 60 synthetic numbered conversation lines, then repaints its last ten lines and status block on width changes. These are synthetic fixture bytes, not an owner session or user data.

Replay order: initial at 240 x 24; resize to 160 x 24 and feed 160; settle; resize to 240 x 24 and feed 240; settle. The unit tests embed the exact captures and launch no child process. Original sizes: 26,118 / 4,416 / 4,416 bytes. Binary terminal CR/LF and escape sequences are intentional; text files use LF.
