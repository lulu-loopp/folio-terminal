> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.4-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.4-preview/folio-0.2.4-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit). Unpack, run folio.exe.

**下载:**上方 zip 即为完整下载,其余为校验和、物料清单与源码。

0.2.4 reads what a program prints as text and nothing else: a picture, a path, a
link or a folder that arrived in a pane can no longer make Folio read a file,
reach another machine, run a program of somebody else's choosing, or grow
without a limit. The shells took the rest of the release. A bash pane keeps its
startup files and its arguments, zsh has integration of its own, `sh` says it
has none, Command Prompt reads a folder with a `#` in its name, and a PowerShell
prompt of your own sees whether your last command actually succeeded. Every pane
on screen typesets what it printed rather than only the pane holding the
keyboard, a path Git Bash or WSL printed in its own spelling is a link, a tab is
named after the folder its pane is standing in, and a pipe table is drawn whole
or not at all.

## What a program prints is treated as untrusted

A pane draws whatever a program writes into it, and that text can come from
anywhere: a repository you cloned, a file someone sent you, an agent quoting a
web page. These are the places where Folio was acting on it.

- **A picture cannot read a file it names.** An SVG can point an `<image>` at a
  path, and Folio's renderer used to follow it, anywhere on the machine or onto a
  network share. A `.svg` only has to be printed in a pane to be drawn on sight,
  so a file someone sent you could read `C:\Users\you\.ssh\id_rsa` into the
  picture, or reach a share of theirs and hand over your Windows sign-in. Folio
  now draws only the images an SVG carries inside itself.
- **A path or a link a program printed cannot send Folio to another machine.** A
  link, a path, or a picture in a Markdown file could name a place that is not on
  this machine, and resting the pointer on it was enough: Windows offered your
  account name and password to whatever answered, and a link written
  `file://./pipe/name` reached a read with no end that stopped every later
  preview in that window. Without being asked, Folio now reads only a drive of
  this machine, or the files of the WSL distribution the pane is standing in, and
  it reads what a shortcut points at before following it. A path that fails gets
  the card that says so, a picture draws a placeholder, a link is plain text.
- **A repository Folio only reads cannot run a program of its choosing.** Two of
  the settings a repository carries in its own folder name programs for git to
  run, one on every status check and one whenever a file is diffed, so opening
  the Git page on a folder somebody sent you ran whatever that folder asked for.
  Every git command Folio runs now switches those off, along with the per-file
  diff and text-conversion programs a repository can name.
- **A name ending in a dot cannot smuggle a program past the file column.**
  Double-clicking a row opens a file and refuses to run one, and the refusal read
  the name exactly as written. Windows drops trailing dots and spaces first, so
  `invoice.exe.` was read as a document by Folio and started as a program by
  Windows. Folio now reads the name the way Windows will.
- **An address that shows one host and goes to another is refused.**
  `https://your-bank.example@somewhere-else/` has always been refused in the
  address bar; Ctrl-clicking the same address printed in a pane handed it to your
  browser anyway. The invisible characters that reverse text, which let a tab
  read `report.txt` over a pane running `report.exe`, are taken out of tab titles
  and pane heads. Titles in Chinese, Arabic and Hebrew are unchanged.
- **Output cannot make a pane grow without bound.** A request to hold output back
  until the picture is complete was kept whole so a resize could replay it, long
  after the parser had given up on it. One address behind a whole line of text
  was copied once per cell on every repaint, so a 48 KB address across 200
  columns cost nine megabytes a frame. And nine bytes of `.gif` behind a header
  claiming a 65535 by 65535 screen had Folio ask for seventeen gigabytes. Each
  now stops at a limit, and decoded pictures are let go of when they have gone
  longest unlooked at.
- **A program Folio starts to ask a question is the one that is installed.** The
  agent version check and the PowerShell probes named their programs without a
  path, and Windows looks in the folder the process is standing in first, so a
  `copilot.cmd` left in a folder you had opened would run when you opened
  Settings > Agents. Folio now finds these programs where programs are installed.
- **A command line a program printed is never typed into a restored pane.** A
  program could print shell markers around any text it liked, and that text
  became the tab's last command and went back on the prompt of the restored pane.
  Only a line your own keyboard was present for is kept now.

## Each shell is integrated on its own terms

- **A bash pane keeps the startup files, the hooks and the arguments it was
  supposed to have.** A profile asking for a plain interactive shell now reads
  `~/.bashrc`, and only a login profile reads `/etc/profile` and
  `~/.bash_profile`. Arguments you wrote yourself, such as `--noediting` or
  `-O globstar`, reach the shell instead of being dropped the moment that profile
  had shell integration. A list `PROMPT_COMMAND`, which bash 5.1 and later allow,
  runs your hooks once per prompt rather than twice, and a DEBUG trap you had
  already installed keeps running.
