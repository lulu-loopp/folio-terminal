# M1.9m 任务书:备用屏公式呈现模型(净高+对称 padding+居中+底部锚定+源码态保持)

档位 high(渲染几何 + free-height 投影 + alt 生命周期,踩过同族坑 m1.9g/l)。
基线 = 提交 `bac8d5a`。来源:用户实测(2026-07-20,image 216/217)+ 用户定的呈现模型。

## 背景

前几刀(m1.9e 扩展、m1.9l 收拢)逐个补几何,反复撞交互:高块顶裁、短块留空白、
间距不一致、收拢把 Claude Code 输入框拱上来、切源码后一滚又渲染。根因是
**free-height(可变像素行高)与 Claude Code 固定 N×M 栅格在备用屏打架**。
用户拍板一个统一模型,本片按它成建制实现,取代 m1.9l 的 band=raster(已 stash)。

## 用户定的呈现模型(本片的规范)

**不变量 A — 块高:**
> 一个公式块的**呈现像素高 = 公式净高(alpha 紧盒)+ 上下对称 padding**;公式在
> 这个盒子里**竖直居中**。**与它覆盖了几行源码无关。**

- padding **可配置**:挂在 `MathLayoutOptions`(已有的 math 布局选项结构)上,
  一个字段,**默认上下各 0.25 × cell_height**(按 cell 高等比,跨 DPI 自动正确);
  配置系统落地后可调,现在给默认值即可。
- 净高用现有 alpha 紧盒(已有裁切);padding 对称 → 天然居中,raster 上下都有
  padding 缓冲,**不可能碰到 clip 边**(一举根治顶裁,含 m1.9l 查到的
  "bottom-following 把增量投到 pane 顶外被 pane scissor 截"那条:盒子含上下 padding
  且居中,raster 顶永远在盒内)。
- 块**覆盖** N 个源码逻辑行(藏住原始 LaTeX)但**按盒高呈现**;N 行按 free-height
  折叠/展开到盒高,live 与 frozen 两条呈现路径都遵此规范。

**不变量 B — 备用屏底部锚定:**
> 备用屏呈现**底部锚定**:最后一行(app 的输入框/状态行那行)**永远贴窗口底**;
> 块高变化腾出/占用的像素**一律在顶部消化**(顶部留白或顶内容进出),**底部不动**。

- 解决 Issue 4:块收拢使栅格总像素变短时,余量去顶部,而不是把输入框拱上来;
- 与用户既有裁决一致("溢出落顶部,底部始终可见");扩展(顶内容被顶出)行为保持;
- 主屏(我们自有滚动区)不受此约束、行为不变。

**不变量 C — 源码态按内容保持:**
> 用户点按钮切"显示源码"后,**即使滚动/重绘也保持源码态**,不因备用屏重检测被重置。

- 现状(Issue 3):切源码 → 滚动 → Claude Code 整屏重绘 → 我们重检测 → 新装饰不带
  `show_source` → 又渲染。修法:把"这段**源码内容**要显示为源码"的偏好按内容记住
  (与 artifact 缓存同思路,存偏好而非像素),重检测/命中时尊重;
- 反向:用户切回渲染后,同样按内容记住"要渲染"。偏好键 = 源码内容(+ 必要 layout)。

## 复现(alt 屏,`BT_PROBE_INPUT`;`\begin` 用真 `\\`)

- 症状 A 高块顶裁:bare `\begin{align}` + 分数(3 行,含 `\frac`),贴近顶;
- 症状 B 短块空白:`$$` 换行、中间多空行、`x = 1`、多空行、`$$`(源码 7+ 行,渲染 ~1.5cell);
- Issue 4:满屏输出 + 若干上述块,底部有一行"输入行"占位,收拢后断言输入行仍贴底、
  栅格无底部空白;
- Issue 3:一个块 → 切 show_source → 触发一次备用屏整屏重绘 → 断言仍是源码态。

## 硬约束(不可回退)

- 九类误报守卫、CJK 散文守卫、错配红线(M1.9k 的 Ambiguous)、CommonMark 代码上下文、
  M1.9k 检测形态全集(多行内联 `$$`/`\[`/环境)——全不动;
- m1.9g 单块 `d` 上升部不裁、m1.9h 左缩进、借空行非重叠证明、`N rows above` 顶部溢出、
  光标/输入/状态行可见、帧级铁律 B(clip 覆盖 block)——全保持;
- 第①层终端坐标不变量(ConPTY 固定 N×M、富内容绝不触发 PTY resize)不破;
- 横向无关,不动横滚/wrap。

## 回归(至少,各配变异)

1. 块高 = 净高 + 2×padding(±1px),公式竖直居中;变异:padding 设 0 → 断言变化;
2. 症状 A:高块 raster 顶 subpixel ≥ clip 上沿(不裁);变异:盒高少算一 padding → 红;
3. 症状 B:多行短公式块高 ≈ 净高+2padding、无源码行数空白;变异:恢复"band 不缩到
   source_rows 以下" → 红并显示空白;
4. Issue 4:备用屏收拢后,输入行映射到窗口底(±1cell)、无底部空白 band;变异:改回
   顶锚 → 红(输入行上移);
5. Issue 3:切 show_source 后经一次备用屏重绘仍为源码态;变异:去掉偏好保持 → 红(又渲染);
6. padding 可配:改 `MathLayoutOptions` 的 padding 字段 → 块高随之变(一条断言);
7. 主屏呈现、单块 `d` 不裁、邻块不重叠、`N rows above` 溢出回归全绿。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。门禁数字从实跑输出抄写并附原始片段;第一遍
任何失败如实报告。

## 交付(写进 output 文件)

三条不变量的实现与 file:line、padding 配置位、竖向几何算式、底部锚定如何与既有
top-overflow/`N rows above` 协调、源码态偏好的键与生命周期、新增回归(含各变异)、
门禁实跑数字。停下等审,不提交。

若判断某条不变量与既有不变量有不可调和冲突,**停下说明**,不要擅自取舍。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
