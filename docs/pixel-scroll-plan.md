# 像素级平滑滚动——实施方案(评审定稿 2026-07-31)

> 缘起:滚动微跳确诊=行量化滚动 × 可变行高(带边界步进突变 1.75-2.75px,证据
> `docs/reviews/` 与 scroll-jitter 确诊报告)。原五阶段方案出自该确诊单;本文是
> 评审后的定稿:**顺序重排为"先契约、再状态、后输入"**,使每阶段独立可红门、
> 且到阶段 C 之前用户可见行为零变化。

## 阶段 A:frame 契约扩展(先行,行为零变化)

- ViewportFrame 解耦「PTY 网格行数」与「呈现行数」:支持顶部行的负 top(部分可见)
  与底部一行 overscan;cells/anchor/cursor/band placement 全部由同一份呈现行列表生成。
- bt-render 放宽固定 N 行假设,保留既有 subpixel 绘制路径;命中/选区继续以 frame 自带
  row_map 为唯一依据。
- **本阶段滚动偏移恒为 0**:overscan 行存在但不可见,全部录制回放逐字节不变=硬门。
- G1「frame 恒矩形」断言重新裁决:矩形保持,行数定义改为"呈现行数(含 overscan)"。

## 阶段 B:亚像素滚动状态与不变量迁移

- 滚动位置 = 语义锚 + 行内精确 subpixel 偏移(history 用像素 HeightTree,live 用
  live_row_prefix);`ScrollAnchor.local_offset` 不再被除回行数。
- 六不变量逐项迁移,各带钉死:
  1. 底部跟随:输出/异步换装/zoom 后,Bottom 最后像素不动、不留缝(高风险);
  2. 回看语义锚:append/reflow/ED3 重印/artifact 高度变化后,同一内容点同一屏幕 y(高风险);
  3. resize reflow hold(33fb866 链)在像素锚下语义不变;
  4. 粘滞本地回看:alt 溢出容量改精确像素;回到严格 0 才恢复 wheel 转发(中风险);
  5. 选区/鼠标命中:部分裁切行、图片/公式带上下缘命中正确(中风险);
  6. 性能:shaping/row cache 不因位移失效;24 录制双模 exit 不劣化(中风险)。
- **North-star 红门**:恒定像素步进驱动下,任意内容锚点的逐步视觉位移严格等于步进
  (把 .tmp-scroll-diagnosis 的几何探针固化为永久测试;纯文本与混合带场景双覆盖)。
- 派生 `scroll_offset_rows` 保留供状态条/诊断。

### 阶段 B 契约裁决与落地记录

- 权威位置量是距 Bottom 的精确 `scroll_offset_subpixels`。非 Bottom 帧把窗口顶端编码为
  `ContentAnchor + ScrollAnchor.local_offset`；history 由像素 `HeightTree` 定位逻辑块后
  在块内按真实 row-height 前缀定位，staging 按 cell height，live 按
  `live_row_prefix`。`scroll_offset_rows` 只由权威像素量派生，不参与恢复位置。
- `scroll_by_rows` 仅是兼容入口，严格换算为 `rows * cell_height_subpixels` 后调用内部
  `scroll_by_subpixels`。阶段 B 不接 PixelDelta：`bt-app` 仍先沿既有路径量化成整行，
  再作上述换算，因此输入可见行为与 PTY 字节保持不变；原始像素输入累计留到阶段 C。
- 投影从窗口顶端所在的真实像素行开始，首行 `top_subpixels =
  -presentation_offset_subpixels`；offset 非零时底部 overscan 可进入 pane。Bottom 始终
  以内容最后像素对齐 pane 最后像素，异步 artifact 换装和 zoom 重新测高也不得留缝。
- 六不变量的永久钉死分别为：
  `bottom_follow_keeps_the_last_pixel_flush_through_async_artifact_and_zoom`；
  `review_anchor_keeps_exact_y_through_append_and_artifact_height_change`；
  `resize_reflow_hold_restores_the_exact_subpixel_displacement`；
  `alternate_sticky_review_uses_exact_pixel_capacity_and_exits_only_at_zero`；
  `partial_first_and_overscan_rows_share_exact_selection_hit_and_cursor_geometry`；
  `subpixel_motion_does_not_invalidate_layout_or_row_materialization_cache`，并由 bt-render 的
  content-addressed row-cache 几何重映射测试钉死 shaping/row cache 不因 offset 失效。
- North-star 永久红门为纯文本与混合图片/公式两场景：
  `north_star_*_content_moves_by_every_exact_subpixel_step`。每个样本都比较同一内容锚的
  相邻 frame y 差与输入 step，禁止用最终总位移掩盖中间量化。
- cursor/IME 契约债裁决：cursor 只要其 `row_map` 像素区间与 pane 有严格正面积交集就
  可见，允许位于部分可见首行或已进入 pane 的 overscan；完全在 pane 外才隐藏。
  renderer 仍按原始呈现几何绘制并由 pane clip 裁切。IME 候选框使用同一 cursor 行，
  但把 top/bottom 钳到 pane 的 `[0, grid_rows * cell_height]` 可见区间，故系统候选框
  锚到裁剪后的可见 caret，不会落到 pane 外。对应永久测试为
  `ime_candidate_anchor_clips_with_partial_first_and_overscan_cursors`。

## 阶段 C:输入接线与手感

- 滚轮 PixelDelta 原样累计;LineDelta 换算固定名义像素(不再先按内容行取整);
  Shift+wheel/粘滞回看入口语义不变。
- 手感参数(名义行高、每格像素)由 Claude 亲调(codex-scope-no-ui)。

## 全程硬门

- 24 录制 × 双模逐字节不变(滚动是 app 态,不进录制——任何字节漂移=阶段实现越界)。
- 全 workspace 测试、clippy -D warnings、fmt;既有滚动/选区/保持/回看钉死全绿。
- 每阶段一单,验收(红检+回放)后才进下一阶段。

## 阶段 A 的 G1 契约裁决

- `ViewportFrame.grid_rows` 是 PTY 网格高度；`ViewportFrame.rows` 是呈现高度，包含底部
  overscan。`cells.len == cell_anchors.len == columns * rows`，`row_map.len == rows`；
  “frame 恒矩形”按这组呈现维度判定。
- `presentation_offset_subpixels` 是呈现列表的精确行内偏移；阶段 A 的生产投影恒为
  `0`。此时 `drawable_rows == grid_rows`，overscan 仍在同一矩形中，但不参与像素、
  命中、选区或可见内容摘要。
- resize trace 的 `FramePublished.rows/cells/anchors` 记录呈现矩形，并另带
  `grid_rows`；PTY resize/grid-match 断言只比较 `grid_rows`。生命周期矩阵中原
  `columns * grid_rows` 的 frame 矩形断言相应改判为
  `columns * (grid_rows + FRAME_OVERSCAN_ROWS)`。
- IME/preedit 与 cursor 的越界钳制在阶段 A 继续受 `grid_rows` 约束；部分首行下的
  cursor/IME 策略是后续像素偏移阶段的契约债，不在本阶段借 overscan 改行为。