- **zsh gets shell integration, and `sh` stops pretending to.** A zsh pane, on
  Windows or inside WSL, is served through `ZDOTDIR`, so it draws command marks,
  reports where it is standing and carries exit codes the way a bash pane does,
  and your own `.zshenv`, `.zprofile` and `.zshrc` still run. A `sh` or `dash`
  profile is told it has no integration rather than being handed bash's flag,
  which it accepted and ignored.
- **A Command Prompt in a folder with a `#` or a space in its name is not
  forgotten.** The one spelling `cmd` has for a directory was read as an address,
  so `D:\Code\C# Projects` was recorded as `D:\Code\C` and a folder with a `%` in
  it was dropped.
- **A WSL pane sitting at `/` still knows where it is.** The root was refused as
  a directory, so new tabs opened from that pane started somewhere else.
- **A PowerShell prompt of your own is told the truth.** A prompt this terminal
  wraps, whether it is oh-my-posh, conda's or one you wrote, now sees whether
  your last command succeeded instead of always seeing success, and a prompt that
  changes directory is reported from where it left the shell rather than one
  prompt behind. The line written into your `$PROFILE` survives a script path
  holding a `$` or a backtick.
- **Three things a program asks of the terminal land where it meant them to.** A
  colour set right after a command mark is a colour again; with origin mode on
  the cursor lands where a program put it inside a scrolling region; and a mouse
  encoding this terminal cannot write is refused rather than promised.

## Every pane on screen draws what it is showing

- **A formula printed in the pane you are not typing in gets drawn.** A block
  between `$$` in a split pane that did not hold the keyboard stayed as its own
  source until you clicked into that pane.
- **A display, font, theme or language change re-lays every pane.** All four hand
  every pane new measurements, but only the pane holding the keyboard was told
  that the pictures it had already drawn were built for the old ones, so a split
  pane kept formulas rastered for the previous cell size.
- **A card shows the frame its pane is showing.** A card in the focus column was
  a frame behind whenever the terminal, rather than the program, was the one to
  put a held screenful up.

## Marks stay on their commands, and a tab ends when its shell does

- **A resize while a full-screen program is up leaves the command marks where
  their commands are.** Dragging the window edge while an editor or a coding
  agent is on screen re-wraps the screen behind it, and the ticks kept their old
  rows, so leaving the program put them in the middle of some other line.
- **Running the same command twice keeps a mark for each run.** Two prompt lines
  that read exactly alike were treated as one when the window was resized.
- **The tick on the prompt you are typing at no longer says a command is
  running.** Hovering the newest tick, the prompt with nothing typed into it yet,
  read `running · command`. It now says `at the prompt`.
- **The last shell exiting closes its tab, and closing the last tab ends the
  program.** A tab holding one pane whose shell exited stayed open with a dead
  shell inside it.
- **A pane starts in the environment this window is standing in, and closing a
  pane closes what the pane started.** Every pane used to rebuild its environment
  from the machine's registry over the one Folio was launched with, throwing away
  the `PATH` your shell exported, and a build or a server started inside a pane
  went on running after the pane was gone.

## A path a shell prints in its own spelling is a link

- **`/d/Demo/report.md` and `/mnt/d/Demo/report.md` are links now.**
  `D:\Demo\report.md` was recognised in every pane and the Unix spellings in
  none, so in the two shells that spell a path their own way, the names on the
  screen were the ones you could not click, hover, or `Ctrl`-click into an
  editor. A pane reads the spelling its own shell prints: `/d/Demo` and
  `~/notes/a.md` in a Git Bash pane, `/mnt/d/Demo` in a WSL one. Which spelling
  comes from the profile the pane was started from and not from the text, so
  `/d/Demo` typed into a PowerShell pane is still ordinary words.
- **A file inside a WSL distribution opens too.** `/etc/hosts` and `~/notes.md`
  printed in a WSL pane are links to the same file Windows opens at
  `\\wsl.localhost\<distribution>\…`, the distribution that pane's profile
  starts. A share printed anywhere, and a `\\wsl.localhost\…` written out as
  text, names nothing, exactly as before.
- **A path right after a prompt's `user@host:` opens.** Ubuntu's own prompt
  writes the folder you are standing in behind that colon, and the colon was
  read as the kind that belongs to a scheme, so `alice@box:~/notes` was ordinary
  words.
