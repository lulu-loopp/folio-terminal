# `packaging/homebrew`

`folio.rb` is the source of `Casks/folio.rb` in the
**`lulu-loopp/homebrew-folio`** tap, which `brew install --cask
lulu-loopp/folio/folio` reads. `scripts/release/update-manifests.ps1` (or, on the
Mac, `scripts/release/macos/cask.sh`) renders it for a release — `version` and
`sha256`, the hash copied out of `SHA256SUMS-macos.txt`; the URL is built out of
`version` — and the result is committed to the tap (`docs/RELEASING.md`,
"Distribution manifests"). A change to the cask is made here; an edit made in the
tap by hand is taken away by the next release.

## The hooks (0.4.6, U-2)

- **`postflight`** writes the extended attribute
  `io.github.lulu-loopp.folio.install` on the installed `Folio.app` bundle
  directory: `{"v":1,"manager":"homebrew","uninstall_hook":false}`, the install
  channel marker Folio reads at start (`crates/bt-app/src/install_channel.rs`).
  It runs at every install, `brew upgrade` and `brew reinstall`, so a bundle
  copied without its extended attributes gets it back at the next of these.
- **`zap`** runs `Folio.app/Contents/MacOS/folio --uninstall-cleanup` with
  `must_succeed: false`, then trashes the data folder. There is no `uninstall`
  hook, and the marker says so: Homebrew runs a cask's `uninstall` steps on
  `brew upgrade` and `brew reinstall` too, with nothing that tells a step which
  command it is (E-10). A zap step runs after Homebrew has moved the app back
  into the Caskroom, which is where the relative path points, and so the door's
  exit 2 (a Folio is running) cannot keep the app: it changes nothing else, and
  the zap goes on.

Checked with no installer by `scripts/ci/check-manager-hooks.ps1` (`ruby -c`
where Ruby is installed), and installed by `scripts/release/check-cask-hooks.sh`
on a Mac with Homebrew.
