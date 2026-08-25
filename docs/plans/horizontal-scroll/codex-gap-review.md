# 横向滚动差距报告裁断

日期：2026-08-24  
对象：`gap-2026-08-24.md` §5 的 G1–G4 送审问题，以及报告自身错漏  
结论：四项均不能原样并回 `plan.md`；G1 选 A，G2 有条件成立，G3 采用“可呈现保留域”，G4(a) 有条件成立。

## 总裁断

| 项 | 裁断 | 必改摘要 | 存疑/不得外推 |
|---|---|---|---|
| G1 | **选 A：只展平 history + staging，活网格保持物理行；接受活带接缝。** | 把 v1 的横向逻辑域和活网格接缝写成显式契约；删去任何暗示 live grid 也按逻辑行展平的文字、公式和验收项。 | A 是一期边界，不是永久证明 B 不可做。以后若做 B，必须另立纵向身份、光标、选择、命中测试和 resize 契约，不能作为本方案的“实现细节”补入。 |
| G2 | **每行索引户口成立；stride-64 仅作为可调的派生缓存策略成立。** | 索引归属逻辑行、惰性构造、随行失效/逐出、不进 `bt-transcript`；checkpoint 必须落在可重新解码的 token/cell 边界。 | `FROZEN_BYTES_PER_LINE = 2048` 不能自动证明 checkpoint 数量有界；压缩空白/run、零宽附着和宽字符都会破坏“字节数≈列数”的推导。 |
| G3 | **取“实际可呈现的 retained flattened domain”。** | `content_extent` 只统计仍保留且可寻址的 history + staging 逻辑行；不含 live grid、已逐出行、仅由截断前原长推测出的不可取回尾部，也不含 cache/high-water。 | 若以后选 B，定义域才可扩为 history + staging + live；不得现在先把 live 的宽度塞进 extent。 |
| G4(a) | **成立，但捕获宽度只是截断门的必要输入之一。** | `PhysicalFragment` 记录不可变的 `captured_columns`；末列判断使用该 fragment 的捕获宽度，不得使用当前 grid 宽度。 | “触达捕获末列”本身不能证明发生截断：恰好占满一行和 VT autowrap-pending 都是合法反例；仍需字节预算耗尽及确有未保留内容/continuation 的证据。 |

## G1：选 A，不在横滚一期补纵向契约

### 裁断

选 A。`history + staging` 以逻辑行展平并进入横向坐标系；`live grid` 继续以现有物理 `grid_row × grid_col` 语义渲染，不把 wrap-chain 压成一个显示行。

理由不是 B 在数学上不可能，而是 B 会改变当前活屏的纵向对象：显示行数、光标所在显示行、选择端点、链接/鼠标命中、脏区、IME/caret、滚到底部锚点以及 resize 后的重排都要从物理行改投影到逻辑行。这已经不是“补一个 `grid_row(column)` 公式”，而是一次纵向模型迁移；它与方案 §2 承诺的本轮纵向语义不变不相容。

### 必改

1. 明定 v1 的三段域：
   - history：sealed logical lines，展平，可横向寻址；
   - staging：已从活网格捕获、可稳定寻址的 fragments/logical lines，展平；
   - live：原生物理网格行，不展平。
2. 所有 `grid_row = f(logical_column)` 一类公式只能出现在“未来 B 案”附注，不得继续作为 v1 live 渲染、选择或 hit-test 的依据。
3. 活带接缝必须成为可测试行为，而不是实现偶然：同一 soft-wrapped logical line 一部分进入 staging、另一部分尚在 live 时，上部按逻辑行、下部按物理行呈现；迁移完成后才原子切换为完整 staging logical line。不得在一次 frame 内重复或漏掉 fragment。
4. 横向 origin 仍是单一状态。live 物理行只拥有其捕获/当前 grid 的列域，超域取空白；不得为了“看起来正常”把 live 单独偷钉在 `x=0`，否则同一指针 x 在两段拥有两套列语义，copy/selection/hit-test 会再次分叉。
5. 到达 live/follow-tail 时是否自动把 horizontal origin 归零必须单独定成 UX 行为；它不能由纵向 clamp 的副作用隐式发生。若本轮无此产品裁决，默认保持 origin，只按新的 `content_extent` clamp。

### 存疑

报告把“backing `grid_row` 可能是 column 的函数”直接等同为“横纵滚动轴耦合”，措辞过强。显示空间中的 `(display_row, logical_col)` 仍可正交，column 只是在取数时选择某个 backing fragment。真正缺失的是 live logical-row 的纵向身份及其全套交互映射，而不是一个函数依赖本身。这个修正不改变本次选 A 的结论，但应避免把 G1 写成错误的数学定理。

## G2：逐行户口成立，stride-64 只作派生策略

### 裁断

每条可横向寻址的逻辑行拥有自己的横向索引户口是正确边界。索引必须是行内容的派生物，不是 transcript 的第二份权威数据。stride-64 可作为初始实现参数，但不得冻结为持久格式或公共契约。

