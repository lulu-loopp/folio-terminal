# 横滚方案 v2:三级台阶(按 Codex 审阅 2026-08-19 修订;2026-08-24 增补)

日期 2026-08-19。仓库 D:\Developer\BetterTerminal。本文取代 v1;v1 的四个阻断性错误(无原生 Line wrapping 设置、O(行长) 布局、alt-screen 切宽不可行、坐标泄漏点漏报)按审阅意见修订;二轮对照后又修三处走样(anchor 身份、行列转换混淆、帧时间门)与四处残留,见二轮记录 `codex-confirm.md`。审阅全文:`codex-review.md`。

**2026-08-24:施工前对表(`gap-2026-08-24.md`)报出四条骨架级差距 G1–G4,裁断见 `codex-gap-review.md`。裁断已折进本文 §5「2026-08-24 增补」,并就地改正了 §0/§2 里全部漂移的行号。§5 与正文冲突时以 §5 为准。**

本文所有列区间一律**半开** `[start, end)`:`content_extent` 是最后一个可呈现列的下一列,`captured_columns` 是捕获网格最后一列的下一列,窗口是 `[x_origin, x_origin + viewport_columns)`。全篇不得出现闭区间写法(§5.4)。

## 0. 事实(修订版)

行号按 2026-08-24 的仓库校准过一遍;括号里是当天核对到的位置。

1. DECAWM 关闭时 `wrapline()` 直接返回,越界字符从未被存储(vendor `term/mod.rs:1457` 定义、`:1461` 缺 `LINE_WRAP` 即 `return`;末列宽字符在 `:1595`–`:1597` 的 `else` 分支被丢弃)。**勘误**:v2 原文写的 `1574, 1616` 中,`1616` 应为 `1595`–`1597`。
2. `HorizontalOverflowOwner` 与 `MathLayoutOptions::line_wrapping` **只属于公式块**(bt-viewport `lib.rs:392`/`:414`;`MathLayoutOptions::line_wrapping` 定义在 bt-term `session.rs:108`,生产点 `:7173`)——不是终端 no-wrap 的半成品。台阶一实现时**同步改名**这两个公式侧标识,杜绝两个 "line wrapping" 混同。**勘误**:v2 原文的 `lib.rs:390/398` 与 `session.rs:6751` 已漂移。
3. **原生产品今天没有 Terminal▸Line wrapping 设置**;小样那一行"still has no setting behind it"(`settings.rs:1542`、`:3445`、`:18290`;`docs/DESIGN.md:1353`/`:1383`)。台阶一/二是**新增**产品能力与设置,不是扩展既有 On/Off。**勘误**:v2 原文的 `settings.rs:1482, 3117` 与 `DESIGN.md:839` 已漂移。
4. `ViewportFrame::validate_shape` 强制稠密 `columns × rows`(bt-viewport `lib.rs:521`,同时校验 `layout_key.width_cells == columns`、`rows >= grid_rows`、`cells.len` 与 `cell_anchors.len`);帧的稠密校验**保留**,横滚经由类型化坐标而非放松校验。**勘误**:v2 原文的 `:519` 已漂移。
5. 冻结层确为已拼合逻辑行(`FrozenLine` 在 bt-transcript `lib.rs:735`,`fragments`/`wrap_split` 在 `:742`/`:744`;`TranscriptStore` 在 `:821`);但 staging 仍是物理行(`StagedRow` `:788`)、live 仍是 vendor 网格;且 resize/所有权边界会 `wrap_split`(`capture_wrap_split` `:1048`、`finalize_all_candidates` `:1063`):**正常批次内可按 WRAPLINE 拼合;跨 resize/冻结的不可信边界只保真内容,不保证恢复为同一逻辑行**。**勘误**:v2 原文的 `760, 839, 1016` 与 `876, 895` 已漂移。
6. 现有布局路径对每条相交逻辑行做全长布局(`continuous_frame` `lib.rs:2398`→`:2488` 整条铺完再 `skip/take`;`layout_frozen_line` `:4327`,其中 `inferred_links_in` 全文扫 `:4335`、`line.styles` 逐 grapheme 线性 `find` `:4373`;`frozen_line_end_cell` `:4302`、`frozen_visual_line_count` `:4250`、`frozen_line_screen_column` `:4207` 各自再走一遍全行):**O(行长),不是 O(视口)**,而且是**四趟**而非两趟。这是台阶一必须新建窗口化投影的原因,不是可以顺路复用的现货。**勘误**:v2 原文的 `2081, 2154, 2231, 3719, 3757` 已漂移。