- **A file inside the folder card previews on hover, like a file anywhere else.**
  Hovering a folder path opens the folder card and its tree; resting on a file
  row inside that tree showed nothing. It now opens that file's preview beside
  the card, on whichever side has more room, after the same 350ms and at the same
  size. The folder card stays where it is, and a folder row still just expands.

## A tab is named after the folder its pane is standing in

- Whatever shell is in it. A PowerShell tab used to be called `PowerShell 7` for
  the life of the pane, with the pane standing in `D:\Demo` and its own pane head
  saying so, because the integration script ends every prompt by announcing the
  name the profile already goes by. A shell that merely repeats its launcher's
  name has announced nothing, so the folder names the tab.
- A shell that sets a title of its own is still shown saying it, such as Git
  Bash's `MINGW64:/d/Demo`, and a tab you renamed keeps your name. A tab also
  wears its folder from the moment it opens, rather than the profile's name until
  its shell reaches its first prompt.

## A second copy of Folio leaves your menu entry alone

- On Windows 11 the entry on the first page of the right-click menu belongs to
  whichever folder Folio was last registered from, and every launch used to claim
  it. So running a second copy once, a build you were trying out or a copy in
  `Downloads`, pointed that menu item at the second copy for good. The item is
  now repaired to point here only when `folio.exe` is gone from the folder it
  names, which is what happens when you move Folio, or when the file there is
  this very one under another spelling. Otherwise nothing is touched, and the
  Explorer row in Settings says the menu item belongs to a copy in another
  folder. The classic entry under *Show more options* has behaved this way since
  0.2.3.
- **Three more things about that menu.** An entry left half written by an
  interrupted registration is recognised and written again. A failed query to the
  Windows package database used to read as "nothing is registered", so turning
  the switch off reported success over an entry that was still there. And the
  entry registers from a folder with `#`, `?` or `%` in its name.

## The repository is nine directories

Fixtures moved under `tests/`, the CI helpers under `scripts/ci/`, the design
prototypes and the icon round under `docs/design/` with the shipped mark under
`assets/app-icon/`, and the August handoff under `docs/handoff/`.
`docs/BUILDING.md` lists what each directory holds. Nothing about the program
changed; this is for anybody who builds it from source.

## A local page in the preview reads only its own folder

- **A `.html` file opened from the file column reads its own folder and the
  folders under it, and nothing else.** It could name a picture, a stylesheet, a
  script or a frame anywhere on the disk, or on a network share, and the preview
  fetched it: only the address of the page itself was checked. Pages opened from
  an address load their own contents as before, and now cannot read the disk at
  all.
- **A local page stays a local page**, rather than following a link, a redirect
  or its own script onto the network carrying whatever the local document had
  reached. A page that starts a download has that download cancelled, since the
  preview writes no files, and the address goes onto the card the cancelled
  download raises rather than straight to your browser.
- **A page cannot hold a preview pane shut with its own message boxes.** The
  engine's default was to open a modal window for every `alert`, `confirm` and
  `prompt`, and a page asking in a loop opened another the moment you dismissed
  one. The pane now answers those itself and says so on its bottom strip.
- **A preview recovers instead of sitting empty.** A browser that exits under an
  open page rebuilds the page, and a pane that asked for an engine and heard
  nothing drew a card whose Retry button did nothing; it now asks again. Closing
  a preview pane also lets go of everything opening it took, so a pane torn out
  and closed can be torn out again.

## What Folio has written down

- **A saved tab whose shell is no longer on the machine opens as a tab.**
  Uninstalling Git with a Git Bash tab pinned stopped Folio before its first
  window, on every launch, until the saved session was edited by hand. Such a tab
  now opens in the folder it was standing in, running the shell the machine does
  have, with a first line naming the one it could not start. A
  `terminal_font_size` of `0` in `settings.json` ended the launch the same way;
  any stored size is now brought into the range Appearance offers.
- **A preferences or session file Folio cannot read is kept.** A file that would
  not parse, or that a newer Folio wrote, was replaced by defaults with one line
  on a console nobody was watching. Folio now copies it beside itself as
  `<name>.rejected-` and the date and time before anything can replace it, and
  says on the window which file it was and where the copy is. The same is true of
  `keybindings.json`, `profiles.json` and `pins.json`.
- **A second Folio no longer erases the first one's preferences and tabs.** Two
  processes each held the whole of `settings.json` and `session.json` from the
  moment they opened and wrote all of it back, so whichever wrote last erased the
  other's work. Only one process now writes those two files; a second one keeps
  its window and every gesture in it, and says once that nothing in it is saved.
