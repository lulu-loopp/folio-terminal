# M2 Restart Shell 契约（草案）

> 本文是 M2 规格债第三块料：`docs/DESIGN.md` §7.1.7③「Restart shell 的完整进程契约
> （elevation/token 处理——最可能被平台收窄的承诺,见审核记录）」的正式化。上位裁决权威
> 是 `docs/DESIGN.md` §7.1.4「会话生命周期与持久化」原文里 **Restart shell** 那一段、
> `docs/UI-UX.md` §8.5.2「Pin 与恢复：'上下文'是三样东西」的可恢复性表格,以及
> `docs/M2-persistence-schema-v1.md`（`session.json` 字段、哨兵崩溃判定、三条退出路径）。
> 本文不推翻三者的任何一条,只把"重启这件事到底做了什么"钉死到字段/进程/失败路径级别,
> 并且——这是本文存在的真正理由——**把两个共享同一个菜单动词名字、却是完全不同事件的
> 东西分开钉死**：per-seat 的 **Restart shell**（右键菜单,杀一个进程重开）和 app 级的
> **restart/restore**（关掉整个 app、下次启动时重建窗口）。二者在"这次重开丢了什么"
> 这件事上给出的答案不一样,混着说会两头都说错。
>
> 状态：草案；原 §5 三项待裁已于 **2026-08-03 用户裁决**,详见 §5（现为已裁记录）。其中
> ②（elevation/token）是**临时裁决**,标注为 provisional,待 §1.7 登记的 Windows 提权/
> 令牌 API 探测完成后再定稿最终裁决——本文其余部分不受此影响。

## 0. 两种"重启"，一张对照表

**先把最容易读混的地方摆平**：`docs/DESIGN.md` §7.1.4 说 Restart shell「**重启前的转录
保留**、插入一条重启边界记录」，而 `docs/UI-UX.md` §8.5.2 说转录「**不能**」恢复。这两句
话都对，因为它们在回答两个不同的问题：

| | Per-seat **Restart shell**（本文§1） | App **restart/restore**（本文§2） |
|---|---|---|
| 触发方式 | 单个 pane 右键菜单 | 关闭整个 app → 下次启动 |
| app 进程本身 | **同一个** app 进程,只杀这一个 seat 的 shell | app 进程本身也重开了一份新的 |
| 转录（canonical transcript）所在 | 活在触发这次重启的**那个 app 进程的内存里**,该进程没死,转录没跟着死 | 转录只活在**上一个** app 进程的内存里,那个进程已经不存在了 |
| 转录的命运 | **保留**（`docs/DESIGN.md` §7.1.4 原文）——旧内容照常经 Live→Frozen 定稿进转录,插一条边界记录,新 shell 的输出接着往下长 | **不存在**（`docs/UI-UX.md` §8.5.2 已决）——没有旧进程内存可继承,磁盘本就不存转录（`docs/M2-persistence-schema-v1.md` §0/§4） |
| 位置（cwd/树/pin/名字） | 保留（在世 app 进程的运行时状态,不需要序列化） | 从 `session.json` 反序列化恢复 |
| 进程 | 杀掉、重开 | 全部重开（app 本身也是重开的） |

**统一的认识论**：两条路径都只承诺"最后一次可信已知"，不承诺"读到活进程的真实状态"——
per-seat 重启用的 cwd 是**最后一次 OSC 7 上报**，不是去读那个即将被杀掉的进程的真实工作
目录；app 恢复用的 cwd 是**上一次 debounce 落盘时点的快照**（`docs/M2-persistence-schema-v1.md`
§5.1），不是崩溃前最后一刻的真实状态。两者都诚实地标为"最后已知"而非"当前"，这不是各自
独立的两个决定，是同一条认识论纪律的两处应用。

## 1. Per-seat Restart shell 契约

菜单项：终端右键菜单第三项，`Restart shell…`（省略号，`docs/DESIGN.md` §7.1.6：破坏性
动作沉底带省略号）。作用对象是**一个 pane**，不是整个 tab/窗口。

