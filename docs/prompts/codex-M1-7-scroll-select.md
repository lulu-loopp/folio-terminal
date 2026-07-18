# M1.7 滚动回溯 + 选区复制片 kickoff

基线=已提交 64483dd。这是终端日用的最后两块大缺口:上滚看历史、选中复制。二者共享鼠标基建,一片交付。架构约束:DESIGN 已裁决 vendor scrollback=0,**转录层拥有历史**(bt-viewport 的 staging/frozen 基建已在,M0 起就在捕获 scroll-out 行)——滚动视图 = 视口在「转录 + live 网格」连续体上的窗口,不是 alacritty display_offset。

## 0. 侦察先行(写进交付报告,不必停下等裁决,除非发现架构级阻塞)

摸清现状:frozen/staging 行的存储形态、渲染层现在如何只画 live 网格、视口向上扩展到转录需要动哪几层(bt-viewport/bt-term/bt-render/bt-app)。报告里给出接线图。

## 1. 滚动回溯

- 输入:鼠标滚轮(行数遵循系统每格行数设置)、Shift+PgUp/PgDn(整页)、Ctrl+Home/End(顶/底)。
- 视图锚定语义(核心,不许打折):上滚后视图**冻结**——新输出到来不得拉动视图;显示「底部还有 N 行」的最小指示(样式从简,右下角文本即可);任何会向 PTY 发字节的按键 → 跳回底部;回到底部后恢复跟随。
- alt-screen(vim/less 活动时):无本地滚动语义——滚轮转发为方向键(alternate scroll 约定),退出 alt-screen 恢复本地滚动。
- 滚动条视觉本片不做(UI 片的活),最小指示即可。
- 性能:上滚渲染冻结行不得整转录重排——视口窗口化取行,复用既有行缓存/整形缓存机制。

## 2. 选区 + 复制

- 鼠标基建:像素→格命中(用渲染 metrics,含 padding/DPI),**做成可复用模块**——P2-15 超链接下一片就要用它。
- 选区模型:优先复用 vendor alacritty 的 selection.rs 语义(anchor/range);单击拖拽=线性选区(可跨转录/live 边界)、双击=词(分隔符集写明)、三击=行;**宽字符/emoji 簇不可被切半**(选区边界钳到簇边界,M1 的宽度判定是唯一裁判)。
- 渲染:选区高亮为背景层矩形(theme 加 selection 底色常量,前景色不动;走既有 rect 管线,在文字之下光标之上/之下你裁量并写明)。
- 复制:**Ctrl+C 有选区时=复制并清选区,无选区时=照旧发 ^C**(WT 默认行为);Ctrl+Shift+C 恒复制;copyOnSelect 不做(记为将来设置项)。文本抽取:软换行(视口折行)重接**不插换行符**,硬换行按 CRLF;尾随空白修剪写明政策。剪贴板写入经 bt-platform 现有 Win32 边界(镜像 M1.6 的读)。
- 选区失效:开始新选区/任何输出到达选区所在行(行内容变化)/发送输入时清除,政策写明。
- 鼠标转发:应用开启鼠标模式(vendor TermMode 有 MOUSE_* 与 SGR_MOUSE)时,点击/拖拽/滚轮按 SGR 1006 编码转发给 PTY(vim 里点击定位要能用);**按住 Shift 绕过转发、走本地选区/滚动**(业界约定)。模式状态一律从 vendor Term 读。
- IME:组字进行中鼠标操作不得破坏 preedit(至少不崩、不吞组字;交互细节从简记录)。

## 3. 测试与验收

- 单测矩阵:选区词/行/宽簇边界、软硬换行抽取、滚动 clamp/锚定、alt-screen 滚轮转发字节、SGR 鼠标编码、Ctrl+C 双语义。
- 已知限制如实记录:注入鼠标/键盘进不了 winit(ui-probe 已记录),鼠标手感与滚动跟手性靠人工验收——给用户写分页验收脚本(紧凑,参照 m1-6 脚本的格式);协调者可用 BT_PROBE_INPUT 灌长输出后做「渲染正确性」侧的像素验收,但滚动/选中操作本身自动化不了,honest 分工写清。

## 门禁

cargo test --workspace --locked;cargo clippy --workspace --all-targets --locked -- -D warnings;fmt;bt-render 46、vendor 各计数不降;宽度/字形/盒线基线零回退。完成停下等审。若侦察发现视口跨转录渲染需要大改(架构级),先停下报方案。
