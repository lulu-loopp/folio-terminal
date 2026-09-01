# 回合结束通知带上原话:两次实测(2026-09-01)

`docs/DESIGN.md` §7.51 的两条实证。一条决定**摘哪一句**,一条决定**第三臂收不收**。

---

## 1. 摘首句还是摘末句(35 份真实 transcript)

### 方法

样本 = 本机 `~/.claude/projects/**/*.jsonl` 里 **>20 KB 且不是 `agent-*.jsonl`**(后者整份是
sidechain,不是任何一条 `Stop` 的 `transcript_path`)的会话文件,按 mtime 取最近 35 份,跨
Folio 本身与另外四个项目(LaTeX 报告、控制课作业、研究笔记、一个小工具)。

对每一份:倒着找**最后一条 `type == "assistant"` 且 `isSidechain` 不为真**的条目,取它
`message.content` 里全部 `text` 块;然后同一段文字摘两遍——

* **首句**:第一个是散文的 markdown 块(标题/段落/列表首项/引用首行;代码块、表格、公式、
  图片、分隔线跳过),取它的第一句;
* **末句**:同一算法作用在**行序反转**后的同一段文字上。

两边共用同一套 markdown 清洗与同一套句末判定,所以差别只在「取哪一头」。

**只读,不写。** 脚本没有修改过 `~/.claude` 下的任何东西。下面不抄任何一条私人会话的内容,
只给计数和几句没有信息量的收尾套话——它们正是本节的论据。

### 结果

| | 摘出来是「这一轮做完了什么」 | 摘出来是收尾邀请 / 旁白 / 表格单元格 / 折行后半截 |
|---|---|---|
| **首句** | **33 / 35** | 2 / 35 |
| **末句** | 16 / 35 | **19 / 35** |

两边一致的 9 份是整条消息只有一句话的(`ok`、`No response requested.`、一条报错结论),
它们不区分两种读法。

末句摘坏的三类,各举一句(都不含任何项目内容):

* 收尾邀请:「想再展开哪部分?」「说一声就行。」「要我把其中任何一道展开得更细……」
* 旁白:「现在可以 /clear 了。」「顺带说一句,……」
* 结构碎片:消息以 markdown 表格结尾时,末句 = **一个表格单元格**。

### 结论

**摘首句。** 原因不是统计巧合:一条回合末尾的消息是**先给结论再给细节**写的,首句就是作者
自己已经写好的那句摘要;末句是写给已经读完前面的人看的,所以它默认省略了「发生了什么」。

### 代价实测(同一批文件)

尾读实现是「64 KiB 起,不够 ×8,8 MiB 封顶,倒着逐行找」。35 份文件里最大的 **287 MB**,
逐份计时 **0.1 ms – 0.8 ms**(Python 原型;Rust 只会更快)。整份读进来的做法在这批文件上是
不可行的。

---

## 2. 屏幕尾行启发式,收不收(3 屏真 codex TUI)

单子里的第三臂原案:「从 pane 屏幕尾部向上找最后一行有实义的文本(跳过框线字符、spinner、
输入框装饰、空行)」,并要求先实测误摘率再定收不收。

### 方法(可复现)

仓里没有任何 codex 的 `.btcr`(`corpus/claude-code-session.btcr` 是 Claude Code 的),所以
现录。用仓里已经构建好的 `target/release/bt-record.exe`,在仓根跑(该目录已被 codex 信任,
避开信任对话框),`-s read-only` 使 codex 碰不到任何文件:

```
target/release/bt-record.exe <out>/codex-1.btcr --size 120x30 \
  --input-plan <armc>/plan1.txt \
  -- C:/Users/Weiyi/.codex/packages/standalone/current/bin/codex.exe \
     -c check_for_update_on_startup=false -s read-only
```

两段录音,提问分别是 plan1「in exactly three sentences, explain what a binary search tree
is」与 plan2「list three benefits of a hash table, one short line each, then a final one
sentence conclusion line」,末尾 `/quit`。用 `truncate_btcr.py`(按 `BTCRP002` 帧格式在指定
`at_micros` 处截断事件表)各截在**回答流完、`/quit` 之前**的那一帧,于是渲染出来的末帧是真正
的「回合刚结束、composer 空闲」屏,而不是退出屏。渲染走 `bt-replay.exe <file> --render`,
即本 build 自己的网格模拟器。