### 1.1 保留

| 项 | 来源 | 细节 |
|---|---|---|
| **cwd** | 最后一次可信 OSC 7 上报 | 无上报则该 seat 的初始 cwd；再退 `HOME`。**不承诺读活进程的真实 cwd**（`docs/DESIGN.md` §7.1.4 原文）——这是"最后已知"，不是"当前"，即使旧进程在被杀死前一刻确实 `cd` 去了别处，只要它没再发一条 OSC 7，重启就看不见那次 `cd` |
| **profile / 可执行路径** | 该 seat 创建时敲定的 `profile_id`（`docs/M2-persistence-schema-v1.md` §3.3：v1 取值是启动时实际使用的 shell 可执行路径） | 重启用的是**这个 seat 自己的** profile_id，不是"当前默认 profile"；同一 seat 反复重启永远用同一个可执行文件（除非 §3「shell 可执行文件缺失」的降级触发） |
| **手动名** | 用户设置的 `manual_name` | 原样保留，重启不清空、不改写 |
| **在布局树中的座位** | 二叉树结构（`docs/DESIGN.md` §7.1.1） | 重启只换叶子内容的运行时状态，**不触碰树形状**；pin 归属、tab 归属、pane 在树中的位置全部不变——这不是"重建一棵新树再放回原位"，是同一棵树的同一个叶子换了个运行时 handle |
| **转录（canonical transcript）** | 触发重启的 app 进程自己的内存 | 见 §0——旧 shell 产生的全部内容（已冻结历史 + 即将冻结的 live/staging）保留，只是插入一条重启边界记录（见 §1.3） |

### 1.2 切断

| 项 | 细节 |
|---|---|
| **进程本体** | 先温和退出信号，超时強杀；`docs/DESIGN.md` §7.1.5b：`Restart`/关 pane 走 Job Object 杀整棵进程树，不留孤儿 |
| **该进程的全部子进程** | 同上，Job Object 语义覆盖整棵树，不只是根进程 |
| **旧进程尚未被消费的挂起输出** | 以 **generation token** 作废（`docs/DESIGN.md` §7.1.4 原文）——与 §3.2 worker 任务用 generation 校验丢弃是同一机制的另一处实例：旧进程可能已经把字节写进了 PTY 管道但还没被读到/渲染，杀进程发生后这些字节不得继续影响画面，用 generation 比较而非"祈祷时序凑巧"来保证 |
| **旧进程持有的 OSC 133 未闭状态** | 见 §1.3 |
| **旧进程的环境块继承** | 见 §1.4——新进程不继承旧进程的环境块，从系统重新取 |

### 1.3 重新建立

- **全新 PTY / ConPTY 句柄**：不是复用旧 PTY 再灌入新程序，是新建一个 ConPTY 会话。旧 live
  网格的当前内容（无论是否已经因为 restart 而被冻结）不会被新进程继承或看见——新进程从一
  个空白网格开始打印。
- **全新 OSC 133 状态机**（`docs/shell-integration.md`「Authority is scoped per screen」）：
  重启产生的是一个新的 screen 身份，旧屏幕上任何处于 B→C 之间（命令已开始输入/输出、尚未
  见到 D）的未闭合区域，在重启边界处**按既有的"格式错误标记保守恢复"同一纪律强制闭合**
  （`docs/shell-integration.md`「Accepted v1 trust boundary」段落旁的恢复规则：A/C/D 会闭合
  一个未终止的命令）——重启边界起到一个隐式 D 的作用，不留悬空区域,但不虚构一个退出码。
  新屏幕的 OSC 133 authority 从零开始：只有新 shell 真的重新 source 了集成脚本并发出新的
  A/C/D,这块新live区域才重新获得基于标记的权威,否则退回既有的 cursor/WRAPLINE 启发式
  （与任何从未发过 OSC 133 的屏幕同等对待）。
