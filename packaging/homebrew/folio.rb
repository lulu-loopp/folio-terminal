cask "folio" do
  version "0.4.5"
  sha256 "7052892ef8343085e4c7be8b46392089abc55956ade37458c9ad686e2ef79b53"

  url "https://github.com/lulu-loopp/folio-terminal/releases/download/v#{version}-preview/Folio-#{version}-macos-arm64.dmg"
  name "Folio"
  desc "Terminal that typesets formulas where a command prints them, with files previewed beside the prompt"
  homepage "https://github.com/lulu-loopp/folio-terminal"

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
  # the step runs, not Ruby interpolation.
  postflight_steps do
    run "/usr/bin/xattr",
        args: [
          "-w",
          "io.github.lulu-loopp.folio.install",
          '{"v":1,"manager":"homebrew","uninstall_hook":false}',
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
      ]
end
