我会把 §0 视为不可变前提，只审 §1、§3 与现有 preview pane 的接缝。先读方案、设计文档和实现，再用 WebView2 官方文档/已知 issue 核验 composition、像素获取、输入、数据目录与恢复时序；不会改仓库文件。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -Command "Get-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppData\\Local\\Temp\\claude\\D--Developer-Better`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppDa
本地只读命令第一次被 Windows 沙箱的登录会话错误拦住了（不是文件或仓库错误）。我换一种启动方式继续读取；若临时目录仍受限，会明确标出证据缺口。
### Shell: `"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe" -NoProfile -Command "Get-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppData\\Local\\Temp\\claude\\D--Devel`
execution error: Io(Custom { kind: Other, error: "windows sandbox: runner failed during SpawnChild: CreateProcessAsUserW failed: 1312 (A specified logon session does not exist. It may already have been terminated.) | cwd=D:\\Developer\\BetterTerminal | cmd=\"C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\\pwsh.exe\" -NoProfile -Command \"try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nGet-Content -Raw -LiteralPath 'C:\\Users\\Weiyi\\AppDa
仓库里已有一份 2026-08-13 的 `docs/spikes/spike-webview2.md`，而且 §2.3 的 DComp/wgpu 地基已于 2026-08-17 落地。也就是说，待审方案把“W0 尚未做”写成了前提，但现状是 W0 已做过一轮、其中一部分已进主干；接下来重点不是重跑原五问，而是补齐它没覆盖的硬边界，并纠正“composition=离屏纹理”的暗示。
结论先说：**条件性 GO，但不能按当前文本原样立项。** §0 不需要重开；§1 必须改成“既有 spike 的补洞与验收”，并明确 composition 是 visual hosting，不是 WebView 像素进入 wgpu 管线。修改后可以按 **W0′→W1→W2** 推进。

仓库事实也需要先对齐：

- [已有 WebView2 spike](D:/Developer/BetterTerminal/docs/spikes/spike-webview2.md:1) 已经证明 DComp visual 同窗叠放、透明洞、基础鼠标、焦点、IME、DPI、Runtime 探测可行。
- [DESIGN.md §2.3](D:/Developer/BetterTerminal/docs/DESIGN.md:144) 记录的 wgpu CompositionVisual + `PreMultiplied` 地基已经落地。
- 但这个 spike 没有闭合像素取得、完整输入、accessibility、进程恢复、UDF、导航安全等问题，所以也不能直接把 W0 标成完成。

## 1. Composition 五问是否完备

**Verdict：不完备，且“离屏合成”一词会误导实施。**

### 1.1 visual 叠加与取纹理

真实边界是：

| 能力 | composition visual | `CapturePreview` | Windows Graphics Capture |
|---|---:|---:|---:|
| 同窗正确层叠、wgpu overlay 盖网页 | 是 | 否 | 否 |
| 网页成为 wgpu 可采样纹理 | **否** | 否，返回编码图片 | 可得到最终可见窗口帧，但属捕获旁路 |
| 单次网页当前视口截图 | 间接 | **是** | 是 |
| 隐藏 tab/未挂 visual 的稳定截图 | 无承诺 | **必须实测** | 否，通常只捕获最终可见窗口 |
| 低延迟远程投影/逐帧共享纹理 | **无受支持路径** | 成本高 | 可做旁路，但不是 WebView2 纹理合同 |

官方定义是把 WebView2 自己的 visual tree 接到宿主的 `RootVisualTarget`，不是把 `ID3D11Texture2D`/`IDXGISurface` 交给宿主。[WebView2 composition API 总览](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis#rendering-webview2-using-composition)、[CompositionController 文档](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller?view=webview2-1.0.3912.50)。

“直接渲染到纹理”仍是 WebView2 官方反馈库里的独立需求；这反证 visual hosting 不能等价解释为共享纹理。[WebView2Feedback #547：Off-screen rendering](https://github.com/MicrosoftEdge/WebView2Feedback/issues/547)。

但“拿不到纹理”不等于“拿不到任何像素”。`ICoreWebView2::CapturePreview` 能异步把当前显示内容写成 PNG/JPEG；它属于 CoreWebView2 公共 API，composition controller 同样可用。[CapturePreview 官方接口](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2?view=webview2-1.0.1462.37)。它只保证当前 viewport，不是整页，[已知边界 #733](https://github.com/MicrosoftEdge/WebView2Feedback/issues/733)。

因此：

- **F2 静态/低频缩略图：拿得到像素，原则上可做。**
- **F2“活缩略图”：不能假定免费可做。** 周期性 `CapturePreview → PNG/JPEG 编码 → 解码 → 上传 wgpu` 会产生 GPU readback、编码、CPU 解码和纹理上传；隐藏 tab 是否持续刷新也必须实测。可以先缓存“最后一次成功帧”，不能把它写成实时承诺。
- **远程投影：**若投影的是当前可见整窗，可考虑 Windows Graphics Capture；若要求每个隐藏 tab/pane 的独立低延迟纹理，当前 WebView2 公共 API 不支持。
- **截图取证：**必须区分 WebView 内部截图与最终屏幕截图。已知案例中 CDP 截图能看到正确页面，而 DComp 最终呈现是黑/陈旧的，[WebView2Feedback #5574](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5574)。W0 应同时保存 `CapturePreview/CDP` 和最终 HWND capture，两者不是同一个 oracle。

现有 Folio visual 树的正确说法应是：**WebView visual 在下，wgpu visual 在上；wgpu 在网页座位处保持 alpha=0，让网页透出，overlay 继续由上层 wgpu 绘制。**这与已有 spike 一致，不是把网页导入 wgpu render pass。

### 1.2 输入、快捷键和 IME

**Verdict：现有 spike 只证明了 happy path，距产品输入合同还有明显距离。**

官方要求 composition host 手工转发空间输入：鼠标移动/离开、按键、双击、垂直与水平滚轮、触摸和笔；光标形状通过 `CursorChanged` 回传。[CompositionController 输入接口](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller?view=webview2-1.0.3912.50)。拖放还需要 `ICoreWebView2CompositionController3::DragEnter/DragOver/DragLeave/Drop`；composition 模式下 OS 只把 OLE drop 给宿主 HWND，[官方说明及反馈 #2907](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2907)。

键盘边界更深：

- WebView 持焦后，普通字符、编辑键和表单 Tab 属于 WebView。
- `AcceleratorKeyPressed`覆盖 Ctrl/Alt 和部分非字符键，但并不是“宿主能收回所有键”的总开关。官方特别指出部分移动/文本编辑组合不因 `Handled` 被禁用；必须用 Folio 的真实绑定表逐项验证，尤其是 `Ctrl+Z/C/V/A`、`Ctrl+Shift+Z`、Alt、Escape、keyup、autorepeat。[Controller accelerator 文档](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller?view=webview2-1.0.3650.58)、[Accelerator event args](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2acceleratorkeypressedeventargs?view=webview2-winrt-1.0.3800.47)。
- `AllowHostInputProcessing` 对 composition controller 无效，不能拿它补一条宿主优先的键盘通路。[官方限制](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2controlleroptions.allowhostinputprocessing)。
- 现有 spike 说“Tab 捞不回来”只说对了一半：宿主不能截获网页内部每一次 Tab，但 WebView 在焦点走到边界、准备离开时会发 `MoveFocusRequested`。正确产品合同应是“Tab 在网页内遍历控件，走出网页时回 Folio”，而不是让 Tab 永远归宿主。
- composition 下本机 IME 能由 WebView 的隐藏 HWND 自理，这是好消息；仍需补测 pane 偏移、跨 DPI、窗口移动、RDP。已有 composition/RDP 候选窗错位报告：[WebView2Feedback #5570](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5570)。

Accessibility 也缺了一整问。Composition controller 提供 `AutomationProvider`，对象实现 `IRawElementProviderSimple`；自绘 Folio 必须决定如何把它接入窗口的 UIA 树，而不能只检查“网页能显示”。[CompositionController2 accessibility API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller2?view=webview2-1.0.3650.58)。

### 1.3 W0 必须补的验收项

建议把“五问”改成以下八扇门：

1. visual 层叠、clip/opacity、圆角遮罩、动画原子性——已有基线，主仓复验。
2. 像素矩阵：`CapturePreview`、最终 HWND capture、隐藏/不可见 tab、视频/DRM、截图性能。
3. 完整鼠标：leave、capture、双击、水平滚轮、XButton、右键菜单、选择拖拽。
4. touch/pen、多指、pointer cancel、OLE drag/drop、cursor。
5. 键盘/焦点/快捷键实际绑定矩阵、Tab 边界、IME 本机/DPI/RDP。
6. UIA/Narrator/键盘可达性。
7. Runtime、`ProcessFailed`、`BrowserProcessExited`、renderer hang、GPU reset、重建。
8. 后台 WebView 的内存、CPU、计时器节流、隐藏 tab 截图与多 origin 进程数。

## 2. localhost 默认判定

**Verdict：必须做纯语法判定；不要通过 DNS/hosts 解析决定默认动作。**

建议规则：先用正规 URL parser 解析并规范化 host。仅当 scheme 为 `http`/`https` 且 host 满足以下之一时，点击默认进预览：

- `localhost` 或以 `.localhost` 结尾；
- IPv4 属于整个 `127.0.0.0/8`，不只 `127.0.0.1`；
- IPv6 精确为 `::1`；
- `0.0.0.0`：接受终端里 dev server 常见输出，但在导航前改写为 `127.0.0.1`，保留端口、路径、query 和 fragment。

依据：IANA 把 `127/8`定义为 Loopback，而 `0.0.0.0` 是“This host/this network”的未指定地址，不是正常 destination。[IANA IPv4 特殊地址表](https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml)。`localhost.`/`.localhost.`是保留名字，[RFC 6761 §6.3](https://www.rfc-editor.org/rfc/rfc6761#section-6.3)。

以下**不自动**进预览，但仍保留 §0 已定的“在预览里打开”显式动词：

- RFC1918/ULA/链路本地/局域网 IP；
- 本机 hostname；
- hosts 文件自定义别名；
- 任意 DNS 解析后碰巧指向 loopback 的域名。

理由是同步 DNS 会增加 hover/click 延迟，还会引入 DNS rebinding 和“今天解析本机、明天解析外网”的不稳定分类。

## 3. User data dir

**Verdict：隔离 profile 方向正确，但 `%APPDATA%` 位置错误。应改为 `%LOCALAPPDATA%`。**

推荐：

```text
%LOCALAPPDATA%\Folio\WebView2
```

或更短的 `%LOCALAPPDATA%\Folio\wv2`。不要放 `%APPDATA%`，因为它是 Roaming：WebView2 UDF 包含缓存、Cookies、IndexedDB、Crashpad 等大量本机数据，不适合漫游、企业 profile 同步或网络重定向。微软也明确要求避免把 UDF 放网络驱动器。[WebView2 UDF 官方指南](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)。

多窗本身不是问题：

- 同一 UDF 在同一登录桌面上只能对应一个 WebView2 session；
- 所有使用它的 WebView 共享该 session/browser process group；
- Folio 应建立**进程级唯一 `CoreWebView2Environment`**，所有窗口/pane 的 composition controller 从它创建；
- 不要每个窗口用同一 UDF 创建一份带不同 language/browser args/options 的 environment。已有 `0x8007139F` 实例说明不同 options 会冲突：[WebView2Feedback #257](https://github.com/MicrosoftEdge/WebView2Feedback/issues/257)。

路径长度没有值得依赖的现代公开上限，但 UDF 内部层次很深，因此基路径应保持短；不要使用工作区路径、UNC、OneDrive 或用户可移动目录。清理/重置 UDF 时还必须等 `BrowserProcessExited`，不能在最后一个 controller `Close` 后立即删目录。

## 4. 会话恢复与登录态竞态

**Verdict：登录态本身没有额外“等 Cookie”阶段，但 controller 异步初始化存在真实竞态；方案没有定义状态机。**

正确时序：

1. 从 session 读出 `desired_url`，只存入 pane 状态，不立即导航。
2. 获取进程级 environment。
3. 创建 composition controller。
4. 安装全部事件和策略：导航过滤、popup、download、权限、external URI、process failure、快捷键。
5. 设置 `RootVisualTarget`、Bounds、DPI、可见性。
6. controller ready 后，对当前 generation 的 `desired_url` 导航一次。
7. `NavigationCompleted(IsSuccess=true)` 后更新“可恢复 URL”；失败页、`about:blank`、取消导航不覆盖最后成功值。

UDF/session 在 environment/controller 建成前已经绑定，因此 Cookie 会随着首个请求自然带上；若 Cookie 过期，页面进入登录流程，不是初始化竞态。

真正的竞态包括：

- controller 尚未 ready，用户又输新 URL；
- pane 已关闭或座位 ID 被复用，旧 callback 晚到；
- 多 tab controller 完成顺序不同；
- 首航时 handler 尚未安装，绕过 allowlist；
- browser process crash 时 `ProcessFailed` 与 `BrowserProcessExited` 到达顺序不保证。

应有 `Uninitialized → EnvironmentPending → ControllerPending → Ready → Closing/Failed` 状态和 generation token；`desired_url` last-write-wins。微软明确要求 browser crash 后重建 WebView，而 renderer crash通常可先 Reload，并说明两个 failure event 的顺序不保证。[WebView2 process recovery 官方指南](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events)。

另一个方案未写的隐私点：session.json 是明文，完整 URL 可能带 OAuth code、signed URL 或 query token。至少要拒绝 URL userinfo，并把“是否原样持久化 query/fragment”写成明确政策。

## 5. `file:///` 与 `javascript:` 允许清单

**Verdict：地址输入只开放 `http`/`https`；`file:`只能由受控文件入口生成；`javascript:`无条件拒绝。**

建议分两条入口，而不是一个统一 scheme allowlist：

### 用户地址输入

允许：

- `http:`
- `https:`
- 无 scheme 的文本：按 §0 进入搜索；识别出的 `localhost:3000` 等先补 `http://`

拒绝：

- `javascript:`：它会在当前页面上下文执行代码，绝不能让自定义地址栏成为脚本执行入口；
- `data:`、`blob:`、`view-source:`、`edge:`、`chrome:`、`devtools:`；
- `file:`；
- `ftp:`、`ws:`、`wss:`；
- `mailto:`、`tel:`、`msteams:` 等 external scheme。当前 [DESIGN.md](D:/Developer/BetterTerminal/docs/DESIGN.md:340) 已裁定 external protocol 全拦。

### Files/.html/.pdf 的受控打开

允许宿主生成 `file:///...`，但必须：

- 从 `PathBuf` canonicalize 后生成，不能直接信任地址栏字符串；
- 只允许显式选中的 `.html/.htm/.pdf`；
- 继续执行现有 UNC/网络路径拒绝或确认政策；
- 不加 `--allow-file-access-from-files`；
- web message/host object bridge 保持关闭；
- PermissionRequested 默认拒绝。

WebView2 官方支持 file URL 加载 HTML/PDF，但 file origin 不属于 secure context，能力有意受限。[本地内容官方文档](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/working-with-local-content)。这对任意本地 HTML 反而是合理的安全边界；不建议为了“更像 HTTPS”就把用户目录大范围映射成 virtual host。

所有顶层导航仍应在 `NavigationStarting` 复查 scheme，不能只检查地址栏；页面脚本、重定向也能发起导航。事件支持取消，并提供 URI、redirect 与 user-initiated 信息。[NavigationStarting 官方接口](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2navigationstartingeventargs?view=webview2-1.0.4022.49)。

`window.open/target=_blank` 还要补一条：只把 `IsUserInitiated=true` 的请求改为同 pane 导航；非用户触发 popup 默认取消。WebView2 内 popup blocker 默认并不会替宿主完成这个政策。

## 6. 成本高估/低估

**Verdict：composition 基建成本明显高估，产品接线和运行时状态成本明显低估。**

高估的部分：

- W0 从零验证 composition：仓库已经完成。
- 全局 wgpu HWND→CompositionVisual 改造：已经落地。
- “没有纹理，所以任何缩略图/截图都不可能”：过度悲观；低频 `CapturePreview` 可行。
- WebView2 COM 绑定本身体积：现有 spike 测得约 +50 KB，不是主要成本。

低估的部分：

- [PreviewSource](D:/Developer/BetterTerminal/crates/bt-app/src/preview.rs:1438) 目前只有 File/Git 身份，[PreviewPane](D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:1038) 只有文档/图片视图状态；Web URL、controller 生命周期、标题、当前/待导航 URL、zoom、find 状态都要新增。
- 现有 session schema只序列化文件 path，非文件 source 明确被跳过，[main.rs](D:/Developer/BetterTerminal/crates/bt-app/src/main.rs:51923)。网页并不能“白拿”现有恢复，必须做 schema 扩展和向后兼容。
- “复用既有 file watcher”不准确：当前 app 有 git/scheme/profile watcher，但 `preview.rs` 是异步 head-read，没有通用 preview-file watch。静态 HTML 自刷需要新增或抽出通用 watcher、debounce、rename/delete/recreate 处理。
- `.pdf 一行路由`只对类型分发成立；导航、快捷键、下载、打印禁用、外链和恢复仍走完整 WebView 产品面。
- 下载“转交系统浏览器”不是可靠转交：POST body、请求头、隔离 profile Cookie 都无法交给系统浏览器。对登录后生成的下载链接尤其会失效。必须定义为“取消并外开可重放的 GET URL”，或接受 WebView2 自己下载。
- 多 origin 会因 Chromium site isolation 增加 renderer 进程；共享 UDF 不等于只用一个 renderer。[WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)。
- 还缺权限、认证、客户端证书、TLS 错误、全屏、context menu、file upload dialog、DevTools 生命周期、renderer hang、UDF 清理和隐私设置。
- F2 活缩略图和远程投影应是独立性能片，不能藏在 WebView2 host 实现成本里。

## 方案里必须改的句子表

| 位置 | 当前句意 | 必改为 |
|---|---|---|
| §1 第 19 行 | “W0 先于一切实现；composition/visual hosting（离屏合成）” | “已有 2026-08-13 spike 与 §2.3 地基；W0′做主仓复验与补洞。composition 是 visual hosting，不是 render-to-texture。” |
| §1 第 19 行 | HWND 下 F2、截图、远程投影“全部失效” | “应用自有 wgpu 截图不含 WebView；WebView `CapturePreview`与整窗 OS capture 仍可能取像素，但无共享纹理、隐藏 tab 与实时性无保证。” |
| §1 第 23 行 | “若只能 visual 叠加，F2/远程投影仍无纹理可用” | “公开 API 无共享纹理；W0′分别验收低频 CapturePreview、最终整窗 capture、隐藏 tab 和帧成本，F2-live/远程低延迟另立性能门。” |
| §1 第 24 行 | 只列鼠标、滚轮、键盘、IME | 补“leave/capture/双击/水平滚轮/XButton/cursor/touch/pen/pointer cancel/OLE drag-drop/MoveFocusRequested/实际快捷键矩阵/UIA/RDP IME”。 |
| §1 第 26 行 | 生命周期只写 Runtime、崩溃、内存 | 补“进程级共享 environment、UDF、ProcessFailed 与 BrowserProcessExited 协调、renderer hang、Runtime update、隐藏 tab 节流和多 origin 进程账”。 |
| §1 第 27 行 | HWND 退路固定写“无截图取证” | 改成“无 wgpu 内管线纹理和可靠 overlay；截图可走 CapturePreview/OS capture，但不是相同语义”。 |
| §2 第 31 行 | W0=重新回答五问 | 改为“W0′=复用已有 spike，补像素、完整输入、UIA、UDF、安全、恢复、后台性能；附主仓机器证据”。 |
| §2 第 33 行 | W2 单一实现单，W0 后再切 | 预先拆为“platform host/input、navigation/security/UDF、preview/session model、chrome commands、file/PDF/watch、F2 capture”六片。 |
| §0 第 12 行 | “静态 HTML 自刷复用既有 watcher” | 改成“抽取/新增通用 preview-file watcher，复用现有 watch clock/debounce 纪律”。 |
| §0 第 13 行 | “PDF 一行路由” | 改成“类型路由一行；其余走完整 WebView 生命周期、安全与输入合同”。 |
| §0 第 14 行 | `%APPDATA%\Folio\webview2` | 改成 `%LOCALAPPDATA%\Folio\WebView2`，进程级 environment、多窗共享同一 UDF/profile。 |
| §0 第 14 行 | 所有 `window.open` 同 pane | 改成“仅 user-initiated 新窗请求同 pane；非用户 popup 取消；所有目标 URI重新过 allowlist”。 |
| §3 后新增 | 无正式 URL 规则 | 加入上述 localhost 纯语法规则、地址输入 scheme allowlist、受控 file 入口与导航二次检查。 |
| §3 后新增 | 无恢复状态机 | 加入 `desired_url + generation + init state` 时序及成功导航提交规则。 |
| DESIGN.md 第 340 行 | 旧规格默认只允许 localhost，其他 origin 确认 | 必须标明已被 2026-08-19“任意 URL 通用浏览”裁决取代；保留的是入口默认动作与 scheme 安全政策，不是 origin 白名单。 |

## 总 verdict

**可立项，但只能按修订后的 `W0′→W1→W2`：**

- **W0′：GO。** 不重做已完成的 DComp 地基；补齐像素合同、全输入/UIA、UDF、恢复、安全和后台性能，形成明确 pass/fail。
- **W1：GO。** §0 形态不重开，只把失败态补入小样：Runtime 缺失、加载失败、进程崩溃、导航被拦、下载无法外交。
- **W2：条件 GO。** 必须拆片，且 F2 活缩略图/远程投影不能作为 WebView host 的默认附赠能力。

阻断立项的不是 composition 可行性——它已经被仓库证明；真正的 blocker 是当前方案仍把 **visual hosting、截图像素和可采样纹理混成一件事**，并且尚未给输入、安全、UDF 与异步恢复写出可执行合同。