- **全新 TERM_PROGRAM/TERM_PROGRAM_VERSION/COLORTERM 声明**：这不是"设计上应该"，是
  `crates/bt-pty/src/lib.rs` 已经实现的行为——每次 spawn（含 restart 触发的这次）都会声明
  `TERM_PROGRAM=BetterTerminal`、`TERM_PROGRAM_VERSION`、`COLORTERM=truecolor`（除非调用方
  环境已显式设置，此时不覆盖）；重启不需要任何特殊代码路径去"记得"重新声明，走的是与任何
  spawn 相同的一条路。
- **重启边界记录**：插入转录，作为旧内容与新内容之间**可见的分界**（`docs/DESIGN.md` §7.1.4
  「插入一条重启边界记录」）。这条记录与 §2.3 app 恢复时的「new shell in `<cwd>`」首行横幅
  是**同一条纪律的两个实现**——都是"下一批内容开始于此，之前的不是它接着说的"这句诚实声明
  在两种上下文里的版本；建议二者复用同一条边界渲染机制而非各写一套（非本文待裁项，是
  实现层面自然的复用，`docs/DESIGN.md`「文本永远是真相 / 用户必须能分清哪些像素是谁说的」
  同一条纪律的直接推论）。
- **新进程的环境块**：见 §1.4。

### 1.4 环境重建（P2-12）

`docs/PROBLEM-LIST.md` P2-12「Restart shell 承诺拿到新值」——新进程 spawn 前从注册表
重建环境块，**不继承**触发重启的 app 进程启动时的旧环境拷贝（对齐 Windows Terminal
`compatibility.reloadEnvironmentVariables` 先例）。这解决的是"改了 `PATH` → 右键重启 shell →
生效"这个顺路工作流，且不需要向运行中进程注入刷新命令（那条路是 P2-12 里明确排除的邪道）。
`docs/DESIGN.md` §7.1.4 原文把这条与 cwd/手动名保留并列，本文只是把它接到 P2-12 的完整
定义上——两处原文说的是同一件事。

### 1.5 「↑ 历史不丢」的措辞边界

`docs/DESIGN.md` §7.1.4 原文：「↑ 历史不丢」的 UI 文案**仅限检测到 PSReadLine 且其持久
历史启用的 profile**，对 `cmd.exe`/`bash` 不许诺——这两者的历史机制不由 BetterTerminal
控制，重启后历史是否还在完全取决于该 shell 自己的持久化行为，我们没有能力也不应该在文案
里替它打包票。`docs/PROBLEM-LIST.md` P2-12 的旁注同样指出：PSReadLine 历史落在
`ConsoleHost_history.txt`，本就与进程生死无关，用户对"重启会丢历史"的担心大半是误解，
文案可以点明这点，但**仅限已验证的 profile**。

### 1.6 busy 判定与重启确认（已裁，2026-08-03）

`docs/DESIGN.md` §7.1.4：无 OSC 133 时保守视为 busy。这与 §7.1.5b 的七态状态分类学共用
同一套信号源（OSC 133 C/D 边界）；一个 screen 若从未发过 OSC 133，重启前无法区分它"正在
跑一个耗时命令"还是"安静地停在一个自绘 TUI 里"，保守假设是 busy。

**裁决（§5#1 B）：仅 busy 时弹确认，空闲直接重启。** 空闲 = 已见 OSC 133 A/B、停在提示符
（或该判定的等价稳态）；busy = OSC 133 C 之后未见 D，**或该 screen 从未发过 OSC 133（保守
视为 busy）**。确认对话框只出现在后一种情况，前一种情况点击 `Restart shell…` 立即执行，
无中间步骤。这让本节的 busy 判定第一次有了真正的消费者——此前 `docs/DESIGN.md` §7.1.4 定义
了这条判定却没有下文，是一条死计算；本裁决把它接上。

### 1.7 elevation/token 处理（临时裁决，2026-08-03；追踪项待定稿）

