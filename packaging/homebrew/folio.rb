cask "folio" do
  version "0.4.7"
  sha256 "d97b9f103aaa645b44d872505a2eb25f42e060812779b3d1b8bea65d300d56a1"

  url "https://github.com/lulu-loopp/folio-terminal/releases/download/v#{version}-preview/Folio-#{version}-macos-arm64.dmg"
  name "Folio"
  desc "Typesets formulas in terminal output and previews files beside the prompt"
  homepage "https://github.com/lulu-loopp/folio-terminal"

  livecheck do
    url :url
    strategy :github_latest
    regex(/^v?(\d+(?:\.\d+)+)-preview$/i)
  end

  # Folio updates itself in place from its own card (the bundle at the app
  # target this cask recorded, replaced as a whole). With this, a plain
  # `brew upgrade` upgrades Folio only when the bundle's own version is older
  # than this cask's, and never puts back a version Folio has moved past.
  auto_updates true

  depends_on arch: :arm64
  depends_on macos: :sonoma

  app "Folio.app"

  # The install channel marker, which Folio reads at start: this copy is
  # Homebrew's. Written again at every install, upgrade and reinstall, since a
  # copy that drops extended attributes loses it. `uninstall_hook` is false:
  # Homebrew runs a cask's `uninstall` steps on `brew upgrade` and
  # `brew reinstall` as well, with nothing that tells them which, so Folio's
  # cleanup runs from `zap` only. A step list, not a Ruby block (Homebrew 7
  # deprecates `postflight`): `{{appdir}}` is Homebrew's token, expanded when
  # the step runs, not Ruby interpolation. The second attribute names this
  # cask's Caskroom folder (`{{caskroom_path}}`), where Folio reads which app
  # Homebrew installed before it updates one in place.
  postflight_steps do
    run "/usr/bin/xattr",
        args: [
          "-w",
          "io.github.lulu-loopp.folio.install",
          '{"v":1,"manager":"homebrew","uninstall_hook":false}',
          "{{appdir}}/Folio.app",
        ]
    run "/usr/bin/xattr",
        args: [
          "-w",
          "io.github.lulu-loopp.folio.caskroom",
          "{{caskroom_path}}",
          "{{appdir}}/Folio.app",
        ]
  end

  # `brew uninstall --zap`: Folio's cleanup, then its data. By the time a zap
  # step runs, Homebrew has already taken the app out of the Applications
  # folder and back into the Caskroom, which is where this path points; so a
  # refusal (exit 2, a Folio is running) cannot keep the app, and the zap goes
  # on.
  zap script: {
        executable:   "Folio.app/Contents/MacOS/folio",
        args:         ["--uninstall-cleanup"],
        must_succeed: false,
      },
      trash:  [
        "~/Library/Application Support/Folio",
        "~/Library/Application Support/Folio-uninstall",
      ]
end
