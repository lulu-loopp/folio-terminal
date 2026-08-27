# 视频播放器控件条 — 实机证据（2026-08-27，§7.23 ⑪）

> **The pictures are not in the repository.** The terminal pane beside the player shows the machine's own account name in its prompt, and `docs/screenshots/README.md` forbids a committed picture that names an account. The eight frames described below were taken and reviewed at merge time (2026-08-27) and live in the session's scratchpad; retake them with a fixture prompt if they are needed again.


用户在 `next12` 上报的两条缺陷的验收照。八张，亮暗各四张，同一台机器、同一份 fixture、同一条取证路径。

## 怎么取的

release 构建（`cargo build --release -j 4`），**每次都是一个全新的隔离档案**：

```
$env:APPDATA      = <scratch>\iso-<mode>\Roaming     # settings / session / schemes
$env:LOCALAPPDATA = <scratch>\iso-<mode>\Local       # WebView2 的 UDF 与 %LOCALAPPDATA%\Folio\player
$env:TEMP/$env:TMP= <scratch>\iso-<mode>\Tmp
$env:BT_PTY_DUMP  = <scratch>\iso-<mode>\pty.log
$env:BT_BG        = #1B1B1B (dark) | #FFFFFF (light)
folio.exe <repo>\test-assets\folio-video-test.mp4
```

`BT_BG` 是本仓自己的诊断入口（`ThemeState::from_environment`），它锁住画布底色，而
`chrome_palette()` 正是按底色的亮度选亮/暗那一套 —— 所以这一条同时换掉窗口和壳页的整套令牌，
不需要去改一份 `settings.json`。**档案每轮清空**：不清的话，上一轮被 `Stop-Process` 结束掉的窗口会让
下一轮先弹一张「Reopen your other tabs?」，而那张卡会把探针的点击吃掉（第一轮就是这么废掉的）。

驱动与取像都走 `scripts\dev\ui-probe.ps1`（`click` / `hover` / `capture`），1:1 物理像素。

## 八张

| 文件 | 是什么 |
|---|---|
| `dark-1-frame.png` / `light-1-frame.png` | 还没播：首帧 + 正中的播放键，下面两行事实 |
| `dark-6-playing-bar-shown.png` / `light-6-…` | 播放中，控件条**显形**：暂停键、`0:01`、进度、`0:05`、喇叭、音量、`1×` |
| `dark-7-playing-bar-hidden.png` / `light-7-…` | 仍在播，指针停在画面上不动 2 秒后，控件条**隐去** |
| `dark-8-bar-detail.png` / `light-8-…` | 上面那条 bar 的等比放大（近邻，没有重采样），看清楚发丝线、墨色与强调色 |

## 这两张对照着看的是什么

**① 视频填满 pane。** `-1-` 那张里 160×120 的录像是屏幕中间一块邮票（那是**首帧**，走本窗自己的
光栅，`fit` 只缩不放，§7.23 ③，本片没有碰它）；`-6-` 那张里同一段录像铺满整块 pane 的宽度、
上下留出 pane 自己的底色 —— 那是壳页的 `object-fit:contain`。`next12` 的壳页在这一格里画的还是那块邮票。

**② 控件条是本窗的。** `-8-` 那张里没有一件 Edge 的东西：没有白底、没有 Material 图标、没有 `⋮`，
播放/暂停与喇叭是符号表里 `#i-play` / `#i-pause` / `#i-speaker` / `#i-speaker-mute` 四枚，
数字是 `--ink2`，进度与音量是 `--accent`，条顶一根 `--border-soft`。亮暗两张的差别正好是那一套令牌的差别。

## 顺手抓到的一条缺陷（已修，写在 §7.23 ⑪(f)）

第一版把「条不落」的第三条判据写成 `bar.contains(document.activeElement)`。用鼠标按一下播放键就把
焦点给了它，于是条一升起来再也不落 —— 上一轮的 `dark-7` 拍到的正是这个。改成 `:focus-visible` 之后
这一格才是现在这张。
