已完成只读审核，未修改任何文件。

总体判断：原型的核心方向与 [DESIGN.md](/D:/Developer/BetterTerminal/docs/DESIGN.md:97)、[PROBLEM-LIST.md](/D:/Developer/BetterTerminal/docs/PROBLEM-LIST.md:28) 基本一致；但不能直接把浏览器行为当作原生实现说明书。开工前必须先定死五件事：布局约束冲突、持久化 schema、flyout 的窗口模型、焦点/按键所有权、文件树虚拟化。

说明：当前没有可用的浏览器运行实例，本次对原型的审核基于 HTML 中明确的状态机、事件处理和几何算法进行静态路径推演，未做逐帧指针实测。

## 1. 原生实现风险清单

1. **[高] 固定列与弹性分屏混排**

   - 原型行为：Files 叶子按像素宽度布局，终端子树吸收剩余宽度；普通 pane 按 ratio 分配。相关常量为 `FILES_W=240`、`FILES_W_MIN=170`、`MIN_PANE_W=260`，见 [原型布局常量](/D:/Developer/BetterTerminal/design/ui-mockup.html:1672)。
   - 原生难点：浏览器 flex 自动处理嵌套测量、溢出、舍入和重排。原生需要自己计算每棵子树的 intrinsic demand、固定宽度总和、divider、DPI 舍入余数和不可满足约束。尤其 Files 子树嵌套、Files 被拖到上下分屏、多个 Files pane 并存时，已经不是简单的二叉 ratio 布局。
   - 建议：**照做，但实现为独立纯布局求解器，不要模拟 CSS flex。** 输入是树、可用矩形、DPI 和最小尺寸，输出完整叶子矩形；预览和提交必须调用同一个函数。

2. **[高] CSS transition 与 FLIP 动画**

   - 原型行为：pane 增删使用约 200ms FLIP；tab 重排约 160ms；flyout 进出 120ms；大量 hover、rail、divider 都有独立 transition。
   - 原生难点：浏览器免费提供插值、合成层、动画取消、重排前后测量和 reduced-motion。wgpu 下需要动画时钟、旧/新布局快照、damage 调度、动画中命中测试规则，以及窗口失焦、DPI 切换、连续操作时的取消/接管。
   - 建议：**简化。** 保留 flyout 淡入/位移、tab 落位和 hover 渐显；首版取消 pane 的缩放 FLIP，或只做位移/透明度，避免对终端纹理做非等比缩放。动画期间交互应以最终布局还是当前视觉布局命中，也要写入规格。

3. **[高] “钉住的 flyout”并不是真正的系统浮窗**

   - 原型行为：它是同一 DOM 中的 fixed 合成层，可拖拽、缩放，但被 viewport 边界夹住，见 [flyout 状态机](/D:/Developer/BetterTerminal/design/ui-mockup.html:2138)。
   - 原生难点：
     - 同窗口合成层：焦点、主题、Dock 动画简单，但不能越出主窗口、不能独立置顶或出现在 Alt-Tab。
     - winit 子窗口/顶层窗口：涉及独立 swapchain、DPI、焦点切换、Z-order、最小化联动、关闭顺序、跨显示器和 AccessKit。
   - 建议：**首版明确采用同窗口合成层并限制在主窗口内。** UI 文案宜称“浮动面板”，不要暗示系统窗口。只有确认“必须拖出主窗口”后再升级为原生子窗口。

4. **[高] hover intent 与“左侧弛豫时间”**

   - 原型行为：打开延迟 180ms；一般离开 220ms；从左侧离开 420ms；边缘容差 8px；还要把 trigger、flyout、菜单视为一个逻辑 hover region，见 [hover 常量](/D:/Developer/BetterTerminal/design/ui-mockup.html:2282)。
   - 原生难点：winit 只给指针事件，没有 DOM 祖先、`relatedTarget` 和 CSS `:hover`。需要自己的 hit-test graph、最后指针位置、计时器失效 token、pointer capture、窗口失焦处理和输入设备区分。
   - 建议：**照做，但封装成单一 `FlyoutController` 状态机。** 不要让各控件分别启动 timer。触控/笔输入应完全禁用 hover-open，只接受点击。

