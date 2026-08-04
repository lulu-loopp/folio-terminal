# M2 预览内容矩阵与打开动词(用户裁决 2026-07-28)

> 本文是 M2 规格债的第一块料:内联/预览两个形态的内容准入表 + 统一打开动词梯度。
> 上位模型:DESIGN.md §7「内容×形态」(三法则、七级形态、peek→pin→dock 晋升路径)。
> 本文所有"政策位"标注遵循法则③:限制是政策不是结构,可翻案。

## 1. 两道门,两个标准

- **内联锚定(转录输出流里)= 窄门**:准入标准是"一眼可读、不打断阅读流、不产生持续动画"。
  持续动画会破坏事件驱动空闲 0 帧的架构承诺(呈现层无固定 tick,见 bt-app 事件循环)。
- **预览 pane = 宽门**:一切"打开看"的正宫,固定右席(§7.1 已裁)。

## 2. 内联准入表

> **2026-08-03 修订(用户裁决,见 §6.1 补遗):图片全族退出内联,两屏皆然。**
> 下表「直接显示 / 静态光栅显示 / 首帧静态」三行**当前一律读作「不进内联,hover peek」**;
> 单击进预览 pane 随 M2 图像族落地。表原文保留,因为它就是政策位翻案后要回到的那张表。
> **公式/数学 band 不在本次裁决范围内,逐字不变。**

| 格式 | 内联给什么 | 备注 |
|---|---|---|
| PNG/JPEG/WebP/BMP | ~~直接显示~~ → **不进内联,hover peek** | 图片片 v1(协议先行,路径检测跟上);2026-08-03 退出内联 |
| SVG | ~~静态光栅显示~~ → **不进内联,hover peek** | resvg 基建现成(bt-math 管线);同上 |
| GIF/APNG/动图 WebP | ~~**首帧静态 + 可播放角标**~~ → **不进内联,hover peek** | 动画只在预览播;「hover 播放」= 政策位,默认关 |
| 视频/PDF/HTML/音频/其它 | 不进内联,显示为链接/图标 | 视频=只读媒体缓冲已裁(§7 压测补遗) |

## 3. 预览 pane 四族(v1 优先级按 agent CLI 工作流排)

