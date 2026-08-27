# README 隐私节 — 草稿

门 4 产出,门 7 合进 `README.md`(中英各一份,与 README 其余部分同处理)。口吻按
`docs/plans/ui-style/copy-guide.md`:只陈述事实,不替读者下结论,不写「我们非常重视
您的隐私」这类旁白。

下面两版内容一一对应。改口吻时请两版一起改。

---

## English

### Privacy

Folio sends nothing anywhere. There is no telemetry, no analytics, no crash
reporting, no update check, and no network client of its own. The only thing that
reaches the network is a page you open in the web preview, fetched by the
WebView2 engine that Windows provides.

Everything Folio remembers is on your machine, in two directories.

#### `%APPDATA%\Folio` — settings and session

Roaming configuration. Delete it and Folio starts as it did the first time.

| File | What it holds |
| --- | --- |
| `settings.json` | Your settings. |
| `keybindings.json` | Shortcuts you changed. Written only once you change one. |
| `profiles.json` | Shell profiles, including any command line and environment you set. |
| `schemes\` | Colour schemes you added. |
| `session.json`, `session.lock` | The windows, tabs and panes to restore. See below. |
| `pins.json` | Pinned folders, files and addresses. |
| `shell-integration\` | The scripts Folio writes for PowerShell and bash integration. |
| `diagnostics.log`, `diagnostics.prev.log` | Program output for a run started without a console. Checked once at startup: at 4 MiB the current log becomes `.prev.log`, replacing the older one. |
| `hang-reports\` | Written only when the window stops answering. Module names and offsets, not stack contents. |

```powershell
# Everything Folio remembers about your settings and session
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# Just the diagnostics
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

#### `%LOCALAPPDATA%\Folio\WebView2` — the web preview's profile

A separate directory, and not the one above. It is the WebView2 engine's own
profile for the preview: cookies, local storage, the disk cache, and the profile
directory autofill would use. It is local rather than roaming so that a cache and
a cookie jar do not travel between machines. Folio does not delete it.

Autofill and password saving are switched **off** in this profile — not left at
the engine's defaults — so a form you fill in a previewed page is not saved into
it. Cookies and cache still are, as they are in any browser.

```powershell
# Clear the preview's cookies, storage and cache. Close Folio first.
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

#### What is in `session.json` and `pins.json`

Both are plain text, unencrypted, and readable by anything running as you. That
is the same footing as your shell history, and worth knowing because of what is in
them:

- **Addresses in full**, including query strings and fragments. If you preview a
  URL with a token in it, that token is in the file.
- **Working directories** of every pane.
- **File and folder paths** — the root of each files column, which entries were
  open, which one was selected, and the path of every file the preview pool held.
- **Names you typed for panes.**
- **Page titles** of previewed pages.
- **Timestamps** on recently closed tabs.
- **Git branch names** you filtered a commit graph by.
- **Window geometry, per-monitor DPI, and a monitor identifier.**

`profiles.json` holds any command line and environment variables you put in a
profile. Do not put a secret in one.

#### Elsewhere

- `%TEMP%\bt-app-panic.log` — appended to if Folio panics.
- The three hooks installers, when you switch them on, write one file each into
  Claude Code's, Codex's and Copilot's own configuration directories. A dated copy
  of the file as it stood is kept beside it first. `SECURITY.md` has the details.
- Several `BT_*` environment variables make Folio write terminal content to a file
  you name — `BT_PTY_DUMP` writes every byte of every pane. None is set unless you
  set it. `docs/BT-ENVIRONMENT.md` lists all of them.

`SECURITY.md` describes the boundaries these sit inside, and how to report a
vulnerability.

---

## 中文

### 隐私

Folio 不向任何地方发送数据。没有遥测、没有统计、没有崩溃上报、没有更新检查,自己也
没有网络客户端。唯一联网的是你在网页预览里打开的页面,由 Windows 自带的 WebView2
引擎抓取。

Folio 记住的一切都在本机,分在两个目录里。

#### `%APPDATA%\Folio` — 设置与会话

漫游配置。删掉它,Folio 就回到第一次启动的样子。

| 文件 | 内容 |
| --- | --- |
| `settings.json` | 你的设置。 |
| `keybindings.json` | 你改过的快捷键。改过才会写。 |
| `profiles.json` | Shell 配置档,含你填的命令行与环境变量。 |
| `schemes\` | 你添加的配色。 |
| `session.json`、`session.lock` | 待恢复的窗口、标签页与分栏。见下。 |
| `pins.json` | 收藏的文件夹、文件与地址。 |
| `shell-integration\` | Folio 为 PowerShell 与 bash 整合写出的脚本。 |
| `diagnostics.log`、`diagnostics.prev.log` | 无控制台启动时的程序输出。只在启动时查一次:到 4 MiB 就把当前这份转成 `.prev.log`,顶掉上一代。 |
| `hang-reports\` | 只在窗口失去响应时写。记模块名与偏移,不记栈内容。 |

```powershell
# Folio 记住的全部设置与会话
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# 只清诊断
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