- **A restored window always comes back with its title bar on a monitor.** A
  stale or hand-edited position could bring the window back above the top of the
  desktop with nothing to take hold of. A window parked half off the side is
  still left where you parked it.
- **A setting that could not be saved is saved when you choose it again**, and a
  file that keeps refusing to be written says which one it was and stops trying
  rather than attempting a save every second and a half for as long as the window
  stays open.
- **Your PowerShell profile is replaced in one step, and every write takes a
  copy.** Adding the integration line emptied the file and then wrote it, so a
  shell starting at that instant read a profile with nothing in it. The
  integration file that line points at is now compared against what ships and
  written again when it differs, so an upgrade or a cleaner no longer leaves your
  shells reading an old copy while the setting says it is installed.

## A pipe table is drawn whole or not at all

- A table an agent prints arrives a row at a time, and a row can arrive damaged.
  A cell holding a bare `|` counts as extra columns, and a program that lays out
  its own output wraps a long row onto a second line. Either one used to stop the
  table where it stood, so the rows above were drawn as a table and everything
  after them stayed text, with no sign that a row was missing.
- A row the program itself wrapped is now put back together and drawn as the one
  row it was printed as, at the width the terminal was when it was printed, so
  dragging the window does not split it again. And a line that starts with `|`
  and is not a row of the table takes the whole table down, including the rows
  already drawn, so output that merely starts with a pipe no longer leaves half a
  table on the screen.

## Also fixed

- **The key that summons the quick terminal raises it when it is on screen and
  you are working somewhere else**, rather than sending it away and giving the
  keyboard to a third window. A hung program in front no longer stops Folio doing
  it, and the recorder refuses a bare letter, which used to claim that key from
  every program on the machine for as long as Folio ran.
- **One toast per turn, whatever a program prints**, the way the attention dot
  always has, and one pane's hook stuck in a loop no longer spends the whole
  window's allowance.
- **Hovering and browsing cost what they show.** A rendered Markdown page reads
  the pictures on screen and several either side rather than every image in the
  document at once; a hover over a three hundred megabyte `.pdf` no longer holds
  up every card behind it; and a folder with a hundred thousand names in it keeps
  only the two thousand rows the file column will draw.
- **A search hit on a line with wide characters is highlighted on the character
  it found**, rather than one cell to the right for every CJK character in front
  of it.
- **Show in Explorer only hands over a file that is really there**, rather than
  opening whatever folder Explorer falls back to when the path has gone. The card
  that refuses to read a file on another machine also lost its *Open in default
  app* button, which handed the share straight to the shell.
- **A profile row that cannot name a variable changes nothing**, rather than
  quietly setting a different variable in every pane of that profile, and a
  profile whose program is a `.cmd` or `.bat` with an `&`, a `|` or a `^` in its
  path runs the file you picked.
- **Dismissing the update mark while a check is running is not undone by it**, a
  window that could not come back after a graphics device was lost is no longer
  reported as recovered, and the watchdog now asks a window parked with nothing
  to wait for whether it is still answering.

## Upgrading from 0.2.3

**Nothing to do.** Your existing settings are preserved when you upgrade. Unpack
over the old folder, or beside it, and run `folio.exe`. The archive holds the
same **nine** files 0.2.3's did, and `folio.msix` still belongs in the same
folder as `folio.exe`.

## Download and run

Take `folio-0.2.4-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer; keep the extracted files
together in one folder. `SHA256SUMS.txt` is the hash of what you downloaded, and
`folio-0.2.4.cdx.json` is the bill of materials for what is in the build.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service and a Microsoft time stamp, as they have
been since 0.2.0.

**`folio.msix` is not an asset of its own.** It is in the zip, and that is the
only place it works from: the package names the folder it was extracted into, so
a copy downloaded on its own points at a folder with no `folio.exe` in it.

**Folio is not on winget yet.** A manifest is with the winget community
repository and is waiting for a reviewer. This zip is the way to install Folio
until it is accepted.

**Folio has no telemetry, no analytics and no crash reporting.** Two things reach
the network: a page you open in the web preview, and the update check, one `GET`
of the releases list at most once a day, which you can switch off at Settings >
General > **Update check**. `docs/PRIVACY.md` says what is written to disk.

## Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise
  **"Windows protected your PC"** on the first run. **More info** names
  **Weiyi Shi** as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` in the
  archive makes it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **Two web previews can overlap for about a fifth of a second while panes are
  moving to new places.** Panes standing still never overlap, and a divider drag
  does not do it.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.
