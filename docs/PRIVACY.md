# What Folio stores, and where

Both languages are in this file: English first, 中文 below.

`SECURITY.md` describes the boundaries these sit inside, and how to report a
vulnerability privately.

## English

Folio sends nothing about you anywhere. There is no telemetry, no analytics and
no crash reporting. Two things reach the network: a page you open in the web
preview, fetched by the WebView2 engine that Windows provides, and the update
check below.

Everything Folio remembers is on your machine, in two directories.

### The update check

Folio asks GitHub whether a newer release exists, and does nothing else with the
answer: it draws a mark on the settings gear and a line in Settings > General.
Nothing is downloaded and nothing is replaced.

| | |
| --- | --- |
| **Address** | `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` |
| **Method** | `GET`. No query string, no request body. |
| **What is sent** | One header: `User-Agent: Folio`. No version, no build, no operating system, no identifier, no cookie. GitHub refuses a request with no user agent at all, which is why the header is not empty. |
| **How often** | At most once every 24 hours, across every Folio window on the machine. A failure - no network, a proxy, a rate limit - counts as the attempt for that day and is not retried. |
| **Where the answer goes** | `%APPDATA%\Folio\update-check.json`: when the page was last asked, the tag it named, and the tag you have already been shown. |
| **How to switch it off** | Settings > General > **Update check**, or `"update_check": false` in `settings.json`. Off, no thread is started, no request is made and `update-check.json` is never written. |

GitHub receives the request the way it receives any request: your IP address and
the time. Folio adds nothing to that. The request goes through Windows' own HTTP
stack, so it follows your machine's proxy settings and certificate store.

### `%APPDATA%\Folio` — settings and session

Roaming configuration. Delete it and Folio starts as it did the first time.

| File | What it holds |
| --- | --- |
| `settings.json` | Your settings. |
| `keybindings.json` | Shortcuts you changed. Written only once you change one. |
| `profiles.json` | Shell profiles, including any command line and environment you set. |
| `schemes\` | Colour schemes you added. |
| `session.json`, `session.lock` | The windows, tabs and panes to restore. See below. |
| `pins.json` | Pinned folders, files and addresses. |
| `update-check.json` | When the releases page was last asked, and the two version tags that answer whether the gear wears a mark. Written only while the update check is on. |
| `shell-integration\` | The scripts Folio writes for PowerShell and bash integration. |
| `diagnostics.log`, `diagnostics.prev.log` | Program output for a run started without a console. Checked once at startup: at 4 MiB the current log becomes `.prev.log`, replacing the older one. |
| `hang-reports\` | Written only when the window stops answering. Module names and offsets, not stack contents. |

```powershell
# Everything Folio remembers about your settings and session
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# Just the diagnostics
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

### `%LOCALAPPDATA%\Folio\WebView2` — the web preview's profile

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

### What is in `session.json` and `pins.json`

Both are plain text, unencrypted, and readable by anything running as you. That
is the same footing as your shell history, and worth knowing because of what is
in them:

- **Addresses in full**, including query strings and fragments. If you preview a
  URL with a token in it, that token is in the file.
- **Working directories** of every pane.
- **File and folder paths** — the root of each files column, which entries were
  open, which one was selected, and the path of every file the preview held.
- **Names you typed for panes.**
- **Page titles** of previewed pages.
- **Timestamps** on recently closed tabs.
- **The last five folders you pointed a files column at**, and when you last
  opened each of them.
- **Git branch names** you filtered a commit graph by.
- **Window geometry, per-monitor DPI, and a monitor identifier.**

`profiles.json` holds any command line and environment variables you put in a
profile. Do not put a secret in one.

### Elsewhere

- `%TEMP%\bt-app-panic.log` — appended to if Folio panics.
- The three agent installers, when you switch them on, write one file each into
  Claude Code's, Codex's and Copilot CLI's own configuration directories. A dated
  copy of the file as it stood is kept beside it first. `SECURITY.md` has the
  details.
- Several `BT_*` environment variables make Folio write terminal content to a
  file you name — `BT_PTY_DUMP` writes every byte of every pane. None is set
  unless you set it. `docs/BT-ENVIRONMENT.md` lists all of them.

---

## 中文

Folio 不向任何地方发送与你有关的数据。没有遥测、没有统计、没有崩溃上报。联网的只有两
件事：你在网页预览里打开的那个页面，由 Windows 自带的 WebView2 引擎抓取；以及下面这个
更新检查。

Folio 记住的一切都在本机，分在两个目录里。

### 更新检查

