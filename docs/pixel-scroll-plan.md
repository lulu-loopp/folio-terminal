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

## 阶段 C:输入接线与手感

- 滚轮 PixelDelta 原样累计;LineDelta 换算固定名义像素(不再先按内容行取整);
  Shift+wheel/粘滞回看入口语义不变。
- 手感参数(名义行高、每格像素)由 Claude 亲调(codex-scope-no-ui)。

## 全程硬门

- 24 录制 × 双模逐字节不变(滚动是 app 态,不进录制——任何字节漂移=阶段实现越界)。
- 全 workspace 测试、clippy -D warnings、fmt;既有滚动/选区/保持/回看钉死全绿。
- 每阶段一单,验收(红检+回放)后才进下一阶段。