推荐的最小契约是：

- key：稳定 logical-line id + content generation/version；
- value：按 logical terminal column 单调排列的 checkpoints；
- checkpoint：至少含 logical column、fragment/token 定位和可安全续扫的位置；
- build：首次发生非零横向 seek 或重复 seek 时惰性构造；短行允许完全不建；
- lifetime：sealed history 行可一直随行；mutable staging 每次 append/replace 使旧 generation 失效；行 eviction 时同步释放；
- persistence：不写入 `bt-transcript`，载入后按需重建；
- correctness：索引缺失、被丢弃或 stride 改变，只能影响耗时，不能影响所取 cell。

### 必改

1. “64”按 logical terminal columns 计，不按 UTF-16 code units、Unicode scalar、UTF-8 bytes 或 glyph 数计。
2. checkpoint 只能落在自同步边界。不得落在宽字符的 continuation cell、组合附着序列中间、可变长 token 中间或依赖前序 attribute/run 状态却未把该状态带入的位置。实际锚点应是“不大于目标倍数的最近安全边界”，随后有界前扫。
3. checkpoint 必须足以恢复解码状态。若 token 格式的颜色/旗标/run 采用增量状态，仅存 byte offset 不成立；要么 checkpoint 同存必要状态，要么只锚到完整 fragment/token 起点。
4. 建索引和 seek 全程使用 checked/saturating arithmetic，并以已验证的 record/fragment 边界为上限。损坏记录不能借索引越过 `FROZEN_BYTES_PER_LINE` 的已存 payload。
5. 给索引另设内存预算和退化路径：预算不足时丢弃/不建索引并回退到从行首线性扫描。正确性不得依赖分配成功。
6. 验收至少覆盖：ASCII、CJK 宽字符跨 64 边界、combining mark、空白/run 跨多个 stride、截断 record、mutable staging append 后旧索引、eviction、随机 seek 与线性解码逐 cell 对拍。

### 存疑

报告若以 `2048 / 64 = 32` 证明“每行最多约 32 个 checkpoint”，该证明不成立。2048 是序列化字节顶，不是 logical-column 顶；一个短 run token 可能代表大量空白列，零宽附着消耗字节却不推进列，宽字符又一次推进两列。必须从编码规范证明“每个可寻址列至少消耗固定字节”才可用该上界；现有裁决没有给出这条不变量。因此 stride-64 的最坏索引规模仍需独立 cap/预算。

另外，2048 字节顶使单次线性扫描本身已有严格字节上限。首版可以先实现正确的逐行 seek API 与线性 fallback，再用基准证明稀疏索引值得常驻；不得把索引预建到全部 scrollback 行上。

## G3：`content_extent` 采用可呈现保留域

### 裁断

定义：

```text
addressable_lines = retained(history logical lines)
                  ∪ retained(addressable staging logical lines)

content_extent = max(viewport_columns,
                     max(presentable_end_column(line)
                         for line in addressable_lines))

max_horizontal_origin = max(0, content_extent - viewport_columns)
```

其中 `presentable_end_column` 是通过当前保留 payload 能实际 seek/render/copy 的半开末列；对不完整宽字符只计到最后一个完整 cell span。它不是“曾观察到的原始行宽”，也不是捕获宽度，更不是被 2048 字节顶截掉之前的推测长度。

### 必改

1. 明确排除：live grid、已 eviction 行、alternate-screen 非持久内容、损坏/拒收记录、截断后不可重建的尾部、仅存在于 raster/layout cache 的范围。
2. `content_extent` 随 retained set 变化：加入/增长 staging 行可增大；最宽行被逐出或变得不可寻址时必须缩小，并立即 clamp horizontal origin。不得用 session high-water 冒充当前 extent。
3. 实现不得每 frame 全量扫 scrollback。维护按行宽计数的 multiset/直方图/有序计数结构，或等价的可删除 max；行加入、staging generation 变化、seal、eviction 时成对更新。
4. extent 的权威输入是 transcript/logical-line 层的可呈现宽度，不是 renderer 测得像素宽度。像素 scrollbar 只做 `columns × cell_width` 的显示换算。
5. 空域、全空白行及 viewport resize 的 clamp 顺序要固定：先得到新的 viewport columns 和 retained extent，再一次性 clamp origin，再发布 frame，避免一帧越界。

### 存疑

若三种候选中包含“所有曾见行的单调 high-water”，应明确否决：它在最宽行逐出后留下永远不可到达的空白尾部。若包含“当前 viewport/render cache 最大宽度”，也应否决：滚动或 cache eviction 会改变 scrollbar，而内容本身没有改变。若包含 `history + staging + live`，它只适用于未来 B 案；在 A 案下会让 scrollbar 宣称存在 live 横向列，而 live renderer 没有对应的逻辑行服务契约。

## G4(a)：记录捕获宽度成立，但不得把它当截断事实

### 裁断

