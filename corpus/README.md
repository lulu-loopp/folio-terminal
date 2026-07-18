# M-1 ConPTY corpus

New `.btcr` recordings use the byte-exact `BTCRP002` format implemented by `bt-corpus`. Each record
stores the selected ConPTY source/version, a monotonic microsecond timestamp, and either raw PTY
bytes, a resize marker, or the child exit code. Replay prints that provenance before feeding the
recorded stream. Legacy `BTCRP001` fixtures remain readable and report their source as
`legacy-unknown`. The checked-in fixtures are deliberately small enough for deterministic tests.

The source/command for every fixture and any environment substitution is recorded in
`docs/spikes/01-corpus.md`.
