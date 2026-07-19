# m1.9e-r3 返工单:公式块覆盖邻行(阻断)——锚点用错了行

档位 high(根因已由复审精确定位到 file:line,照单修)。
基线 = M1.9e-r2 工作树(不要回退)。

---

## 阻断:placement 用 `start.row` 定位,但几何以 `band_start_row` 为基准

**协调者 oracle 现场**:alt 屏,四个单行公式块、每个下面紧跟一行普通
文本。三个块渲染(`math_blocks=3`),但**每个块都与其下一行文本叠印**
——违反裁决 B 铁律「绝不覆盖邻行」。

**根因(复审已实测复现)**:

`crates/bt-viewport/src/lib.rs:1087-1095` 覆盖 `top_subpixels` 时用
`start.row`(公式**源行**)查 `row_map`:

```rust
if let MathBlockAnchor::Live { start, .. } = placement.anchor
    && let Some(mapped) = row_map.iter()
        .find(|mapped| mapped.live_grid_row == Some(start.row))   // ← 错
{ placement.top_subpixels = mapped.top_subpixels; }
```

而该块的**几何全部以 `band_start_row` 为基准**算出:
- `:971-978`:`block_first = live_math.band_start_row`,
  `band_height = prefix[band_last+1] - prefix[band_first]`,
  并作为 `clip_height_subpixels`(`:1004`)下发;
- `:1413`:`per_row_extra[artifact.band_start_row] += extra`
  ——**额外高度只加在 band 的第一行上**。

`MathBlockAnchor::Live` 只带 `start`/`end`(`:992-997`),不带 band 行,
所以覆盖时只能用 `start.row`:**top 用带底的行、高度用整条带** ——
只要 `band_start_row != start.row`,块就整体下移
`(start.row − band_start_row)` 行,尾部必然压进 band 之外的下一行。

**为什么探针必然触发**:公式行紧跟普通文本 → `size_live_task_band_for_
artifact`(`session.rs:2702-2733`)向下借失败(下一行非空)→ 走
`:2718-2733` **向上借**那一行空行 → `band_start_row = start.row − 1`。
四个块里除最顶上那个全部命中。`extend_live_task_band`(`:2664-2675`)
同理。

**复审实测(cell=18px,12 行 alt 屏,start.row=3 / band=[2,3])**:

| 公式高度 | row_map(高度树,正确) | 实际下发的块 | 后果 |
|---|---|---|---|
| 30px(extra=0) | row2 top=36 h=18,row3 top=54,row4 top=72 | `top=54, clip_h=36` | 块占 [54,90) = 完整盖掉 row4 |
| 50px(extra=14) | row2 top=**22 h=32**,row3 top=54,row4 top=72 | `top=54, clip_h=50` | 块占 [54,104) = 盖掉 row4 + 半个 row5;而腾出的 22–54 **无人使用** |

注意后者:高度树**正确地**把空间腾在 row2,渲染器**正确地**按 placement
画在 row3——两边都没错,**是 placement 的锚点选错了行**。

### 要求

1. 把 `band_start_row`/`band_end_row` 带进 `MathBlockAnchor::Live`
   (或 `MathBlockPlacement`),**用 band 行查 `row_map`**;
2. `:987-1000` 的兜底值 `top_rows * cell_height_subpixels` 一并换成
   前缀表——自由高度下 `rows*cell` **恒错**,那是错误回退;
3. **同源问题一并修**:`crates/bt-term/src/session.rs:1655` 与
   `:1689-1690`,`decorate_math_frame` 里 show-source 的两条 placement
   仍用 `first_row * cell_height_subpixels` / `visible_row * cell_height`
   定位,**完全绕过 row_map**(上一轮"无 row*cell_height 残留"结论的
   真实反例——它不在文本绘制路径上所以被漏扫)。触发条件:同屏上方
   存在已展开的 rendered 块时,下方 show-source 块整体偏上,偏移量 =
   上方所有 extra 之和。

## 测试盲区(必须一并закрыть)

复审确认:唯一的帧级自由高度断言
`bt-viewport/src/lib.rs:2030-2093` 用了 `band_start_row = 0,
start.row = 0`——**正是 bug 消失的退化配置**,且**从未断言过
`math_blocks[0].top_subpixels`**;`lifecycle_matrix.rs:303/311/319`
只断言 top_subpixels 在多次 publish 间自比不变(错值稳定复现照样绿);
`bt-render` 侧 row_map 全是合成的,viewport/renderer 的分歧结构性不可见。