#### `%LOCALAPPDATA%\Folio\WebView2` — 网页预览的 profile

这是另一个目录,不是上面那个。它是 WebView2 引擎给预览用的 profile:cookie、本地
存储、磁盘缓存,以及自动填充会用的 profile 目录。放在 local 而不是 roaming,是为了
让缓存和 cookie 不跟着账户在机器之间跑。Folio 不会删它。

这个 profile 里的自动填充与密码保存是**关**的——不是留给引擎的默认值——所以你在预览
页面里填的表单不会存进去。cookie 和缓存照常存,和任何浏览器一样。

```powershell
# 清掉预览的 cookie、存储与缓存。先关掉 Folio。
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

#### `session.json` 与 `pins.json` 里有什么

两份都是明文,不加密,任何以你的身份运行的程序都能读。这跟你的 shell 历史是一个待
遇;之所以值得说,是因为里面有这些:

- **完整地址**,含 query 与 fragment。你预览过一个带 token 的 URL,那个 token 就在
  文件里。
- 每个终端分栏的**工作目录**。
- **文件与目录路径**——每个文件列的根、展开过哪些、选中的是哪个,以及预览池里每个
  文件的路径。
- **你手动给分栏起的名字。**
- 预览过的**页面标题**。
- 最近关闭标签页的**时间戳**。
- 你在提交图里筛选用的 **git 分支名**。
- **窗口几何、每显示器 DPI,以及一个显示器标识。**

`profiles.json` 里有你写进配置档的命令行与环境变量。不要往里面放密钥。

#### 其它位置

- `%TEMP%\bt-app-panic.log` —— Folio 崩溃时追加。
- 三个 hooks 安装器打开时,各写一个文件到 Claude Code、Codex、Copilot 自己的配置目录
  里;写之前先在旁边留一份带日期的原件副本。细节见 `SECURITY.md`。
- 若干 `BT_*` 环境变量会让 Folio 把终端内容写到你指定的文件——`BT_PTY_DUMP` 写的是每
  个分栏的每一个字节。你不设,它们就都不生效。全部列在
  `docs/BT-ENVIRONMENT.md`。

`SECURITY.md` 写了这些机制各自的边界,以及怎么私下报告漏洞。

---

## 给门 7 的两条提醒

1. **旧版本留下的目录。** 2026-08 之前的构建把 WebView2 profile 放在
   `%APPDATA%\Folio` 下(那里可能还留着 `Cache`、`Local Storage`、`Network`、
   `Preferences` 等目录)。现在的构建只用 `%LOCALAPPDATA%\Folio\WebView2`。上面的
   `Remove-Item -Recurse "$env:APPDATA\Folio"` 会一并清掉那些残留,不必单列一条;
   但若 README 要给「只清预览缓存」的单独命令,得说明老装机上残留在
   `%APPDATA%\Folio` 里的那份也要清。
2. **两条清理命令不是同一件事。** 删设置不清预览缓存,清预览缓存不删设置。旧审计把
   两者当成一个目录一个动作,是错的。
