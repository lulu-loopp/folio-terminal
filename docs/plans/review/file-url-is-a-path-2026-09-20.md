# A `file:` URL is a path — issue #7, branch `fix/a-file-url-is-a-path`

Baseline `main` @ 0b8eb7c8. Implementer's report; reviewer is CI plus a different model.

## Every place a `file:` URL was compared, sliced or decoded on `main` (written before judging H1–H3)

| where | what it did |
|---|---|
| `webnav.rs:148` `Mint::file` | encoder: `\`→`/`, escapes `% # ?` and space, everything else raw |
| `webnav.rs:221` `Mint::path_and_tail_of_file_url` | **decoder A** — four escapes, `None` for any other; slices `url[..8]`, splits the tail |
| `webnav.rs:301` `Mint::admits` | **compares the two strings** with `eq_ignore_ascii_case` after cutting the tail |
| `webnav.rs:347` `local_path_form` | display, through decoder A |
| `webnav.rs:366` `file_url_of_local_path` | path → URL, through `Mint::file` |
| `webnav.rs:614` `resource_request` | the `file:` arm, through the folder test |
| `webnav.rs:784` `names_a_file_host` | slices `url[..5]` — a *scheme* test, not a path one |
| `webnav.rs:807` `file_url_is_inside_the_folder_of` | `to_lowercase()` + `starts_with` over decoder B's output |
| `webnav.rs:824` `strip_the_tail` | slices at the first `?`/`#` |
| `webnav.rs:848` `decoded_file_path` | **decoder B** — every escape, UTF-8, folds `\` to `/`, rejects `.`/`..` |
| `webnav.rs:1401` `blocks` (test) | prefix-matches a content-blocker *pattern*'s scheme; not a file-URL comparison |
| `webnav.rs:1412` `outside_the_read_access` (test) | slices `candidate[..5]` for the scheme, then the folder test |
| `webhost.rs:2604` | `self.minted.target() == Some(url.as_str())` — **string equality of two `file:` URLs** |
| `main.rs:9507, 24561, 24596, 33278, 33351, 38081, 53158` | callers of decoder A |
| `webhost.rs:1312` | `file_url_of_local_path` |
| `preview.rs:7437` `link_action` → `preview::file_url_path` | **decoder C**, for a link *inside* a document — a different fact, untouched, see Left standing |

**H1: half right.** `admits` and `file_url_is_inside_the_folder_of` are the same defect; the prefix test at `:1401` is not (it evaluates a pattern, not a URL). The third live site is `webhost.rs:2604`, which the brief did not list — and it is not only cosmetic: `page_destination` hands back `…report.html#ch3` while the mint holds `…report.html`, so a restored local page **with a fragment** lost its mint and was refused. **H2: unverified** (no Mac in this run); the repair does not depend on it — comparing decoded paths is correct whatever either engine's safe set is. **H3: confirmed by construction** — `local_path_form` was decoder A, which answers `None` for `%E6`, so the engine's committed URL was shown raw.

## The one parser

`LocalFileUrl` (`webnav.rs`), total, `None` = refuse: `"file:///"` (ASCII case-blind) `body` `[("?"|"#") tail]`; `body = *( "%" HEXDIG HEXDIG | ANY )`. Decode, then: invalid UTF-8 → `None` (never lossy); any control character or NUL → `None`; a decoded `\` → `None` (separator on one machine, a name on the other); split on `/`, any `""`/`"."`/`".."` segment → `None`; first segment `X:` is the drive root (separator `\`, letter upper-cased), anything else is the slash root; no name left → `None`. `Mint::file` now mints *through* it, so no mint exists that nothing can compare, and `Mint::File` carries the parsed value.

## A2 — the security table (every row still refused)

| row | outcome |
|---|---|
| `..`, `%2e%2e`, `%2E%2E`, `.%2e`, `.` in any segment | refused — segments are judged after decoding |
| double encoding `%252e%252e` | refused *as a traversal*: one decode yields the **name** `%2e`, which the disk resolves no traversal out of; outside the folder it is refused, inside it names a child. Same as `main` |
| sibling outside the minted folder (`…/other/`, `…/tmp/`, other drive, `pageant/` beside `page/`) | refused |
| UNC and `file://host/…` (incl. `file://localhost/…`) | refused; `Mint::file` still `NetworkPath`, `resource_request` still `NetworkPath` |
| `file:` typed into the address field, any case | refused, ruling unchanged (`classify_scheme`) |
| mixed-case scheme against a mint | admitted, as on `main` (`eq_ignore_ascii_case` on the scheme) |
| equal only after lossy decoding (`%FF`) | refused — `String::from_utf8`, never `_lossy` |
| NUL `%00` (and every other control byte) | refused |
| `report.html.` / `report.html%20` / `report.html%2E` | **refused — decision below** |
| newly refused vs `main`: `file:///C:` (bare drive), `file:////x` (empty segment), `%5C` anywhere | stricter, never laxer |