* **S1** `codex-1.mid.btcr` — 空闲,二叉搜索树那问。
* **S2** `codex-2.mid.btcr` — 空闲,哈希表那问。
* **S3** `codex-1.gen.btcr` — 生成中(截在 ~0.9 s),忙屏。

codex 0.144.4,120×30,ConPTY。**N = 3**,小,如实写。

### 三屏的屏底(逐字,`--render` 输出的末尾)

S1:

```
• A binary search tree is a tree-shaped data structure where each node has at most two children. Values smaller than a
  node are stored in its left subtree, while larger values are stored in its right subtree. This ordering enables
  efficient searching, insertion, and deletion when the tree remains balanced.


› Find and fix a bug in @filename

  gpt-5.6-sol medium fast · D:\Developer\BetterTerminal
```

S2:

```
• - Fast average-case lookups.
  - Efficient insertions and deletions.
  - Flexible key-based data access.

  Overall, hash tables provide fast and practical data retrieval.


› Improve documentation in @filename

  gpt-5.6-sol medium fast · D:\Developer\BetterTerminal
```

S3:

```
• Starting MCP servers (1/2): codex_apps (0s • esc to interrupt)


› Find and fix a bug in @filename

  gpt-5.6-sol medium fast · D:\Developer\BetterTerminal
```

**三屏共用同一副屏底骨架**,而这本身就是结论:一条常驻的 `模型 · cwd` 状态行,上面一条
`› …` 的 composer 占位行——**占位提示与真的被引用的用户提问用的是同一个 `›`**。

### 摘出来的是什么

| 屏 | 朴素过滤(只跳空行 / 框线 / spinner)摘到 | 再手工把状态行与 `›` 占位行也认成装饰后摘到 | 判定 |
|---|---|---|---|
| S1 空闲 | `gpt-5.6-sol medium fast · D:\Developer\BetterTerminal` | `efficient searching, insertion, and deletion when the tree remains balanced.` | 朴素:**UI 装饰**;手工:**折行后半截** |
| S2 空闲 | 同上 | `Overall, hash tables provide fast and practical data retrieval.` | 朴素:**UI 装饰**;手工:**摘对了**(而它之所以对,是因为提问被特意写成「最后给一句结论」) |
| S3 忙 | 同上 | `• Starting MCP servers (1/2): codex_apps (0s • esc to interrupt)` | 两种读法都是**工具状态行**,不是 agent 的话 |

**计数。** 朴素读法:**3/3 摘到 UI 装饰**,零命中。加了 codex 专属的屏底知识之后:1/3 对、
1/3 折行碎片、1/3 工具状态行。两种读法之下,**0/3 能稳定拿到 agent 的最后一整句**。

### 结论:**不收**

失败是结构性的而不是边角:codex 屏底永远压着一条非空、无框线、非 spinner 的状态行,朴素
的自底向上扫描在这个样本里**每一次**都会摘到它;而把状态行和占位行也认成装饰,需要的正是
「这一家这一版的屏底长什么样」这种知识——那恰好推翻了立这条臂的理由(它本该是与家族无关
的通用启发式)。落到通知里的后果会是 `gpt-5.6-sol medium fast · D:\path`、一句陈旧的
`Find and fix a bug in @filename`,或者半句话——**比不写还差**。

**而且它本来要救的那一家已经被更准的东西救了**:codex 的 `notify` payload 里就有
`last-assistant-message`(`evidence-cli-survey-2026-08-25.md` 第 214 行逐字),而
`attention_codex` 的安装模板从一开始就把 `notify` 数组结在 `--json` 上——payload 一直在
到达这个动词。§7.51 裁决一 ③ 收的是那一条。

### copilot

copilot CLI 本机未安装,没有录任何屏,**本节不对它下任何结论**。它的 `agentStop` payload
里有 `transcriptPath`(`evidence-copilot-cli-2026-08-26.md` §1.6 逐字),但那份文件的格式
没有任何一处上游原文可引,所以 `attention_map::TURN_END` 里它的 `Words` 是 `None`。

---

## 证据文件

第 2 节的录音、截断脚本与渲染输出留在本次会话的 scratchpad
(`.../scratchpad/armc/`),没有进仓:两段 `.btcr` 合计 ~460 KB,而上面逐字抄下来的屏底
就是它们全部的论据。要复现照上面那条 `bt-record` 命令重录即可——它不依赖那两个文件。
