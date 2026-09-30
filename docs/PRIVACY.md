# What Folio stores, and where

Both languages are in this file: English first, 中文 below.

`SECURITY.md` describes the boundaries these sit inside, and how to report a
vulnerability privately.

## English

Folio has no telemetry, no analytics and no crash reporting. Two things reach
the network: a page you open in the web preview, fetched by the web engine your
operating system provides — WebView2 on Windows, WebKit on macOS — and the
update check below. The update check can lead to a third, and only when you
press it: downloading the newer release (see *Updating* below).

Everything Folio remembers is on your machine, in two directories. Where those
two are is the one thing in this document that depends on which machine you are
reading it on, and every row below that has two answers gives both.

### The update check

Folio asks GitHub whether a newer release exists. The check itself does nothing
else with the answer: it draws a mark on the settings gear and a line in
Settings > General. The answer can also bring up a card offering the newer
release; nothing is downloaded until you press **Update** on it.

| | |
| --- | --- |
| **Address** | `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` |
| **Method** | `GET`. No query string, no request body. |
| **What is sent** | One header: `User-Agent: Folio`. No version, no build, no operating system, no identifier, no cookie. GitHub refuses a request with no user agent at all, which is why the header is not empty. |
| **How often** | At most once every 24 hours, across every Folio window on the machine. A failure — no network, a proxy, a rate limit — counts as the attempt for that day and is not retried. |
| **Where the answer goes** | `update-check.json` in the settings directory below: when the page was last asked, the tag it named, the tag you have already been shown, and the tag you chose to skip. |
| **How to switch it off** | Settings > General > **Update check**, or `"update_check": false` in `settings.json`. On a machine that has never run Folio it is also the first row of the first-run card, where it arrives on and can be switched off before it has ever run. Off, no thread is started, no request is made and `update-check.json` is never written. |

GitHub receives the request the way it receives any request: your IP address and
the time. Folio adds nothing to that. The request goes through the operating
system's own HTTP stack on both platforms — WinHTTP on Windows, `NSURLSession`
on macOS — so it follows the proxy settings, the certificate store and the
revocation checking your machine already has, and Folio carries no HTTP client
and no certificates of its own. On macOS the session is an ephemeral one that is
cancelled after each check, so nothing of the request is cached between them.

A command-line flag can point the update check at a local folder instead
(`--update-feed <file-URL>`, for rehearsing an update before a release); that
start asks GitHub nothing, and what the folder delivers must still be signed by
the same signer.

### Updating (Windows)

On Windows, when a newer release is published and this copy is one you
unpacked yourself, a card offers it: **Update**, **Later**, **Skip**. Nothing is
fetched until you press **Update**. Once the new version is ready, **Restart**
on the card restarts Folio into it (the **Restart to update** row in Settings >
General brings the card back); if the new version does not start, the previous
one is put back. A copy that scoop
or winget installed gets no card: its row names the manager's own command, with
a **Copy** button, and the manager does the updating.