## 1. 台阶一:拼接片 —— **有条件批准**(排 Windows 落地块之后)

**一句话**:新增一种 pane 级显示能力——把软折的逻辑行"展平"为单行并横向取景;终端**源存储**不改、对子程序零谎言;投影索引、帧契约与横向状态是实质新建——**anchor 例外:复用既有 anchor 身份,只为其新增横向投影**,不重建 anchor 体系。

### 1a. 通用横轴契约(台阶二复用的地基,不许做成一次性补丁)

```
HorizontalProjection { content_extent, viewport_columns, x_origin }
```
- 类型化坐标:`ContentColumn` ↔ `ViewportColumn`,转换集中一处;`validate_shape` 仍严格校验 `cells.len == viewport_columns × rows`。
- 台阶一的内容源 = WRAPLINE 逻辑行;台阶二未来的内容源 = W_virtual 网格。同一契约,两种源。

### 1b. 性能验收条件(硬门,缺一不合并)

- 每帧只物化 `viewport_columns + overscan` 的 cells/anchors;
- 每条逻辑行缓存累计 cell-width checkpoint,`x_origin` 二分定位首个可见 grapheme;
- style/显式 OSC 8/隐式 URL 用区间游标,禁止每帧全文扫描;
- O(行长) 只允许发生在冻结、内容变更、首次建索引;连续横滚必须 O(视口);
- 基准与预算(在基准机上量,数字写进测试):十万 grapheme 行、密集 style/OSC 8、CJK/组合字符;**连续横滚的每帧布局预算 ≤ 3ms(中位数,P2-9 resize 钉子的同款量法)、临时分配有上限;首次建索引有明确时间上限**——O(视口) 复杂度声明不替代可验收的帧时间门槛。

### 1c. 交互(需在实现单里裁的三件,方案只立约束)

- 进入横滚的**主动手势**必须存在(Shift+滚轮 / 触控板横向 / 命令入口)——"滚离原点才浮现的条"有启动悖论,不能是唯一入口;
- 横条覆盖底行(光标/输入行所在)时的命中与可见性规则要明文;
- 展平开关的 UI 门(pane ⌄ 菜单 / 新设置行三态 / 命令面板)实现单定,横条视觉复用 P2-9 竖条语言。

### 1d. 明说救不了的

自我按宽裁剪的程序;按坐标作画的 TUI;DECAWM Off 被 vendor 丢掉的字符——台阶二也只是把丢弃边界从视口宽抬到 W_virtual:超过 W_virtual 的仍然丢,已丢的不能恢复。**"行尾巴"补丁不做**(纯增熵:不答 CUP/EL/resize/复制等一整套语义就不是终端状态,答完就是第二套可变宽网格;若将来有日志捕获需求,另立非 VT 语义实验)。

## 2. 台阶二:两层片 —— **只批 discovery/spike,不批实现**

**一句话**:新增"宽阅读模式"(新设置,不是既有开关):ConPTY/子程序世界恒为 W_virtual(候选默认 240,可设),视口是取景框;**alt-screen 期间同样恒持 W_virtual,绝不在 1049h/l 上 resize ConPTY**(切宽方案经审已死:ConPTY 单一尺寸、切换观察滞后、与 resize 自愈/PSReadLine 链竞态;"进 alt 切真宽"降级为未获批准的隔离实验,需 parser pause+屏幕切换状态机+双宽语义+完整生命周期矩阵后再议)。

spike 要回答的清单(全部带机器证据):