`docs/DESIGN.md` §7.1.7③ 原文点名的开放项：被重启的 seat 若原本以非默认访问令牌启动
（管理员提权、`runas` 切换用户上下文），新进程该怎么办。

**裁决（§5#2 临时 B）：一律以非提权身份重启，并显式横幅声明降级**（如"已以非管理员身份
重启"）。符合 P2-12「不继承旧进程陈旧状态」的既有精神，且不静默声称一个我们给不出的能力。

**这条裁决是 provisional（临时），不是终裁**：`docs/DESIGN.md` §7.1.7③ 原文已把 elevation/
token 处理标注为"最可能被平台收窄的承诺"，在真正做过 Windows 提权/令牌 API 实测之前，
不确定「一律降级」是不是唯一安全的答案（例如是否存在某些提权语义下降级本身就会导致 shell
无法正常工作，而非只是"体验上不如原来"）。

**追踪项（backlog，本文登记，非 `docs/PROBLEM-LIST.md` 编号）：Windows 提权/令牌 API 探测**——
需要实测两件事：① 非提权父进程能否干净地拉起一个提权子进程（`ShellExecute` `runas` 动词在
"重启一个已有 pane"这个交互序列里的真实体验，UAC 提示是否打断/如何打断）；② 是否存在
无法安全降级的提权语义（例如某些访问令牌下的会话，降级重启会导致比"体验变差"更严重的失败）。
**最终裁决 follows 探测结果**——探测完成前，产品与实现均按本节的临时 B 执行；探测完成后
若结果支持 A 或 C，或支持 B 的某个变体，本节内容需相应更新并去掉 provisional 标注。

## 2. App 级 restart/restore 流程

### 2.1 触发与素材

下次启动读取 `%APPDATA%\BetterTerminal\session.json`（`docs/M2-persistence-schema-v1.md`
§1.2/§3）。三条退出路径由哨兵文件机制区分（同文档 §5.5）：

| 退出路径 | 判定 | 恢复提示措辞基调 |
|---|---|---|
| **正常退出** | 哨兵文件已被删除 | 平实措辞（"恢复上次的 N 个未固定标签？"），不暗示任何异常 |
| **崩溃** | 哨兵文件残留 | 措辞必须**不暗示进程还活着**（`docs/DESIGN.md` §7.1.4 原文硬约束）——如"上次意外退出，是否恢复之前的标签？"，禁止"继续上次会话"这类暗示进程仍在运行的措辞 |
| **系统关机** | `WM_QUERYENDSESSION`/`WM_ENDSESSION` 触发与正常退出相同的清理（`docs/M2-persistence-schema-v1.md` §5.5） | 走**正常退出**分支的措辞，不产生崩溃措辞——用户主动关机不是程序失败 |

### 2.2 三扇门一个库（`docs/DESIGN.md` §7.1.4 原文）

- **pin 自动回**：pinned 的 tab（连带其整棵分屏树）无论走哪条退出路径都**自动、静默**恢复，
  不经过确认提示——pin 本身已经是"留着"的显式承诺，恢复它不需要再问一遍。
- **unpinned 进 restore 提示**：非模态、**不倒计时自动恢复**、**窗口先可用**（用户不必先
  回答提示才能开始用软件）——提示按 §2.1 的表格换措辞，但机制统一：同一个提示组件，只
  换文案。
- **Ctrl+Shift+T 撤销关闭**：与前两扇门共享同一个 Recent 库（`docs/M2-persistence-schema-v1.md`
  §3.5「recent」，去重键 `profile_id+cwd+manual_name`），只是入口不同——启动时的提示是
  被动的，`Ctrl+Shift+T` 是主动的，两者读同一份数据。
- **未答复计划**：restore 提示未答复时若用户又关窗，**未答复计划并回 lastSession**，不得
  丢失（`docs/DESIGN.md` §7.1.4 原文）——这是"提示非模态"的直接代价，必须显式接住。

