# Stress sample 压力样张

A page built to break three rulings at once: a fence that will not reflow, tables
wider than any pane, and a paragraph carrying a token nothing can break at a space.

## 1. A fence with a very long line

```rust
fn short() {}
let very_long_binding_name = compute(argument_number_01, argument_number_02, argument_number_03, argument_number_04, argument_number_05, argument_number_06, argument_number_07, argument_number_08, argument_number_09, argument_number_10, argument_number_11, argument_number_12); // a single fenced line well past two hundred and fifty characters, which must not reflow
}
```

## 2. An eight-column table with long cells

| id | name | a rather long column heading that will not fit anywhere | type | default | notes with a sentence in them that keeps going for a while | since | owner |
|---|---|---|---|---|---|---|---|
| 1 | `field_1` | an extremely long cell value that is deliberately far wider than any sensible pane could ever be | string | **none** | row 1 carries a note that is also long enough to widen its column considerably | v0.1 | team |
| 2 | `field_2` | an extremely long cell value that is deliberately far wider than any sensible pane could ever be | string | **none** | row 2 carries a note that is also long enough to widen its column considerably | v0.1 | team |
| 3 | `field_3` | an extremely long cell value that is deliberately far wider than any sensible pane could ever be | string | **none** | row 3 carries a note that is also long enough to widen its column considerably | v0.1 | team |
| 4 | `field_4` | an extremely long cell value that is deliberately far wider than any sensible pane could ever be | string | **none** | row 4 carries a note that is also long enough to widen its column considerably | v0.1 | team |

## 3. A twelve-column narrow table

| c1 | c2 | c3 | c4 | c5 | c6 | c7 | c8 | c9 | c10 | c11 | c12 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 |
| 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | 30 | 31 | 32 |
| 31 | 32 | 33 | 34 | 35 | 36 | 37 | 38 | 39 | 40 | 41 | 42 |

## 4. A list, for the item gap and the indent

- The first item, which carries `inline code` set at 85% of the prose beside it.
- The second item, which is long enough to wrap at any sensible measure and so
  proves that an item's own box is as tall as the rows it actually draws.
- 第三项:中文条目,确认 `li + li` 的 .25em 间距在中英混排里同样成立。

1. An ordered list starts where the document says it starts.
2. And its second item is one quarter of an em below its first.

## 5. A paragraph with an unbreakable token

Before the token. AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA After the token — the run above is one hundred and ninety
characters with no space in it, and the page must still wrap to the pane rather than
grow an axis of its own for it.

> 引用块也在这里,确认 accent 竖条与内距在压力样张里同样成立。

---

See [the design document](../../docs/DESIGN.md) for the rulings this file exercises.