- **协议一个世界**:CPR/CSI 18t 天然说 W_virtual(vendor 直出,勿再加减 x_origin);鼠标上报 `content_column = x_origin + visible_column`(`ViewportFrame::live_point_at` bt-viewport `lib.rs:700` 列原样透传 → `live_viewport_mouse_hit` `bt-app/src/main.rs:10398` → `bt-app/src/input.rs:70`/`:103` 原样编码,须改;v2 原文记作 `main.rs:6902→7001`);**CSI 14t 像素查询现被 adapter 丢弃(`bt-term/src/adapter.rs:264` 的 `_ => {}` 吞掉 `TextAreaSizeRequest`,v2 原文记作 `:229`)——裁决:继续不答,或答 `W_virtual × cell_width`;不得答真实视口像素**(会破"一个世界")。
- **渲染侧换算全名单**:光标(bt-render `lib.rs:8399` 越帧即隐,v2 原文 `8257`)、IME 候选窗(`ime_caret` `:533`,v2 原文 `8183`)与 preedit 折行(`compose_preedit` `:540`、`overlay_preedit_cells` `:615`/`:621`,v2 原文 `523`)、word/line selection 与搜索高亮(bt-viewport `lib.rs:1056`/`:1090`,v2 原文 `729, 945`)、**OSC 8/link 的裁切后 anchor 与命中映射**(`hyperlink_at` `:731` → `link_span` `:758` → `link_group_run` `:794` → `rejoined_across_break` `:877` → `run_label` `:928`)(投影出的每个可见 cell 携带指向原内容坐标的 anchor——裁除的 cell 不物化也就无 anchor;hover/点击按 ContentColumn 判归属,链接 run 的归属以内容坐标为准而非可见范围)、InlineImage/OSC 133 marker 携带的坐标(`bt-term/src/adapter.rs:130` 的 `InlineImage { row, column }`,v2 原文 `:125`)——**列走 ContentColumn→ViewportColumn;行走既有的纵向映射,与横轴转换无涉**,两轴不得混用一个转换。**本条整段属台阶二**:光标、IME、preedit 只存在于活网格,而 §5.1 已裁定台阶一不展平活网格,台阶一的换算点名单另见 §5.5。
- **输入行投影**:现有 `SemanticInputRegion` 只有锚点/见证,不是带样式的输入 surface(定义 `bt-term/src/session.rs:1016`,扫描 `:3200`/`:3230`,建立 `:3314`;v2 原文记作 `2963, 3766, 3824`);B→C 只盖 input,prompt 在外。宽模式下的输入体验需要**独立的 prompt+input 逻辑投影与光标模型**——spike 必须先证明这个模型可行,否则宽模式对交互 shell 不可用,只能限定为"输出阅读模式"。
- TUI 在 W_virtual 下的实际体验矩阵(vim/htop/fzf/less)与是否需要"TUI 兼容模式"的判断材料。

## 3. 台阶三:投影策略片 —— 记账

表格横滚/散文折行等按内容分策略;并入"终端作为语义文档"远线(与 Web 预览、AI 动词同席),本方案不展开。

## 4. 裁决与排期

- 路线顺序 一→二→三:**成立**(Codex 总裁决)。
- 台阶一:**有条件批准**,条件即 §1a/1b(通用契约+性能门);排 Windows 落地块之后。
- 台阶二:**只批 spike**;实现方案须持 spike 证据另审。
- 台阶三:记账。
- 本文落定后,docs/DESIGN.md §7.1.6g 改写为**本裁决的记录**(台阶一有条件批准、台阶二 spike、台阶三记账)——**不是现在时**;现在时只在各台阶实现通过验收后随实现单改写。**已完成**:§7.1.6g ② 现整段是本方案的立项裁决记录并明写"均为立项状态而非现在时"(`docs/DESIGN.md:1353`/`:1383`),无需再做。公式侧 `line_wrapping`/`HorizontalOverflowOwner` 改名随台阶一实现单。

## 5. 2026-08-24 增补(裁断折入)

**来源与效力**:`gap-2026-08-24.md` 是施工前对表,`codex-gap-review.md` 是对该报告 §5 四问的裁断。裁断效力最高;本节把四条裁断与它们各自的「必改」逐条落成方案条款。与 §1–§4 正文冲突处以本节为准。

### 5.0 全篇统一的列约定(半开)

一个「列」是终端单元格列。所有列区间半开 `[start, end)`:

```text
window            = [x_origin, x_origin + viewport_columns)
content_extent    = 最后一个可呈现列的下一列
captured_columns  = 捕获网格最后一列的下一列(= 捕获时的网格宽度)
末列触达           = fragment.present_end_column == fragment.captured_columns
```

