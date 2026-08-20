# Web 预览块方案 v3 + W0′ 修订(2026-08-20)

**W0′ 已执行,十扇门全 pass(触屏无设备、IME 须人工两条如实标注),W2 六片全部 GO**——证据 `w0-evidence.md`,四条按实测修订本方案的发现:①门 5 的「AcceleratorKeyPressed 覆盖有限」**被推翻**:30 和弦逐测全部被宿主夺回,键盘接线不需要兜底设计;②隐藏 WebView 的 CapturePreview 不返回 → 片⑥ 收窄为**低频抓+缓存最后成功帧**(这是唯一可行实现,活缩略图仍无受支持路径),且隐藏座位是纯黑洞 → **W1 的失败态三张(加载失败/进程崩溃/Runtime 缺失)从可选升为必须**;③`BrowserProcessExited` 有时 280ms 有时永不到 → UDF 收尾 = 两事件之一 + 超时;④`AddVisual(insertAbove=TRUE, NULL)` 落到视觉树最底层,与 airspace 失败同相——web 视觉入树时的插入次序是片① 的第一条红测。其余九条新增约束见 w0-evidence.md §2,开工时落进各片单;片① 验收含人工 IME 记录(spike 04 先例)。

仓库 D:\Developer\BetterTerminal。审阅记录:一轮 `web-preview-codex-review.md`、二轮 `web-preview-codex-confirm.md`。v2 改动:①按必改句子表全部落实(最重的:composition 是 **visual hosting 不是 render-to-texture**,三种像素通道分开说;仓库已有 2026-08-13 WebView2 spike 与 §2.3 DComp/wgpu 地基,W0 改为 W0′ 复验+补洞);②并入用户 2026-08-19 两条追加裁决(历史进预览切换器、钉式收藏)。

## 0. 形态定稿(用户裁决,不重开;※ 为本轮修订/追加)

