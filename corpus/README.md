# M-1 ConPTY corpus

Files ending in `.btcr` use the byte-exact `BTCRP001` format implemented by `bt-corpus`.
Each record stores a monotonic microsecond timestamp and either raw PTY bytes, a resize marker, or
the child exit code. The checked-in fixtures are deliberately small enough for deterministic tests.

The source/command for every fixture and any environment substitution is recorded in
`docs/spikes/01-corpus.md`.

