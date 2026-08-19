# 横滚方案 v2:三级台阶(按 Codex 审阅 2026-08-19 修订)

日期 2026-08-19。仓库 D:\Developer\BetterTerminal。本文取代 v1;v1 的四个阻断性错误(无原生 Line wrapping 设置、O(行长) 布局、alt-screen 切宽不可行、坐标泄漏点漏报)按审阅意见修订;二轮对照后又修三处走样(anchor 身份、行列转换混淆、帧时间门)与四处残留,见二轮记录 horizontal-scroll-codex-confirm.md。审阅全文:`horizontal-scroll-codex-review.md`。

## 0. 事实(修订版)

1. DECAWM 关闭时 `wrapline()` 直接返回,越界字符从未被存储(vendor term/mod.rs:1457, 1574, 1616)。
2. `HorizontalOverflowOwner` 与 `MathLayoutOptions::line_wrapping` **只属于公式块**(bt-viewport lib.rs:390/398, bt-term session.rs:6751)——不是终端 no-wrap 的半成品。台阶一实现时**同步改名**这两个公式侧标识,杜绝两个 "line wrapping" 混同。
3. **原生产品今天没有 Terminal▸Line wrapping 设置**;小样那一行"still has no setting behind it"(settings.rs:1482, 3117;DESIGN.md:839)。台阶一/二是**新增**产品能力与设置,不是扩展既有 On/Off。
4. `ViewportFrame::validate_shape` 强制稠密 `columns × rows`(bt-viewport lib.rs:519);帧的稠密校验**保留**,横滚经由类型化坐标而非放松校验。
5. 冻结层确为已拼合逻辑行(bt-transcript lib.rs:760, 839, 1016);但 staging 仍是物理行、live 仍是 vendor 网格;且 resize/所有权边界会 `wrap_split`(lib.rs:876, 895):**正常批次内可按 WRAPLINE 拼合;跨 resize/冻结的不可信边界只保真内容,不保证恢复为同一逻辑行**。
6. 现有布局路径对每条相交逻辑行做全长布局(lib.rs:2081, 2154, 2231, 3719, 3757):**O(行长),不是 O(视口)**。这是台阶一必须新建窗口化投影的原因,不是可以顺路复用的现货。

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

- **协议一个世界**:CPR/CSI 18t 天然说 W_virtual(vendor 直出,勿再加减 x_origin);鼠标上报 `content_column = x_origin + visible_column`(main.rs:6902→7001 现状列原样返回,须改);**CSI 14t 像素查询现被 adapter 丢弃(adapter.rs:229)——裁决:继续不答,或答 `W_virtual × cell_width`;不得答真实视口像素**(会破"一个世界")。
- **渲染侧换算全名单**:光标(bt-render lib.rs:8257 越帧即隐)、IME 候选窗(8183)与 preedit 折行(523)、word/line selection 与搜索高亮(bt-viewport lib.rs:729, 945)、**OSC 8/link 的裁切后 anchor 与命中映射**(投影出的每个可见 cell 携带指向原内容坐标的 anchor——裁除的 cell 不物化也就无 anchor;hover/点击按 ContentColumn 判归属,链接 run 的归属以内容坐标为准而非可见范围)、InlineImage/OSC 133 marker 携带的坐标(adapter.rs:125)——**列走 ContentColumn→ViewportColumn;行走既有的纵向映射,与横轴转换无涉**,两轴不得混用一个转换。
- **输入行投影**:现有 `SemanticInputRegion` 只有锚点/见证,不是带样式的输入 surface(session.rs:2963, 3766, 3824);B→C 只盖 input,prompt 在外。宽模式下的输入体验需要**独立的 prompt+input 逻辑投影与光标模型**——spike 必须先证明这个模型可行,否则宽模式对交互 shell 不可用,只能限定为"输出阅读模式"。
- TUI 在 W_virtual 下的实际体验矩阵(vim/htop/fzf/less)与是否需要"TUI 兼容模式"的判断材料。

## 3. 台阶三:投影策略片 —— 记账

表格横滚/散文折行等按内容分策略;并入"终端作为语义文档"远线(与 Web 预览、AI 动词同席),本方案不展开。

## 4. 裁决与排期

- 路线顺序 一→二→三:**成立**(Codex 总裁决)。
- 台阶一:**有条件批准**,条件即 §1a/1b(通用契约+性能门);排 Windows 落地块之后。
- 台阶二:**只批 spike**;实现方案须持 spike 证据另审。
- 台阶三:记账。
- 本文落定后,docs/DESIGN.md §7.1.6g 改写为**本裁决的记录**(台阶一有条件批准、台阶二 spike、台阶三记账)——**不是现在时**;现在时只在各台阶实现通过验收后随实现单改写。公式侧 `line_wrapping`/`HorizontalOverflowOwner` 改名随台阶一实现单。