闭区间等价式 `last_present_column == captured_columns - 1` **只在推导中出现,不进代码、不进文档**。宽字形占两列,其起始列是它的列;零宽簇不推进列。

### 5.1 G1 裁断:**选 A** —— 只展平 history + staging,活网格保持物理行

台阶一的横向坐标系覆盖**三段域**,身份与语义各自明确:

| 域 | 形态 | 是否展平 | 是否进 `content_extent` |
|---|---|---|---|
| history | sealed 逻辑行(`FrozenLine`) | 是,可横向寻址 | 是 |
| staging | 已从活网格捕获、可稳定寻址的 fragments/逻辑行 | 是 | 是 |
| live | 原生物理网格行(`grid_row × grid_col`) | **否** | **否** |

理由不是 B 在数学上不可能,而是 B 会改变当前活屏的纵向对象:显示行数、光标所在显示行、选择端点、链接/鼠标命中、脏区、IME/caret、滚到底部锚点以及 resize 后的重排都要从物理行改投影到逻辑行。这不是「补一个 `grid_row(column)` 公式」,而是一次纵向模型迁移,与 §2 承诺的「本轮纵向语义不变」不相容。

落成条款:

1. **纵向契约一个字不改。** `FrameVisualRow::live_grid_row` 仍是 `Option<u32>` 的一对一网格行,`continuous_frame` 仍要求 `live_rows.len()` 恰等于 PTY 行数,`validate_shape` 仍拒 `rows < grid_rows`,活公式块 band 与遮挡仍按网格行记账。
2. **`grid_row = f(logical_column)` 一类公式一律禁止出现在 v1 的 live 渲染、选择或 hit-test 依据里**;它只能作为「未来 B 案」附注存在(§5.9)。
3. **活带接缝是可测试行为,不是实现偶然。** 同一条软折逻辑行,一部分已进 staging、一部分仍在 live 时:上部按逻辑行展平呈现,下部按物理行呈现;迁移完成后**原子**切换为完整的 staging 逻辑行。一帧之内不得重复或漏掉任何 fragment。这道接缝是 A 案的公开代价,产品面必须知情。
4. **横向 origin 是单一状态。** live 物理行只拥有它自己捕获/当前网格的列域 `[0, grid_columns)`,`x_origin` 超出该域的部分取空白;**不得把 live 单独偷钉在 `x = 0`**——否则同一指针 x 在两段有两套列语义,copy/selection/hit-test 会再次分叉。
5. **到达 live / follow-tail 时是否自动把 horizontal origin 归零,是独立的 UX 裁决**,不得由纵向 clamp 的副作用隐式发生。本轮无此产品裁决,故**默认保持 origin,只按新的 `content_extent` clamp**(§5.3 条款 5)。

**报告措辞的更正**(采纳裁断的勘误 1):差距报告把「backing `grid_row` 可能是 column 的函数」直接称为「横纵两轴耦合」,措辞过强——显示空间里 `(display_row, logical_col)` 仍正交,column 只是在取数时选择某个 backing fragment。真正缺失的是 **live 逻辑行的纵向身份及其全套交互映射**,不是一个函数依赖本身。结论(选 A)不变。

**A 案下 G1 所担心的「两轴耦合换算」不存在**:live 列域与网格列一一对应,`live_point_at` 只需在列上做一次 `+ x_origin` 的单轴换算,行仍走既有纵向映射——§2「列走横轴、行走纵轴」这句在 A 案下自洽。

### 5.2 G2 裁断:每逻辑行一份横向索引,**归投影侧、惰性、可丢弃**

`x_origin` 的定位靠每条可横向寻址的逻辑行自己的横向索引。**索引是行内容的派生物,不是 transcript 的第二份权威数据。**

最小契约:

- **key**:稳定的逻辑行 id + content generation/version(history 用 `TranscriptId` + `SourceGeneration`;staging 用 `StagingId` + generation);
- **value**:按**逻辑终端列**单调排列的 checkpoints;
- **checkpoint**:至少含逻辑列、fragment/token 定位、以及可安全续扫的位置;
- **build**:首次发生非零横向 seek 或重复 seek 时**惰性**构造;短行允许完全不建;**禁止把索引预建到全部 scrollback 行上**;
- **lifetime**:sealed history 行可一直随行;可变 staging 每次 append/replace 使旧 generation 失效;行 eviction 时同步释放;
- **persistence**:**不写入 `bt-transcript`**,载入后按需重建;
- **correctness**:索引缺失、被丢弃或 stride 改变,**只能影响耗时,不能影响所取 cell**。

落成条款:

1. **stride-64 只是可调的派生策略参数**,不得冻结为持久格式或公共契约。「64」按**逻辑终端列**计,不按 UTF-16 code unit、Unicode scalar、UTF-8 字节或 glyph 数计。
2. **checkpoint 只落在自同步边界**:不得落在宽字符的 continuation cell、组合附着序列中间、可变长 token 中间,或依赖前序 attribute/run 状态却未把该状态带入的位置。实际锚点是「不大于目标倍数的最近安全边界」,随后有界前扫。
3. **checkpoint 必须足以恢复解码状态。** 本仓的 `FrozenLine::styles` 是**绝对字节区间**(`StyleSpan { byte_start, byte_end, .. }`,按字节升序),不是增量状态,所以按字节二分即可定位样式,checkpoint 只存字节/列/grapheme 序号即成立。**这一条是本仓的事实,不是通则**:若将来样式改成增量 run,checkpoint 必须同存必要状态,或只锚到完整 fragment/token 起点。
4. 建索引与 seek **全程 checked/saturating 算术**,并以已验证的 record/fragment 边界为上限;损坏记录不得借索引越过已存 payload。
5. **索引另设内存预算与退化路径**:预算不足时丢弃/不建索引,回退到从行首线性扫描。**正确性不得依赖分配成功。**
6. 验收至少覆盖:ASCII、CJK 宽字符跨 64 边界、combining mark、空白/run 跨多个 stride、可变 staging append 后旧索引、eviction、随机 seek 与线性解码逐 cell 对拍。

**采纳裁断的存疑**(勘误 2):`FROZEN_BYTES_PER_LINE = 2048` **不能**推出「每行至多约 32 个 checkpoint」——2048 是序列化字节顶,不是逻辑列顶;压缩空白/run、零宽附着与宽字符都会破坏「字节数≈列数」。**stride-64 的最坏索引规模仍需独立 cap/预算**(即条款 5)。另一面:2048 字节顶让单次线性扫描本身已有严格字节上限,所以**首版先实现正确的逐行 seek API 与线性 fallback,再用基准证明稀疏索引值得常驻**。

### 5.3 G3 裁断:`content_extent` = **实际可呈现的 retained flattened domain**

```text
addressable_lines = retained(history logical lines)
                  ∪ retained(addressable staging logical lines)

content_extent = max(viewport_columns,
                     max(presentable_end_column(line) for line in addressable_lines))

max_horizontal_origin = max(0, content_extent - viewport_columns)
```

`presentable_end_column(line)` 是**通过当前保留 payload 能实际 seek/render/copy 的半开末列**;对不完整宽字符只计到最后一个完整 cell span。它**不是**「曾观察到的原始行宽」,不是捕获宽度,也不是被字节顶截掉之前的推测长度。

落成条款:

1. **明确排除**:live grid(A 案,见 §5.1)、已 eviction 的行、alternate-screen 非持久内容、损坏/拒收记录、截断后不可重建的尾部、仅存在于 raster/layout cache 的范围。
2. `content_extent` **随 retained set 双向变化**:加入/增长 staging 行可增大;最宽行被逐出或变得不可寻址时**必须缩小**,并立即 clamp horizontal origin。**不得用 session high-water 冒充当前 extent。**
3. **实现不得每帧全量扫 scrollback**:维护按行宽计数的 multiset/直方图/有序计数结构(可删除的 max);行加入、staging generation 变化、seal、eviction 时**成对**更新。
4. extent 的权威输入是 transcript/逻辑行层的可呈现宽度,**不是 renderer 测得的像素宽度**;像素横条只做 `columns × cell_width` 的显示换算。
5. **clamp 顺序固定**:先得到新的 viewport columns 与 retained extent → 一次性 clamp origin → 再发布 frame。空域、全空白行与 viewport resize 都走这一条,避免出现越界的一帧。**做法**:横向投影只能经由「给定 extent 与 viewport_columns,构造时即 clamp origin」的构造器产生,不提供事后可写的 origin 字段——顺序由类型保证,不由调用纪律保证。
6. **本仓今天的截断事实**:`FROZEN_BYTES_PER_LINE` 的执行路径是 `enforce_frozen_limits` → `evict_oldest`,**整行淘汰,从不截断一条已保留的行**;所以今天 `presentable_end_column(line)` 就是 `FrozenLine::text` 的完整展平宽度。若将来字节顶改成行内截断,extent 必须只跟随保留下来的 payload。这一条写明,是为了不让「截断尾部」在今天被误读成已存在的形态。