- **C-精简**:localhost dev 预览 + 任意 URL 通用浏览;砍浏览器式历史管理页/书签管理器/下载管理/打印/多账户/**扩展**(明说代价:无广告拦截、无密码管理器插件)。定位"手还在终端里时需要的那个浏览器"。
- ※ **历史 = 预览切换器**(用户 2026-08-19):网页与文件同权进预览头切换器(按 URL 去重,带 favicon/地球标),"这个座位开过什么"是产品自己的历史;页内后退/前进管一次浏览会话的导航栈。不另设历史管理页。
- ※ **收藏 = 钉**(用户 2026-08-19):沿用产品 pin 词汇,不引入书签管理器。①OPEN FOLDER 菜单行 hover 出钉,钉住的文件夹进菜单顶部 **PINNED 段**(排 home 之上);②预览切换器行 hover 出钉,钉住的文件/网页进切换器顶部 PINNED 段,跨会话持久(未钉部分维持会话内最近列表);③一份存储 `pins.json`(folder/file/url 三类一张表,独立小文件,将来命令面板同表)——**PinsStore 集中所有权合同**:files 小单先建 `PinsStore`(schema v1 含三类,读写原子:临时文件+rename,坏档保上次好表一张卡,watcher 跟外部编辑照 profiles 先例),W2 只消费同一 store 不另起存储;schema 演进走版本号,未知类目原样保留(前向兼容)。不做层级/导入导出/同步。文件夹钉+文件钉并入 files 小单(F1 后);URL 钉随本块 W2。
- **户口 = 预览 pane** 新内容类;每 tab 单例、可成 tab 根;不做预览内多标签(多页靠无终端 tab 片)。
- **可交互**;入口四个(localhost 链接动词/.html 双击/预览头地址输入(非常驻)/命令面板动词);基础件 = 后退/前进/刷新、地址输入、页内查找(搜索胶囊语言)。
- 五加项:DevTools 动词;非 URL 输入走默认搜索引擎(可设);Ctrl+滚轮/Ctrl+0 缩放;URL 进会话恢复(见 §4 状态机,※ 并非"白拿"——现 session schema 只序列化文件 path,须扩 schema 并向后兼容);※ 静态 HTML 存盘自刷 = **抽取/新增通用 preview-file watcher**(复用 watch clock/debounce 纪律,处理 rename/delete/recreate;现 preview.rs 无通用 watch,不是"复用既有")。
- ※ PDF:**类型路由一行;其余走完整 WebView 生命周期、安全与输入合同**(不是"白捡整个功能")。
- 外部网页:独立 profile 于 **`%LOCALAPPDATA%\Folio\WebView2`**(※ 非 %APPDATA%——UDF 含缓存/Cookies/Crashpad,不可漫游);**进程级唯一 CoreWebView2Environment**,多窗共享同一 UDF/profile,禁止同 UDF 不同 options 重建(0x8007139F 先例);※ `window.open`/target=_blank:**仅 user-initiated 的新窗请求转同 pane 导航,非用户触发的 popup 取消**,目标 URI 重过 allowlist;下载:※ "转交系统浏览器"不可靠(POST body/请求头/隔离 Cookie 带不过去)——定义为**取消并外开可重放的 GET URL,不可重放者提示无法下载**(v1 不做 WebView 自有下载);网页跑在 WebView2 沙箱子进程。
- 姿势:v1 就这些,不够再加(用户原话)。

## 1. 引擎事实与像素合同(※ 全节重写)

**仓库现状**:`docs/spikes/spike-webview2.md`(2026-08-13)已证 DComp visual 同窗叠放、透明洞、基础鼠标、焦点、IME、DPI、Runtime 探测;DESIGN §2.3 的 wgpu CompositionVisual + PreMultiplied 地基已落地(2026-08-17)。**composition 是 visual hosting**:WebView2 把自己的 visual 树接进宿主 `RootVisualTarget`——不是把纹理交给 wgpu。正确层叠:**WebView visual 在下,wgpu visual 在上,wgpu 在网页座位处 alpha=0 让网页透出,overlay 仍由上层 wgpu 画**。

**像素三通道,三种合同,不许混**:

| 通道 | 得到什么 | 用途/边界 |
|---|---|---|
| composition visual | 正确层叠与遮罩 | 显示本体;无纹理 |
| `CapturePreview` | 当前视口的 PNG/JPEG(异步,编码往返) | 低频缩略图/截图;隐藏 tab 是否可截**须实测**;成本 = readback+编解码+上传 |
| OS 整窗 capture(WGC) | 最终可见窗口帧 | 远程投影候选(整窗);不提供隐藏 pane 独立纹理 |

推论:F2 **静态/低频**缩略图可做(CapturePreview 缓存"最后一次成功帧");**F2 活缩略图与低延迟远程逐帧,公开 API 无受支持路径,各自另立性能片,不得写成 WebView host 的附赠**。截图取证须双 oracle(CapturePreview/CDP 与最终 HWND capture 不是同一语义,#5574 有黑帧先例)。HWND 退路的准确表述:无 wgpu 内管线纹理与可靠 overlay;截图仍可走 CapturePreview/OS capture,语义不同。

## 2. W0′:主仓复验 + 十扇门补洞(不重做已证地基)

1. visual 层叠/clip/opacity/圆角遮罩/动画原子性(基线复验);
2. 像素矩阵:CapturePreview、最终整窗 capture、隐藏 tab、视频/DRM、截图性能;
3. 完整鼠标:leave/capture/双击/水平滚轮/XButton/右键菜单/选择拖拽/cursor(CursorChanged);
4. touch/pen/多指/pointer cancel/OLE drag-drop(CompositionController3);
5. 键盘/焦点/快捷键**实际绑定矩阵**(AcceleratorKeyPressed 覆盖有限、AllowHostInputProcessing 对 composition 无效——逐键实测 Ctrl+Shift+Z/Ctrl+Z/C/V/A/Alt/Esc/keyup/autorepeat);**Tab 合同 = 网页内遍历控件,走到边界经 MoveFocusRequested 回 Folio**;IME 补 pane 偏移/跨 DPI/窗口移动/RDP(#5570 先例);
6. UIA:AutomationProvider 接入窗口 UIA 树,Narrator/键盘可达;
7. 生命周期:进程级共享 environment、UDF、ProcessFailed 与 BrowserProcessExited 协调(顺序不保证)、renderer hang、Runtime 缺失(Win10)提示、**Runtime 自更新**(Evergreen 在产品运行中升级:NewBrowserVersionAvailable 的处理与不重启窗口的重建策略)、GPU reset 重建;
8. 后台账:隐藏 WebView 的内存/CPU/计时器节流、多 origin 的 site-isolation 进程数。

9. **安全门**:§3 的 URL 规则作为可验收门禁——allowlist 拒绝矩阵(javascript:/data:/file:/external 逐项)、NavigationStarting 二次检查对重定向与页面脚本发起导航生效、受控 file 入口的 canonicalize 与 UNC 拒绝,全部红测;
10. **恢复门**:§4 状态机作为可验收门禁——generation token 淘汰晚到 callback、事件全装后才首航、失败页不覆盖可恢复 URL、browser crash 重建/renderer crash Reload、UDF 清理等待 BrowserProcessExited,全部红测。

每扇门明确 pass/fail,主仓机器证据。

## 3. URL 规则(※ 新增,按审阅建议)

- **localhost 默认进预览 = 纯语法判定,不做 DNS/hosts 解析**:scheme ∈ {http, https} 且 host 为 `localhost`/`*.localhost`、`127.0.0.0/8`、`::1`、`0.0.0.0`(导航前改写为 127.0.0.1,保留端口/路径/query/fragment)。RFC1918/局域网 IP/本机名/hosts 别名**不自动**进预览,显式动词仍可。
- **地址输入 allowlist**:允许 http/https/无 scheme(补 http:// 或走搜索);拒绝 javascript:/data:/blob:/file:/view-source:/edge:/chrome:/devtools:/ftp:/ws(s):/mailto: 等 external(external protocol 全拦沿用 DESIGN 既有裁决)。
- **受控 file 入口**(files 列 .html/.htm/.pdf 双击):由 PathBuf canonicalize 生成 file:///,不信任地址栏字符串;UNC/网络路径沿用既有拒绝政策;不加 --allow-file-access-from-files;web message/host object bridge 关闭;PermissionRequested 默认拒绝。
- **所有顶层导航在 NavigationStarting 复查 scheme**(重定向/页面脚本也能发起导航,地址栏检查不够);拒绝 URL userinfo;"query/fragment 是否原样持久化" = 明确政策:持久化,**session.json 与 pins.json 同受此隐私条款**(明文存储、可能含 token,记档;含 token 的 URL 恢复失败走登录流程属正常)。
- **钉不是授权**:pin 创建时验证一次,从 pins.json 加载/点击时再验证,不合规(手工篡改/旧版遗留/政策收紧)的 pin 不导航;javascript:/file: 字符串不因来自 pin 而获准;pin 点击走与地址栏相同的导航管线含 NavigationStarting 复查。
- **切换器确定性三则**:去重键 = 规范化后的完整 URL(query、fragment 均参与身份,与持久化政策一致);重定向后以**最后一次成功提交的 URL** 为切换器身份;从切换器选择 = 一次正常顶层导航(进入/截断当前导航栈,不绕导航策略)。已钉 URL 再次出现提升同一条目,PINNED 与 MRU 不留双副本。

## 4. 恢复状态机(※ 新增,按审阅建议)

`Uninitialized → EnvironmentPending → ControllerPending → Ready → Closing/Failed` + generation token;`desired_url` last-write-wins,存 pane 状态不立即导航;事件与策略(导航过滤/popup/download/权限/external/process failure/快捷键)**全部安装完**再对当前 generation 导航;`NavigationCompleted(IsSuccess)` 才更新"可恢复 URL",失败页/about:blank/取消不覆盖;browser process crash 重建 WebView,renderer crash 先 Reload;UDF 清理须等 BrowserProcessExited。

## 5. 切片(※ W2 预拆六片)

- **W0′**:§2 十扇门(spike 复用,补洞,机器证据)。
- **W1**:**已交付(2026-08-20)**——`design/ui-mockup.html` + `DESIGN.md §7.7`,产品代码一行未改。交付内容:①网页作为**预览缓冲**(不是第四种叶)进同一个池、同一张切换器、同一套 tab/Recent/卡片动词;②与 Document/Picture/Graph **逐格同族的头**——favicon/地球标、页面标题即地址输入(双击,继承改名编辑器)、三钮常驻坐在 `Save`/`Edit source` 那一格、DevTools 走悬停工具、查找沿用同一颗搜索胶囊(第二个 host,不是第二份实现);③脚同时是**外交带**与**悬停行**(解析后目标 + `· blocked`,点击惰性,照 §7.1.5g ⑤);④**五张失败态**,一图五行、一句话一事实一动词、无旁白,四张占座 + 下载那张覆页 sheet(Esc 收);⑤切换器网页行与 PINNED 段(同表同索引空间,`pins.json` 的 `url` 首个消费者,钉不是授权);⑥顺带清两笔小样欠账(Recent 的 preview 形、identity 座位改照原生 `seats::identity_seat`)。读法:`?web=ok|runtime|load|crash|blocked|download`(+`&scheme=`)。

  **W1 留给用户的待裁决(五条,W2 片④/③ 开工前需要答案)**:
  1. **三钮的位置**。现画在头的右侧工具区、坐 `Save`/`Edit source` 那一格(本窗每个 pane 的动词都在那儿)。代价:地址在最左、后退在最右,宽座位上手要横跨。备选是放到 mark 与 name 之间(浏览器惯例,但把记号与它标的名字拆开)。**推荐维持现状**——同族优先于外部惯例,本块的定位是终端里的浏览器而不是浏览器。
  2. **下载失败的通知形态**。现为覆页 sheet(同款卡片 + 薄幕 + Esc)。备选是脚带闪一行(本窗既有的通知通道,但一次失败的下载会被错过)。**推荐 sheet**——用户按了一个动词却没拿到文件,这件事不能是可错过的。
  3. **地址输入的键盘门**。现只有双击。`Ctrl+L` 是浏览器惯例但不在 `BINDINGS` 十三行里,加它属于快捷键表的裁决(见「快捷键审计定案」A 案)。**推荐加**,并同时给 `F12`(DevTools)一行。
  4. **一条网页要不要进 Recent / session**。小样已按「网页与文件同权」+ 五加项「URL 进会话恢复」把 preview 种子的 `path` 写成 URL 并画了那一行;但原生 `RecentSeedV1::Preview { path }` 与 session schema 今天只承诺文件路径,**扩不扩到 URL 是 W2 片③ 的 schema 决定**。**推荐扩**(同一形,加一个字段区分),否则关掉一张网页 tab 会是全窗唯一没有退路的关闭。
  5. **`.pv-pin` 与 `row-pin` 两个「钉」同框**。头上的 `pin` 是「钉住这个 pane(下一个文件另开)」,切换器行上的 `pin` 是「收藏这个文件/网页」——两枚同一个图形、同一个词、相距 200px。**这不是本片引入的**(files 小单起就并存),但网页行让它更常同时出现。**建议单独起一个命名小单**,本片未动。

  **发现但未清的账**:①`.foot-path` 的截断方向本片按内容分了岔(路径截头、URL 截尾),files pane 的脚未受影响但两处规则现在写在两个地方;②小样的搜索胶囊在网页宿主上没有命令轨对应物(终端有),这是内容差异不是缺陷,但「胶囊开着时轨变结果轨」这句话在网页座位上只对了一半;③演示页里的 `#/…` 路由是小样自造的假页面,W2 片④ 落地时页内导航栈的真实行为(`CanGoBack`/`CanGoForward` 事件驱动)要以引擎为准,不要照抄小样的 420ms 假节拍。
- **W2**(条件 GO,持 W0′ 证据逐片开):①platform host/input;②navigation/security/UDF;③preview/session model(schema 扩展+向后兼容);④chrome commands(地址/三钮/查找/缩放/DevTools);⑤file/PDF/watch(通用 preview-file watcher);⑥F2 capture(低频缩略图,**独立性能片**)。
- 排期:Windows 落地块之后;F2-live/远程投影不在本块。
- DESIGN 落账时同步标注:DESIGN.md:340 一带旧"仅 localhost、其他 origin 确认"的规格已被 2026-08-19"任意 URL 通用浏览"裁决取代——保留的是入口默认动作与 scheme 安全政策,不是 origin 白名单。

## 6. 给审阅者(终确认)

对照你上一轮的必改句子表逐行确认落实;§0 两条追加裁决(切换器历史、钉收藏)是否引入新的技术矛盾(尤其 pins.json 与切换器持久化、URL 钉与 allowlist 的关系);无新问题则给通过。