1. **文本族(v1 核心)**:代码(语法高亮)、Markdown(渲染态,公式走 bt-math)、纯文本、
   JSON/YAML/TOML。CSV 表格视图后置;diff 复合视图=界外记档(双缓冲一视图破"一座位
   一内容",DESIGN §7 压测补遗已记)。
2. **图像族(v1 核心)**:内联全部格式 + GIF 在此播放(循环/暂停,播放进度=内容状态随行,
   与视频同法则)+ SVG 矢量缩放。
3. **媒体族(M2 后段)**:视频/音频=只读媒体缓冲(§7 压测补遗原文照做);直播流=会话族。
4. **网页族(M2 后段,分三档)**:
   - **本地一等档**:`file://` 本地 html + `http://localhost:*` / `http://127.0.0.1:*`
     ——单击直接进预览 pane 渲染(WebView2 导航白名单默认即此二类,DESIGN §7 已裁;
     开发场景 dev-server 预览是主用例)。本地静态 html=缓冲(路径身份);活 localhost
     服务=web 会话(会话族,受三禁)。
   - **远程保守档**:单击默认系统浏览器;pane 内导航跳出白名单需显式确认(已裁);
     "静态快照/reader 模式"缓冲预览=后续 opt-in 政策位。
   - **PDF 搭车档(待裁)**:WebView2 原生渲 PDF,可搭便车;本地文件进 WebView 的
     安全边界需单独裁决,记 M2 规格债。
5. **未知二进制**:诚实"无预览"+文件信息卡(大小/类型/修改时间)。十六进制视图有需求再说。

## 4. 打开动词梯度(= peek→pin→dock 晋升路径的具象化)

| 手势 | 形态 | 语义 |
|---|---|---|
| hover(~300-500ms 延迟) | 瞬态面(peek) | 轻量浮层,移开/Esc 即散 |
| 单击(click-no-drag) | pane(pin/dock) | 进预览 pane,留下来 |
| Ctrl+单击 | 离开本产品 | 系统默认程序/浏览器 |

四条实现裁决:

1. **单击判定 = click-no-drag**:一拖即选区(浏览器同款);TUI 鼠标捕获开启时转发优先,
   链接动作用 Shift 绕过(M1.7 既有惯例)。**从终端点开预览不抢键盘焦点**——pane 打开,
   输入焦点留在终端,显式点入才转移。
2. **URL 的 hover 零网络请求**:默认只显示"真实目标+域名"信息卡(OSC 8 显示文本≠目标
   的钓鱼面防线)。自动抓取=鼠标扫过哪访问哪,泄露访问信号+预取恶意页面,不做。
   本地文件/图片 hover 可自由 peek(无隐私面)。网页真 peek=显式手势或 opt-in 政策位。
3. **hover 延迟只作用于浮层/提示条(~300ms)**,防链接密集输出的浮层乱闪;**下划线
   affordance 即时跟随指针**(2026-07-30 手感修订:即时性告知"这是链接",延迟保护的
   是浮层不是下划线)。GIF 在 peek 浮层内可动(瞬态生命周期有界,与内联首帧规则不冲突)。
   **2026-08-04 起这套词汇跨内容类型统一**:一条**经 worker 验证过**的图片引用穿的是同一对
   flag(静止点线、指针下实线),分野只在 hover 揭示什么——URL 是信息条,图片是缩略图。详见 §6.1.2。
4. **分期**:hover 信息卡版可先行(不依赖 pane 树);单击进 pane 等 M2 预览 pane;
   Ctrl+单击=系统打开在超链接片 v1 即有。URL 单击在 M2 网页预览落地前无事可做,
   落地后自动升级进 pane——政策位升级,零返工。

## 5. 与既有裁决的衔接

- 超链接片四裁决(2026-07-28):裸 URL 只认 http/https(含 localhost:port);Ctrl+点开;
  文件路径不进链接片(归图片片/缓冲动词);http/https 直开不弹窗+悬停显示真实目标,
  其它 scheme 默认拦截。
- 内联图片=带路径身份的工件,与预览同路径图片同一份缓冲(§7 原文);点击动词 M2 前
  临时=系统看图器,M2 后升级=入共享池开预览(政策位)。
- 视频不搭图片片顺风车:图片管线为静态工件设计(LRU 纹理缓存按内容 key),流式纹理
  路径是独立工程决定,归媒体族。

## 6. 终端文本图片路径 v1 边界(2026-07-28)

- 只检测盘符根绝对路径(`C:\...`及`C:/...`);UNC 与相对路径拒绝(相对路径部分见
  §6.3 的 2026-08-03 修订:有 OSC 7 权威工作目录时 `./`、`../` 与**带分隔符的裸引用**
  (`dir/x.png`)进检测,单段裸名与没有 OSC 7 的会话仍全拒)。未引号路径止于
  空白或`]`,双引号路径允许空格且必须闭合。扩展名只准 png/jpg/jpeg/webp/gif/svg
  (svg 落地 2026-08-02:按 §2 静态光栅,decode worker 经 bt-math/resvg 以固有尺寸
  一用户单位=一像素光栅化;usvg 解析本身即有效性门,不加嗅探)。
- 事件线程只做词法候选扫描;存在性、普通文件、8 MiB 上限、真实图片格式与解码全部
  在 decoration worker。任一门失败只留下原终端文本,不显示错误占位或视觉噪音。
- 路径文本始终保留为可选/可复制引用;图片作为其下方追加 band。相同路径的每次出现
  都有独立 occurrence,worker 按规范化路径只读盘一次并按内容哈希复用纹理身份。
- 首次成功读取后不监视磁盘变化,也不因文件被替换而刷新;后续由 M2 共享缓冲/监视器
  接管。Ctrl+单击仅对成功解码记录开放,经并列的本地文件审计桥交给系统默认看图器。
- `MathLayoutOptions::detect_image_paths` 是 session 政策位。bt-app 显式默认开;
  headless oracle/录制回放默认关,只有设置 `BT_PROBE_IMAGE_PATHS` 才开启,保证旧录制
  不因回放机器的磁盘状态而失去封闭性。
- **光标行抑制(用户裁决 2026-07-28)**:发布帧的可见光标所在逻辑行不新建任何内联
  decoration band(本节路径图片与公式同规则)。逻辑行按网格 WRAPLINE 因果关系向上、
  向下合并全部软换行物理行;所以路径在上半段、光标在下半段也必须抑制。primary 与
  alternate 一致。光标离开后,候选仍须照常通过既有 damage-derived 稳定窗口才可创建。
  DECTCEM 隐藏期间沿用该 screen 最近一次可见光标的 `(logical_start, logical_end)`
  **粘滞记忆**,防止 PSReadLine 的“藏光标→逐字符重画→亮光标”爆发被 PTY chunk
  边界从隐藏窗口劈开后绕过 gate。若该有效逻辑行(可见或粘滞)的全部物理行均为空,
  且 adapter 逐字节 VT 观察器证明光标最近一次物理行落位来自 CUP/HVP(`CSI H/f`)
  绝对定位,gate 同时扩展到其上方最近的非空逻辑行;中间空行跳过,命中的非空行按
  WRAPLINE 整体合并。这覆盖 PSReadLine 把同一多行编辑缓冲逐行 CUP 绘制、最后把
  光标显式放在空续行的形态。LF/VT/FF/CR 流推进、可打印字符自动换行及其它行移动
  会把落位事实改为非 CUP/HVP,所以 `$$…$$\n` 流式输出后光标自然停在下一空行时,
  刚完成的公式照常进入渲染,不得等下一次输出。记忆(含落位种类)只由确定事件失效:
  网格滚动(行号意义变化;含无 transcript removal payload 的显式 CSI S/T)、ED2
  全清、clear/home 或 DEC 2026 重印边界、resize/reset/DECCOLM、primary/alternate
  切换;光标在另一逻辑行重新可见则用新范围与落位种类替换旧值。禁用定时失效。
  全屏 TUI 进入/重印时本就清屏或切屏,因此长期 DECTCEM off 时没有陈旧输入行记忆,
  原“无输入行则不抑制”豁免保持。
  **粘滞记忆存的是两个物理行号,读它的时候必须按当前网格重新 WRAPLINE 合并一次
  (2026-08-04 修订,`merged_logical_line`)**:记忆是上次光标可见时拍的,而 PSReadLine 正是
  在“藏光标→重画→亮光标”这一阵里把输入行**多撑出几行**的——同一行多打一个字符,一行的输入
  就变成两行。不重新合并就等于拿旧网格的行号回答“哪些物理行是一条逻辑行”,新长出来的续行
  逃出 gate,本节“路径在上半段、光标在下半段也必须抑制”那句在**输入行自己变宽的那一刻**失效。
  记忆本身的存废规则一字未改,改的只是怎么把它读成逻辑行。
- 该规则**只 gate 新建**,不参与已有 occurrence 的匹配、保持、重锚、失效或投影。
  已渲染 band 即使在 alternate 应用重绘时被可见光标扫过也保持,不得塌陷或闪回源码。
  判据只读发布帧时刻的 cursor+WRAPLINE 网格状态及上述粘滞记忆,不以键盘/粘贴事件
  或另设定时器猜测。
- **hover peek 升级位**:路径文本在抑制期间仍是原生可选/可复制文本;未来可把“成功
  解码路径的 hover 临时看图”升级为 §4 的本地无网络 peek,但它是独立呈现政策位,
  不得绕过光标行的新建 gate、提前落 band,也不得改变单击进入共享预览池的晋升路径。

### 6.1 alternate 屏不落图片 band(用户裁决 2026-08-02,修订本节)

> **【已被 §6.1.1 取代,2026-08-03】** 本小节整段仍为有效记录,但其「primary 一切照旧」
> 的结论已被下一小节翻案:图片 band 在**两屏**都退役。本小节留档的理由不是历史癖好——
> 它是那条结构性论证的第一次成立,而 §6.1.1 就是同一条论证走完剩下半步。
> 本小节里凡说「alternate 不落 band / primary 照旧」处,今日一律读作「两屏都不落 band」;
> 其余(切屏 live 记录作废、peek 是内联准入的补集、OSC 1337 占位文本属文本基底)逐字仍然有效。

- **裁决**:alternate(副屏)**完全不渲染内联图片 band**;该屏的图片只走 hover peek。
  理由:全屏 TUI 自己拥有并按自己的节奏重绘那块画布,把 band 锚在它的行上必然产生
  「浮动/遮挡/滚动别扭」一族问题。primary(我们自己拥有的转录流)的内联图片**一切照旧**。
  **公式/数学 band 不在本裁决范围内**:alternate 的公式行为原样不变。
- **与本节旧文的关系**:上文「光标行抑制…primary 与 alternate 一致」「已渲染 band 即使在
  alternate 应用重绘时被可见光标扫过也保持」等句仍原样有效,但对图片而言在 alternate 已
  不可达(那里根本不建 band),故只对公式生效;primary 的图片仍逐字照旧。
- **实现即政策**:唯一判据 `inline_image_bands_admitted(screen)`(`crates/bt-term/src/session.rs`),
  两个**新建**接缝各调用一次——OSC 1337 的 `register_inline_image` 与路径检测的
  `reconcile_live_image_paths`。**只 gate 新建**:切屏本身不追认作废 primary 的既有记录,
  history/staging 记录分毫不动(live 平面另按下文切屏政策处理)。
- **OSC 1337 在 alternate**:协议照常解析(payload 被消费、流不失步、不报协议错),
  但不产生记录、不入 worker 队列、不落 band。adapter 仍在网格里写 `[image]` 文本占位
  (那是文本基底,与本节的 band 政策不同层),故副屏上一次 OSC 1337 只留该占位文本。
- **切屏时的 live 记录**:进/出 alternate 都作废**live 平面**上的图片记录,与公式侧
  `invalidate_all_live_decorations` 同策(见 `retire_live_inline_images`)。理由是结构性的:
  live 记录锚在被换走的那块网格上,`RestorePrimary` 还会 bump `grid_generation`,留着的
  记录既不可能再匹配、也不可能再投影或作废——只会是一条永久不可见却占着解码位图的
  occurrence。回到 primary 后,路径经既有稳定窗口重新登记、band 照常回来(与 live 公式
  重新检测同形)。history/staging 记录不是 live,分毫不动:TUI 上方的转录该有的 band 都在。
- **peek 是内联准入的补集(2026-07-31 裁决)因此零成本承接**:`local_image_path_probe_at`
  只在「有未失败记录覆盖该锚」时拒答,副屏既然一条记录都不建,探针就必然作答,
  hover peek 层**一行都不用改**。钉死于 `alternate_screen_path_text_creates_no_record_and_the_peek_answers_instead`。
- **回放/录制影响(2026-08-02 实测)**:默认回放(未设 `BT_PROBE_IMAGE_PATHS`)不检测路径,
  且唯一带 OSC 1337 的录制(`image-accept`)全程在 primary,故**逐字节不变**——54 份
  `.tmp-repaint-capture/*.vt` release 前后对拍 stdout+stderr 全同。设 `BT_PROBE_IMAGE_PATHS=1`
  时,纯 primary 的图片录制(`path-accept` / `image-accept` / `hover-peek-feel` /
  `osc133-accept2` / `pixel-scroll-feel`)仍逐字节全同;在副屏打印过图片路径的五份按裁决
  **预期少掉副屏那些 band**(带 band 的帧数 before→after:`cc-image-repro` 115→36、
  `hover-peek-complement` 329→80、`relief-chip-feel` 253→148、`svg-slice-feel` 79→2、
  `texture-residency-feel` 379→91;残余即同一录制里 primary 段的 band,证明只副屏被裁),
  这是裁决本身,不是回归。

### 6.1.1 图片 band 两屏全退役,只留 hover peek(用户裁决 2026-08-03,取代 §6.1 的结论)

- **裁决**:内联图片 band 在 **primary 与 alternate 两屏都不再存在**。图片**处处只走 hover peek**;
  单击晋升进预览 pane 随 **M2 图像族**一并落地。**公式/数学 band 明确不在本裁决范围内,分毫未动。**
- **理由(结构性,不是手感)**:band 是往一块**shell 按绝对坐标寻址**的画布里**注入行**。这与
  PSReadLine 以及一切用绝对 CUP 重绘的应用**结构性交战**——不是某个 bug,是两套坐标系同时对同
  一块网格声明所有权。§6.1 的副屏裁决(b3769b7)已经证明:band 一死,那一整族「浮动/遮挡/滚动
  别扭」的毛病跟着一起死。primary 只是同一条病灶显形得慢:它同样是 shell 在按绝对坐标写字。
  于是本次把剩下半步走完。
- **实现即政策(法则③,可翻案)**:
  - 唯一政策位 `INLINE_IMAGE_BANDS: bool = false`(`crates/bt-term/src/session.rs`)。每个 session
    构造时把它抄进 `DualPlaneSession::inline_image_bands`;生产代码从不写这个字段。
  - `inline_image_bands_admitted(bands, screen)` **保留 screen 参数**:翻案时 band 只回到 primary,
    §6.1 给出的理由原样成立。
  - **创建侧**三个接缝:`reconcile_live_image_paths`(live 路径)、`detect_frozen_image_paths`
    (冻结转录行——本次新加的门,§6.1 时代它不存在,因为那时 primary 还建记录)、
    `register_inline_image`(OSC 1337,见下)。路径/`file://`/相对引用文本因此**任何平面都不建记录**。
    **(本条已被 §6.1.2 于 2026-08-04 取代:记录回来了,回来的身份是「验证载体」;政策位管的仍然
    只是 band。)**
  - **投影侧**一个接缝:`projected_inline_image` 开头一行。frozen / live / history-path 三条投影
    全部经它,所以这是「图片记录不再是 band」的**唯一**那一行。
  - **调度侧**一个接缝:`take_decoration_worker_task` 不再调用 `request_inline_image_displays`。
    display 光栅的存在意义就是被 band 上传;peek 自己按 `bt_render::peek_thumbnail_extent` 定框、
    自己 `PeekScale`,与记录无关。几何/重采样/纹理身份**机器一行未删**,只是没人再问它。
- **peek 是内联准入的补集,所以这次同样零成本**:`local_image_path_probe_at` 只在「有未失败记录
  覆盖该锚」时拒答。**一条记录都不建 ⇒ 覆盖恒假 ⇒ 探针在一切可准入跨度上必然作答**。这条组合性
  钉死于 `path_text_creates_no_record_anywhere_and_the_peek_answers_everywhere`(live 与 history
  两个平面各一遍)。peek 层**一行都没改**。
  **(§6.1.2 修订:记录回来之后,补集的说法从「一条都不建」换成「一条都不呈现」——
  `inline_image_record_covers` 的第一问仍是政策位,答案因此一样;pin 更名为
  `a_verified_reference_wears_the_resting_underline_in_every_plane_and_bands_in_none`。)**
- **OSC 1337:记录活下来,但它不再是 band,而是「解码载体」**。这是本次唯一需要新机器的地方,
  理由是**不对称的**:打印出来的路径,peek 层随时可以从行文本里重新读出来;OSC 1337 的图**只在
  流里出现过一次**,退役 band 之后如果不锚在占位符上,它就**一点呈现都没有**了。所以:
  - 协议照常解析、payload 照常进 worker、解码照常按内容 key 缓存;`projected_inline_image` 拒绝投影。
  - §6.1 的两道门(副屏、shell 输入区)是**对 band 创建**的门,对一个永不投影的载体无话可说,
    故政策位关时不跑;翻案即原样恢复。
  - **占位符即锚点**:adapter 写 `[image]` 时把**实际写了几列**(右边缘会截断)一路报到 session
    (`AdapterEvent::InlineImage::placeholder_columns`),记录存下这个跨度。
    `DualPlaneSession::inline_image_payload_peek_at(anchor)` 在该跨度上返回解码的**内容 key**;
    bt-app 在 `DecorationWorkerCompletion::InlineImage` 经过时把解码顺手记进 `peek_cache`
    (`remember_stream_payload_for_peek`)。解码未落地/失败则沉默,与打不开的文件同一条诚实降级。
  - app 侧唯一的结构改动是把 peek 的主语从 `PathBuf` 抬成 `PeekSubject { key, path: Option<PathBuf> }`
    ——两种形状只差一件事:**缓存未命中时有没有东西可读**。命名文件有(worker 读盘),流内 payload
    没有(它的字节早已过去)。`show_or_request_peek` 因此仍是一个函数。
  - 切屏作废 live 记录的老规矩对载体同样成立且**正是对的**:副屏被换走时,它承载的那个 `[image]`
    占位符也跟着消失,没有任何东西还能被 hover。
  - 钉死于 `osc_1337_decodes_on_both_screens_and_bands_on_neither`、
    `an_osc_1337_payload_peeks_on_its_placeholder_and_nowhere_else`、
    `a_stream_payload_is_a_peek_subject_with_nothing_to_read`(bt-app)。
- **退役代码一行未删(法则③ 可逆性)**:几何、display 重采样调度、band 投影、创建门全部**门控**
  而非删除。为了让门后面的机器不在政策位关着的这段时间里**腐烂**,`DualPlaneSession` 上有一个
  仅测试可见的 `restore_retired_image_bands()`:**一次裁决用一个字符做的翻案,pin 在运行期做**。
  所有 band 机制 pin(几何/重采样收敛/纹理预算/锚迁移/视口底部让位/输入区抑制门)都经它跑,
  因此翻案回来时它们是**被验证过**的,不是被一句注释承诺过的。
  组合钉死于 `flipping_the_policy_bit_back_on_restores_the_whole_band_pipeline`:同样的字节,
  同一份 fixture,只差这一位——关着是 `(0 记录, 0 band)`,开着是 `(1 记录, 1 band)`。
  **(§6.1.2 修订:关侧现在是 `(1 记录, 0 band)`——记录两侧都在,这一位拥有的是 band。)**
- **对照组**:`formulas_still_band_while_images_do_not`——同一帧里公式照常成 band,图片不成。
- **补集不变量两侧都钉住**:`inline_image_record_covers` 的第一问就是政策位——「覆盖」的含义是
  **在流里呈现**,一条投影不出去的记录什么也没呈现,所以补集恒真、探针必答。翻案回来时旧的拒答
  全部回来,**包括本文件此前不可能有的那一条**:OSC 1337 的 band 在屏上时,同一个占位符不得再浮
  一次。钉死于 `an_osc_1337_band_and_its_placeholder_peek_are_never_both_admitted`(政策位两侧各一句)。
- **Ctrl+单击的连带后果(诚实记账,非裁决内容)**:「Ctrl+单击 → 系统默认看图器」(§4 第三档、
  §6 末条)一向要求 **worker 真的打开并解码过那个文件**,把它挂在装饰记录上。主屏不再建记录之后,
  这份验证改由 **peek 自己那份**承担——同一个 worker、同一个解码器、同一道准入门,记在
  `peek_cache` 里(`local_image_path_hit`)。**代价说清楚:必须先 hover 过一次,Ctrl+单击才开得动。**
  pending / 失败条目照旧完全惰性;政策位翻案后记录重新第一个作答,老行为原样回来。
  **(本条代价已于 2026-08-04 由 §6.1.2 取消:验证载体让记录在政策位关着时也第一个作答。)**
  单击进预览 pane 仍按裁决随 M2 图像族落地。
- **回放足迹(2026-08-03 实测,62 份 `.tmp-repaint-capture/*.vt` × 2 模式,release oracle 前后对拍
  stdout+stderr)**:
  - **默认模式(未设 `BT_PROBE_IMAGE_PATHS`):0/62 有差异,逐字节全同。** 默认回放不检测路径;
    唯一带 OSC 1337 的录制(`image-accept`)的 band 只在 `BT_PROBE_IMAGE_PATHS` 下成形,
    因为该开关同时是这条管线在 oracle 里的总闸。
  - **`BT_PROBE_IMAGE_PATHS=1`:23/62 有差异,另 39 份逐字节全同。** 全部差异都是
    per-frame `rendered=[…]` 里图片 band 的消失,**没有一条差异出现在 audit/summary 行**:
    23 份的 `ISOLATION_GAP` / `OWNERSHIP_LEDGER` / `HELD_UNBACKED` / `OCCLUSION_AUDIT` /
    `CLIP_AUDIT` / `LAYOUT_AUDIT` 前后**逐字节相同**。这是裁决本身的足迹,不是回归。
  - **总量**:带图片 band 的帧 **2802 → 0**;带公式 band 的帧 **1541 → 1541**(语料里只有
    `pixel-scroll-feel` 同时载有公式 band,它一帧未失——这就是「公式不在本裁决范围内」的实测对照)。
  - 逐份(帧数 before→after,`带 band 帧 / 其中图片 band 帧`):
    `alt-peek-only-feel` 191→0 / 191→0、`band-stuck-trace` 38→0 / 38→0、`cc-image-repro` 36→0 / 36→0、
    `cursor-accept` 13→0 / 13→0、`daily-b35ce24` 965→0 / 965→0、`final-input-accept` 18→0 / 18→0、
    `hover-peek-complement` 80→0 / 80→0、`hover-peek-feel-2` 22→0 / 22→0、`hover-peek-feel` 2→0 / 2→0、
    `image-accept` 259→0 / 259→0(全部是 OSC 1337 的 `[image]` band)、`osc133-accept` 34→0 / 34→0、
    `osc133-accept2` 211→0 / 211→0、`osc133-accept3` 285→0 / 285→0、`osc7-relative-feel` 47→0 / 47→0、
    `paste-accept` 13→0 / 13→0、`paste-round3` 12→0 / 12→0、`path-accept` 13→0 / 13→0、
    **`pixel-scroll-feel` 1622→1541 / 81→0**(余下 1541 全是公式 band)、`relief-chip-feel` 148→0 / 148→0、
    `seat-fix2-feel` 7→0 / 7→0、`svg-slice-feel` 226→0 / 226→0、`texture-residency-feel` 91→0 / 91→0、
    `zero-resize-restore` 10→0 / 10→0。
  - 对照 §6.1 当日(2026-08-02)只裁副屏时的表:那次 `cc-image-repro` 是 115→36、
    `hover-peek-complement` 329→80、`relief-chip-feel` 253→148、`texture-residency-feel` 379→91,
    **残余即同一录制里 primary 段的 band**。今天正是把那些残余一并裁掉,所以它们各自的旧「after」
    就是今天的「before」——两次裁决在同一批数字上首尾相接。

### 6.1.2 可 peek 的引用要有静止 affordance,且**先验证后承诺**(用户裁决 2026-08-04)

band 退役之后,图片只剩 hover 一条通路——而 hover 的前提是**先知道这里可以 hover**。§6.1.1 把
呈现拿掉了,却没有给「这里有张图」留下任何静止的告知,于是一整屏路径文本和普通文本长得一模一样。
本节补的就是这一半。

- **裁决(A)affordance 用超链接的同一套词汇**:一条可 peek 的图片引用,**静止时点线下划线,
  指针下变实线**——与 142ca48 给 OSC 8 落的那套**一模一样**(跨格合并、物理像素对齐、hover 升级)。
  不是「像」,是**同一对 flag、同一条渲染路径**。理由在 §4 已经写好:动词梯度**本来就跨内容类型统一**,
  静止态两者必须无法区分;**分野出现在 hover 揭示什么**——URL 揭示一条真实目标信息条,图片揭示一张
  缩略图。
- **裁决(B)先验证后承诺**:只有 **worker 真的验证过**的引用才配这条下划线——文件存在、是文件、
  尺寸在预算内、是我们解得开的格式、**并且真的解开了**。看起来像路径但不存在的字符串,**保持纯文本**,
  peek 也照旧沉默(今天就有的诚实行为)。下划线是一句承诺,**这个终端不承诺自己交付不了的东西**。
  - **顺带的好处,裁决点名要留住**:验证顺手把解码烘热了,所以那张图的 hover peek 是**从缓存直接出**,
    不再等一次解码。
- **实现即扩展,不是新机器**:OSC 1337 早就有「记录活着当解码载体,但永不投影 band」这个形状
  (§6.1.1,828fc9a)。本次只是把**路径 / `file://` / 相对引用**也接进同一个形状:
  - **注册 ≠ 建 band**。`reconcile_live_image_paths` 与 `detect_frozen_image_paths` 不再被政策位
    整段挡住;挡住的是**那些为了「别把行注进某处」而存在的门**——副屏、shell 输入区、光标所在逻辑行——
    它们现在只在 `inline_image_bands` 为真时发问(`band_gates`)。翻案时三道门原样回来。
  - **投影侧多了第二道拒绝,且两侧都成立**:`file://` URI 是**对文件的引用**,不是文件在流里的名字,
    它从来不长 band,不会因为拿到了记录就开始长。记录里存 `ImageReferenceShape::{Native, Uri}`,
    `projected_inline_image` 见 `Uri` 直接拒。
  - **补集因此仍然诚实**:`inline_image_record_covers` 只认**真的会在流里呈现**的记录(政策位为真
    且形状是 `Native`),所以一条 URI 记录不会因为存在就把自己那块地方的 peek 掐掉。
  - **覆盖面由探测器本身保证**:注册走的就是 peek 自己那支 `detect_peek_image_candidates`
    (原生路径 + OSC 7 相对 + 裸 `file://` 文本),再加上 `detected_live_image_links` /
    `frozen_image_link_spans` 补上**文本里根本拼不出来的第四种形状**——OSC 8 的链接目标。
    一份名单,两个消费者,**下划线覆盖面与 peek 覆盖面不可能各走各的**。
  - **热 peek 的接缝只有一处**:bt-app 在 `DecorationWorkerCompletion::InlineImage` 经过时把解码记进
    `peek_cache`(`remember_decode_for_peek`),命名文件用 `normalized_local_image_path_key`
    ——**正是 hover 稍后查的那把 key**(`PeekSubject::from_path`)。`show_or_request_peek` 只在条目
    为 `None` 时才发 `PeekImage`,所以「peek 是热的」与「两把 key 是同一个字符串」是同一句话。
    一个文件仍然只有一条缓存条目。
- **裁决(3)作用域:下划线是文本 affordance,不是 band,所以它落在引用所在的任何地方——包括用户
  正在敲的命令行。** 那些禁止装饰命令区的裁决讲的是 **band 往命令行注入行**,原样不动:
  `semantic_input_overlaps` 仍然在政策位为真时把创建与完成两侧都关掉。**在字符本身上做个记号不是注入**,
  而用户敲的路径不会因为它在命令里就不是路径——他应该看到和两行之下同一条路径一样的点线。
- **渲染侧零新增路径**:静止点线走 `apply_implicit_hyperlinks` 用的同一个 `DOTTED_UNDERLINE`,
  hover 实线走 `underline_hyperlink` 用的同一个 `UNDERLINE`。运行合并逻辑抽成
  `dotted_underline_run_end`,它**只看格子穿了什么,不问为什么穿**——所以一条引用与一条链接并排时
  合成**一条**点线节奏,在交界处不重起相位。这就是「看起来是一个系统」在像素层的含义。
- **新接缝清单**:`bt_doc::content_anchor_between`(从 bt-term 上提,bt-viewport 现在也要用它)、
  `ViewportFrame::underline_reference_span`(按锚跨度上妆,dotted/solid 二选一)、
  `DualPlaneSession::verified_image_reference_spans` / `verified_image_reference_at`(唯一的真值源:
  `record.artifact.is_some()`)、`decorate_image_reference_affordance`(在 `viewport_frame` 里,
  紧接 `decorate_math_frame`)、bt-app 的 `hovered_image_reference`(每次 publish 现算,因为解码落地
  可以在指针没动的情况下把一段文本变成有下划线的)。
- **Ctrl+单击的连带后果被翻回来了(诚实记账)**:§6.1.1 记的「必须先 hover 过一次,Ctrl+单击才开得动」
  **不再成立**——记录回来了,`decoded_local_image_path_at` 重新第一个作答,四种形状(含 OSC 8 目标)
  都在第一次 hover 之前就是活的。
- **被重新裁定的 pin**:`path_text_creates_no_record_anywhere_and_the_peek_answers_everywhere`
  → `a_verified_reference_wears_the_resting_underline_in_every_plane_and_bands_in_none`;
  `alternate_screen_path_text_creates_no_record_and_the_peek_answers_instead`
  → `alternate_screen_path_text_is_verified_across_a_repaint_and_never_bands`;
  `a_relative_path_on_the_alternate_screen_peeks_and_creates_no_record`
  → `a_relative_path_on_the_alternate_screen_is_verified_and_still_grows_no_band`;
  `every_image_reference_shape_answers_the_alternate_screen_peek`
  → `underline_coverage_equals_peek_coverage_for_every_reference_shape`;
  `flipping_the_policy_bit_back_on_restores_the_whole_band_pipeline` 的关侧从 `(0 记录, 0 band)`
  改为 **`(1 记录, 0 band)`**——记录在政策位两侧都在,**band 才是那一位拥有的东西**;
  `soft_wrapped_path_is_suppressed_when_cursor_is_on_its_lower_half` 与
  `psreadline_paste_hidden_chunk_keeps_input_line_suppressed_at_stability` 改为**在翻案侧**跑,
  因为它们钉的是 band 的门。
- **本次的 pin(逐条都做过红检)**:
  - `a_verified_reference_wears_the_resting_underline_in_every_plane_and_bands_in_none`
    ——静止点线正好落在引用自己的字符上(引号都不沾),live / 离开网格 / 冻结三段都成立,
    `image_placements` 全程为空。
  - `nonexistent_path_is_plain_text_at_rest_and_peeks_nothing`——不存在的路径**一个 flag 都不穿**,
    动词惰性;词法探针仍然认得这个形状(那正是 worker 被问到的原因)。
  - `underline_coverage_equals_peek_coverage_for_every_reference_shape`——四种形状同屏,
    **逐格双向**比对:peek 会答的格子必穿 affordance;**非链接**的格子穿了 affordance 就必然 peek 会答。
    唯一的不对称被写进 pin 而不是抹掉:OSC 8 无论指向什么都由作者声明穿点线,它在那里承诺的是
    链接自己的信息条。四种形状**各自**都断言 `decoded_local_image_path_at` 有答——OSC 8 那行是
    唯一「下划线看着对但可能是空心的」的地方,只有验证能说话。
  - `a_reference_inside_the_command_line_is_underlined_like_any_other`——OSC 133 命令区里的引用与
    两行之下输出里的同一条引用,**点线跨度完全一致**;全程无 band。
  - `the_pointer_upgrades_a_verified_reference_from_dotted_to_solid`——指针下整条变实线,旁边一格不沾。
  - `a_verified_references_decode_is_filed_under_the_key_the_hover_asks_by`(bt-app)——验证解码写进的
    key **恒等于** `PeekSubject::from_path(...).key`,一个文件两种拼法仍是一条热条目;流内 payload
    没有路径可 key,仍按内容 key。
  - `a_reference_and_a_link_side_by_side_are_one_dotted_run`(bt-render)——并排合成一条运行;
    实线(hover 升级)与换色各自断开运行;跨行必断。
  - band 回归护栏:上述四条 session pin 全部断言 `image_placements(&frame).is_empty()`,把
    `INLINE_IMAGE_BANDS` 翻回 `true` 会同时点红它们(实测 8 条红)。
- **红检实测(逐条单独做过,做完复原)**:
  1. 静止 flag 不上妆 → 5 条 pin 红(三条 session 覆盖 pin + 命令行 pin + hover pin)。
  2. affordance 改由「记录存在」而非 `record.artifact` 驱动 → `nonexistent_path_...` 红,
     其余正向 pin 全绿——**这正是这条 pin 存在的理由**。
  3. 注册只走 `Native` 形状 → 覆盖一致性 pin 红(裸 URI 那行有 peek 无 affordance)。
  4. 去掉 `detected_live_image_links` → 覆盖一致性 pin 红(OSC 8 目标未验证)。
  5. `INLINE_IMAGE_BANDS = true` → 8 条 pin 红(含 band 护栏与两条 OSC 1337 补集 pin)。
  6. hover 永不升实线 → hover pin 红。
  7. 命名文件的验证解码改按内容 key 归档 → bt-app 热 peek pin 红。
  8. 运行合并每格自断 → bt-render「一个系统」pin 红。
- **回放足迹(2026-08-04 实测,69 份 `.tmp-repaint-capture/*.vt` × 2 模式,release oracle 前后对拍
  stdout+stderr)**:
  - **默认模式(未设 `BT_PROBE_IMAGE_PATHS`):0/69 有差异,逐字节全同。** 默认回放不检测路径,
    所以验证管线整条没开,affordance 也就无从谈起。
  - **`BT_PROBE_IMAGE_PATHS=1`:30/69 有差异,另 39 份逐字节全同。** **全部差异都是纯插入**——
    多出来的是验证解码落地后多发的那几帧 publish(`event=math-ready` / `event=pty`),
    其装饰状态与前后帧逐字节相同;**30 份里一条 `<`(删除/改写)都没有**,每份的**最后一帧完全相同**。
    `rendered=[…]` 全程为空(图片 band 依旧一条不生),语料里唯一带公式 band 的 `pixel-scroll-feel`
    公式帧数 **1541 → 1541**,分毫未动。
  - **affordance 足迹(用一次性插桩量的最大值,量完复原,oracle 未留改动)**:30 份出现新点线,
    39 份为 0。**实线(solid)计数逐份不变**——静止上妆只在 `UNDERLINE` 不在场时插 `DOTTED_UNDERLINE`,
    结构上不可能加减实线。逐份新增点线格数:
    `alt-peek-only-feel` +461、`band-stuck-trace` +416、`bare-relative-feel` +566、`cc-image-repro` +41、
    `cursor-accept` +82、`daily-b35ce24` +205、`final-input-accept` +82、`hover-peek-complement` +170、
    `hover-peek-feel` +123、`hover-peek-feel-2` +123、`image-accept` +98、`inline-trial2` +123、
    `inline-trial3` +164、`inline-trial4` +200、`osc133-accept` +164、`osc133-accept2` +369、
    `osc133-accept3` +328、`osc7-relative-feel` +614、`paste-accept` +82、`paste-round3` +82、
    `path-accept` +82、`peek-only-final` +154、`pixel-scroll-feel` +127、`pwsh-default-verify` +359、
    `relief-chip-feel` +423、`seat-fix2-feel` +118、`svg-slice-feel` +298、`texture-residency-feel` +475、
    `uri-peek-feel` +491、`zero-resize-restore` +118。
  - 逐份多出的帧数(before→after):`alt-peek-only-feel` 4667→4683、`band-stuck-trace` 76→80、
    `bare-relative-feel` 331→337、`cc-image-repro` 424→429、`cursor-accept` 90→92、
    `daily-b35ce24` 1152→1158、`final-input-accept` 40→42、`hover-peek-complement` 732→769、
    `hover-peek-feel` 135→139、`hover-peek-feel-2` 51→55、`image-accept` 350→351、
    `inline-trial2` 50→55、`inline-trial3` 33→37、`inline-trial4` 56→61、`osc133-accept` 129→135、
    `osc133-accept2` 830→841、`osc133-accept3` 320→326、`osc7-relative-feel` 390→402、
    `paste-accept` 110→116、`paste-round3` 90→92、`path-accept` 95→97、`peek-only-final` 83→89、
    `pixel-scroll-feel` 3427→3432、`pwsh-default-verify` 186→195、`relief-chip-feel` 426→455、
    `seat-fix2-feel` 58→60、`svg-slice-feel` 4038→4064、`texture-residency-feel` 679→717、
    `uri-peek-feel` 621→628、`zero-resize-restore` 57→60。
  - **源不变**:下划线只写进投影出来的那一帧的 cell,转录/网格的字节**一个都没动**——
    `BT_PROBE_STYLES` 打印的冻结行 `StyleSpan` 因此前后无差异,pin 里也逐条断言
    `session.terminal().visible_text()` 原样。

#### 6.1.1 散点下划线:affordance 只能是「格子现在写着什么」的函数(用户实录 2026-08-04)

上面那版 affordance 由**锚点跨度**直接上妆。锚点是「引用当时在哪」的记忆,而记忆比文字活得久:
用户在窄窗里走 PSReadLine 历史,行编辑器就地改写自己的缓冲并用空格擦掉上一条更长命令的尾巴
(录像 `.tmp-repaint-capture/affordance-verify.vt` 里就是 `CUP 13;1` 后面跟十六个空格)。
记录要等到下一个稳定窗口才被 `reconcile_live_image_paths` 重新落座或退休,这中间那一帧里,
锚点指着的格子已经空了——**点线就横躺在空白行上**。用户截到两条:一条占某空白行右半,
一条占下一空白行左三分之一。120x25 回放该录像,单帧空白带线格数峰值 **146**。

修法是**内容见证,不是坐标记忆**——与 band 一直用的「按行文本认身份,不认行号」是同一条纪律,
也正是 `local_image_path_probe_at` 给 peek 的那套答法(两边都重读平面,所以两边都在回答现在这一帧)。
**两半,互不包含**:

- **平面这一半**(`DualPlaneSession::reference_span_content` / `verified_image_reference_span`):
  记录多带一个 `reference_text`——引用**自己**的那串字符(不是 `source_text` 那整条逻辑行)。
  取跨度时按当前网格 / 转录读回两锚之间的文字,不等就不给跨度。`Staging` 一律不给,与
  `local_image_path_probe_at` 对暂存行的沉默完全一致。下划线、hover 升实线、Ctrl+单击
  (`decoded_local_image_path_at`)因此共用**同一个**真值源,不会出现「格子空了还能点出图」。
- **帧这一半**(`ViewportFrame::underline_reference_span` 多收一个 `reference_text`):
  reflow 之后帧的 cell 锚点可能与平面还认可的跨度**对不上**——`image-accept` 在一次加宽 resize 上
  就把一条 44 字符路径的跨度算成了「本行剩下的 48 个空格 + 下一行头三格」。只有格子自己的文字
  能裁决,所以上妆前先拼出选中格的文字:视口只截头(第一格)或只截尾(最后一个可绘格)时按
  前缀/后缀放行,其余情况必须**逐字相等**,否则**一格都不上**。

- **pin**:`the_resting_affordance_follows_the_text_and_never_the_remembered_coordinate`
  (bt-term,三段:被覆盖的行同帧掉线 / 滚动后只在新位置 / 任何阶段都不给空白格上妆;
  引用故意做成**软换行行里的子串**,以便同时钉住「读整行」与「只读一行」两种错法)与
  `an_anchor_span_the_frame_cells_do_not_spell_paints_nothing`(bt-viewport,帧那一半)。
- **红检(逐条单独做过,做完复原,五条各自点红)**:① 跳过 `reference_span_content` 直接给跨度
  (即 84764c1 的行为)→ 第二段红,一三段仍绿,**正是缺陷本身的形状**;② 拿 `source_text` 当见证
  → 第一段红;③ `reference_span_content` 只读 `start_point.row` 而非 WRAPLINE 合并行 → 第一段红;
  ④ 去掉 `capture_rows_transaction` 给存活 live 锚的行位移 → 第三段红;⑤ 删掉帧那一半的
  `spells_the_reference` 闸 → bt-viewport pin 红。
- **回放足迹(70 份 `.tmp-repaint-capture/*.vt` × sync/latency × 两种探针,104x26,release oracle
  前后对拍 stdout+stderr;「前」= 两半见证都关掉的当前树,以隔离行为改动)**:
  - **默认模式:140/140 逐字节全同**(stdout 与 stderr 都是)。默认回放不检测路径。
  - **`BT_PROBE_IMAGE_PATHS=1`:stdout 140/140 逐字节全同**,`rendered=[…]`、帧数、装饰状态
    分毫未动;**50/140 的 stderr 有差异,且差异只有 `UNDERLINE_AUDIT` 这一行**(为这次排查加进
    oracle 的 opt-in 普查:每帧统计「写着空白却带下划线」的格数)。
  - **空白带线格数(单帧峰值之和):5334 → 78。** 剩下的 78 **逐份、逐帧等于把路径检测整条关掉
    时的数值**(`envsteal-verify` 2、`links-accept` 5、`perf-check` 2、`quantum-verify` 3、
    `rearch-acceptance` 2、`repro-unified` 20、`resize-repro` 3/4、`uri-peek-feel` 1、
    `zoom-accept` 1、`zoom-flash` 1/3),即 OSC 8 链接标签与应用自己的 SGR 4 落在空格上——
    与图片引用无关,本次未动。**引用 affordance 的贡献现在正好是 0。**
  - 清零幅度最大的几份:`affordance-verify` 146/211→0、`osc133-accept2` 349→0、
    `band-stuck-trace` 263→0、`svg-slice-feel` 265→0、`alt-peek-only-feel` 202→0、
    `bare-relative-feel` 197→0、`osc7-relative-feel` 171→0、`pwsh-default-verify` 166→0、
    `image-accept` 51→0。
- **顺带定案(不是我们的)**:同一份录像里最后一条提示符渲染成 `(base) PS ` 加命令,而更早的
  提示符是完整的 `(base) PS D:\Developer\BetterTerminal>`。**回放逐字节重现**(`BT_PROBE_DOCDUMP`
  的 `DOC[6]`),因为它就在字节流里:录像从 37→29→53 列改过尺寸,我们对 `CSI 6 n` 的回答是
  `CUP 10;40`(39 列提示符之后的真值,子进程随即照抄),然后子进程**自己**用 `CUP 10;11` 去重画
  缓冲——那是它从 29 列窄窗记住的输入锚。与 `RECORDED_STALE_ANCHOR_REPAINT` /
  `the_recorded_cursor_report_is_answered_from_the_committed_pseudoconsole_grid` 钉的是同一族:
  子进程用了过期锚,终端忠实合成。与本节的 affordance 无关,前后回放该行完全一致。

#### 6.1.1.1 翻案:affordance 是**帧的纯函数**,不是任何被记住的位置(用户裁决 2026-08-04)

§6.1.1 那版没修好。用户复报:**同屏出现两条相似的行(同一条路径被 echo 两次,或历史召回打出一条
几乎一样的行)时,下划线「出错且不消失」,黏在旧地方。** 根因就在 §6.1.1 的修法本身:内容见证问的是
「两锚之间的格子还拼不拼得出这条引用的字」——而**两次打印同一条路径,拼出来的字一模一样**。见证在
过期坐标上照样成立,于是标记留在原地。**锚点 + 文字相等仍然是坐标记忆,只是换了件衣服。**

裁决:**affordance 变成帧的纯函数。**

- **每帧现扫**:对每一帧发布出去的画面,拿 peek 探针用的**同一支探测器**扫**这一帧自己显示的文字**
  (原生路径 / OSC 7 相对 / 裸 `file://` / 加上 OSC 8 的链接目标),把每一处**解析出的路径落在
  VERIFIED 集合里**的出现,画上下划线。没有锚,没有记住的跨度,没有任何「按记录逐条出现」的上妆。
- **VERIFIED 只回答「这个文件打得开吗」**,按路径归档(`normalized_local_image_path_key`),由
  worker 一如既往地喂(`image_path_is_verified`)。**记录仍然是问 worker 的理由与解码的载体,
  但它不再供给位置。** 解码失败或仍在飞的路径,哪里都不画。
- 于是那一家子缺陷是**构造性**地没有了,而不是被见证挡住的:同一路径两次出现就是两处出现,各画各的;
  被改写的行在**改写它的那一帧**就换了扫描结果;滚动 / 重新折行只是格子换了地方;空白格不在任何一处
  出现里,所以永远不可能带线。

- **一份名单,四个动词**。`DualPlaneSession::frame_image_references(&frame) -> Vec<FrameImageReference>`
  是唯一的接缝,`{ path, cells, verified }`——`cells` 是**这一帧的格子下标**。静止点线
  (`decorate_image_reference_affordance`,在 `viewport_frame` 里现扫现画)、hover 实线、Ctrl+单击、
  hover peek 全部读它;bt-app 在 publish 时算一次并连同帧一起存着(`FrameImageReferences { columns,
  references }`),指针事件查的就是那一份。**四个动词逐格一致因此是结构事实,不是两套扫描之间的约定。**
- **帧只画行,探测器只读行(line)**,两者的唯一调和点是 `frame_row_continues`:同一条转录条目的两
  行是一行;两行 live 是一行当且仅当网格把上一行软折到了下一行(`TerminalAdapter::visible_row_continues`,
  只读末列 WRAPLINE,不 capture 整行)。平面变了、屏变了、generation 变了、暂存行——一律断行,
  所以扫描永远不会把终端没接起来的文字接起来。软折行的**行尾空白**在接行前丢掉:它之所以折行是因为
  写满了,末尾的空格只可能是宽字形挪到下一行留下的空当。
- **顺带取消的一条特例**:旧探针对 `Staging` 锚一律沉默(「在途的坐标不作答」)。帧扫描没有这个概念——
  暂存行是**帧正在显示的一行文字**,于是它和别的行一样被扫、一样能带线能 peek。这不是放宽,是坐标语汇
  消失之后那条特例失去了主语:它保护的是「锚点指的地方可能已经不是它了」,而现在没有锚点。
- **删掉的机器(不留两套)**:`ImageReferenceSpan`、`verified_image_reference_spans` /
  `verified_image_reference_at` / `verified_image_reference_span`、`reference_span_content`、
  记录里的 `reference_text` 见证(连同 `DetectedLiveImagePath` / `register_local_image_path` 的那一列
  与 `frozen_text_between`)、`decoded_local_image_path_at`、`local_image_path_probe_at`、
  `ViewportFrame::underline_reference_span`(改为 `underline_cells(&[u32], hover)`)、
  bt-app 的 `peek_target_from_sources`。**peek 的词法探针也一并删掉**:它与帧扫描回答的是同一个问题,
  留着就是第二套答案。
- **代价(实测,release,best-of-20 × 50 次)**:满屏文字 200x60 **85 µs/帧**(纯文字)、
  **137 µs/帧**(每行都带路径);104x26 是 **19 / 31 µs/帧**。同一帧的 `viewport_frame` 整体是
  **737–875 µs**(200x60)/ **132–142 µs**(104x26),扫描占其 **11–14%**。
  **裁决给的备选(按 `frame_content_digest` memoize)实测不划算**:只哈希 index+text 的**下限**就已经
  是 **88 µs/帧**(200x60),而真的 `frame_content_digest` 每格还要再哈两个颜色——**memo key 比被 memo
  的扫描还贵**,因为两者都得把每个格子流一遍。所以不 memoize,改为把逐行的 `String`/`Vec` 缓冲提到整帧
  复用(同机多次:91 → 79–86 µs),并让扫描在 `detect_image_paths` 关闭时直接返回空。
  一次 publish 实际扫**两遍**(session 在 `viewport_frame` 里画静止点线一遍,bt-app 为四个动词存一遍),
  最坏合计 200x60 约 **170 µs**、104x26 约 **40 µs**。没有把它并成一遍:唯一的并法是把扫描结果作为状态
  挂在 session 上,而那正是这次要拆掉的那一类东西——**为了省一遍扫描而重新引入一份「上次算的位置」,
  是把刚翻掉的架构再请回来。**
- **pin(逐条红检)**:
  - `the_same_path_echoed_twice_is_marked_at_both_echoes_and_nowhere_else`——**本次翻案的那一条**:
    同一路径三处出现(一行两处 + 下一行一处)各带各的线;随后就地把第二行改写成**另一条**路径,
    同一帧里旧标记消失、新文字立刻可 peek 但**未验证故不带线**,且 `inline_images` 一条没动。
  - `the_resting_affordance_follows_the_text_and_never_the_remembered_coordinate`——原三段保留
    (软折行子串 / 被覆盖同帧掉线 / 滚动后只在新位置),phase B 额外断言
    `frame_image_references(&frame)` 空且**文件仍然 verified**——「不再被标记的是**位置**,不是文件」。
  - `the_frame_scan_answers_on_the_live_cursor_line_without_any_record`、
    `the_frame_scan_follows_wrapline_continuation_rows`、
    `the_frame_scan_reads_frozen_lines_the_frame_shows`——三平面各一条,替下原来的三条探针 pin。
  - `underline_coverage_equals_peek_coverage_for_every_reference_shape` 保留并改述:两个方向现在问的
    是**同一份名单**,四种形状同屏、逐格双向。
  - `the_painter_marks_the_cells_it_is_given_and_never_weakens_a_solid`(bt-viewport)——替下
    `an_anchor_span_the_frame_cells_do_not_spell_paints_nothing`:画笔的合同只剩「给什么画什么、
    已有实线不被降级、越界格子什么都不画」,因为**调用方交出来的格子就是从这一帧读出来的**,
    没有「哪里」可供核对了。
  - `one_hit_resolves_to_one_reference_through_the_frames_own_stride`(bt-app)——替下
    `a_link_target_is_the_peeks_second_source_and_only_for_local_images`:一次 `GridHit` 按帧自己的
    列宽落到一个格子下标,打印文字排在链接目标之前,所以指针答的是它真正站着的那串字。
  - band 护栏原样:上述 session pin 仍全部断言 `image_placements(&frame).is_empty()`,
    `INLINE_IMAGE_BANDS` 依旧是 `false`。
- **红检实测(逐条单独做过,做完复原)**:①`frame_row_continues` 恒 `false` → 软折行 pin 与 phase A 红;
  ②上妆去掉 `verified` 过滤 → `nonexistent_path_...` 红;③扫描去掉链接那一段 → 覆盖一致性 pin 红;
  ④按候选整行(而非候选自己的字节区间)上妆 → phase A 与命令行 pin 红(共 9 条);⑤同一路径只保留
  **第一处**出现——这正是「一条记录只能说出一个地方」的形状 → 双 echo pin 红,连带覆盖一致性与
  命令行 pin 红(共 3 条)。
- **回放足迹(72 份 `.tmp-repaint-capture/*.vt` × sync/latency = 144 次,104x26,release oracle
  对拍 stdout+stderr;「前」= 1378a55 的 release oracle)**:
  - **默认模式:144/144 逐字节全同**(stdout 与 stderr;语料自上次起长到 72 份)。默认回放不检测路径,
    扫描直接返回空。
  - **`BT_PROBE_IMAGE_PATHS=1`:144/144 逐字节全同**,stdout 与 stderr 都是。**注意这不表示两版画得
    一样**——`rendered=[…]`、帧数、装饰状态、`UNDERLINE_AUDIT` 的空白带线峰值都没变,而逐帧的下划线
    足迹是 `BT_PROBE_UNDERLINE` 的 opt-in 转储,不进这条对拍。
  - **空白带线普查**:`UNDERLINE_AUDIT` 的 `max_blank_underlined` 逐份、逐模式**等于把路径检测整条关掉
    时的数值**——`envsteal-verify` 2、`links-accept` 5、`perf-check` 2、`quantum-verify` 3、
    `rearch-acceptance` 2、`repro-unified` 20、`resize-repro` 3/4、`uri-peek-feel` 1、`zoom-accept` 1、
    `zoom-flash` 1/3,**合计 78**,与 §6.1.1 记的检测-off 基线逐项相同。即 OSC 8 标签与应用自己的
    SGR 4 落在空格上,**引用 affordance 的贡献仍然正好是 0**。
  - **逐帧下划线足迹(`BT_PROBE_UNDERLINE=1`,144 次全跑,逐帧逐行按格集合对拍)**:
    **多出 156905 格,少了 4012 格**,81 份出现差异;**空白带线格数两侧完全相同(各 1177,全是 OSC 8
    标签与应用自己的 SGR 4)**。三处「少了」逐一看过,没有一处是丢标记:
    - `shellcheck` −826(且一格都没多):那一行是 `…\sunset.svgp`,1378a55 给里面的 59 格
      `…\sunset.svg` 上了妆,而**那一串并不是这行现在写的词**——探测器按 `is_path_tail_char` 收到 `p`
      才收尾,于是 peek 在那里从来不作答。**旧版在语料里就违反了「下划线 ⊆ peek」这条 pin**,
      现在两边同源,自然一起沉默。
    - `underline-fix-verify` −1003 / `peek-only-final` −177(同时各多出 3632 / 2596):同一行同一批格子,
      **只是帧号早了几帧**——逐帧看,帧扫描从第 202 帧就开始画,而记录版要等到第 207 帧(下一个稳定窗口
      把锚重新落座)才画;敲下一个键、文字不再拼出这条路径时,帧扫描当帧就撤,记录版还留到第 213 帧。
      **早到、早走,正是「每帧现扫」的定义。**
    - 典型的「多出」在用户自己那份 `affordance-verify` 里:某帧第 9 行,1378a55 只给上一行折下来的
      `.jpg` 四格上妆,而**同一行右半那条一模一样的引用**(它自己还折到第 10 行)一格没有;帧扫描把
      两处都画上。**这正是用户报的形状**——不是「多画了一处」,是旧版**只认得一处**。

### 6.2 图片引用的三种形状都要能 peek(用户报告 2026-08-02)

副屏只剩 hover 这一条图片通路之后,用户在 Claude Code 里 hover 了三种引用形状,一种都没出图。
根因是两条独立的:词法边界只认 ASCII,以及 peek 只读原生路径文本。

- **词法边界改为按字符判定(A)**。`detect_local_image_path_candidates` 的开闭两端原来都是
  按字节:开端只接受 ASCII 空白与 `[ ( = : '`,闭端只在 ASCII 空白或 `]` 处停。CJK 语境里
  `（D:\...\layout-preview.png）` 因此两头同时判错——`D` 前面是 UTF-8 续接字节,`.png` 后面
  也是,于是既开不了口也收不了尾。现在:
  - **开端**:`start == 0`,或前一个**字符**不是「可能是更长 token 的尾巴」的字符。后者定义为
    `is_path_tail_char` = 任意脚本的 alphanumeric ∪ `/ \ . - _`。任意脚本的开括号/引号/分隔号
    (`(`、`（`、`「`、`【`、`“`、`：`、`=`、`,`)与任意宽度的空白都因此开口。
    `/` 在这条排除表里是**承重**的:它正是 `file:///D:/…` 里那截 `D:/…` **不**被当成原生路径的
    唯一原因,URI 只准走本节下文的 URI 通道被正确解码。
  - **闭端**:ASCII/Unicode 空白,或任意**闭合定界符**(`is_closing_delimiter`:Unicode Pe 类在
    终端文本里的常见成员,加 ASCII `>` 与全角 `＞`)。这是把原来「在 `]` 处停」这条既有规则从
    单个字符推广到整类。Pf 类里 `»`、`›`、`”` 一并收(它们只作闭合),`’` 不收(它在普通文件名
    里当撇号:`Bob’s photo.png`)。真的以闭合定界符结尾的文件名仍可用**引号**表达,引号分支
    原样保留。
  - 该改动落在**共享**的检测函数上,因此 primary 的内联准入同样受益(`(D:\a.png)` 从前也是
    检测不到的)。钉死于 `path_candidates_open_after_any_non_token_character_and_close_at_any_closing_delimiter`。
- **`file://` URI 加入 peek,且只加入 peek(B)**。URI 是对文件的**引用**,不是文件在流里的名字,
  所以它不长 band,只答 hover。
  - `file_uri_to_local_image_path(uri)`(`crates/bt-term/src/inline_image.rs`)是唯一裁决点:
    scheme 大小写无关;authority 必须为空或 `localhost`(`file://host/share/a.png` 这类远程共享
    直接拒);query/fragment 不属于路径;**逐 segment** 百分号解码后以 `\` 连接(`%2F` 因此是某个
    文件名里的字面斜杠而非分隔符,符合 RFC 3986,且这种名字自然不存在);随后过**与打印路径完全
    相同**的准入门 `is_admissible_local_image_path`(盘符绝对 + 扩展名白名单),存在性/大小/解码
    照旧只在 worker。没有任何形状因此获得别的形状没有的特权。
  - 两个来源、一个接缝:行文本里的裸 URI 由 `detect_local_image_uri_candidates` 词法扫出
    (URI 按 RFC 3986 是 ASCII,故全角 `）` 天然收尾,尾随句读按 `bt_transcript::detect_http_urls`
    的同一条规则释放),与原生路径合流于 `detect_peek_image_candidates`,`local_image_path_probe_at`
    读的就是它;OSC 8 链接的目标只有帧知道,由 bt-app 的 `peek_target` 取出后交给同一个
    `file_uri_to_local_image_path`。文本优先、链接兜底(`peek_target_from_sources`)。
  - **补集不变量仍按内容点判定**,不按文件身份:`peek_admits_at(anchor)` 是唯一判据,`peek_target`
    在问任何来源之前先问它一次(所以横跨已成 band 内容点的链接不能夹带第二次呈现)。反过来,
    primary 上一处原生路径已经成 band 时,别处指向同一文件的 `file://` URI **仍然 peek**——那是
    两处引用指向一个文件,不是一处引用被呈现两次。
  - 钉死于 `every_image_reference_shape_answers_the_alternate_screen_peek`(全角括号 / 裸 URI /
    OSC 8 目标 / `.txt` 不出图,全部在副屏)、`file_uris_resolve_to_local_image_paths_under_the_same_admission_gates`、
    `file_uri_candidates_are_found_in_prose_and_carry_the_resolved_path`、
    `a_link_target_is_the_peeks_second_source_and_only_for_local_images`。
- **回放影响(2026-08-02 实测)**:55 份 `.tmp-repaint-capture/*.vt` 在**默认**与
  **`BT_PROBE_IMAGE_PATHS=1`** 两种模式下,release oracle 前后对拍 stdout+stderr **全部逐字节相同**
  (0/55 差异)。即录制语料里没有一条 CJK 邻接或括号包裹的路径,边界推广对既有录制无观测效应;
  peek 本就不在回放里跑。

### 6.3 相对路径:只在 shell 报了工作目录时才认(用户裁决 2026-08-03,修订本节)

§6 首句「相对路径拒绝」在此**部分翻案**:`./x.png`、`../a/b.svg`,以及**带分隔符的裸引用**
`local-images/sunset.svg`(当日加宽,见下「范围」)这类相对引用**进检测**——peek 与内联**两条
通路都进**——但**只在该会话有一个由 shell 经 OSC 7 报出的权威工作目录时**。没见过 OSC 7 的
会话里,相对路径**仍然完全不被检测**。

- **权威规则(裁决核心)**:工作目录的**唯一**来源是 OSC 7(`ESC ] 7 ; file://<authority>/<path> BEL`,
  WT/iTerm 同款约定)。**绝不猜**:不拿进程启动目录兜底,不从提示符文本里刮路径,不做任何
  启发式。这与 OSC 133 那一片是同一条诚实降级路线——没有权威来源就退回"什么都不做",
  而不是退回"猜一个"。钉死于 `relative_path_text_is_inert_without_a_reported_working_directory`
  与 `relative_text_is_no_candidate_at_all_without_a_working_directory`(红门:给检测层加一条
  `std::env::current_dir()` 兜底,两条 pin 立刻转红)。
- **范围(用户裁决 2026-08-03 当日加宽,取代本条原文)**:两类候选都收。
  - **带锚的**:`.` 或 `..` 开头**后接分隔符**(`./`、`.\`、`../`、`..\`;`.\` 只是 `./` 的
    Windows 写法,PowerShell 补全就打这个)。
  - **裸的**:没有 `./`、`../` 前缀的相对引用,**必须至少含一个分隔符**(`/` 或 `\`)且落在
    图片扩展名白名单里——`local-images/sunset.svg`、
    `.tmp-a85-parent/docs/spikes/artifacts/03-visual/c-fraction.svg`(两条都是用户实测复现)。
    **单段裸名(`readme.png`)仍然不收**:一个带点的词在文本里与散文无从分辨,歧义太高。
    分隔符就是这条**唯一的界**——它是裸引用身上散文没有的那个记号。
  - **加宽的代价是有界的**:worker 的存在性门让散文误报**静默**(没有文件就没有 band),所以
    余下的成本只是有限次 `stat`,不是错图。这也是这次能翻案的全部理由。
- **裸候选的边界(纯字符类判定,不做 URL 嗅探)**:`is_relative_image_reference` 三拒一准入
  ——不以分隔符**开头**(`/usr/share/x.png`、scheme 剩下的 `//host/x.png` 是从盘根起算的地方,
  拼到工作目录上会凭空发明一个位置)、必须**含**分隔符、**不含 `:`**(冒号正是把文本变成
  绝对(`D:\…`)或带 scheme(`file:`、`https:`)的那个字符,那是原生扫描与 URI 扫描的地盘,
  一段文本绝不被认领两次)、扩展名白名单;外加 `bare_candidate_opens_at` 两条开口规则
  ——候选自身必须是**一整段 `is_path_tail_char` 的连续路径字符**(带锚的引用自报起点,裸的
  不自报,只能由字符类替它说话:所以 `路径：local-images/sunset.svg` 读成冒号之后那一段,
  而不是从 `路` 起的一长条),且**前一个字符不是 `:`**(冒号向左结合,`https://host:8080/img/x.png`
  的 `8080/img/x.png` 本身是个完全合法的相对名,靠这条挡掉)。
  开闭边界(`is_path_tail_char` / `is_closing_delimiter` / 引号分支)与 §6.2 **逐字相同**,
  不新开一套;引号自报跨度,所以引号候选不要求锚,也不要求纯路径字符段。
- **互不相扰**:`file:///D:/x.png` 的 `D:/…` 尾巴前面是 `/`(`is_path_tail_char` 判为续接,
  开不了口),`https://a.b/x.png` 的 `//a.b/x.png` 以分隔符开头,盘符根 `D:\…` 含 `:` ——三条
  扫描线在同一行文本上各取各的,零重复候选。钉死于
  `bare_relative_candidates_need_a_separator_and_never_open_inside_a_url` 与
  `overlapping_scans_never_claim_the_same_text_twice`(后者还断言跨度互不重叠)。
  **红门**:去掉 `candidate.contains(['/', '\\'])` 一条,所有单段裸名 pin 立刻转红;去掉
  「不以分隔符开头」`/usr/share/pixmaps/x.png` 转红;去掉「不含 `:`」引号包裹的 `"D:\dir\x.png"`
  被相对扫描重复认领而转红;去掉纯路径字符段,`见图（./图片.png）` 读成 `见图（./图片.png` 转红;
  去掉「前一个字符不是 `:`」,`https://host:8080/img/x.png` 转红。五条全部实测跑过。
  三个答案的组合性钉死于 `a_bare_relative_reference_with_a_separator_answers_like_an_anchored_one`
  (主屏成 band / 副屏只 peek 零记录 / 无 OSC 7 全惰性,一条 test 三段)。
- **扫描代价必须与行长成正比(加宽带来的新账)**:绝对扫描有盘符前缀把开口锁死在极少数位置,
  裸引用没有前缀,所以一行**不含空白也不含闭合括号**的文本(`a:1,b:2,…`——minified JSON、
  CSV 都是这形状,而且换行折叠后是**一条**逻辑行整个进扫描)几乎每个字符都开一次口。
  两条使它保持线性:① token 末端**每 token 求一次**并在同一 token 内复用(不是每次开口求一次);
  ② **先问谁开的口、再问它说了什么**——裸开口测试停在第一个非路径字符处,而每个开口前面必有
  一个这样的字符,于是任意两次开口读不到同一段文本;若先跑读全串的扩展名/分隔符/冒号三问,
  整行会被按字符重读一遍。钉死于
  `a_long_line_without_terminators_is_scanned_in_time_proportional_to_its_length`(两条各去掉
  一条,该 test 从 1.4 秒变成跑几十秒仍未完而转红,均已实测)。带锚候选仍按整个 token 求值——
  与原生扫描的代价形状**完全一致**(`,D:\a,D:\a,…` 早有同样的形状),本次未加宽也未收窄。
- **引号在词后也开口**:候选被拒后游标只前进一个字符,所以 `--input="./my pic.png"`、
  `src="dir/a.png"` 这类紧贴前一个词的引号照样自成候选(引号自报跨度,不看边界)。
  拒绝从来不是关于它后面那段文本的证据。
- **接缝**:
  - **摄入**:OSC 7 在**既有的** `Osc1337Scanner` 流式前置过滤器里解析(与 OSC 133/1337 同一
    个接缝,vendor 面零改动,守 R11')。`AdapterEvent::WorkingDirectory { uri }` →
    `LifecycleDirective::WorkingDirectory` → session。URI 解码复用 `file_uri_to_local_image_path`
    的机器:抽出 `decode_file_uri`(scheme/authority/percent/逐 segment 拼接),**图片扩展名门
    不套在目录上**——目录没有扩展名可准入。目录多两条:结尾 `/` 是目录自己的斜杠(不是空
    segment),authority 除空与 `localhost` 外还接受本机名(`local_host_name()`,file URI 里
    "本机"的第三种拼法);打印出来的图片 URI **不**享受这两条(仍是 `TrailingSlash::Reject` +
    不认主机名)。
  - **状态**:`DualPlaneSession::working_directory`,**每会话一份、跨主/副屏**——工作目录属于
    shell 进程,副屏的全屏 TUI 是 shell 的孩子,自然继承它最后一次报告。切屏不带走也不清空。
  - **解析**:候选与会话相遇处(`detect_inline_image_candidates` / `detect_peek_image_candidates`
    的 `working_directory` 参数)才把相对文本拼成绝对路径,**词法归一化**(`..` 弹掉左边一段,
    越过盘符根即非候选),**事件线程零文件系统调用**;存在性/普通文件/8 MiB/真实格式/解码
    照旧全在 worker。
  - 解析出的绝对路径此后**走完全既有的管线**:记录、worker 解码、peek 探针、补集不变量、
    §6.1 副屏不落 band——全部一字未改。组合性钉死于
    `a_reported_working_directory_resolves_a_relative_path_into_a_primary_band`(主屏成 band)与
    `a_relative_path_on_the_alternate_screen_peeks_and_creates_no_record`(副屏只 peek、零记录)。
  - 候选跨度(`byte_start..byte_end`)覆盖**打印出来的相对文本**,`path` 是**解析后的绝对路径**
    ——与 §6.2 给 `file://` URI 定下的约定同形。三种形状互不抢文本:URI 里的 `./` 和原生路径
    里的 `\.\` 前面都是 `/` 或 `\`,`is_path_tail_char` 一律判为"续接 token",开不了口。
- **陈旧语义(接受的语义,非缺陷)**:相对引用在**被检测的那一刻**解析,不是在被打印的那一刻。
  shell 换了目录之后,屏幕上一字未改的旧文本**按当前工作目录**重新解析(旧 occurrence 退休、
  新的登记)。理由是结构性的:live 网格本来就在被持续重扫,要"按打印时的目录钉死"就得给每
  一行转录挂一个目录,而 live 平面上这仍然做不到。一个权威、永远是最新的那个,才是这个终端
  对自己所知的诚实交代。钉死于 `re_detection_resolves_old_text_against_the_current_working_directory`。
- **集成脚本**:`scripts/shell-integration/betterterminal.ps1` 的 prompt 钩子每次提示符发一条
  `ESC ] 7 ; file:///<百分号编码的 $PWD> BEL`,在 `133;A` **之前**。authority 留空(file URI 里
  "本机"的写法,不用每次查主机名,也不会过期)。编码是最小且正确的:UTF-8 逐字节,保留
  RFC 3986 的 unreserved + sub-delims + `:` `@` `/`,其余全部 `%XX`——空格成 `%20`,CJK 成它的
  UTF-8 转义。PS 5.1 安全(`[char]27` 一路,`String.IndexOf(char)`,不用 `String.Contains(char)`)。
  当前位置不在 FileSystem provider(`HKLM:` 一类)时发**空**的 `ESC ] 7 ; BEL`:那是**撤回**,
  不是沉默——留着旧目录去回答一个 shell 已经离开的地方,正是裁决禁止的猜测。同理,解不出的
  报告(远程共享、畸形 URI、超长被截断)一律把已存目录**清空**而不是保留。端到端钉死于
  `crates/bt-term/tests/shell_integration_script.rs`(真起一个 Windows PowerShell 5.1 跑脚本,
  把它吐的字节喂回 session)。