### 2.3 每个 seat 的重建

`session.json` 的 `tabs[].root`（`split{dir,ratio}|leaf` 树，`docs/M2-persistence-schema-v1.md`
§3.2）按形状重建；每个 `term` 叶子按 §3.3 的字段（`profile_id`、`cwd`、`manual_name`）
**各自独立**spawn 一个全新 shell 进程——这里的 spawn 机制与 §1.3「重新建立」一节描述的
per-seat restart 是**同一条代码路径**：全新 PTY、全新 OSC 133 状态机、全新
TERM_PROGRAM/COLORTERM 声明、按 §1.4 的规则从注册表重建环境块。二者的唯一区别是
cwd/profile/name 这三个字段的**来源**（内存中最后已知 vs. 反序列化自磁盘），不是重建
本身的机制。

**每个恢复出的 tab 首行**显示「new shell in `<cwd>`」横幅（`docs/UI-UX.md` §8.5.2 原文：
「恢复出来的 tab 首行如实说明『new shell in `<cwd>`』——不假装什么都没发生」）——这是
§1.3 重启边界记录在"没有旧转录可承接"场景下的版本：per-seat restart 有旧转录在场，边界
记录是"分界线"；app 恢复没有旧转录（§0 已述），首行横幅是唯一能说的话，起到同样的诚实
声明作用。

### 2.4 "restore" 与"全新起步"分别指什么

| | 含义 |
|---|---|
| **restore**（本节主体） | 位置层完整重建：cwd、profile、手动名、pin 状态、tab 顺序/pin 分区、`active_tab`/`focused_leaf`——但**没有任何一个 seat 是"接续"上次的活进程**，每一个都是全新 shell + 首行横幅。restore 恢复的是"你上次把东西摆在哪儿"，不是"你上次那些进程还在跑" |
| **全新起步** | 无 `session.json`（首次启动）、或用户在 restore 提示里显式选择不恢复、或文件整体不可解析回退默认值（`docs/M2-persistence-schema-v1.md` §5.4 情形 2）——以默认 profile 的占位 shell 起步（`docs/DESIGN.md` §7.1.4 原文），**没有首行横幅**（没有"上次"可供声明，横幅本身会是一句谎） |

### 2.5 viewport 定位：无需新规则

重启/恢复产生的边界记录或首行横幅是**追加到转录/新 live 平面的内容**，不是特殊事件。既有
的 viewport 滚动态规则（`docs/DESIGN.md` §3.1：只有 `Bottom` 与语义 `Anchored`）已经完整
回答"新内容到达时视口怎么办"——若用户停在 `Bottom`，新的边界记录/横幅随新内容一起可见；
若停在某个 `Anchored` 位置（app 恢复场景下这甚至无从谈起，因为每个恢复出的 seat 都是全新
空白起点），既有内容位置不动。不需要为重启单独定义一条 viewport 规则。

## 3. 失败诚实

**统一纪律**：per-seat 与 app-restore 共用同一张降级表——同一种损坏，同一种补救，不因
"从磁盘读到的"还是"运行时活着发现的"而给出两套答案。基表来自
`docs/M2-persistence-schema-v1.md` §5.4（原为磁盘读取场景所写），本节把它显式扩展到
per-seat restart 的运行时等价物：

