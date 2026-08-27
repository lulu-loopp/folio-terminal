# M-1 ConPTY corpus

New `.btcr` recordings use the byte-exact `BTCRP002` format implemented by `bt-corpus`. Each record
stores the selected ConPTY source/version, a monotonic microsecond timestamp, and either raw PTY
bytes, a resize marker, or the child exit code. Replay prints that provenance before feeding the
recorded stream. Legacy `BTCRP001` fixtures remain readable and report their source as
`legacy-unknown`. All seven fixtures checked in today predate the metadata field and are `BTCRP001`.
The checked-in fixtures are deliberately small enough for deterministic tests.

Every one of them is a raw capture of a program running on a development machine, not a synthesised
byte stream, so they carried the recording account's name, mail address, home directory and
repository path until they were scrubbed on 2026-08-27. `corpus/PROVENANCE.md` lists source, licence
and scrub date for every file here, and the classes of identity that were replaced. The
substitutions are length-preserving, so replay feeds the same byte counts through the same
length prefixes and the same column alignment as the original capture.
`no_recording_carries_a_person` in `crates/bt-corpus/tests/corpus_privacy.rs` scans every `.btcr` —
both the raw bytes and the logical stream with ConPTY's line wrapping undone — and fails on any
identity that reappears.

What each fixture covers, and why two of them are labelled stand-ins, is recorded in
`docs/spikes/01-corpus.md`.
