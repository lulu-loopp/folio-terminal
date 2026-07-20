# M1.9j 任务书:备用屏回看公式闪回源码(内容寻址复用 artifact)+ show-source 漏闭合 $$

档位 high(①是缓存正确性 + 视觉一致,②是映射修正)。基线 = 提交 `30a7aea`。
来源:用户在 M1.9i-B 验收时实拍(2026-07-20,image 212)。

## 背景(协调者已证根因,勿重复推导,可挑战)

M1.9i-B 让备用屏(Claude Code)公式开始渲染。用户随即发现两点。

---

## 部分 ① 备用屏回看时公式闪回源码(主要)

**现象**:在 Claude Code 里**上滚回看**时,已渲染的公式先闪回源码再重新渲染。

**协调者的因果链(file:line,待你复核)**:
- `observe_live_damage`(`crates/bt-term/src/session.rs:901-921`)按**内容指纹**失效:
  指纹未变则跳过(:912,同内容同行不闪的快路径);指纹变则
  `invalidate_live_row`(:924-943)**把整条 live 装饰连同 artifact 直接删除**。
- Claude Code 上滚回看不是"顶部滚掉一行"(那条走
  `preserve_live_after_top_scroll`:2503 平移保留 artifact、不闪),而是**整屏
  重绘成它自己的历史内容**——公式落到**不同的行**,旧行内容变 → 指纹变 →
  装饰被删;新行要**从头重新检测**。
- 重新检测后要等 worker 渲染(Typst→SVG→raster),`apply_live_worker_completion`
  (:1159-1234)**渲染完成才装回 artifact**;这段空窗里该块显示为源码 = 闪。

**根因判定**:失效是**按行(band_start_row..band_end_row)**的,而 artifact 的
有效性其实只取决于**源码内容 + 布局(DPI/宽度)**。公式换行时,同一份源码的
已渲染像素被丢弃、又从头渲一遍。

**修法方向:源码内容寻址的 live-artifact 缓存**
- 维护一个**有界**缓存 `(source, layout) -> PlaceholderArtifact`(建议小容量
  LRU;`source` 用 `span.source` 或其 hash,`layout` = 现有 `LayoutKey`)。
- 装饰因内容失效被移除时(`invalidate_live_row` / 相关路径),把它的 artifact
  **存入缓存**而非直接丢弃。
- 重新检测产出候选、要调度/装配该块时,**先查缓存**:`(source, layout)` 命中
  则**立即把 artifact 装回(Ready、不显示源码)**,跳过 worker 渲染空窗;未命中
  才照常走 worker。
- **红线(不可让步)**:命中判定必须是**源码字节完全一致 且 layout 完全一致**
  ——只有这样像素才保证等价。**任何**源码/布局不一致**绝不复用**(否则会把旧
  公式的像素贴到变了的内容上=显示错误公式,比闪烁严重得多)。给出命中判定的
  正确性论证。
- 缓存生命周期:layout 变化(DPI/宽度/字号)、`ParkPrimary`/`RestorePrimary`
  切屏、resize epoch 等处**清空或失效**对应条目,避免跨布局误命中。
- **不得回退**既有 `content_fingerprint` 快路径(同内容同行不失效)、
  `preserve_live_after_top_scroll` 的干净顶滚平移、frozen 交接
  (`pending_live_handoffs`)、内容级失效的语义。
- 复杂度:缓存有界,查/插 O(1)~O(log n);不得引入按全历史/全网格的扫描。

**回归(至少)**:
1. 构造**整屏重绘把公式移到新行**的场景(非干净顶滚):断言重新检测时
   **artifact 被复用**(worker 未被再次调用 / 渲染计数不增 / artifact 复用),
   且**不存在显示源码的中间帧**(可用 show_source/装饰状态断言)。
2. **变异验证**:把命中判定改成忽略 source(或忽略 layout),断言复用了**错误
   像素**的情形被一条回归抓红;恢复后通过。证明命中判定是 load-bearing。
3. layout 变化后**不得**命中旧 artifact(DPI/宽度变 → 重新渲染)。
4. 同内容同行的快路径行为不变(不因缓存引入多余渲染或多余复用)。

---

## 部分 ② show-source 时闭合 `$$` 未被纳入(次要)

**现象**(image 212):单行 `$$…$$` 块点按钮显示源码时,**行尾的 `$$` 没被
"选进来"**——源码高亮/可选区停在 `\sqrt{\pi}`,末尾 `$$` 落在高亮之外。

**已知事实**:`detect_block_math`(`crates/bt-detect/src/lib.rs:397-404`)的
`byte_end = leading + trimmed.len()` **确实包含闭合 `$$`**,块的字节 span 是全的。
所以漏的是**下游把 span 字节范围映射到高亮/可选列**时丢了尾部定界符(可能在
render 的 show-source 高亮宽度,或用了渲染公式的像素宽而非源码列宽)。

**任务**:定位 show-source 状态下高亮/命中区的宽度/列映射(render 或 session
侧),修正使其**覆盖 `byte_start..byte_end` 的完整列范围(含闭合 `$$`)**。给出
file:line 根因。

**回归**:单行 `$$X$$`,`show_source=true`,断言高亮/可选区列范围覆盖到块的
`byte_end`(含末尾 `$$`),±0 列误差。

---

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、m1.8/M1.9 全回归、
clippy `-D warnings`、fmt、adapter boundary;vendor/glyphon 零改。门禁数字从
实跑输出抄写并附原始片段;第一遍任何失败如实报告。

## 交付

最终答复:①因果链复核 + 缓存设计与**命中正确性论证** + 变异验证;②show-source
根因 file:line;新增回归清单;门禁实跑数字。停下等审,不提交。把最终答复写进
output 文件。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
