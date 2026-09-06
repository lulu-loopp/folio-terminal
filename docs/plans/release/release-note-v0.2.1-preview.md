> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.1-preview

Fixes and polish for 0.2.0-preview, and the one thing 0.2.0 said was coming:
`Open in Folio` on the page Windows 11 opens first.

## Open in Folio, on the first page

![The first page of the Windows 11 right-click menu on a folder, with Open in Folio on it, above Show more options](https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.1-preview/docs/screenshots/context-menu.png)

- **Settings > General > First page of that menu** puts the verb on the page a
  right-click opens first, so `Open in Folio` is there without `Show more
  options`. It works on a folder and on the empty space inside an open one.
- Windows 11 takes entries on that page only from a signed package, so the
  switch registers `folio.msix` — the small file that now ships beside
  `folio.exe` in the archive. It is registered **for your account**: no
  elevation, no installer, no service, and nothing written outside that account.
  It takes a second or two, and the switch says when it is done.
- **The entry you already had does not move.** `Open Folio here` under
  `Show more options` is still its own switch and still two registry keys, and
  it is what Windows 10 and any machine without the package have. Neither
  switch touches the other.
- Move `folio.exe` to another folder and the entry follows it: the next launch
  notices that the registration names the old folder and re-registers it where
  the program actually is. That needs `folio.msix` to have travelled with it,
  and where it did not, the row says so.
- The same switch takes it off. It also comes off in
  **Settings > Apps > Installed apps**, and the row reads the machine rather
  than a remembered answer, so it agrees with whatever you did there.
- The row is not on the page at all below Windows 11, which has no such menu.

## Both of a pane's clearing verbs do what they say

- `Clear screen` used to leave the pane blank with no prompt in it, and the next
  characters typed appeared on their own, part-way across an empty pane. It now
  keeps the row you are typing on, moves it to the top, and puts the rows that
  were above it into the scrollback, where they can still be scrolled back to,
  searched and copied. The shell is told the same thing, so it goes on drawing
  where the window does.
- Those rows used to be thrown away rather than kept, which is why
  `Clear scrollback…` on the same pane then appeared to do nothing at all, and
  why the marks down the right-hand edge went on pointing at lines that were no
  longer anywhere. Both verbs are on a pane's right-click menu.

## A sentence's own punctuation is not part of the file it names

- A line ending `see docs/notes.md.` opened `notes.md.` — a name nothing on the
  disk holds and the preview could make nothing of. An ASCII stop, comma,
  semicolon, colon or quote at the end of a name is now read as the sentence's.
- The printed string is still asked about first, so a file that really carries
  one still wins; where no such file exists, the name without it does.

## Upgrading from 0.2.0

**Nothing to do.** There is no migration in this release: `settings.json`,
`session.json`, `profiles.json`, `keybindings.json` and `pins.json` are read and
written exactly as 0.2.0 left them, and no shortcut, default or file location
has changed. Unpack over the old folder, or beside it, and run `folio.exe`.

The one thing worth knowing is that the archive now holds **nine** files rather
than eight: `folio.msix` is the new one, and it belongs in the same folder as
`folio.exe`. Unpack the whole archive rather than lifting the executable out of
it, or the first-page switch has nothing to register.

## Download and run

Take `folio-0.2.1-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer, and nothing is written
outside that folder until you run it. `SHA256SUMS.txt` is the hash of what you
downloaded.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service and a Microsoft time stamp, as they were in
0.2.0.

## Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise
  **"Windows protected your PC"** on the first run. **More info** names
  **Weiyi Shi** as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` in the
  archive makes it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

---

# Folio 0.2.1-preview

0.2.0-preview 的修复与打磨，外加 0.2.0 说过将会补上的那一件：`在 Folio 中打开` 进入 Windows 11 先打开的那一页。

## 「在 Folio 中打开」进入一级菜单

![Windows 11 文件夹右键菜单的第一页，「在 Folio 中打开」位于其上，在「显示更多选项」之上](https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.1-preview/docs/screenshots/context-menu.png)