**采纳裁断的存疑**:(a)「所有曾见行的单调 high-water」**否决**——最宽行逐出后会留下永远不可到达的空白尾部;(b)「当前 viewport/render cache 最大宽度」**否决**——滚动或 cache eviction 会改变横条而内容没变;(c) `history + staging + live` 只适用于未来 B 案,A 案下会让横条宣称存在 live 横向列而 live renderer 没有对应的逻辑行服务契约;(d) trimmed trailing blanks:默认只统计**可呈现的最后一个非空/有样式 cell span**,但带背景色、下划线、链接或 selection 相关 metadata 的空格**不是**可随意 trim 的空白——本仓 `normalize` 已按这条裁过(硬行尾只 trim 视觉惰性的填充),extent 直接沿用它的结果。

### 5.4 G4(a) 裁断:`PhysicalFragment` 记 `captured_columns`,但它是 **provenance,不是截断事实**

`PhysicalFragment` 增加**不可变**的 `captured_columns`:该 fragment 被捕获时的物理网格宽度。**resize 后不得用当前 `CellGrid.Columns` 回算。** 这补上的是 §7.1.5k ① 末列门在跨 resize fragment 上**没有稳定参照系**的缺口。

```text
touches_captured_right_edge = fragment.present_end_column == fragment.captured_columns
```

落成条款:

1. 捕获时写入 `captured_columns`;取值须大于 0,fragment 的物理 start/end 均经验证不越界。本仓 `FrozenLine`/`PhysicalFragment` **不进任何持久化格式**(`bt-persist` 不存转录),故无版本兼容项;**若将来持久化,须沿用 `bt-transcript` 的统一整数编码与 record validation,不得另开未封顶的整数域**。合成行(测试夹具、无捕获几何的行)以 `captured_columns == 0` 表示「无 provenance」,末列门在这样的 fragment 上**不开火**——这是边界事实的如实表达,不是回退。
2. **一条逻辑行可含不同捕获宽度的 fragments。末列门逐 fragment 判断**,不得拿首 fragment、末 fragment 或当前 viewport 的宽度覆盖整条逻辑行。
3. **末列门读的是逻辑行的最后一个 fragment**,因为它按定义就是「没有任何东西被重接上去」的那一个;上面每个 fragment 都是这一层已经重接过的软折,门绝不能看见它们(§7.1.5k ① 原文)。`wrap_split` 行的最后一个 fragment 虽 `soft_wrapped == true`,但它的下文**没有**被重接,所以它同样是门要看的那一个。
4. **`edge-suspect`(压住不画)与 `truncated`(断言被截断)是两件事,本轮不合并。** §7.1.5k ① 的门是一个**必要条件的保守压制**:候选触到捕获末列时,「刚好写满的完整引用」与「被切成两半的前半截」在一行之内是同一张图,门不去分辨,只是**不承诺**。它从不主张「后面还有数据」。裁断要求的**三证合取**——(i) 保留/序列化预算确实耗尽或明确拒收后续 payload、(ii) 源端确有未保留的 cell/token/continuation、(iii) 该 fragment 按自己的 `captured_columns` 满足末列条件——约束的是**任何断言「此处发生了截断」的判定**。本仓今天**没有**这样的判定(§5.3 条款 6:整行淘汰,不做行内截断),所以三证是**给未来立的门**;`captured_columns` 只让其中的子谓词 (iii) 有了稳定参照系。
5. **exact-fit 与 autowrap-pending 必须有反例测试**:写到最后一列但没有下一个 printable character 时,**不得仅因触达末列就标 truncated**;有 wrap/continuation 证据时才按门处理。exact-fit 仍然是 `edge-suspect`(条款 4 的分界),但**不是** `truncated`。
6. **`captured_columns` 不得充当逻辑内容宽度**:前者是捕获几何 provenance(服务 §5.4),后者是实际可呈现内容的末列(服务 §5.3),两者分开。

