# m1.9b-r2 返工单:live 公式已渲染但从不进入视口帧(阻断)

## Oracle 证据(协调者真窗口实测,M1.9b 工作树构建)

探针:alt 屏(`\x1b[?1049h`)+ 三个 `$$` 块(短/中/超高)+ 两行邻居。
BT_PERF_TRACE 输出证明**前四步全部成功**:

```
BT_PERF_TRACE live_math_detect=3 live_math_invalidations=0
BT_PERF_TRACE live_math_render_us=21086 row=2 resident_bytes=45696
BT_PERF_TRACE live_math_render_us=16038 row=4 resident_bytes=115480
BT_PERF_TRACE live_math_render_us=15672 row=6 resident_bytes=285160
```

但**每一帧**都是:

```
BT_PERF_TRACE frame=7 ... alt=1 ... math_blocks=0 math_texture_resident_bytes=0
```

屏幕实拍:三个公式全部仍是源码文本。即:检测✓ 排版✓ 纹理驻留✓,
**投影✗** —— 渲染结果从未进入 `ViewportFrame.math_blocks`。

## 根因(协调者已定位,你复核后修)

`crates/bt-viewport/src/lib.rs`:

- `math_blocks` 仅在 **history 分支**内被 push(`if primary && window_start
  < history_rows` 约 :722-756)。
- **live rows 的投影分支(约 :801-820)没有任何 math_blocks 填充逻辑**,
  只把 CapturedRow 转成 visual row。
- 因此 alt 屏(无 history)与 primary 的 live 区,live artifact 永远
  投影不到帧上。

## 要求

1. **补 live 投影**:live rows 分支按 live math artifact 的 GridPoint 行区间
   生成 `MathBlockPlacement`(anchor = 你新增的 live/Grid 变体),
   `top_subpixels` 按块首行在窗口内的位置算,`clip_height_subpixels`
   **严格等于该块行带的高度**(裁决 B 精修:绝不越出行带、绝不盖邻行),
   `render_scale_milli` 反映等比缩放,低于 0.65 地板的块**不产生 placement**
   (保持源码)。
2. **端到端回归(本轮缺的正是这层)**:构造 session→projection→frame 的
   完整链路测试,断言:
   - alt 屏 + 稳定 live 块 → `frame.math_blocks.len()==1`,且
     `clip_height_subpixels <= 行带高度`;
   - 邻行文本在 `frame.cells` 中**完好可见**(不被裁掉/覆盖);
   - 超高块缩放后仍在行带内;低于地板的块 `math_blocks` 为空且源码文本在;
   - 块区域重写/alt 退出 → 下一帧 `math_blocks` 归零;
   - primary live 区(非 alt)同样成立。
   这些断言必须**在 frame 层**,不能只在 session 层——本轮的教训就是
   session 层全绿而 frame 层空手。
3. 顺带核对:`math_texture_resident_bytes` 在帧行上恒为 0 是否也是同一
   断裂的表征(renderer 侧统计与 session 侧 285KB 不一致),如是一并修。
4. 既有 M1.9b 的其余部分(交互、几何、stale)不动,除非与上述冲突。

## 门禁

- 全 workspace --locked --offline、310 语料、G1/G2/G3、m1.8 系回归、
  clippy -D warnings、fmt;vendor/glyphon 零改。
- 结果写在最终答复:根因确认、修复点 file:line、新增 frame 层回归清单、
  门禁数字。停下等审,不提交。