| 失败 | 触发场景 | 降级 |
|---|---|---|
| **cwd 已不存在** | app-restore：`session.json` 里的 `cwd` 路径不存在/不可访问；per-seat restart：最后一次 OSC 7 上报的目录后来被删除/不可达 | 落到 `HOME`，并在该 seat 首行提示（与「new shell in `<cwd>`」横幅同一机制，只是 `<cwd>` 换成实际落地的 `HOME`） |
| **未知/失效 profile_id** | app-restore：`session.json` 引用了本版本不认识的 `profile_id` | 落到默认 profile（`docs/M2-persistence-schema-v1.md` §5.4 原表列出这条动作,但未在原表逐字写出横幅措辞）。**本文统一裁定：该 seat 首行同样带一条可见降级横幅**（"已改用默认 profile"一类文案）,与「cwd 已不存在」行的横幅是同一渲染机制,不是新造一套——同一份文档已经用「显式告警,绝不假装成功」（`docs/M2-persistence-schema-v1.md` §5.3）统摄所有降级路径,让 profile 替换和 cwd 替换共用同一条"从不静默"的纪律，只是原表没有把这条落到字面 |
| **shell 可执行文件缺失**（已裁,2026-08-03,§5#3） | per-seat restart：该 seat 自己的 `profile_id` 曾经有效，但对应的可执行文件此刻在磁盘上已不存在（卸载/移动） | **落到默认 profile，并显式给出与上一行「未知 profile_id」相同的首行降级横幅**（不是静默替换）——这是运行时才会遇到的新失败模式（磁盘 schema 认识这个 ID、只是它指向的可执行文件不在了，与"schema 根本不认识这个 ID"性质不同），裁决把它并入同一条补救与同一条横幅纪律，不另起一套 |
| **PTY/ConPTY spawn 失败** | 两条路径都可能遇到（资源耗尽、权限被拒） | 该 **seat** 进入 dead 态（`docs/DESIGN.md` §7.1.5b 既有的 dead 态视觉语言：图标变暗去色）；tooltip 文案与"进程曾运行后以非零码退出"的 dead 态区分——这里没有退出码可报，文案改为闪光式失败原因（如"无法启动：`<原因>`"）；右键 `Restart shell…` 在 dead seat 上依然可用，允许重试。**绝不是整个 app 起不来**，只是这一个 seat 起不来 |
| **`session.json` 整体不可解析** | app-restore：语法错误、来自未来 `schema_version` | 整份文件回退默认值+显式告警（`docs/M2-persistence-schema-v1.md` §5.4 情形 2），走§2.4「全新起步」 |
| **单叶不满足不变量**（`ratio` 越界、`leaf.kind` 未知等） | app-restore | 逐叶降级，不因单叶损坏丢整棵树（同上 §5.4 情形 3） |

**硬底线复述**：`docs/DESIGN.md` §7.1.4 原文「逐叶降级」——无论失败来自哪条路径，粒度
永远是单个 seat/单个叶子，绝不因为一个 seat 起不来就让整个 app 或整个 tab 失败。

## 4. 交互契约

### 4.1 running-process 状态与重启确认（已裁，2026-08-03，§5#1）

`docs/DESIGN.md` §7.1.4 在 Restart shell 那句话里紧接着定义了 busy 判定规则（§1.6），此前
没有说这个判定被用来做什么——`docs/reviews/ui-mockup-review-2026-07-16.md`（第 105 行）
建议过"运行中必须确认"，但那只是审核意见，不是被采纳的裁决；`docs/DESIGN.md` §7.1.6
定稿的菜单顺序只把 Restart shell 归入"破坏性沉底带省略号"一类，省略号本身不等价于
"总是弹确认"。**现已裁定（详见 §1.6）：仅 busy 时弹确认，空闲直接执行**——busy 判定
第一次有了消费者，而空闲场景（已见 OSC 133 A/B 停在提示符）不为一个明显安全的操作
增加摩擦。

### 4.2 OSC 133 区域连续性

**不跨重启存活**。见 §1.3——重启产生新 screen 身份，旧屏幕的区域是旧 screen 命名空间下的
`Live` 锚（`docs/DESIGN.md` §3.2），随该 screen 一起在重启边界处终结（未闭合区域按隐式 D
强制闭合，见 §1.3）；新 screen 的 OSC 133 authority 从空白状态开始，与任何从未见过标记的
屏幕同等对待（`docs/shell-integration.md`「A screen that has never emitted OSC 133 is
byte-for-byte on the existing fallback path」）。app-restore 场景下这个问题甚至不需要单独
回答——每个恢复出的 seat 都是全新 screen，天然从零开始，没有"旧区域"这回事。