**Windows trailing-dot/space aliases: not folded, i.e. refused.** Windows opens `report.pdf.`, `report.pdf ` and `report.pdf` as one file; macOS opens three. Folding them would make the gate admit a name the engine never emits, and would be wrong on the other machine; refusing them is exactly what `main` did, costs at most a refused page (Windows cannot create such a name through a normal path anyway), and I could construct no case where *not* folding admits something extra. A gate may be wrong by refusing a page; not by opening one.

## Callers moved (before → after)

`main.rs:24561` `Mint::path_and_tail_of_file_url(url)?` → `LocalFileUrl::parse(url)?.into_path_and_tail().0`; `main.rs:24596` same; `main.rs:33278` `if let Some((path, tail)) = …` → `if let Some(named) = LocalFileUrl::parse(target)` + `into_path_and_tail()`; `main.rs:33351` `.is_none_or(|(path,_)| …)` → `.is_none_or(|named| path_opens_as_a_page(named.path()))`; `main.rs:38081` `.and_then(Mint::path_and_tail_of_file_url)` → `.and_then(LocalFileUrl::parse)`; `main.rs:53158` `named == path` → `named.path() == path`; `webhost.rs:2604` `target() == Some(url)` → `self.minted.admits(url).is_some()`; `webnav.rs:614/807` → `LocalFileUrl::is_inside_the_folder_of`; `webnav.rs:1412` → `split_scheme(..) == DISK` + the parsed folder test; `web_trace.rs:120` → `url.as_str()`. Pins updated: `webhost.rs:3904` (the whitespace-stripped needle), `main.rs:119937` (`LocalFileUrl::parse`). `main.rs:9507` and `webhost.rs:1312` keep their function names and gain the new reader underneath.

## Budget

One `admits` call on a `Mint::File`: **1 allocation before** (the returned `String`; 0 on a refusal), **4 after** on success — the decoded byte buffer, the path, the kept URL text, the returned `String` — and 1–3 on a refusal. It runs at navigation events, restores and per subresource request, never per frame. The minted side is parsed once, at the mint.

## What ran, and what has not been compiled

`bt-app` was **not** built, checked, tested or clippy'd (owner's machine, other agents compiling); `cargo fmt` ran and therefore every changed file parses. The parser, the encoder and the tables were extracted **verbatim** into `scratchpad/file-url-scratch` and run there (`cargo test -j 4`, 8 tests green): the 17 recorded pairs, the A2 table, the round trip on both roots, the two stored session spellings, the display form. What has never been compiled in place: the type's integration — `Mint::File(LocalFileUrl)` and its ~30 use sites, the six `main.rs` callers, `webhost.rs`, `web_trace.rs`, and the new `webnav::file_url_tests` module. CI is the first compiler to see them.

## Left standing (recorded, with reproduction)

1. **`preview::file_url_path`** (`preview.rs:7437`) is a third `file:` decoder, for a link inside a Markdown document. Different fact (what does this link name), different lane, not a gate — out of this brief's scope.
2. **`switcher_key`** (`webnav.rs:1097`) normalises a URL as text and returns a `file:` URL unchanged, so two spellings of one file would be two switcher rows. Not reachable in practice — only the engine's spelling is ever committed — but it is the same class. (3) `admits` allocates a `String` that `webhost.rs:2604` throws away.

## Rollback

Revert the commit. Persisted URLs stay readable by the old code **only for ASCII paths**: a `session.json` or `pins.json` row written after this change is the same spelling `Mint::file` has always written (verified stable across save → load → save), but a row holding the *engine's* spelling — which 0.4.2 already wrote — goes back to being unreadable, which is the defect, not a new loss.
