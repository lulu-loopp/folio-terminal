# 现有终端痛点调研（2026-07）

> 调研方法：microsoft/terminal GitHub issues 按 👍/评论数结构化检索 + iTerm2 GitLab + Reddit/HN + danluu.com、jeffquast.com 等深度文章。

## 一、按主题分类的痛点

### 1. 性能与延迟（渲染/输入响应）
- **iTerm2 大量输出后全局变卡**：大 buffer 后连打字都卡，输出速度逐渐衰减（iTerm2 GitLab #6897，长期未解决）。
- **iTerm2 GPU 渲染反而更慢**：GPU 渲染 53 万行耗时 57s，关闭后仅 ~3s（GitLab #6767）。
- **输入延迟被系统性忽视**（danluu.com《Terminal latency》）：Terminal.app ~5ms 最优；iTerm2、Hyper 达 32–44ms；99.9 分位不少终端超 100ms。业界普遍用 stdout 吞吐量做基准而非输入延迟，优化方向长期错位。
- **GSync/Freesync 掉帧**（WT #649，214👍，199 评论，长期开放）：144Hz 显示器帧率反复横跳。
- **Windows Terminal 启动慢**（#5907/#3437/#4886/#9905）：相比 conhost"秒开"，WT 需 3–7 秒。
- **XTerm on WSL 冷启动 1.6s vs Linux 原生 300ms**（Tavis Ormandy, lock.cmpxchg8b.com/slowterm.html）："死于一千个小口子"。
- **WSL 终端打字手感反差**（WT #327，218👍）：WSL 终端 ~10ms，普通 Windows 应用 75ms+，Electron 应用 200–250ms+。

### 2. 字符宽度 / 字体渲染 / Unicode
- **CJK 宽字符显示错位**（WT #370，长期开放）：ambiguous width 处理不一致，几乎所有终端都踩过。
- **Emoji 显示错位/半宽**（WT #900、#16852）：East Asian Width 属性 vs 实际字形宽度不一致。
- **连字与 shell 提示符冲突**（#938、#1903）。
- **jeffquast.com《State of Terminal Emulation 2025》**（HN 267 分）：零宽连接符、变体选择符、grapheme clustering 宽度计算复杂；仅 Kitty、Ghostty 支持 VS15；协议兼容性高度碎片化，各终端"谎报"能力查询。

### 3. 焦点 / 输入 / 交互 bug
- **新建 Tab/Pane 后无法立即输入**（WT #13388，200 评论，4 年+未修复）：焦点被转移到隐藏 ConPTY 窗口。
- **鼠标/触控绑定缺失**（#1553，133👍）：无中键粘贴等。
- **面板无法鼠标拖拽调整大小**（#992，94👍）。
- **Ctrl+C/Ctrl+V 与 WSL/Vim 语义冲突**（#5790，118👍）：缺按 profile 区分快捷键的能力。

### 4. 功能缺失（高呼声请求）
- **水平滚动条**（#1860，252👍 —— 开放 issue 中最高）。
- **tmux Control Mode**（#3656，221👍）。
- **关闭光标闪烁**（#1379，218👍）。
- **图形协议/终端内图片**（#5746，170👍；sixel 已关闭 issue #448 曾达 526👍）。
- **无限回滚**（#1410，153👍）。
- **Quake 模式全局呼出**（#9996，148👍）。
- 历史高热度（已实现）：quake 呼出 653👍、右键在此打开 673👍、emoji 568👍、OSC 7 同目录新标签 536👍。

### 5. 配置管理
- **settings.json 更新被静默重排/丢注释**（#18110）。
- GUID 标识 profile 心智负担；"手写 JSON 太难 vs GUI 覆盖不全"两难。

### 6. 剪贴板
- **WSL2 粘贴混入控制字符**（#18579，仅 WT 出现）。
- **OSC 52 支持请求**（#9479，69👍）。
- 网页"paste and run"安全风险是 HN 反复讨论的话题。

### 7. SSH / 远程
- SSH 会话延迟可达数秒；tmux+SSH 下滚动回溯行为诡异（#18357，alt buffer 与滚动条冲突）。

### 8. 2026 最新
- **Win11 24H2 GPU 冻结 bug**：后台 3D 应用导致 Terminal 冻结（AtlasEngine × NVIDIA WDDM 3.2）。
- Warp：强制登录/隐私顾虑、UI 视觉噪音；但 AI 能力领先。

## 二、Windows 平台特有痛点
1. conhost/PowerShell legacy 模式 ANSI/VT 不渲染
2. Ctrl+C/V 语义冲突（缺 per-profile 快捷键）
3. WSL 剪贴板混入控制字符
4. 水平滚动条缺失
5. 新建 Tab/Pane 焦点丢失（ConPTY 架构 bug，4 年+）
6. GSync/Freesync 掉帧
7. settings.json 更新破坏用户配置
8. 启动慢
9. Win11 24H2 GPU 冻结
10. Run as Administrator 支持不足（#4459、#4217）

## 三、最值得新终端解决的 Top 10
1. **输入延迟作为架构级核心指标**（而非吞吐量）
2. **焦点管理健壮性**（新建 pane 立即可输入）
3. **Unicode/CJK/emoji 宽度计算正确性**（对中文用户尤其关键）
4. **大输出量下性能不衰减**（buffer 数据结构层面设计）
5. **水平滚动 + 无限回滚**作为一等公民
6. **配置系统人类友好 + 程序友好 + 更新不破坏自定义**
7. **剪贴板跨子系统（本地/WSL/SSH/tmux）一致且干净**（含 OSC 52）
8. **快捷键按 profile/上下文可覆盖**
9. **秒开启动/新建窗口**
10. **终端能力可被程序准确探测**（图形协议、DEC 模式查询不"谎报"）

## 来源
- microsoft/terminal issues（gh api 按 reactions/comments 排序）
- https://danluu.com/term-latency/
- https://www.jeffquast.com/post/state-of-terminal-emulation-2025/
- https://lock.cmpxchg8b.com/slowterm.html
- iTerm2 GitLab (gnachman/iterm2)