| | |
| --- | --- |
| **What a press downloads** | Two files of the offered release, nothing else: the Windows archive (`folio-<version>-windows-x64.zip`) and its checksum list (`SHA256SUMS.txt`). |
| **From where** | `https://github.com/lulu-loopp/folio-terminal/releases/download/<tag>/<file>`, named by the offered version and never by "latest"; GitHub redirects to its own download host. The same WinHTTP stack and the same one header, `User-Agent: Folio`, as the check. |
| **What is checked** | The archive against its checksum, and every signed file in it against the signer of the Folio you are running. Anything else is refused and nothing is changed. |
| **Where it goes** | A `.folio-update` folder inside Folio's own folder, beside `folio.exe`: the download, the new files, a copy of the running version to fall back on, the update's journal and its log. It goes with the folder. |
| **What is written outside Folio's folder** | While the new files are being swapped in, one value, `FolioUpdate-<id>`, under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`: it finishes or reverses, at your next logon, an update cut off by a power loss. The update removes it when it ends, and `folio.exe --uninstall-cleanup` removes one left behind for this copy. If the Explorer menu's package is registered, it is registered again at the new version (the Explorer row's own undo, below, covers it). |
| **How to switch it off** | Switch the update check off (above): no check, no card. |

### Updating (macOS)

On a Mac the card and the row are the same, and so is what a press does until
the restart; what differs is what is replaced and where the pieces live. A copy
Homebrew installed gets no card: its row names `brew upgrade --cask folio`, with
a **Copy** button. A copy macOS runs from a read-only place (an application
opened where it was downloaded, without being moved, runs from a randomized
copy), or from a folder your account may not write, cannot replace itself:
**Update** says so before anything is written, and the card offers the
releases page.

| | |
| --- | --- |
| **What a press downloads** | Two files of the offered release, nothing else: the disk image (`Folio-<version>-macos-arm64.dmg`) and its checksum list (`SHA256SUMS-macos.txt`). |
| **From where** | The same address as on Windows, `https://github.com/lulu-loopp/folio-terminal/releases/download/<tag>/<file>`, never "latest". The same `NSURLSession` stack and the same one header, `User-Agent: Folio`, as the check. |
| **What is checked** | The image against its checksum; the `Folio.app` inside it against the signature of the Folio you are running (the same Developer ID), its version against the offer and its architecture against this Mac — once on the image and again after it is copied. Anything else is refused and nothing is changed. |
| **Where it goes** | A hidden folder beside the application, `.Folio.app.folio-update` (in `/Applications` for a copy installed there): the new `Folio.app`, a copy of the running one to fall back on, the update's journal and its log. The image is attached there while it is read and detached after; the download itself is removed once it is read. |
| **What is written outside that folder** | The application itself, replaced whole in one step with the new one. While that happens, one file, `~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<id>.plist`: it finishes or reverses, at your next login, an update cut off by a crash or a power loss. While it exists, macOS may tell you that software from Folio's developer can run in the background. The update removes it when it ends, and `Folio.app/Contents/MacOS/folio --uninstall-cleanup` removes one left behind, together with the hidden folder. |
| **How to switch it off** | Switch the update check off (above): no check, no card. |

### Settings and session

| | |
| --- | --- |
| **Windows** | `%APPDATA%\Folio`, roaming configuration. |
| **macOS** | `~/Library/Application Support/Folio`. Nothing is migrated into it from anywhere: Folio has never shipped on a Mac under another name, so a directory there under one is somebody else's. |

One directory, the same file names in it, and the same keys inside those files.
Delete it and Folio starts as it did the first time.

| File | What it holds |
| --- | --- |
| `settings.json` | Your settings. |
| `keybindings.json` | Shortcuts you changed. Written only once you change one. |
| `profiles.json` | Shell profiles, including any command line and environment you set. |
| `schemes` | Colour schemes you added. |
| `session.json`, `session.lock` | The windows, tabs and panes to restore. See below. |
| `pins.json` | Pinned folders, files and addresses. |
| `update-check.json` | When the releases page was last asked, the two version tags that answer whether the gear wears a mark, and the version you chose to skip. Written only while the update check is on. |
| `shell-integration` | The scripts Folio writes for the PowerShell, bash and zsh integrations. On Windows the PowerShell one is referenced from a line added to your own `$PROFILE`; the bash and zsh ones are handed to the shell as Folio starts it and touch no file of yours. |
| `diagnostics.log`, `diagnostics.prev.log` | Program output for a run started without a console. Checked once at startup: at 4 MiB the current log becomes `.prev.log`, replacing the older one. |
| `hang-reports` | Written only when the window stops answering. Module names and offsets, not stack contents — and on macOS not even those: the entry says that a stack capture is a Windows facility and records the times instead. |

```powershell
# Windows: everything Folio remembers about your settings and session
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# Just the diagnostics
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

```sh
# macOS: the same two. Quit Folio first.
rm -rf ~/Library/Application\ Support/Folio
rm -rf ~/Library/Application\ Support/Folio/diagnostics*.log \
       ~/Library/Application\ Support/Folio/hang-reports
```

The local diagnostics log can also contain file-read totals by lane and up to
three repeatedly read file **basenames**. The file-read ledger never writes
parent directories or file contents; it clears its bounded name table each
minute. Basenames may still be meaningful, so review them before attaching a
log to an issue. These diagnostics are not sent anywhere automatically.

When a step of an update fails to start a program, `diagnostics.log` records the
full path of the program, where Folio is installed, and the folder the failing
process was started in (its working directory), so that the failure can be
diagnosed from the log alone. These paths can contain your account name or the
names of your folders. They stay on this computer; review the log before you
share it.

### The web preview's profile

A separate place from the directory above, and where it is depends on the
engine. The preview keeps what any browser keeps — cookies, local storage and
the disk cache — and Folio does not delete it.

| | |
| --- | --- |
| **Windows** | `%LOCALAPPDATA%\Folio\WebView2`: the WebView2 engine's own profile directory for the preview, including the one autofill would use. It is local rather than roaming so that a cache and a cookie jar do not travel between machines. |
| **macOS** | `~/Library/WebKit/<Folio's bundle identifier>` and `~/Library/Caches/<Folio's bundle identifier>`: the application's own website data, which is where WebKit puts it for every application. Folio's compiled page rules live separately, in `~/Library/Application Support/Folio/WebKit`, and are not browsing data. |

A form you fill in a previewed page is **not** saved. On Windows the engine's
autofill and password saving are switched off rather than left at their
defaults; on macOS the engine has neither feature to switch off — form autofill
and the keychain belong to Safari and not to the view Folio hosts. Cookies and
cache still are kept, as they are in any browser.

```powershell
# Windows: clear the preview's cookies, storage and cache. Close Folio first.
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

```sh
# macOS: the same. Quit Folio first.
rm -rf ~/Library/WebKit/<Folio's bundle identifier> ~/Library/Caches/<Folio's bundle identifier>
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
- **The last command a pinned tab of the summoned terminal ran** — the line
  itself, so a command with a token in it puts that token in the file. Only that
  kind of tab, and only where shell integration is installed to say which line
  was a command. Settings > Summoned terminal > **What comes back** decides
  whether it is kept; the other two answers keep no command at all. What is
  restored is typed at the prompt and never run.
- **Git branch names** you filtered a commit graph by.
- **Window geometry, per-monitor DPI, and a monitor identifier.**

`profiles.json` holds any command line and environment variables you put in a
profile. Do not put a secret in one.

### Elsewhere

- `%TEMP%\bt-app-panic.log` — appended to if Folio panics on Windows. On macOS
  the same report goes to the temporary directory of that run as
  `folio-panic.log`, and the failure itself is in `diagnostics.log` above.
- **macOS: `~/Library/Logs/DiagnosticReports`.** When a run ends in a crash the
  system writes the report there, for every program on the machine and not only
  this one — it is macOS's file, not Folio's. Folio neither writes nor copies
  it: the next launch finds the newest one belonging to this application and
  prints its path in `diagnostics.log`, once. Delete them as you would any
  other program's.
- **macOS: a runtime directory under `$TMPDIR`.** One lock file and one socket,
  in `folio-<your uid>/`, created readable only by you and used by a second
  Folio to hand its arguments to the first. They hold no content — a path, at
  the moment you open one — and the next Folio to take the lock clears what a
  dead one left. `$TMPDIR` is already per-account on macOS; the check that
  refuses a directory somebody else owns is there because the same code runs
  where it is not.
- **macOS: the notification permission.** The first notification Folio has to
  send is where macOS asks. The answer is recorded by macOS against the
  application, not by Folio, and it is changed in System Settings ▸
  Notifications like any other program's. Refused, Folio stops asking and says
  so on the Agent page.
- **macOS: the Finder Services entry.** **Open in Folio** is declared inside the
  application itself and registered with the system when Folio runs. Nothing is
  written into your account for it and there is nothing to undo: remove the
  application and the entry goes with it.
- The Explorer context-menu row, when you switch it on. It writes two keys under
  `HKEY_CURRENT_USER\Software\Classes`, and on a Windows 11 that has
  `folio.msix` beside `folio.exe` it also registers that package for your
  account — that is a Windows package registration, not a file of ours, and it is
  what `Settings > Apps > Installed apps` then lists. Both are per-user, need no
  elevation, and are undone by switching the row off.
- The three agent installers, when you switch them on, write one file each into
  Claude Code's, Codex's and Copilot CLI's own configuration directories. A dated
  copy of the file as it stood is kept beside it first. `SECURITY.md` has the
  details.
- **An update in progress, on Windows**: one `FolioUpdate-<id>` value under your
  account's `Run` key, removed when the update ends — *Updating* above.
- **An update in progress, on macOS**: one LaunchAgent file,
  `~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<id>.plist`,
  removed when the update ends, and the hidden `.Folio.app.folio-update`
  folder beside the application, which holds a staged update and keeps two
  small lock files after it — *Updating (macOS)* above.
- The PowerShell integration, when you ask for it, appends one line to the
  `$PROFILE` a PowerShell names for itself, after copying that file as it stood
  to a dated backup beside it. Delete the line to undo it.
- **Those, and the update check, are what the first-run card can ask about.** It
  offers only the rows this machine can honour, so a machine with no agent
  installed sees fewer. It is shown once, on a machine that has never run Folio,
  and it writes nothing on its own: pressing **Done** presses the same Settings rows listed
  above, and **Not now** presses none of them. The only thing the card itself
  records is that it has been shown.
- **A copy installed with scoop or Homebrew carries a note of that**, written
  by the package manager and not by Folio: `folio-install.json` in scoop's
  version folder, or an extended attribute on `Folio.app` from Homebrew. It says
  which manager installed the copy and whether it runs Folio's cleanup when it
  uninstalls it, nothing else, and it goes with the folder or the app.
- **A copy installed with winget is told by winget's own record of it.** winget
  writes no note into the folder; at start Folio reads, without changing
  anything, the uninstall entries of your account in the Windows registry
  (`HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall`)
  and looks for the one winget wrote for Folio, to see whether it names this
  copy's folder. Other programs' entries are passed over. What it found is one
  line of `diagnostics.log`, naming winget's entry and no folder.
- Several `BT_*` environment variables make Folio write terminal content to a
  file you name — `BT_PTY_DUMP` writes every byte of every pane. None is set
  unless you set it. `docs/BT-ENVIRONMENT.md` lists all of them.

**And what is not stored anywhere.** There is no telemetry, no analytics and no
crash reporting on either platform: nothing above is sent, and nothing above is
written by anything but the program running as you. The update check is the only
request Folio makes on its own, and the sections at the top of this file are
the whole of it; the download it can lead to happens only when you press
**Update**.

---

## 中文

Folio 不向任何地方发送与你有关的数据。没有遥测、没有统计、没有崩溃上报。联网的只有两
件事：你在网页预览里打开的那个页面，由操作系统自带的网页引擎抓取——Windows 上是
WebView2，macOS 上是 WebKit；以及下面的更新检查。更新检查还可引出第三件——下载较新的
版本（见下文「更新」一节），只在你主动按下时发生。

Folio 记住的一切都在本机，分在两个目录里。

### 更新检查

Folio 向 GitHub 询问是否存在更新的版本。检查本身对答案只做一件事：在设置齿轮上画一个标
记，并在设置 > 常规里显示一行。答案还可能弹出一张卡片提供较新的版本；在你按下
**Update** 之前不会下载任何内容。

| | |
| --- | --- |
| **地址** | `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` |
| **方法** | `GET`。无 query，无请求体。 |
| **发送的内容** | 一个请求头：`User-Agent: Folio`。不含版本号、构建号、操作系统、任何标识符或 cookie。GitHub 拒绝不带 user agent 的请求，这是该请求头不为空的原因。 |
| **频率** | 每 24 小时至多一次，本机所有 Folio 窗口合计。失败——无网络、代理、限流——计入当天的那一次，不重试。 |
| **答案存放位置** | 下文那个设置目录里的 `update-check.json`：上次询问的时间、返回的 tag，以及你已看到过的 tag。 |
| **如何关闭** | 设置 > 常规 > **检查新版**，或在 `settings.json` 中写 `"update_check": false`；在从未运行过 Folio 的机器上，它也是初次设置卡的第一行，在那里它默认开启，且可在第一次请求之前关掉。关闭后不启动线程、不发出请求，也不写 `update-check.json`。 |

GitHub 收到的信息与任何请求一样：你的 IP 地址和时间。Folio 不额外附加任何内容。请求通过操作系统自身的 HTTP 栈发出——Windows 上是 WinHTTP，macOS 上是 `NSURLSession`——因此遵循本机已有的代理设置、证书存储和吊销检查，Folio 不携带自己的 HTTP 客户端和证书。macOS 上使用临时会话，每次检查后即销毁，请求之间不缓存任何内容。

命令行参数 `--update-feed <file-URL>` 可将更新检查指向本地文件夹，用于发版前的演练；该次启动不向 GitHub 发出任何请求，且文件夹提供的内容仍须由同一签名者签名。

### 更新（Windows）

在 Windows 上，当有较新版本发布且当前副本是你自行解压的，会弹出一张卡片：**Update**、**Later**、**Skip**。在你按下 **Update** 之前不会下载任何内容。新版本就绪后，卡片上的 **Restart** 将 Folio 重启到新版本（设置 > 常规中的 **Restart to update** 行可重新调出卡片）；若新版本未能启动，自动恢复到前一版本。通过 scoop 或 winget 安装的副本不会看到卡片：对应行显示包管理器自己的命令和一个 **Copy** 按钮，由包管理器负责更新。

| | |
| --- | --- |
| **按下后下载的内容** | 所选版本的两个文件：Windows 压缩包（`folio-<version>-windows-x64.zip`）和校验列表（`SHA256SUMS.txt`）。 |
| **来源** | `https://github.com/lulu-loopp/folio-terminal/releases/download/<tag>/<file>`，按所选版本命名，不使用 "latest"；GitHub 重定向到自己的下载主机。HTTP 栈和请求头与更新检查相同：WinHTTP、`User-Agent: Folio`。 |
| **验证内容** | 压缩包与校验值比对，包内每个已签名文件与当前 Folio 的签名者比对。不符则拒绝，不改动任何文件。 |
| **存放位置** | Folio 自身目录内的 `.folio-update` 文件夹，与 `folio.exe` 同级：下载文件、新版文件、用于回退的旧版副本、更新日志与记录。随目录一起走。 |
| **目录外写入** | 替换文件期间，在 `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` 下写入一个 `FolioUpdate-<id>` 值：若更新被断电中断，下次登录时由它完成或回滚。更新结束时删除；`folio.exe --uninstall-cleanup` 可清理残留值。若已注册资源管理器菜单的包，则以新版本重新注册（撤销方式见下文资源管理器行）。 |
| **如何关闭** | 关闭更新检查（上文）：不检查就没有卡片。 |

### 更新（macOS）

在 Mac 上，卡片、设置行以及按下后到重启之前的流程与 Windows 相同；不同的是替换对象和文件所在位置。通过 Homebrew 安装的副本不会看到卡片：对应行显示 `brew upgrade --cask folio` 和一个 **Copy** 按钮。若 macOS 从只读位置运行该副本（下载后未移动就打开的应用会从一个随机副本运行），或该副本所在文件夹不允许当前账户写入，则无法自行替换：**Update** 会先告知，不写入任何内容，卡片转而提供发布页。

| | |
| --- | --- |
| **按下后下载的内容** | 所选版本的两个文件：磁盘映像（`Folio-<version>-macos-arm64.dmg`）和校验列表（`SHA256SUMS-macos.txt`）。 |
| **来源** | 与 Windows 相同的地址：`https://github.com/lulu-loopp/folio-terminal/releases/download/<tag>/<file>`，不使用 "latest"。HTTP 栈和请求头与更新检查相同：`NSURLSession`、`User-Agent: Folio`。 |
| **验证内容** | 映像与校验值比对；映像内的 `Folio.app` 与当前 Folio 的签名者比对（同一 Developer ID），版本与 offer 比对，架构与本机比对——在映像上验一次，拷贝后再验一次。不符则拒绝，不改动任何文件。 |
| **存放位置** | 应用旁边的隐藏文件夹 `.Folio.app.folio-update`（安装在 `/Applications` 的副本即在该目录下）：新的 `Folio.app`、用于回退的当前版本副本、更新日志与记录。映像在读取期间挂载于此，读完即卸载；下载文件读完即删除。 |
| **目录外写入** | 应用本身，以新版本一步整体替换。替换期间写入一个文件：`~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<id>.plist`——若更新被崩溃或断电中断，下次登录时由它完成或回滚。该文件存在期间，macOS 可能提示你 Folio 开发者的软件可在后台运行。更新结束时删除；`Folio.app/Contents/MacOS/folio --uninstall-cleanup` 可清理残留文件及隐藏文件夹。 |
| **如何关闭** | 关闭更新检查（上文）：不检查就没有卡片。 |

### 设置与会话

| | |
| --- | --- |
| **Windows** | `%APPDATA%\Folio`，漫游配置。 |
| **macOS** | `~/Library/Application Support/Folio`。不从任何位置迁入数据：Folio 从未在 Mac 上以其他名称发布过，因此该目录下其他名称的文件夹不属于 Folio。 |

两个平台各一个目录，目录里文件名相同，文件里的键名也相同。删除该目录后 Folio 恢复为初次启动的状态。

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
# Windows：Folio 记住的全部设置与会话
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# 只清诊断
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

```sh
# macOS：同样这两件。先退出 Folio。
rm -rf ~/Library/Application\ Support/Folio
rm -rf ~/Library/Application\ Support/Folio/diagnostics*.log \
       ~/Library/Application\ Support/Folio/hang-reports
```

更新的某一步未能启动程序时，`diagnostics.log` 会记录该程序的完整路径、Folio 的安装位置，以及启动失败的进程所在的文件夹（它的工作目录），以便仅凭日志就能定位故障。这些路径里可能带有你的账户名或文件夹名称。它们留在本机；分享日志前先看一遍。

### 网页预览的 profile

跟上面那个目录不是一处，具体在哪里取决于引擎。预览保存的东西和任何浏览器一样——cookie、
本地存储、磁盘缓存——Folio 不会删它。

| | |
| --- | --- |
| **Windows** | `%LOCALAPPDATA%\Folio\WebView2`：WebView2 引擎给预览用的 profile 目录，也包括自动填充会用的那个。放在 local 而不是 roaming，是为了让缓存和 cookie 不跟着账户在机器之间跑。 |
| **macOS** | `~/Library/WebKit/<Folio 的 bundle identifier>` 与 `~/Library/Caches/<Folio 的 bundle identifier>`：这个应用自己的网站数据，WebKit 给每个应用都放在这里。Folio 编译出来的页面规则另放在 `~/Library/Application Support/Folio/WebKit`，那不是浏览数据。 |

你在预览页面里填的表单**不会**被保存。Windows 上是把引擎的自动填充与密码保存关掉，而不
是留给它们的默认值；macOS 上引擎根本没有这两样东西可关——表单自动填充和钥匙串是 Safari
的，不属于 Folio 承载的这个视图。cookie 和缓存照常存，和任何浏览器一样。

```powershell
# Windows：清掉预览的 cookie、存储与缓存。先关掉 Folio。
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

```sh
# macOS：同样一件事。先退出 Folio。
rm -rf ~/Library/WebKit/<Folio 的 bundle identifier> ~/Library/Caches/<Folio 的 bundle identifier>
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
- **快捷终端中被钉住的标签最后运行过的命令**——存的是那一行本身，命令里带 token，token
  就在文件里。只有这一种标签会写，且只在装了 shell 整合、有东西说明哪一行是命令时才有。
  是否保留由 设置 > 快捷终端 > **恢复内容** 决定，另外两个选项一条命令也不
  存。恢复出来的内容只填在提示符处，不执行。
- 你在提交图里筛选用的 **git 分支名**。
- **窗口几何、每显示器 DPI，以及一个显示器标识。**

`profiles.json` 里有你写进配置文件的命令行与环境变量。不要往里面放密钥。

### 其它位置

- `%TEMP%\bt-app-panic.log`——Folio 在 Windows 上发生 panic 时追加写入。macOS 上同样的报告写入当次运行的临时目录，文件名为 `folio-panic.log`，错误本身记录在上文提到的 `diagnostics.log` 中。
- **macOS：`~/Library/Logs/DiagnosticReports`。** 崩溃退出时系统将崩溃报告写在这里——所有程序都是如此，不只是 Folio。这是 macOS 的文件，Folio 不写入也不复制。下次启动时 Folio 找到属于自己的最新一份，将其路径记入 `diagnostics.log`。
- **macOS：`$TMPDIR` 下的运行时目录。** 一个锁文件和一个 socket，在 `folio-<你的 uid>/` 下，仅当前用户可读，用于第二个 Folio 将启动参数交给第一个。不含任何内容，下一个取得锁的 Folio 会清除前一个遗留的文件。
- **macOS：通知权限。** Folio 首次需要发送通知时 macOS 会询问。答案由 macOS 记录在应用标识下，不由 Folio 管理，在系统设置 ▸ 通知中修改，与其他程序相同。
- **macOS：Finder 服务条目。** **Open in Folio** 声明在应用内部，Folio 运行时向系统注册。不向账户写入任何内容：移除应用，条目随之消失。
- 打开资源管理器菜单行时，写入两个注册表键到 `HKEY_CURRENT_USER\Software\Classes`；若
  Windows 11 的文件夹里有 `folio.msix`，则注册该包到当前账户（这是 Windows 包注册，不是
  Folio 的文件；该注册会出现在「设置 > 应用 > 已安装的应用」中）。这些都只针对当前账户，
  无需管理员，关闭时移除。
- 三个 agent 安装行打开时，各写一个文件到 Claude Code、Codex、Copilot CLI 自己的配置
  目录里；写之前先在旁边留一份带日期的原件副本。细节见 `SECURITY.md`。
- **更新进行中（Windows）**：在当前账户的 `Run` 键下写入一个 `FolioUpdate-<id>` 值，更新
  结束时删除——见上文「更新」一节。
- **更新进行中（macOS）**：一个 LaunchAgent 文件 `~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<id>.plist`，更新结束时删除；应用旁边的隐藏文件夹 `.Folio.app.folio-update`，存放暂存的更新并在完成后保留两个小锁文件——见上文「更新（macOS）」一节。
- PowerShell 整合被要求时，在 PowerShell 自己报出的 `$PROFILE` 末尾追加一行，追加前先
  在旁边留一份带日期的原件副本。删除该行可撤销。
- **以上五项加上更新检查，正是初次设置卡所问的全部。** 它在从未运行过 Folio 的机器上
  出现一次，自己什么也不写：按下**完成**按的就是上面列举的那几行设置，而**暂不**一行
  也不按。卡本身记下的只有一件事：它已经出现过。
- 若干 `BT_*` 环境变量会让 Folio 把终端内容写到你指定的文件——`BT_PTY_DUMP` 写的是每个
  窗格的每一个字节。你不设，它们就都不生效。全部列在 `docs/BT-ENVIRONMENT.md`。

**不存于任何位置的。** 两个平台上没有遥测、没有统计、没有崩溃上报：上述内容不会被发送，写入它们的也只有以你身份运行的程序。更新检查是 Folio 唯一主动发出的请求，本文开头几节就是它的全部；检查可能引出的下载只在你按下 **Update** 时发生。

### Input recordings (2026-09-20)

`BT_PTY_INPUT_DUMP=<path>` is the send-side companion to `BT_PTY_DUMP`. It is off
unless you explicitly set a nonempty path, including in release builds. It writes raw
queued input and a timed, per-pane manifest with the caller's reason label to
`<path>.in` and `<path>.in.chunks` (later panes add `.2`, `.3`, etc. to the raw path).
It **records your keystrokes, including anything typed at a password prompt; for a
diagnosis you run yourself, never to be shared unread**. See [BT-ENVIRONMENT.md](BT-ENVIRONMENT.md).