`PhysicalFragment` 必须记录该 fragment 被捕获时的物理 grid 宽度，建议字段名 `captured_columns`。它是 immutable provenance；resize 后不能用当前 `CellGrid.Columns` 回算。这个补充足以修复 §7.1.5k 末列门在跨 resize fragment 上没有稳定参照系的问题。

末列触达应使用半开列约定：

```text
touches_captured_right_edge = fragment.present_end_column == fragment.captured_columns
```

若内部采用闭区间，等价式才是 `last_present_column == captured_columns - 1`；方案必须选一种并全篇统一，不得混用。

### 必改

1. 捕获时写入 `captured_columns`，范围必须大于 0，且 fragment 的物理 start/end 均经验证不越界；反序列化外来/损坏记录时同样验证。
2. 一个 logical line 可含不同捕获宽度的 fragments。末列门逐 fragment 判断，不能拿首 fragment、末 fragment 或当前 viewport 的宽度覆盖整条逻辑行。
3. 真正的截断判定必须是合取条件，至少同时证明：
   - 保留/序列化预算确实耗尽或明确拒收后续 payload；
   - 源端确有未保留的 cell/token/continuation，而不是恰好结束；
   - §7.1.5k 要求的末列条件以该 fragment 的 `captured_columns` 成立。
4. exact-fit 行和 autowrap-pending 必须是反例测试：写到最后一列但没有下一 printable character 时，不能仅因触达末列就标 truncated；有 wrap/continuation evidence 时再按门处理。
5. `captured_columns` 不得被拿来充当 logical content width。前者是捕获几何 provenance，后者是实际可呈现内容的末列，两者分别服务 G4 与 G3。

### 存疑

如果报告 G4(a) 的文字是“新增捕获宽度后即可判定截断”，应改为“新增捕获宽度后，末列这个子谓词才可稳定判定”。它修复的是参照宽度，不提供“后面还有数据”的事实。否则恰好填满末列的正常输出会被系统性误报。

## 报告本身的错漏

### 必改

1. **G1 的因果表述过界。** `grid_row = f(column)` 说明 backing-store 寻址非笛卡尔直取，不自动说明两个 viewport scroll offset 相互决定。报告应把缺口准确命名为“live logical-row 的纵向身份与交互映射未定义”。
2. **G2 混用了字节顶和列顶。** `FROZEN_BYTES_PER_LINE` 只能界定已存/已扫字节，不能在未证明编码密度下界时界定 logical columns 或 checkpoint 数量。
3. **G2 没把索引与持久格式隔离干净。** 稀疏索引是可丢弃的加速数据；若报告把它列为 `bt-transcript` record 字段或恢复正确性的前提，应删除。冻结格式只保存权威内容和必要 provenance。
4. **G3 若未区分 observed、retained、presentable 三种宽度，术语不足。** 2048 字节截断后可能知道“原来更长”，却不能呈现其尾部；scroll extent 只能承诺可到达内容。
5. **G3 若没有 eviction 的减法路径，只有加入时更新 max，不完整。** 单调最大值不是 retained extent；需要可删除 max 及 origin clamp 的原子发布。
6. **G4(a) 把必要条件写成充分条件会误报 exact-fit。** 捕获宽度解决 resize 参照，不能替代 byte-budget-exhausted 与 source-remains/continuation evidence。
7. **G1 与 G3 必须联动。** 既然选 A，extent 就不能仍覆盖 live；否则同一方案同时声明“live 不展平”与“横向范围包含 live logical width”，又产生新的可达性矛盾。

### 存疑

1. “staging”是否包含仍会被原位改写的开放尾行，报告若没有给稳定 id + generation 规则，G2 索引失效和 G3 宽度减法都无法实现。本文按“可稳定寻址的 staging 才入域”裁断；开放尾行若入域，必须补 generation 事件。
2. 报告若把 trimmed trailing blanks 算入 content width，需要另给产品语义。默认只统计可呈现的最后非空/有样式 cell span；但带背景色、下划线、链接或 selection-relevant metadata 的空格不是可随意 trim 的空白。
3. `captured_columns` 的持久化位宽、上限和版本兼容尚未由 G4(a) 决定。应沿用 `bt-transcript` 的统一整数编码与 record validation，不应因本裁断另开未封顶的整数域。

## 合并门

在以下文案与测试同时补齐前，G1–G4 仍视为“报告已发现、方案未闭合”，不能把差距报告标为 resolved：

1. plan 删除/隔离 live flattening，写明 A 案三段域与接缝原子迁移；
2. horizontal seek API 有无索引结果一致，stride 边界与 Unicode/wide-cell 对拍通过；
3. extent 对 staging growth、最宽行 eviction、resize 和截断尾部的期望明确；
4. fragment 跨 resize 使用各自 `captured_columns`，且 exact-fit 不误报 truncated；
5. A 案下 live 不进入 extent，所有 renderer/copy/selection/hit-test 都遵守同一 horizontal origin 与各自可寻址域。