- **回放影响(2026-08-03 实测)**:56 份 `.tmp-repaint-capture/*.vt` 在**默认**与
  **`BT_PROBE_IMAGE_PATHS=1`** 两种模式下,release oracle 前后对拍 stdout+stderr **全部逐字节
  相同**(0/56)。语料里一条 OSC 7 都没有(全量扫过),故按构造即恒等;OSC 前缀树多一个 `7;`
  分支对 `OSC 777` 一类只是把同样的字节晚一拍吐出,输出等价,单独钉死于
  `osc_7_reports_its_working_directory_uri_across_chunks_and_both_terminators`。

## 7. OSC 133 输入区权威来源(2026-07-30)

- PowerShell opt-in shell integration 发 FTCS `133;A/B/C/D`;安装与状态机详见
  `docs/shell-integration.md`。某个 screen 一旦见过有效标记,B..C 内容锚区域就是图片与公式
  的共同权威 gate,该 screen 的光标/WRAPLINE/CUP/粘滞记忆退出裁决。
- B..C 包含 live 编辑与提交后的命令回显,永远不装饰;C..D 是输出,照常装饰。区域内容锚随
  live→staging→history 迁移,resize/reflow 以命令文本见证重新落位,不得清空后猜测。
- **换行时撑开的是「被问的那条逻辑行」,永远不是权威区域(用户裁决 2026-08-04)**:
  `133;C` 是一个**排他的语义边界点**,区域严格是半开跨度 `[B, C)`。命令一旦软换行占了多个
  物理行,「点」就不再等于「范围」——行编辑器爱把光标停哪停哪(Home、←、点一下),它按自己
  以为的宽度算出的绝对列在更窄的网格上会被**夹回第一行**,而 reflow 之后旧坐标干脆指向别处。
  于是四行命令的第四行上的引用长出 band,而同一个引用在第一行上不长。
  - **正确的做法是把问题放大,而不是把区域放大**:候选先按 WRAPLINE 合并到**它自己那条逻辑行
    的行首**,再与 `[B, C)` 求交(`merged_live_candidate_start`)。跨度因此是
    `[逻辑行首, 引用末尾]`,它与 `[B, C)` 相交**当且仅当**「引用结束于 B 之后」——命令文本
    的任何一行都满足,提示符里(A..B 段)的引用正确地不满足,第一行输出也不满足(它的逻辑行
    从 C 开始)。**只放大起点**:把终点也往下长会把下一次绝对 CUP 重画写到同一物理行上的东西
    一并拖进来(`inline-trial3.vt` 实测:拖进来的正是下一个提示符,于是输出行丢了 band)。
  - **终点必须是左黏(`Bias::Before`)**:C 命名的是命令**停止**的地方,而 shell 的第一行输出
    恰恰就写在那个点上;右黏的终点会把它吸进区域——这正是用户看到的「输出那行一个 band 都没有」。
  - **行粒度投影同样保排他**:`semantic_input_region_rows` 给出 `B.row ..= (C.column==0 ?
    C.row-1 : C.row)`,绝不无条件 `B.row..=C.row`。
  - **冻结面问同一个问题**:`semantic_input_overlaps_history_through(id, 引用末尾)` ——转录行
    本身就是 WRAPLINE 合并的结果,所以起点无需再放大,终点则与 live 面同理停在引用处。
    问「整行有没有碰到命令区」会让提示符里的引用在**冻结的那一刻**改判,而迁移只许换坐标载体,
    不许换答案。
  - **翻案记录**:本条取代 2026-08-04 早些时候「终点沿 WRAPLINE 链走完」的写法(38b9aea)。
    那一版把 C 往下长,恰好吞掉第一行输出;用户在 trial 版上同时看到「回显长了 band」和
    「输出没有 band」两种症状,就是这一条的两半。
  - 钉死于 `a_wrapped_command_echo_never_decorates_on_any_of_its_rows`、
    `a_wrapped_command_echo_gets_one_verdict_at_every_migration_stage`、
    `a_frozen_line_answers_for_the_reference_it_was_asked_about`、
    `a_command_spans_the_rows_its_half_open_range_touches_and_no_more`。
  - **已知残余(诚实记账,非本次裁决内容)**:reflow 之后区域的坐标仍由**改宽之前**捕获的
    staging 行承载,而检测器读的是改宽之后的 live 网格;两种载体之间没有公共坐标
    (`compare_anchors` 先比载体类别)。因此「先 reflow、再滚出屏幕」的组合在 staging 中段会
    短暂改判。该缺陷早于本条(e78e6b9 与 38b9aea 同样复现),记录在
    `a_reflow_keeps_the_verdict_while_both_ends_share_a_carrier` 里。
- 从未见标记的 screen 完全保留本节 §6 的既有启发式。外层 PowerShell 标记不冒充 nested
  alternate-screen TUI 的内部轮次;未标记的 Codex/Claude Code screen 继续走降级模式。
- v1 接受 OSC 标记可由子进程伪造的风险:影响仅限装饰门控/命令元数据,不赋予资源权限;
  nonce 留待后续加固。