5. **[高] 文件树在原型里没有真实规模**

   - 原型行为：每次展开都扁平化并重建所有可见行，数据只是很小的静态树，见 [filesVisibleRows](/D:/Developer/BetterTerminal/design/ui-mockup.html:2794)。
   - 原生难点：真实目录可能有十万项、网络盘、权限错误、循环链接、长路径和快速变更。`ReadDirectoryChangesW` 还会带来事件合并、溢出重扫和后台线程取消问题。
   - 建议：**必须从第一版就使用虚拟化扁平列表。** 目录枚举异步化，节点使用稳定 ID；滚动只生成可见行；展开请求可取消；监听溢出后重扫。Dock/pop-out 时转移的是树模型和滚动锚，不是渲染节点。

6. **[高] 关闭/恢复只是内存模拟，不是真实序列化**

   - 原型行为：关闭时把 workspace 转成 plan，再清空运行状态；恢复只重开 profile/cwd/名字/布局，不恢复输出，见 [planOf/revivePlan](/D:/Developer/BetterTerminal/design/ui-mockup.html:2379)。
   - 原生难点：原型 plan 内仍包含函数引用、SVG 字符串等不可落盘对象，也没有 schema 版本、原子写、损坏恢复、profile 删除或迁移策略。
   - 建议：**行为照做，但持久化格式必须另行正式设计。** 只保存稳定 `profile_id`、路径、用户命名、pin、树结构、files 状态等数据；采用版本化 schema、临时文件+原子替换、严格校验和逐字段默认值。

7. **[中] 拖拽预览依赖浏览器几何测量**

   - 原型行为：先克隆树、执行和真实 drop 相同的布局变换，再绘制蓝框，并拒绝不能满足最小尺寸的结果，见 [planDrop](/D:/Developer/BetterTerminal/design/ui-mockup.html:3370)。
   - 原生难点：需要 ghost、捕获、跨 divider 命中、无效 drop、根边缘与叶子边缘优先级，以及高速移动时避免预览滞后。
   - 建议：**照做。** “同一规划器同时驱动预览与提交”是应保留的关键约束。预览只使用本帧已计算的矩形，不要另写估算公式。

8. **[中] tab hover“缩略图”实际上是结构示意图**

   - 原型行为：仅多 pane tab 在 hover 350ms 后显示一个简化的 split tree 和名称列表，并非终端画面。
   - 原生难点：若理解成实时缩略图，就需要额外离屏纹理、更新节流、隐私遮蔽和显存预算。
   - 建议：**固化为“布局示意缩略图”，不要做实时终端快照。** 可直接从 split tree 绘制，成本小且不会泄露终端内容。

9. **[中] 浏览器免费提供的焦点、可访问性和控件语义**

   - 原型行为：按钮、tree role、tab 顺序、focus-visible、滚动条、reduced-motion 都由浏览器承担。
   - 原生难点：wgpu 绘制本身没有控件树。需要 AccessKit 节点、键盘焦点、屏幕阅读器名称、快捷键导航、自定义滚动条和高对比度模式。
   - 建议：**不能后补。** UI 状态树从第一天就同时生成绘制节点、命中节点和 AccessKit 节点。

10. **[中] 自绘标题栏与 Windows 非客户区**

   - 原型行为：标题栏、设置按钮和最小化/关闭按钮全部是 DOM。
   - 原生难点：原生需要处理拖动区域、双击最大化、系统菜单、Snap Layout、触屏命中尺寸、DWM 阴影和多 DPI。
   - 建议：若产品没有强烈品牌要求，**优先保留系统标题栏**；若必须自绘，应把 Win32 non-client 行为列为独立验收项，而不是只画三个按钮。

## 2. 规格含糊点

1. **[高] 最小尺寸不可满足时怎么办**

   当前逻辑只在“新增 split”时拒绝不合格布局；窗口随后缩小时，已有 pane 仍会被压到最小值以下。Files 固定宽与终端最小宽冲突时也没有让步顺序。

   必须定义：

   - 禁止窗口继续缩小；
   - 允许 pane 暂时低于最小值；
   - Files 先从 240 缩到 170，再自动折叠/隐藏；
   - 或进入单 pane/滚动布局。

   建议顺序：Files 240→170，之后窗口最小宽由布局树决定；若 OS 强制更小，则保留 focused pane，其余显示折叠占位，不静默销毁树。