**这一条改了什么(必须知情)**:今天 `frozen_line_end_cell(line, columns)` 按**当前 pane 宽**重放折行来判「是否恰好写满」。改用 `captured_columns` 后,**同一条冻结行在任何 pane 宽下给出同一个门判决**,而今天它会随窗口宽度变来变去——一条在 80 列下写满的行,窗口拉到 100 列后门就哑了。这是修复,不是改名;`layout_frozen_line` 用当前 `columns` 折行的行为不变(读者看到的折行仍随窗口走),变的只是**门问的是哪一把尺子**。

### 5.5 台阶一(A 案)的换算点名单 —— §1a 的显式契约

`viewport_columns == PTY columns` 在台阶一成立,因此几十处读 `frame.columns` 的地方**不需要动**。真正需要 `x_origin` 的「可见列 ↔ 内容事实」换算点,全名单如下:

- `ViewportFrame::live_point_at`(`bt-viewport/src/lib.rs:700`)+ `live_viewport_mouse_hit`(`bt-app/src/main.rs:10398`)+ `bt-app/src/input.rs:70`/`:103` —— 鼠标上报;A 案下是**单轴**换算(§5.1);
- `ViewportFrame::word_selection`(`:1056`)/`line_selection`(`:1090`) —— 今天在帧行两端截断;展平后**词的归属按内容坐标**,不得按可见范围;
- `ViewportFrame::hyperlink_at`(`:731`)/`link_span`(`:758`)/`link_group_run`(`:794`)/`rejoined_across_break`(`:877`)/`run_label`(`:928`)/`hyperlink_cells`(`:976`)/`underline_hyperlink`(`:1004`) —— 见 §5.6 红线;
- `bt-render` 光标绘制(`lib.rs:8399`,越 `frame.columns` 即隐)、IME 候选窗位(`ime_caret` `:533`)、preedit 覆写与折行(`compose_preedit` `:540`、`overlay_preedit_cells` `:615`/`:621`) —— 这些都在**活平面**,A 案下走 §5.1 条款 4:live 只拥有 `[0, grid_columns)`,越域取空白;
- `bt-render::CellMetrics::hit_test_frame`(`:462`)与 `clamped_hit_test_frame`(`:491`) —— 像素→列的唯一 oracle,产出 `ViewportColumn`,之后才 `+ x_origin`;
- `frozen_line_screen_column`(`bt-viewport/src/lib.rs:4207`)/`staged_row_screen_column`(`:4236`) —— 锚点→列,展平后返回 `ContentColumn`。

**不需要动、记在此免得复审当漏项**:`schedule_visible_artifacts`(`bt-term/src/session.rs:6305`)读每行首格锚点判可见历史行,横向开窗后首格是 `x_origin` 处的格子,但解析出的仍是同一个 `TranscriptId`;`frame_matches_grid`(`bt-app/src/main.rs:69365`)与 `presentation_equivalent` 在 `viewport_columns == PTY columns` 下继续成立;CSI 14t / CSI 18t / InlineImage / OSC 133 坐标属台阶二(网格未变宽,子程序世界与今天一致)。

**锚点身份复用成立,一个字不改**:`ContentAnchor::History { id, offset, bias, generation }` 由 `layout_frozen_line` 逐 grapheme 盖章、`pad_frozen_row` 给填充格盖行尾章;横向取景只改「哪些 grapheme 被物化」。

### 5.6 §7.1.5k/§7.1.5h 红线 —— 不许绕

**链接与路径推断永远在整条逻辑行上跑,绝不为了帧预算改到窗口上跑。**

立论就在 `implicit_hyperlinks` 的 doc 里:只读一个可见行会把 `…> https://support.cla` 解析成一个**别人拥有的合法主机**,点击去了读者从没打过的地方——2026-08-20 才修掉。横向窗口是**同一刀**。

