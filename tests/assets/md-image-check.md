# Pictures in a markdown page

The acceptance corpus for `docs/DESIGN.md` §7.1.3k. Every section below is a
shape the preview has to answer, and the answer is written under it.

## 1. `![alt](src)`, relative to this file

The path is relative to `tests/assets/`, so it climbs one level.

![The Folio hero, light](../assets/readme/hero-light.svg)

## 2. A title beside the destination

The title is parsed off; what reaches the disk is a path.

![A terminal pane with typeset mathematics](../docs/screenshots/terminal-math-light.png "The hook")

## 3. `<picture>`, one file per theme

Dark page draws the dark file, light page draws the light one.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="../assets/readme/hero-dark.svg">
  <img src="../assets/readme/hero-light.svg" width="100%"
       alt="The Folio hero, chosen for the theme in force.">
</picture>

## 4. A bare `<img>`

<img src="../assets/readme/surfaces-light.png" width="100%" alt="Folio's surfaces">

## 5. A remote picture is never fetched

Folio has no network client, so this one is the alt text and a link.

![A build badge from somewhere else](https://img.shields.io/badge/build-passing-brightgreen.svg)

## 6. A picture that is not there

![A screenshot that was never taken](../docs/screenshots/does-not-exist.png)

## 7. An empty alt is still a picture

![](../assets/readme/hero-light.svg)

## 8. Not a picture, and printed as it stands

<div align="center">
  <b>Every other tag is text.</b>
</div>

## 9. A picture written inside a code span

`![not a picture](x.png)` and a fence:

```markdown
![not a picture either](x.png)
<img src="nor-this.png" alt="">
```

## 10. Badges on one line are chips, not cards (§7.1.3k ⑬)

Four pictures from the web in one paragraph, which is how a README opens. They
stand in a row, wrap like words, and each says its alt text — the last one has
none, so it says the last segment of its address. Resting on one says why there
is no picture and shows the address; pressing it opens the address.

[![License](https://img.shields.io/badge/license-MIT-green)](#)
[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg)](#)
[![Release](https://img.shields.io/badge/release-v0.3.0-blue)](#)
[![](https://img.shields.io/badge/no-alt-lightgrey.svg)](#)

And one inside a sentence: the chip for ![a logo](https://img.shields.io/badge/logo-here-orange) sits on this line, between these words.

## 11. A picture from the web alone in its paragraph still gets its card

![A badge standing on its own](https://img.shields.io/badge/alone-yes-purple.svg)