2. **[高] Files 的“固定宽列”只在水平 row split 中成立**

   Files 被拖到上/下位置时，它会进入 col split 并横向铺满；多个 Files 子树嵌套时，外层 divider 也未必直接对应可调整的 Files 叶子。

   需要定义：Files 是否允许 top/bottom drop；如果允许，所谓固定宽是否只适用于它位于 row axis 时。建议禁止 Files 作为上下分屏目标，或明确它在 col split 中变成普通全宽 pane。

3. **[高] Clear 与 DESIGN 中 ED3 的关系**

   原型把 Clear 描述为“shell 继续运行、清除 scrollback”；而 [DESIGN 的 ED3](/D:/Developer/BetterTerminal/docs/DESIGN.md:62) 会原子删除 transcript、staging、索引和缓存。还需区分 VT `ED 2`、`ED 3`、`Ctrl+L` 和产品菜单“Clear”。

   建议菜单拆成：

   - `Clear screen`：发送 shell/VT 语义，不删 canonical transcript；
   - `Clear scrollback…`：执行 ED3，并明确影响搜索/选区，必要时确认。

4. **[高] Refresh terminal 的精确定义**

   原型实际是杀掉并重启 shell、保留 pane/cwd/手动名。对运行中的构建或 agent 是破坏性操作，但文案只是 Refresh。

   建议改为 `Restart shell…`，运行中必须确认，并定义退出超时、强杀、环境变量、管理员权限和 cwd 不存在时的回退。

5. **[高] 恢复格式的向前兼容**

   未定义 schema 版本、未知 node kind、非法 ratio、缺失 profile、路径不存在、旧主题/快捷键字段、files root 不可访问时怎么办。

   建议定为“逐叶降级”：未知 profile → 默认 profile；无效 cwd → HOME 并提示；未知 pane kind → 占位 pane；非法 ratio → clamp；整棵树不能因单个叶子损坏而丢失。

6. **[高] 正常关闭、崩溃、系统关机是否同一恢复策略**

   [需求](/D:/Developer/BetterTerminal/docs/PROBLEM-LIST.md:28) 规定 pinned 自动回来、unpinned 才询问，但没有说明强杀、崩溃和 Windows shutdown 是否也显示同一提示。

   建议分别保存：

   - `clean_shutdown=true`：按当前策略；
   - 崩溃/断电：提示“恢复上次窗口布局”，不要暗示进程仍然活着；
   - 写盘失败：显式告警，不能假装 pin 成功。

7. **[中] 恢复后的 active tab、focused pane 和滚动位置**

   原型恢复后选择第一个 revived tab，并把焦点放到树的第一个叶子；没有保存关闭前 active/focus。应明确是否恢复 active workspace、focused terminal、Files 选择和展开状态。输出不落盘，因此终端滚动位置没有恢复意义。

8. **[中] pinned flyout 的作用域和生命周期**

   未定义它是否：

   - 切 tab 后继续存在；
   - 只属于来源 tab，还是全窗口单例；
   - 主窗口 resize/DPI/显示器变化后自动重新夹紧；
   - app 最小化后一起隐藏；
   - Dock 时回来源 tab还是当前 tab。

   原型目前是全局单例，切 tab 会关闭；Dock 会切换到来源 tab。两者都应明确写入规格。

9. **[中] flyout 越界行为**

   原型打开和拖拽时夹在 viewport 内，但主窗口随后变窄时不会立即重新约束旧几何。应规定至少保留多少标题栏可见，以及窗口 resize 后是整体平移、缩小还是保持尺寸。

10. **[中] 分屏 run 的重均分会覆盖手工比例**

    原型在 pane 加入或离开同方向 run 时重新均分，可能覆盖用户刚拖过的 divider ratio。这个取舍已经存在于算法中，但用户不会预期布局因新增 pane 全部重新均分。

    建议明确为以下之一：

    - 新增/删除后同方向 run 全部均分；
    - 只分割目标 pane，其他比例保持；
    - 提供 `Equalize` 命令，由用户主动触发。

11. **[中] 最后一个 pane/tab 的关闭语义**

    原型拒绝关闭最后一个 tab。需明确关闭按钮是否禁用、是否创建默认 shell、是否关闭窗口，以及 pinned tab 是否必须先 unpin 才能关闭。

12. **[中] Recent 去重键**

    当前按 `profile title + cwd` 去重。同一 profile/cwd 下两个不同手动名称的 agent 会合并。需要决定 Recent 的身份是“位置”还是“具体会话种子”。

