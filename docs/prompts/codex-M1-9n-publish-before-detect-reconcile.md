# M1.9n 任务书:发布前 reconcile —— 备用屏任何重绘都不闪源码(审因 R5+R6)

档位 high(踩"绝不显示错误公式"红线 + 性能 + alt 生命周期)。基线 = 提交 `20846b1`。
**先读审因报告** `docs/reviews/M1.9-formula-pipeline-audit.md` 的 R5/R6 与"零闪现"
架构结论——本任务书是它的执行版。

## 要解决的现象(用户实拍,仅 Claude Code / 备用屏)

- 滚动回看时已渲染公式**退回源码**;
- **点击**画面 → 整页抽搐、公式闪一下源码再变回;
- **双击**公式所在行 → 公式变回源码。

同一根:Claude Code 是备用屏 TUI,**任何鼠标事件都触发它整屏重绘** → 我们内容级
失效 → 重新检测 → 稳定窗口(200ms)内先发一帧源码 → 闪。M1.9j 的缓存只省了重渲染
时间、命中在稳定窗口**之后**,不够(已 stash,见 `git stash list` 的 M1.9j)。

## 目标架构(审因 R6)

**在 terminal 完成一次 PTY drain 之后、`viewport_frame` 之前,做发布前同步 reconcile:**
1. 对当前 screen revision 的**可见范围**做 **bounded 同步检测**(复用 M1.9k 的统一
   scanner,不是新写一套);
2. 每个检出的块,若其 **(源码精确相等 + 渲染键精确相等)** 命中**可复用 artifact
   store**,就**原子地**在新位置安装同一 artifact(Ready、不发源码帧);
3. 只有 **miss** 才落到既有路径:等 200ms 稳定窗口 + 调度昂贵 raster;
4. 纯 winit Expose(内容未变)**不检测、不 reconcile**;
5. 净效果:**重绘那一帧就带着已渲染公式发布,从不先发一帧源码** → 滚动/点击/双击
   都不闪。

## 红线(不可让步)

- **绝不显示错误公式**:命中判定必须 **源码字节完全相等 且 渲染键完全相等**。审因
  指出现有 renderer 仍按 **64-bit hash 字符串**复用 texture(`session.rs` 的 artifact
  key、`bt-render` 的 texture 复用),**理论碰撞会贴错像素**。本片必须让复用键
  **无碰撞**(可比较的结构键 / 唯一 artifact id,或命中时做全文相等校验),**端到端**
  满足"绝不错公式"。给出无碰撞论证;
- **绝不错配散文 / 绝不误渲染代码**:M1.9k 的 Ambiguous 配对、CommonMark 代码上下文、
  九类误报守卫、CJK 散文守卫全部不动;同步检测走的就是 M1.9k scanner,自动继承;
- **性能**:发布前同步检测**只覆盖可见范围 + 必要上下文**,不扫全历史;给复杂度论证
  (每帧 O(可见行),不是 O(历史));winit Expose 零成本;不得让普通(无公式)输出
  的每帧路径变重——无公式时同步 reconcile 应快速短路;
- M1.9m 呈现模型(净高+对称 padding+居中+底锚+show_source 保持)、坐标不变量、
  帧级铁律 B、`N rows above`、m1.9g/h 全保持。

## R5 可复用 artifact store(R6 的地基)

把 M1.9j 的一次性缓存改造成合格 store(审因 R5):
- **`get` + Arc clone,命中不删除**(可多次复用);
- 同一公式多 occurrence、live/frozen、primary/alt **共享**同一 artifact;
- **按 resident bytes 淘汰**(不是按条数),给内存上界;
- store 生命周期与单个 decoration **解耦**;layout 变化按渲染键自然失效;
- 键 = 源码 + 渲染键(含 layout/mode),**无碰撞**(见红线)。

可 `git stash show -p` 看 M1.9j 那份缓存作**参考**,但按 R5/R6 要求实现,不是简单还原。

## 回归(至少,各配变异)

1. **零闪现核心**:备用屏整屏重绘(模拟点击/滚动:同内容重绘 + 内容平移重绘),
   断言**重绘后第一帧**公式即为 Ready、**无任何源码帧**(检查该帧 show_source/cells
   无 `$$`);变异:去掉发布前 reconcile → 断言变红(出现源码帧);
2. **无碰撞红线**:构造两个不同源码但 64-bit hash 相同(或直接伪造键碰撞)的场景,
   断言**不复用错 artifact**;变异:把命中判定退回纯 hash → 变红(贴错像素);
3. store 复用:同源多 occurrence 共享同一 Arc;命中不删除、可再命中;按 bytes 淘汰;
4. winit Expose(无内容变化)不触发检测/reconcile(检测计数不增);
5. 无公式的普通输出每帧路径不变重(性能断言:同步 reconcile 在无候选时快速短路);
6. 双击/点击若经由选区或其它交互路径触发过 show_source 或强失效,定位并修正(查
   双击是否误命中"显示源码"切换);把该路径的正确行为写成回归;
7. M1.9k 检测形态、两条红线、M1.9m 几何、`N rows above`、单块 `d` 不裁全绿。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。门禁数字从实跑输出抄写并附原始片段;第一遍
任何失败如实报告。

## 交付(写进 output 文件)

发布前 reconcile 的接入点 file:line、bounded 同步检测的复杂度论证、复用键的**无碰撞
证明**、R5 store 设计(共享/淘汰/生命周期)、双击/点击路径的定位与修正、新增回归
(含各变异)、门禁实跑数字。若判断"零闪现"与某既有不变量不可调和,停下说明。
停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
