# T-PASTE-1 native acceptance — pending coordinator lane

No row below has been run in this worktree. The ticket forbids live clipboard tests,
key injection, Folio/windows launches and shell paste experiments. Pure tests do
not prove an opened file or a recipient's interactive parser. This is the remaining
release gate, not a passing measurement record.

## Policies in this implementation

- PowerShell PROBE 2 is unrun. U+0027/U+2018/U+2019/U+201A/U+201B refuse with no
  insertion. The separate pure fixture explicitly enables measured doubling; it
  does not enable production. Observe these refusals, rather than trying to run
  the refused filename (review 5 correction 2).
- Nushell PROBE 10 is unrun: every path refuses. No raw-string arm ships.
- Codex's parser spelling is inherited from the design's pinned revision
  `a8964cb1bad67bc26a826fb07d1bef99c6a3f008`. Claude Code, Copilot CLI, Kimi, Pi,
  Hermes and OpenCode inherit it without a native measurement. Multiple paths
  are separate quoted text tokens; multiple automatic attachments are unproven.
- WSL/MSYS translation assumes default drive mounts. It never calls a launcher
  or changes the environment. Git Bash's native-consumer fallback is
  `{"id":"gitbash","paste_paths_as":"windows-slash"}`.
- Existing Copy as path text stays on the text rung unchanged, including its
  original quotes. Files from Explorer/Finder use the new path encoder.
- Literals promise a fresh argument boundary. Open quotes/tokens, wrapped-column-zero
  context, a changed foreground shell, REPL languages and shell paste hooks remain
  outside that promise.

## Direct CRT fixture (review 5 correction 4)

The consumer is [paste_paths_crt.cpp](../../crates/bt-app/tests/fixtures/paste_paths_crt.cpp),
compiled with the MSVC CRT as a console executable. Its `wmain` prints `argc` and
the UTF-16 units of `argv[1]` as hexadecimal. It launches nothing.

The ignored Rust fixture
`shell_literal::tests::direct_crt_receives_the_exact_literal_after_the_program_token`
is registered in `scripts/ci/ignored-tests.txt`. Set `BT_PASTE_CRT_CONSUMER` to the
separately compiled consumer and explicitly select this test in the coordinator lane.
Windows `Command::raw_arg` hands the literal unchanged to `CreateProcessW`, with
`CREATE_NO_WINDOW` and redirected output; no shell is involved. Its contract is:

1. `lpApplicationName`: the full executable path of that consumer.
2. Mutable, NUL-terminated `lpCommandLine`: a correctly quoted consumer-name token
   as **argv[0]**, a space, then the **exact** output of the unknown-program
   `Cmd` encoder as the tested **argv[1]**. Do not give the literal as the whole
   command line, and do not pass it through cmd, PowerShell, Python or Node.
3. Redirected output and no window. Record the CRT/toolchain version.
4. Assert exit 0, `argc == 2`, and that the printed `argv[1]` units equal the
   intended path. Inputs: one/two/three trailing backslashes, a space, an
   apostrophe, lone `%`, and paired `%NAME%`.

Separate terminal Cmd-profile observations must refuse both `%` and `%NAME%`
with a toast and zero inserted bytes. An explicitly named `paste_as: cmd`
wrapper inherits these refusals; an unknown-program CRT default does not.
Pair `!NAME!` with and without `/v:on` or `/v on` on both origins (correction 3).

## PROBE 1 source record (all unrun)

For each row record platform, source/bridge version, gesture, advertised types,
chosen rung, count and success/refusal. Keep clipboard content out of diagnostic
logs. Any opt-in PTY record is an acceptance artifact with the design's privacy
qualification.

| Source | Gestures to observe |
| --- | --- |
| Explorer | one file, three files, folder, Copy as path |
| Finder | one file, three files |
| Excel | cell range, copy as picture |
| Word | text, image |
| Browser | text selection, Copy image |
| Snipping Tool | capture copy |
| Windows capture | Win+Shift+S |
| macOS capture | Control+Shift+Command+4 |
| Screenshot tool configured with text | image plus saved path |
| WSLg GUI file manager | file |
| WSL clip.exe bridge | text |
| RDP | file, image |

Pictures alone are silent Nothing in T-PASTE-1. Files precede text; readable
empty text wins; promised files are visibly refused without fetching promises.

Windows additionally needs first delayed-render success, a competing open failing
while Folio holds its open interval, that snapshot completing, a replacement after
close, and the next gesture receiving the replacement. Never use sequence equality
as acceptance. macOS instead needs changed-changeCount refusal without reopen.

## Shell/agent records (all unrun)

Run the design §6.2 shell matrix under the coordinator's separate authorization:
PowerShell 5.1/7 with PSReadLine and bracketing variants; cmd builtins and a native
child with delayed expansion on/off; bash, zsh including bracketed-paste-magic,
fish, WSL; Git Bash MSYS/native consumers with argument conversion exclusions
set/unset and the windows-slash fallback. Assert the actual argument and opened
file, recording refusals as refusals. Nushell remains the unmeasured refusal row.

For each of the seven agents record version, spaced/apostrophe paths on each
platform, Windows drive/UNC roots, two paths and a file URI; distinguish attach,
reference and ignored. The pure shlex assertion applies only to POSIX output.
Text fields still use `clipboard_text`; verify Markdown, palette, search, settings
and address-bar text behavior in the separate native acceptance session.