落成条款:

1. 推断(裸 URL、裸路径、OSC 8 重接)输入永远是整条逻辑行的文本;首次建索引时 O(行长) 是**允许**的,连续横滚每帧不得重跑;
2. 窗口化物化只决定「哪些 cell 被造出来」,**不决定链接的身份、目标与 run 的两端**;run 的归属以 `ContentColumn` 判,不以可见范围判;
3. `PrintedPathPass` 的探询副作用(`bt-viewport/src/lib.rs:1482`–`:1494`,每帧 256 个上限)必须跟着「整行一次」走,否则读者横滚到的路径要等一次探询往返才亮;
4. `layout_frozen_line` 的 `Option<&mut PrintedPathPass>` 里 `None` 表示「只要行数」;展平后行数恒为 1,这条快路径的存在理由消失,`frozen_visual_line_count` 整个退化——**是化简,不是障碍**。

### 5.7 台阶一的施工分级与量法

台阶一按三级施工,每级独立可验收;**级与级之间不得半接线**:

- **一级(地基)**:横轴类型与 `HorizontalProjection` 构造器(§5.0/§5.3 条款 5)、逐行横向索引与 seek API(§5.2)、单行 flatten-and-window 物化器、`FlattenedExtent` 可删除 max(§5.3)、`captured_columns` provenance 与末列门换尺(§5.4)、样式区间游标、公式侧改名。**不切换任何消费者**,故无用户可见面,以测试与性能数字为证。
- **二级(接线)**:`layout_frozen_line` 走展平、`ViewportFrame` 携带横向投影、§5.5 名单逐点转换、`LayoutKey` 增展平成员(`bt-doc/src/versions.rs:111`,并补 `every_layout_key_field_has_an_independent_cache_identity`)与 `settings.json` 版本推进。
- **三级(产品面)**:§1c 的三件(主动手势、横条覆盖底行的命中与可见性、展平开关的 UI 门)。

**性能门量法**(§1b 的硬门,按 P2-9 resize 钉子的同款量法):在基准机上量,数字写进测试与实现单——

- 基准数据:十万 grapheme 行、密集 style/OSC 8 行、CJK/组合字符行;
- **连续横滚每帧布局中位数 ≤ 3 ms**;一级只能量到「seek + 物化 `viewport_columns + overscan`」这一段,二级接线后须在真实帧路径上复量;
- **首次建索引有明确时间上限**,并与「无索引线性 fallback」同表对照——若稀疏索引在本仓 2048 字节顶下证明不值得常驻,按 §5.2 存疑处理;
- 临时分配有上限;O(行长) 只允许发生在冻结、内容变更、首次建索引。

### 5.8 采纳的报告勘误

1. G1 因果表述过界 —— 已在 §5.1 更正;
2. G2 混用字节顶与列顶 —— 已在 §5.2 存疑段更正;
3. G2 索引与持久格式未隔离 —— §5.2「persistence:不写入 `bt-transcript`」;
4. G3 未区分 observed / retained / presentable 三种宽度 —— §5.3 已分开并给出本仓今天的事实(条款 6);
5. G3 缺 eviction 的减法路径 —— §5.3 条款 2/3(可删除 max + 原子 clamp);
6. G4(a) 把必要条件写成充分条件会误报 exact-fit —— §5.4 条款 4/5;
7. G1 与 G3 必须联动 —— §5.1 表格与 §5.3 条款 1 同时写明 live 不入 extent。

### 5.9 明确排除 / 挂在未来的

- **B 案(同时展平活带)**:A 是**一期边界**,不是永久证明 B 不可做。将来若做 B,必须**另立**纵向身份、光标、选择、命中测试与 resize 契约,不得作为本方案的「实现细节」补入。
- **staging 的开放尾行**:本方案按「**可稳定寻址的 staging 才入域**」裁断。开放尾行(仍会被原位改写的 `live_tail`)若要入域,必须先补 generation 事件,否则 §5.2 的索引失效与 §5.3 的宽度减法都无法实现。
- §1d 三条(自我裁剪的程序、按坐标作画的 TUI、DECAWM Off 丢字)不变;「行尾巴」补丁仍不做。