### 4.3 装饰/artifact 命运

| | per-seat restart | app-restore |
|---|---|---|
| **已冻结转录里的装饰**（公式渲染、图片预览、超链接 span、已闭合命令的高亮区域） | **存活**——转录本身没死（见 §0），`SourceLifecycle` 是 `Live→Frozen→Tombstoned` 单向、无 thaw（`docs/DESIGN.md` §4.1），重启不是这三个状态里的任何一个转移事件，不触碰已冻结内容 | **不存在**——转录本身不存在（`docs/UI-UX.md` §8.5.2），没有"冻结内容"可言 |
| **live/staging 里尚未冻结的内容** | 在重启边界处强制经正常定稿路径冻结（如同一次同步的 scroll-out），随后成为"已冻结转录"，适用上一行 | 不适用——app-restore 时旧进程内存已经不存在，没有 live/staging 可冻结 |
| **peek 缓存**（KaTeX 渲染位图、图片解码纹理等，`docs/DESIGN.md` §3.4/§7.5.2） | **存活**——缓存键是 `(span, source_gen, detection_rev, LayoutKey)`（`docs/DESIGN.md` §3.3），不含进程身份；触发重启的是同一个 app 进程，内存里的缓存原样可复用，只要它锚定的转录 span 还在（见上两行） | **不存在**——新 app 进程的运行时缓存从空开始（`docs/M2-persistence-schema-v1.md` §4：解码产物不落盘，"磁盘上不留副本"），即使 cwd/文件路径一致，也要重新解码 |

## 5. 已裁（2026-08-03，用户裁决）

三项原待裁项已裁决，均已在正文对应小节落实。此处只留判定记录，不复述已在正文展开的选项
分析。

1. **Restart shell 遇到 busy/运行中前台命令时是否需要确认？** → **裁决 B：仅 busy（含
   "无 OSC 133 时保守视为 busy"）时弹确认，空闲直接执行**。理由：让 §1.6 已经在计算的
   busy 判定第一次有一个真正的消费者，而不是一条定义了却没有下文的死计算；摩擦只落在
   "可能丢东西"的场景，明显空闲（已见 OSC 133 A/B 停在提示符）的常见情况不多一次点击。
   落实于 §1.6、§4.1。

2. **Restart shell 的 elevation/token 处理**（`docs/DESIGN.md` §7.1.7③ 原文点名的开放项）
   → **裁决 INTERIM B：一律以非提权身份重启，并显式横幅声明降级，标注为临时默认值**，
   最终裁决 follows 一项新登记的 Windows 提权/令牌 API 探测（backlog，见 §1.7）。理由：
   §7.1.7③ 原文已把这条标注为"最可能被平台收窄的承诺"，在做过真实的提权/令牌 API 实测
   前不确定"一律降级"是否是唯一安全的答案；B 是三个选项里唯一不静默声称一个给不出的
   能力的选项，作为探测完成前的产品与实现默认值足够安全，但不构成终裁。落实于 §1.7。

3. **shell 可执行文件在 restart 时已从磁盘消失，如何降级？** → **裁决 A + 横幅：落到
   默认 profile，且与「未知 profile_id」路径共用同一套首行可见降级横幅，绝不静默替换**。
   理由：与已裁定的磁盘降级表（`docs/M2-persistence-schema-v1.md` §5.4「未知 profile_id」
   一行）保持同一失败、同一补救的对称性——这是运行时才会遇到的新失败模式（schema 认识
   这个 ID、只是它指向的可执行文件不在了），但补救动作和可见性纪律没有理由与"schema 不
   认识这个 ID"的既有降级分道扬镳；横幅是"绝不静默"这条硬底线（`docs/M2-persistence-schema-v1.md`
   §5.3）在 profile 替换场景下的落地，不是新造的机制。落实于 §3。