**要求做成帧级不变量,而不是点断言**:

```rust
// 对每个 display == Rendered 的 live 块:
let band_top = row_map[band_start_row].top_subpixels;
let band_bottom = row_map[band_end_row].top_subpixels
                + row_map[band_end_row].height_subpixels;
assert_eq!(block.top_subpixels, band_top);                     // 锚在带首
assert!(block.top_subpixels + block.clip_height_subpixels <= band_bottom);
// 铁律 B 的直接表述:块的绘制区间与 band 外任何一行的 row_map 区间不相交
for r in row_map.iter() {
    if !(band_start..=band_end).contains(&r.live_grid_row) {
        assert!(no_overlap(block_extent, (r.top_subpixels, r.height_subpixels)));
    }
}
```

- **必须**用 `band_start_row != start.row`(向上借)与
  `band_end_row != end.row`(向下借)两种配置各跑一遍——这两种才是
  真实排版下的常态输出;
- **把这条不变量下沉到 `ViewportFrame::validate_shape()`**
  (`bt-viewport/src/lib.rs:237`),让**每一帧自证不覆盖邻行**,
  而不是靠零散用例。

---

## 阻断 2:`N rows above` 覆盖层未真修,且比修前更差

复审实测判词:

- **"cells 未被改写"在 HEAD 本来就成立**(HEAD 的 `status_row_cells`
  同样返回新 Vec)——该项断言没有证明任何新东西;
- **"网格外"没做到**:覆盖层被 `row.min(grid_rows-1)` **夹回最后一格
  网格行**(`bt-render/src/lib.rs:2597`、`:2619`),底色矩形仍画在
  `rows-1`(`:2257`);
- **比 HEAD 更差**:底部行的真实文字现在**照常整形**,覆盖层是**另一条**
  row,两者一起画(`:2596-2644` → `:1874`)= **叠印**;HEAD 是替换,
  只画一份;
- **常驻问题未解决**(公式渲染期间一直显示,不像 `N lines below` 只在
  滚动时出现);
- **该项回归 `bt-render/src/lib.rs:4265` 是恒真的**——`source` 从没传进
  被测函数,断言不可能失败。

### 要求

1. 覆盖层**真正脱离网格**:不夹取到 `grid_rows-1`,不复用网格行绘制
   管线;它是渲染层的独立浮层(自有几何,与 cells/row_map 无关);
2. **不得与底部行文字叠印**:要么浮层绘制在其上方且底行文字保持完整
   可读(需论证可读性,如浮层自带不透明底),要么改变呈现时机;
3. **呈现时机与 `N lines below` 家族一致**(仅在有意义时出现,不常驻);
4. **回归必须能失败**:断言要驱动真实的 `source`/绘制路径,构造
   "有溢出/无溢出"两态,并断言底部行文字的可读性(如底行 cells 与
   浮层区域的关系)。恒真断言一律不接受。

## 其它复审发现(一并处理)

- **第 8 条的三条断言是恒等式**:`top=0/bottom=0` 在当前裁切算法下
  **不可能失败**(复审语)。改为真断言:用**已知含空白边距的合成栅格**
  验证裁切确实裁掉了它们,而不是验证"裁完之后是 0"。
- **flaky 如实报告**:复审在其机器上第一遍
  `real_powershell_input_reaches_a_viewport_owned_frame` 就失败,
  `--no-fail-fast` 重跑才过;而上一轮答复写"本轮没有 flaky 失败"。
  今后:**第一遍出现的任何失败必须如实报告**(含重跑是否通过),
  不许只报最好的一次。若该测试确属 flaky,给出加固方案。

## 低危(同处代码,一并处理)

`sync_live_math_artifacts`(`bt-viewport/src/lib.rs:1354-1431`)计算
`per_row_extra` 时**不做 screen/generation 过滤**,而 `continuous_frame`
(`:960-970`)出块时会过滤。切到另一块屏、artifact 尚未失效的那一帧,
高度树会保留一段无人占用的空白(方向与阻断相反,但同为"band 记账与
出块条件不一致")。

---

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、m1.8 系与 M1.9
全部回归、clippy `-D warnings`、fmt;vendor/glyphon 零改。
门禁数字从实跑输出抄写并附原始片段。
结果写在最终答复:根因修复点 file:line、帧级不变量的实现位置与覆盖的
配置、同源问题修复、门禁数字。停下等审,不提交。