- **设置 > General > 一级菜单** 把这个动词放到右键先打开的那一页上，于是不必再点「显示更多选项」就能看到「在 Folio 中打开」。对文件夹和已打开文件夹内的空白处都有效。
- Windows 11 的那一页只收签名包所声明的条目，因此这个开关会登记 `folio.msix`——现在随压缩包发在 `folio.exe` 旁边的那个小文件。它**只为当前账户**登记：无需提权、无安装程序、无服务，该账户之外不写入任何内容。过程需要一两秒，完成时开关会说明。
- **原有的条目不动。**「显示更多选项」下的「在此处打开 Folio」仍是它自己的开关、仍是两个注册表键，也仍是 Windows 10 与没有这个包的机器所用的那一条。两个开关互不干涉。
- 把 `folio.exe` 移到别的目录，条目会跟着走：下一次启动发现登记指向的是旧目录，就把它改到程序实际所在之处。这需要 `folio.msix` 一同搬过去；没搬过去时，该行会说明。
- 取消也是同一个开关。它同样会出现在 **设置 > 应用 > 已安装的应用** 中，且该行读的是机器的实际状态而非记住的答案，因此与用户在那里做过的事一致。
- 在 Windows 11 以下的系统上根本没有这一行——那里没有这样一页菜单。

## 窗格的两个清屏动词都名副其实

- `Clear screen` 此前会把窗格清成空白且不留提示符，之后打的字孤零零地出现在空窗格中间。现在它保留用户正在输入的那一行并将其移到顶部，把原本在它上方的行送入回滚区，仍可回滚、搜索与复制。同一件事也告知了 shell，因此 shell 继续在窗口所认为的位置绘制。
- 那些行此前是被丢弃而不是被留存的，这正是同一窗格上的 `Clear scrollback…` 此后看起来毫无反应的原因，也是右侧标记条继续指向已不存在的行的原因。两个动词都在窗格的右键菜单里。

## 句子自己的标点不属于它提到的文件名

- 以 `see docs/notes.md.` 结尾的一行此前打开的是 `notes.md.`——磁盘上没有这个名字，预览也无从处理。现在名字末尾的 ASCII 句点、逗号、分号、冒号与引号被读作句子的标点。
- 打印出来的字符串仍然先问一遍磁盘，因此真带这个字符的文件依旧胜出；只有在没有这样的文件时，去掉标点的名字才作数。

## 从 0.2.0 升级

**无需任何操作。** 本次发布没有迁移：`settings.json`、`session.json`、`profiles.json`、`keybindings.json` 与 `pins.json` 均按 0.2.0 留下的原样读写，快捷键、默认值与文件位置一律未变。解压覆盖旧目录或解压到旁边，运行 `folio.exe` 即可。

唯一值得知道的是压缩包内现为**九**个文件而非八个：新增的是 `folio.msix`，它须与 `folio.exe` 处于同一目录。请整包解压，而不要只把可执行文件挑出来，否则一级菜单开关将没有可登记的对象。

## 下载与运行

从本次发布中获取 `folio-0.2.1-windows-x64.zip`，解压至存放程序的目录，运行 `folio.exe`。无安装程序；运行之前，解压目录之外不会写入任何内容。`SHA256SUMS.txt` 为所下载文件的哈希值。

需 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。

与 0.2.0 一样，`folio.exe` 与 `folio.msix` 由 **Weiyi Shi** 签名，证书来自微软 Artifact Signing 服务，并带有微软时间戳。

## 已知问题

- **新签名尚无信誉。** 首次运行时仍可能出现「Windows 已保护你的电脑」。此时「更多信息」中所列发布者为 **Weiyi Shi**，程序名为 `folio.exe`；点击「仍要运行」即可，请勿为此关闭 SmartScreen。
- **Folio 不能作为 Visual Studio Code 的内嵌终端面板。** 压缩包内的 `folio-here.cmd` 使其成为 VS Code 打开的外部终端。
- **存放于枚举较晚的显示器上的窗口在主显示器上打开。** 显示器只在窗口创建之前清点一次。
- **`.webm` 需要 Microsoft Store 的 VP9 或 AV1 视频扩展。** 出厂 Windows 两者均未安装；缺少扩展时，首帧与播放均不可用。

完整清单见仓库中的 `CHANGELOG.md`。
