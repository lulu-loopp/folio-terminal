# The application icon

What `folio.exe` wears in the taskbar, in Alt-Tab, on the desktop and in the
Start menu — and, since the first-run card was built, the mark at the top of the
first thing a new reader ever sees (`docs/DESIGN.md` §7.56 ⑦). One file is
what Windows links — `folio.ico` — one is what the macOS bundle is built from
— `folio-1024.png` — and the two scripts beside them are how those and the
package logos are drawn. All of it is one drawing: the generator states the mark
in units of the square, and every file here is that statement resolved at a size.

**`folio.ico` is the icon** (user ruling, 2026-09-06). It was drawn as a
placeholder, it was put on a contact sheet against five hand-set candidates, and
the ruling on that sheet was to keep it. Nothing here is waiting on a decision
any more; the round that was run before it was kept is in
`docs/design/app-icon/`, with the five drawings, the contact sheet and what each
one cost at 16 pixels.

Two consequences worth stating plainly, because they are what "shipped" means
for a file this small:

* **Changing it changes the first impression twice over.** The taskbar icon and
  the first-run card read the same file, and `first_run.rs` `include_bytes!`s it
  so that they cannot drift.
* **The nine sizes are load-bearing.** `first_run.rs` picks the smallest
  uncompressed entry that is at least as wide as the box it needs, so an `.ico`
  rebuilt with fewer entries would make the card's mark an upscale.

| File | What it is |
| --- | --- |
| `folio.ico` | **The icon.** A sheet folded once, which is what a folio is. Drawn by the script below, kept by the ruling of 2026-09-06. |
| `folio-1024.png` | **The macOS icon source.** The same drawing at 1024, which is `icon_512x512@2x` — the largest slot an icon set has. `scripts/release/macos/bundle.sh` resamples all ten slots of `Folio.icns` from it. |
| `make-folio-ico.py` | How they are drawn — geometry in code, no input file. It is the source of record for the mark. |
| `make-msix-logos.py` | The same drawing at the three sizes `packaging/msix/AppxManifest.xml` names. It owns no geometry; it imports the file above. |

## What the colours are, and why there are so few

Two papers and a graphite: `#F4F1EA` for the half facing the light, `#DDD7C9`
for the half turned away, `#202027` for the tile — which is seven levels off the
dark card's own `#202020`, and the reason the first-run card gives the mark an
edge there (`docs/DESIGN.md` §7.56 ⑦). **No accent colour anywhere**, which is
the standing decision from the wordmark study
(`docs/design/wordmark-r2/DECISION.md`): in every option tried there, the cobalt
was the first stroke that looked borrowed. A graphite tile is also what
guarantees the mark survives a light taskbar, where a cream-on-cream icon
disappears.

## How it reaches the binary

`crates/bt-app/build.rs` reads exactly one path — `assets/app-icon/folio.ico` —
turns it into the `.res` file that carries the icon and the version block, and
hands that to the linker. It also declares that path as a rebuild trigger, so
replacing the file is the whole change:

```
python assets/app-icon/make-folio-ico.py
cargo build -p bt-app
```

The macOS source is the same script with one flag, and it is the only file here
`build.rs` does not read:

```
python assets/app-icon/make-folio-ico.py --png
```

`--png` takes a size and defaults to 1024. Nothing about it is an upscale of the
`.ico`: `render` is called at 1024, on the same geometry every `.ico` entry is
drawn from — size reaches that drawing at two points only, widening the fold
below 84 pixels and withholding the page dot below 48, so from 84 up the 1024 and
the 256 are one drawing at two resolutions. **Why a PNG and not a tenth `.ico`
entry:** `sips`, the only converter a stock Mac has, reads an `.ico` as its
largest entry alone, so a bundle built from the `.ico` gets every icon-set slot
resampled from the 256 and the three slots above 256 not at all.

Two things to know about a swap:

* **The icon group must stay group 1.** `build.rs` writes it as group one
  because Explorer draws an executable with its lowest-numbered group, and
  `bt_platform::context_menu_shape` registers `folio.exe,0` meaning that one.
  Nothing here changes that; it is only a reason not to add a second group later.
* **Windows caches icons per file path.** After a rebuild, Explorer and the
  taskbar can keep showing the old drawing for a while. `ie4uinit.exe -show`
  clears the shell's cache; a fresh `folio.exe` in a fresh directory always
  shows the truth.

No candidate was chosen, so **`make-folio-ico.py` is the source of record** and
stays where it is. If a future round ever does replace the mark, the winner's
SVG takes that role and this script retires with the drawing it makes; the
builder that turns an SVG into a nine-size `.ico` is
`docs/design/app-icon/make-ico.py`, kept with the round it was written for.
