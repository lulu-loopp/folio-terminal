# 模态背后的留存帧 —— 实机取证(2026-08-30,DESIGN §7.8 ⑩)

用户在 `next22` 报的缺陷 #203:开着 pdf/网页的 pane,打开设置对话框,那一片 pane 整个空白。

**跑法**:debug 构建,隔离 `APPDATA`/`LOCALAPPDATA`(所以是隔离的 WebView2 UDF)、`BT_PTY_DUMP`、
`BT_WEB_TRACE` 各自一个文件,`BT_WEB_DEV` 指向本机 `127.0.0.1:8642` 上的一张静态页与同一台服务器上的
`folio-pdf-test.pdf`(不访问外网)。窗 1920×1200 物理像素、DPI 192/scale 2。只关自己起的窗。

| 图 | 是什么 |
|---|---|
| `1-page-live.png` | 网页 pane 活着:渐变底、标题、正文。 |
| `2-page-under-the-dialog.png` | 设置对话框开着。对话框右侧露出来的那一条**是那张页最后一帧**(渐变与 `…URE` / `…ws this` 两行),照常被遮罩压暗——修之前这里是 pane 的底色。 |
| `3-page-live-again.png` | 对话框关掉,活页回来,恢复全亮。 |
| `4-pdf-under-the-dialog.png` | 同一件事在 pdf 上:对话框右侧露出的是 PDF 阅读器最后一帧(正文、滚动条、工具条的搜索与 `…`),压暗。 |
| `5-pdf-scrolled-after.png` | 关掉对话框之后滚轮把 pdf 翻到 **2 of 3** —— 页是活的,不是一张冻住的图。 |

**`BT_WEB_TRACE` 的对照行**(两个数必须同时成立:页真的藏了,而屏幕上有像素):

```
place tab=1 seat=2 … presence=Hidden       above=- obstructed=1 carded=0 front=1 sourced=0
place tab=1 seat=2 … presence=Shown(…)     above=- obstructed=0 carded=0 front=1 sourced=0
```

网页那一跑里还留了一行 `presence=Hidden … obstructed=0 carded=1`:地址栏里打错的地址导航失败、
座位换上失败卡——**那一格不画留存帧**,因为卡本来就是该看见的东西(§7.8 ⑩「五个理由里只有模态这一个画帧」)。

收尾:两扇窗都由本进程关掉,机上残留的 `msedgewebview2` 全部属于 Windows 自己的 shell(`…\MicrosoftWindows.Client.CBS_…\EBWebView`),
本次跑出来的 UDF 一个进程都没剩。