## 3. 交互设计问题

1. **[高] `Ctrl+B` 与终端生态严重冲突**

   `Ctrl+B` 是 tmux 默认前缀，也可能是应用/TUI 的控制键。全局拦截会让终端程序收不到它，与“终端输入所有者”原则冲突。

   建议更换默认快捷键，或至少仅在明确的 app command 模式中拦截，并允许 profile 覆盖。可见菜单入口应继续保留。

2. **[高] 右键菜单缺少 Copy/Paste**

   Windows 终端用户对右键最稳定的预期是复制、粘贴、全选、打开链接；当前只有 Restart/Clear，而且两个都是有副作用的操作。

   建议菜单顺序为 Copy、Paste、Select all、分隔线、Clear screen、Restart shell…。破坏性动作不应占据菜单首项。

3. **[高] Esc 必须只关闭最上层 UI，否则传给 PTY**

   原型全局处理 Esc。原生若照搬，会破坏 vim、fzf、less 和大量 TUI。

   必须规定优先级：菜单/flyout/对话框打开 → Esc 关闭最上层；没有 UI layer → 原样发送 ESC 到输入 owner。

4. **[高] 焦点所有权仍是实现前置条件**

   原型本身已经承认没有完整焦点模型；需求也明确记录过输入跑进隐藏终端的问题，见 [P2-5](/D:/Developer/BetterTerminal/docs/PROBLEM-LIST.md:49)。

   原生必须有唯一 `InputOwner = Terminal(viewport) | FilesTree | Rename | Menu | Dialog | None`，不能依靠“如果设置页开着就 return”式豁免表。

5. **[中] flyout 入口和 pop-out 控件依赖 hover，违反“可发现优先”**

   文件夹按钮、pane pop-out、close 等多处默认不可见。鼠标用户可能偶然撞见，键盘用户和触屏用户则很难知道功能存在。

   建议至少让 focused pane 的文件按钮常驻低对比度；Files pane 的 pop-out 放进明确的标题栏菜单；命令面板提供同名命令。

6. **[中] 点击文件夹图标同时承担“打开、钉住、关闭”三种语义**

   无 peek 时点击直接打开 pinned；已有瞬态 peek 时点击升级；已经 pinned 时同一点击关闭。图标本身没有明显表示当前状态或下一步动作。

   建议瞬态 peek 标题栏提供明确 `Pin` 按钮；来源图标始终只负责打开/聚焦，关闭由 `×` 完成。

7. **[中] Dock 可能静默切换标签**

   对非活动 tab 的 transient peek 点击 Dock，会切到来源 tab 再插入 Files pane。用户刚才仍在当前 tab，突然切换上下文会显得突兀。

   建议 Dock 文案写明目标，例如 `Dock in “BetterTerminal”`，或先 pin，再由用户显式切到目标 tab。

8. **[中] pane 中心 drop 的 swap/replace 不符合常见 Windows 拖放预期**

   边缘代表 split 很直观；中心让两个 pane 交换或让目标 pane 被顶回 tab strip，结果更强、更难预测。

   建议中心预览内显示文字 `Swap panes` / `Replace pane`，或者要求按住修饰键；不能只用同一种蓝框表达不同后果。

9. **[中] pinned tab 没有关闭按钮**

   “防误关”有价值，但用户会寻找关闭方式。必须先 unpin 再 close 的两步操作应有 tooltip、菜单和键盘命令，不能只靠图标替换暗示。

10. **[中] 启动恢复提示不宜成为阻塞式 tollbooth**

    原型目标是“窗口先可用再询问”，但若 restore overlay 捕获焦点并遮住 pinned tab，实际上仍然阻塞。

    建议使用窗口顶部非模态 restore bar；倒计时不自动恢复，用户可立即在 pinned tab 工作。

11. **[低] “缩略图”只覆盖多 pane tab**

    单 pane tab 没有 hover preview。若这是“布局预览”则合理；若文案称“tab thumbnail”，用户会预期所有 tab 都有。建议统一称“布局预览”。

## 4. 建议固化进规格的部分

建议在 DESIGN.md 新增独立的“UI 状态与交互规格”章节，而不是继续让这些参数只存在于原型。

### 4.1 分屏布局

**[高] 应固化：**