Folio 向 GitHub 询问是否存在更新的版本，对答案只做一件事：在设置齿轮上画一个标记，并在
设置 > General 里显示一行。不下载任何内容，也不替换任何文件。

| | |
| --- | --- |
| **地址** | `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` |
| **方法** | `GET`。无 query，无请求体。 |
| **发送的内容** | 一个请求头：`User-Agent: Folio`。不含版本号、构建号、操作系统、任何标识符或 cookie。GitHub 拒绝不带 user agent 的请求，这是该请求头不为空的原因。 |
| **频率** | 每 24 小时至多一次，本机所有 Folio 窗口合计。失败——无网络、代理、限流——计入当天的那一次，不重试。 |
| **答案存放位置** | `%APPDATA%\Folio\update-check.json`：上次询问的时间、返回的 tag，以及你已看到过的 tag。 |
| **如何关闭** | 设置 > General > **检查新版**，或在 `settings.json` 中写 `"update_check": false`。关闭后不启动线程、不发出请求，也不写 `update-check.json`。 |

GitHub 收到该请求的方式与收到任何请求相同：你的 IP 地址与时间。Folio 不在此之上附加任何
内容。请求走 Windows 自带的 HTTP 栈，因此遵循本机的代理设置与证书存储。

### `%APPDATA%\Folio` —— 设置与会话

漫游配置。删掉它，Folio 就回到第一次启动的样子。

| 文件 | 内容 |
| --- | --- |
| `settings.json` | 你的设置。 |
| `keybindings.json` | 你改过的快捷键。改过才会写。 |
| `profiles.json` | Shell 配置文件，含你填的命令行与环境变量。 |
| `schemes\` | 你添加的配色。 |
| `session.json`、`session.lock` | 待恢复的窗口、标签与窗格。见下。 |
| `pins.json` | 收藏的文件夹、文件与地址。 |
| `update-check.json` | 上次询问发布页的时间，以及决定齿轮是否带标记的两个版本 tag。仅在更新检查开启时写入。 |
| `shell-integration\` | Folio 为 PowerShell 与 bash 整合写出的脚本。 |
| `diagnostics.log`、`diagnostics.prev.log` | 无控制台启动时的程序输出。只在启动时查一次：到 4 MiB 就把当前这份转成 `.prev.log`，顶掉上一代。 |
| `hang-reports\` | 只在窗口失去响应时写。记模块名与偏移，不记栈内容。 |

```powershell
# Folio 记住的全部设置与会话
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# 只清诊断
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

### `%LOCALAPPDATA%\Folio\WebView2` —— 网页预览的 profile

这是另一个目录，不是上面那个。它是 WebView2 引擎给预览用的 profile：cookie、本地存储、
磁盘缓存，以及自动填充会用的 profile 目录。放在 local 而不是 roaming，是为了让缓存和
cookie 不跟着账户在机器之间跑。Folio 不会删它。

这个 profile 里的自动填充与密码保存是**关**的——不是留给引擎的默认值——所以你在预览页面
里填的表单不会存进去。cookie 和缓存照常存，和任何浏览器一样。

```powershell
# 清掉预览的 cookie、存储与缓存。先关掉 Folio。
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

### `session.json` 与 `pins.json` 里有什么

两份都是明文，不加密，任何以你的身份运行的程序都能读。这跟你的 shell 历史是一个待遇；
之所以值得说，是因为里面有这些：

- **完整地址**，含 query 与 fragment。你预览过一个带 token 的 URL，那个 token 就在文件里。
- 每个终端窗格的**工作目录**。
- **文件与目录路径**——每个文件列的根、展开过哪些、选中的是哪个，以及预览过的每个文件的
  路径。
- **你手动给窗格起的名字。**
- 预览过的**页面标题**。
- 最近关闭标签的**时间戳**。
- **你最近用文件列打开过的五个目录**，以及每个目录最后一次打开的时间。
- 你在提交图里筛选用的 **git 分支名**。
- **窗口几何、每显示器 DPI，以及一个显示器标识。**

`profiles.json` 里有你写进配置文件的命令行与环境变量。不要往里面放密钥。

### 其它位置

- `%TEMP%\bt-app-panic.log` —— Folio 崩溃时追加。
- 三个 agent 安装行打开时，各写一个文件到 Claude Code、Codex、Copilot CLI 自己的配置
  目录里；写之前先在旁边留一份带日期的原件副本。细节见 `SECURITY.md`。
- 若干 `BT_*` 环境变量会让 Folio 把终端内容写到你指定的文件——`BT_PTY_DUMP` 写的是每个
  窗格的每一个字节。你不设，它们就都不生效。全部列在 `docs/BT-ENVIRONMENT.md`。
