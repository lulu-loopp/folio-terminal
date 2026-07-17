# M1.5 字形品质片(彩色 emoji + fallback 适配)kickoff

M1(e34e078)已交付宽度正确性:网格记账与逐格绝对定位有像素级证据。本片解决「格子对了但里面的字形不行」的三件事,同属 fallback 字形品质一个主题。产品裁决背景:用户明确要求彩色 emoji(「毕竟我们还希望在终端里显示图片视频」),且实测确认 ☆(ambiguous 窄)与相邻 CJK 墨迹在所有缩放下都重叠。

## 交付项

### 1. 彩色 emoji(主项)

- 现状:emoji(👨‍👩‍👧‍👦/👍🏽/🇺🇸/☂️ VS16)全部渲染为 .notdef tofu。M1 的 fixture 与占格断言已就位,本片让这些格子出真彩色字形。
- 先做**能力侦察**(写进交付报告):cosmic-text/swash/glyphon 当前版本对 COLR/CPAL(Segoe UI Emoji)与位图 emoji 的支持程度;字体回退链里 Segoe UI Emoji 是否可达(bt-render 的 FontSystem 用 locale+db 构建,检查 emoji 字体是否入库);ZWJ 序列在宽槽 buffer 内能否 shape 成单一 emoji 字形。侦察结论决定路线:优先 COLR v0(Segoe UI Emoji 是 COLR)→ glyphon 的彩色渲染路径;若栈不支持,评估最小补丁面(vendor 补丁或换 glyph 光栅路径),**先报方案再动手**,不要一头扎进大改。
- 语义要求:emoji 簇渲染为**单一彩色字形**,em 归一进其双格槽(2027 下 family=1 簇 2 格;legacy 下每人各 2 格,各画各的);VS15 文本样式保持单色文本字形、VS16 出彩色;裁剪维持宽槽双格政策。
- 明确边界:不做动画 emoji、不做 emoji 选择器;图片/视频协议(Sixel/iTerm2/kitty)不在本片。

### 2. fallback 窄字形按格自适配(用户实测驱动)

- 现状:窄格 fallback 字形(☆ U+2606、│ 等 CJK 字体里按全宽设计的 ambiguous 字符)按原始尺寸绘制,墨迹越格压到右邻字形上,所有缩放下均可见(150%/200% 实测)。
- 要求:**非主字体的窄格字形,当其墨迹/advance 超出 1 格时,按格宽 em 归一(等比缩小,保基线对齐,留侧边距)**——即把宽槽的归一化机制推广到窄 fallback 格。主字体字形(斜体出锋、重音 bearing)**不动**,整行 overhang 政策对它们保留。判定标准写清楚(如何区分「主字体窄字形的正常出锋」与「fallback 全宽字形的越格」——按 font id 是否主字体判定,不按墨迹宽度猜)。
- 归一后 ☆ 在 1 格内完整可见(小一号是预期代价);P2-7 的 ambiguous=wide 配置是另一条腿(本片不做配置 UI,seam 已在 bt-unicode)。
- shaping 缓存注意:归一参数进不进缓存键由你裁量(同一 text 在主/fallback 字体间不会变,理论上不用进键),说明理由。

### 3. 验收配套

- 扩展 scripts/dev/width-probe-input.vt 或新增 glyph fixture:emoji 各类 + ☆│ ambiguous 行 + 主字体斜体行(证明出锋没被误杀);.md 写期望(颜色出现与否、占格、☆ 墨迹不越格)。
- 渲染层回归测试:emoji 宽槽出非零彩色内容(能断言到什么程度依栈能力,至少断言字形非 .notdef 且占格正确);☆ 归一后 ink 宽 ≤ cell_width(若栈可测)。
- 像素级验收由协调者用 BT_PROBE_INPUT + 截图量测执行,你不用截屏。

## 门禁

cargo test --workspace --locked;cargo clippy --workspace --all-targets --locked -- -D warnings;cargo fmt 一方 crate;既有测试计数不降(bt-render 27、vendor 182)。M1 的 14 行像素基线不得回退。

侦察结论 + 实现完成后停下等审。若侦察发现彩色 emoji 需要大改(如换字形光栅库),先停下报告方案与代价,等裁决再动手。