- `MIN_PANE_W = 260 logical px`
- `MIN_PANE_H = 120 logical px`
- `FILES_W_DEFAULT = 240 logical px`
- `FILES_W_MIN = 170 logical px`
- divider：视觉线 1px，命中宽度 7px
- 所有数值是 DPI-independent logical pixels
- row 中固定 Files 子树与弹性终端子树的分配公式
- 不可满足约束的降级顺序
- ratio 序列化精度与 load-time clamp
- pane 加入/离开后是否重新均分整个同方向 run
- 根 drop rim：48px
- pane edge zone：最近边缘距离小于 pane 尺寸的 35%
- 中心 drop 的准确语义
- invalid drop 的视觉与无副作用保证
- 蓝框必须展示提交后真实矩形，预览/提交共用布局规划器

### 4.2 Flyout 三级升级

**[高] 应固化：**

- hover-open：`180ms`
- 普通 leave grace：`220ms`
- 左侧 leave grace：`420ms`
- hit tolerance：`8px`
- trigger 到 flyout 间距：`6px`
- viewport 安全边距：`8px`
- 默认宽度：`264px`
- 默认最大高度：`min(62vh, 460px)`
- pinned 最小尺寸：`200×150px`
- 进出动画：`120ms`，reduced-motion 下关闭
- transient 不抢键盘焦点
- 点击哪个控件完成 Pin，不能只写“点击钉住”
- pinned 是同窗口浮层还是系统子窗口
- 是否全窗口单例、切 tab 行为、resize 后重新夹紧规则
- Dock/Pop-out 必须延续 root、展开项、选择、滚动位置
- Dock 的目标 tab 规则

### 4.3 文件树

**[高] 应固化：**

- 打开时捕获当前终端 cwd，此后绝不自动跟随焦点/cwd
- 键盘规则：↑↓移动；→展开/进子项；←折叠/回父项；Enter/Space 动作
- 文件 Enter 当前是“仅选择”还是“打开预览”
- 异步 loading、permission denied、deleted、unavailable root 的显示
- 稳定节点 ID 与选择恢复规则
- 虚拟化：建议无条件使用，而非达到阈值后切换两套实现
- watcher 溢出、重命名和目录删除后的恢复行为
- UNC/网络路径的枚举、超时和预览安全策略

### 4.4 会话生命周期与持久化

**[高] 应固化：**

- 明确“不保存输出、不恢复进程，只恢复位置和布局”
- `WorkspacePlan` schema 版本
- `profile_id`，禁止持久化函数或展示用图标
- split node：axis、ratio、children
- terminal leaf：profile、cwd、manual name
- files leaf：root、width、expanded IDs、selection
- pinned、tab order、active tab、focused leaf 是否保存
- 正常退出、崩溃恢复、OS shutdown 三种路径
- 原子写与损坏降级
- pinned 自动恢复；只有 unpinned 才询问；没有未知项则不显示提示
- placeholder shell 的创建和删除条件
- pin 随内容移动的三条规则，见 [P1-11](/D:/Developer/BetterTerminal/docs/PROBLEM-LIST.md:39)

### 4.5 焦点与命令路由

**[高] 应固化：**

- 唯一 `InputOwner`
- UI layer 的键盘优先级
- Esc 只有在存在 UI layer 时才被消费
- 新建 tab/pane 后立即把输入 owner 交给新终端
- Files tree、rename、菜单、restore、pinned flyout 的焦点进入/退出规则
- IME 只能绑定当前终端 viewport
- 鼠标按键和快捷键按 profile 可重映射
- `Ctrl+B` 不应作为不可覆盖的全局保留键

### 4.6 右键菜单、主题和布局预览

**[中] 应固化：**

- 终端右键菜单必须包含 Copy/Paste 等平台惯例项
- `Restart shell` 与 `Clear screen/scrollback` 的准确语义及确认条件
- theme：System/Light/Dark，响应系统变更；高对比度和 reduced-motion
- 主题切换是否动画；建议首版不做整窗 cross-fade
- tab hover 延迟 `350ms`
- 缩略图定义为布局示意，不包含实时终端内容
- preview 的消失条件：pointer leave、drag start、tab close、窗口失焦

最优先的规格补丁顺序是：**焦点所有权 → 最小尺寸冲突 → 持久化 schema → flyout 窗口模型 → Clear/Restart 语义 → 文件树虚拟化。** 这六项不先定，原生实现者会被迫在底层架构中替产品做决定。