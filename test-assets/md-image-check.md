# Pictures in a markdown page

The acceptance corpus for `docs/DESIGN.md` §7.1.3k. Every section below is a
shape the preview has to answer, and the answer is written under it.

## 1. `![alt](src)`, relative to this file

The path is relative to `test-assets/`, so it climbs one level.

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
