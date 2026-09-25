# Synthetic CJK regression fonts

These six tiny test fonts were generated from rectangles, not copied or subset
from an installed font. `generate.py` (Python fontTools) recreates them with fixed
timestamps. Rust tests load the checked-in bytes; fontTools is not a test dependency.

Test CJK has weights 400 and 700. Test Other has only 400. Test Escape has only 700,
so a fallback escape cannot accidentally pass a family assertion. Test Sans has
no Han. CJK faces declare OS/2 bit 18 and map two Han code points to a rectangle.
The outlines and generator are covered by the repository license.

Test Strokes (ticket 61) has only 400 and maps eight Han code points to glyphs
built of separate stroke rectangles, every contour clockwise. Four of them
(口 日 回 中) close each stroke on its outward edge and list the strokes
counter-clockwise, so all their points read as one polygon wind the other way
from the contours; 一 二 三 十 do not.
