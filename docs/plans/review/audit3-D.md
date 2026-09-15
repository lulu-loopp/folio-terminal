STATUS COMPLETE — Review D at 6a414963; phases 1–4 complete; six findings.

## D-1 — high — macOS downloads omit the licence and third-party notices

**Location:** `scripts/release/macos/bundle.sh:296` at `6a414963`.

```sh
build_icns "$icon" "$app/Contents/Resources/Folio.icns"
```

The complete assembly at lines 259–302 creates only the executable, plist, PkgInfo and icon before producing the external dSYM. `scripts/release/macos/dmg.sh:178` then copies only that bundle into the image:

```sh
run ditto "$app" "$staging/Folio.app"
run ln -s /Applications "$staging/Applications"
```

**Trigger:** Produce a macOS release using the new bundle/DMG scripts or the macOS release workflow.

**Consequence:** Neither downloadable container includes `LICENSE-MIT`, `LICENSE-APACHE` or `THIRD-PARTY-NOTICES.md`; the executable does not embed them either. The recipient receives the compiled dependencies and bundled assets without their accompanying licence texts and notices. The claim at `scripts/release/macos/dmg.sh:21`, “the application carries its own,” is false. A CycloneDX file with licence identifiers does not supply those texts.

**Smallest correct fix:** Copy the licence files, generated third-party notices and trademark notice into `Folio.app/Contents/Resources` before signing. Make missing files fatal and include these exact names in the bundle verification.

## D-2 — medium — the documented macOS release sequence fails before notarization

**Location:** `docs/RELEASING.md:716` at `6a414963` (also `packaging/macos/README.md:140` and the manual recipe at `docs/DESIGN.md:10001`).

```sh
scripts/release/macos/sign.sh   --app target/macos/Folio.app \
    --identity "Developer ID Application: … (TEAMID)"
scripts/release/macos/notarize.sh --path target/macos/Folio.app
```

**Contradicting code:** `scripts/release/macos/sign.sh:259` runs `spctl -a -vvv "$app"`; lines 269–278 tolerate rejection only for identity `-` and otherwise `exit 1`.

**Trigger:** Follow this sequence for a freshly built Developer ID application which Apple has not notarized yet, with shell error stopping enabled or while treating a failed step as a release failure.

**Consequence:** Gatekeeper reports `Unnotarized Developer ID`, the signing step fails, and the sequence stops before the next command can notarize the application. The packaging README even describes this rejection as expected while directing the reader to the script's failing exit code.

**Smallest correct fix:** Add `--no-spctl` to the pre-notarization signing command in all three manual recipes. Keep the post-notarization `spctl` and stapler checks fatal, as the workflow already does.

## D-3 — medium — bundle assembly can stamp a stale executable with the current version

**Location:** `scripts/release/macos/bundle.sh:281` at `6a414963`.

```sh
(cd "$repo" && cargo run -q --locked -p bt-winres --bin render-info-plist) >"$app/Contents/Info.plist"

# `Contents/MacOS/folio` — the name `CFBundleExecutable` in the template gives.
cp "$binary" "$app/Contents/MacOS/folio"
```

**Trigger:** A checkout at 0.4.0 has a 0.3.0 executable left in `target/release/folio`, or the caller explicitly supplies it with `--binary`. The only input validation at lines 173–182 checks that the file exists.

**Consequence:** The script renders a 0.4.0 plist and puts the old executable inside it; the signing/notarization/DMG scripts do not compare the executable's version or commit with the checkout. The manual release route can therefore ship an old program labelled as the new release. The workflow's later `--version` comparison does not protect that route.

**Smallest correct fix:** Before assembly, compare the supplied executable's `--version` response with the workspace version and expected commit, reusing the workflow's existing comparison, and refuse a mismatch.

## D-4 — medium — a failed notarization-log download is reported as success

**Location:** `scripts/release/macos/notarize.sh:229` at `6a414963`.

```sh
xcrun notarytool log "$id" \
    --key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" "$log" || true
echo "notarize.sh: log kept at $log"
```

**Trigger:** Submission returns `Accepted`, but the subsequent log request exits nonzero without creating the requested file, for example after a transient HTTP error. Stapling and validation subsequently succeed.

**Consequence:** The script exits successfully and claims the current log was saved although it is missing. The workflow does not repair this: `.github/workflows/release.yml:423` copies the DMG log only if it exists. The release therefore loses the evidence that `docs/RELEASING.md:772` requires retaining once per submission.

**Smallest correct fix:** Download the log to a fresh temporary file, require success and a submission ID matching `$id`, and only then replace `$log` and print success. Fail the successful-release path if the current log cannot be retained; preserve the original submission failure when submission itself failed.

## D-5 — low — the platform allowlist reader treats brackets inside comments and filenames as array delimiters

**Location:** `scripts/check-portable-core.ps1:227` at `6a414963`.

```powershell
'const\s+FILES_THAT_MAY_NAME_A_PLATFORM\s*:\s*\[&str;\s*\d+\]\s*=\s*\[(?<body>[^\]]*)\]\s*;'
```

**Trigger:** Add the valid line comment `// The remaining #[cfg(windows)] sites are listed below.` inside `FILES_THAT_MAY_NAME_A_PLATFORM`. Alternatively, rename the allowed `wsl.rs` to the legal filename `wsl].rs`, update its module's `#[path]`, and update that entry in the array.

**Consequence:** The regular expression stops at the bracket inside the comment or quoted filename and fails to match the array. Lines 229–231 throw that the constant is missing; comment removal at lines 236–237 happens too late. The Rust constant and recursive walk at `crates/bt-app/src/main.rs:163034` and `:163067` accept these inputs, so the advertised equivalent checks disagree and CI rejects a valid source layout. Both regex counterexamples were checked on in-memory copies only.

**Smallest correct fix:** Find the array's closing delimiter with a reader that skips Rust comments and string literals, then extract the string entries. A bracket inside either must not terminate the array.

## D-6 — low — the READMEs promise Chinese for an English-only Finder item

**Location:** `README.md:83` at `6a414963`; the same claim is at `README.zh-CN.md:73`.

> English and Chinese — every string in both, switched from one row in Settings.

**Contradicting code:** `packaging/macos/Info.plist.in:150` fixes the Folio Services item's title:

```xml
<key>NSMenuItem</key>
<dict>
    <key>default</key>
    <string>Open in Folio</string>
</dict>
```

`scripts/release/macos/bundle.sh:260` creates `Contents/Resources`, and its only populated resource is `Folio.icns` at line 296. It never installs localized Services resources. Folio's language setting cannot replace the title in this plist.

**Trigger:** Install this bundle on a Mac using Chinese, select Chinese in Folio, and open Finder's Services menu for a file or folder.

**Consequence:** Folio's own menu item still says `Open in Folio`; the advertised switch does not provide Chinese for every Folio string.

**Smallest correct fix:** Qualify both READMEs to state that the Finder Services item remains English. Supplying localized Services resources is the alternative if the broader localization claim is retained.
