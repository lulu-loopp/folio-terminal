# 下划线加固：对抗场景复核

本文按“下划线是一项可兑现的点击承诺”审阅。风险标签含义如下：

- **指错风险**：可能画出下划线，但点击目标不是输出者表达的目标；这是阻断级风险。
- **不认风险**：目标本来可解析，但系统保守地不画线；可以作为受控降级。
- **无风险**：要么能得到唯一正确目标，要么按既有裁决明确拒绝且不会指错。

代码现状决定了几条分析前提：`paths.rs` 的裸绝对路径只认盘符根路径，裸相对路径必须含分隔符且不能含冒号，裸 UNC、`\\?\`/`\\.\`、盘符相对路径 `C:foo` 均不在语法内；ASCII 双引号是唯一“声明范围”的引号，未引号 token 主要由空白和闭合括号截断，ASCII 逗号、分号、冒号等通常留在 token 中；候选随后以解析出的 `PathBuf` 进入 viewport 的异步存在性探测和 term 的 yes/no 判决账本。下面的“期望”是加固后应满足的行为，而不是把现状偶然行为当规范。

## 新形态

### 1. 空白、引号和转义造成的真实前缀

- **[指错风险] 未引号的含空格绝对路径**：`At D:\Program Files\Tool\run.ps1:12 char:3` 会先截出 `D:\Program`；只要该目录存在，就可能给一个真实但错误的前缀画线。这比硬换行样例更常见，PowerShell、npm 的 `npm ERR! path ...`、编译器命令回显都能产生。需要“空白截断可疑”门，至少在候选后紧跟可组成路径的文本时不得凭前缀存在就兑现。
- **[指错风险] 单引号不是范围声明**：`'D:\Program Files\Tool\run.ps1'` 中开单引号只充当普通边界，候选仍在第一个空格处截止；真实的 `D:\Program` 会被误指。PowerShell、Python `repr`、shell 日志常用单引号。
- **[指错风险] 弯引号不是对称范围声明**：`“D:\Program Files\Tool\run.ps1”` 的右弯引号虽属于闭合分隔符，左弯引号却不会建立“直到配对引号”的范围；仍可能误指 `D:\Program`。
- **[指错风险] 转义双引号会过早闭合**：JSON/日志中的 `"D:\Program Files\Tool\run.ps1"`，以及双引号内出现 `\"` 的字符串，不能把第一个字面 `"` 一律当配对边界；若先反转义一半或直接按显示字符闭合，会得到可存在的错误前缀。词法层应坚持“屏幕文本”语义，不猜宿主转义；无法证明范围就不认。
- **[不认风险] JSON 双反斜线**：`{"file":"D:\\case\\src\\main.rs"}` 显示的是序列化值而非本地路径字面量。Windows 可能把重复分隔符归一到同一文件，但这属于偶然；除非有明确 JSON 解码上下文，否则不应以文件路径承诺画线。
- **[指错风险] 引号后缀与编译器后缀混合**：`File "D:\case\a.py", line 7` 是安全的；但 `"D:\case\a.py:7", line 7`、`"D:\case\a.py"(7,2)` 必须先确定“位置后缀在引号内还是引号外”，不能把闭引号外文本并入磁盘目标，也不能把引号内合法文件名的数字 ADS 静默改成行号。
- **[不认风险] 路径中的 tab/换行转义**：打印为 `D:\case\a\tb.rs` 的两个字符 `\`+`t` 是合法路径文本；打印成真实 tab 则 token 必然断开。不能把屏幕上的 `\t` 猜成 tab，也不能跨真实 tab 拼接。

### 2. 编译器、linter、traceback、git、npm、PowerShell 的位置语法

- **[不认风险] MSVC/Clang-cl 括号位置**：`D:\case\src\main.cpp(12,34): error C2143` 的当前 token 会含入 `(12,34` 再在 `)` 截止，磁盘探测失败；应显式解析 `path(line,col)`，而不是扩张普通路径字符集。
- **[不认风险] TypeScript/部分 linter 括号位置**：`src/app.ts(7,19): error TS2322` 同上；相对路径与绝对路径都应有同一位置后缀解析器。
- **[不认风险] .NET 的 `:line N`**：`in D:\case\Foo.cs:line 42` 会把 `:line` 留在 token，不能命中。可作为明确、带空格终止的生产者语法解析；不能全局把单词 `line` 当路径终止符。
- **[不认风险] ripgrep/grep 的 `path:line:text`**：`src/main.rs:12:let x = 1` 的位置数字不在 token 尾，现有尾向解析不会切出 `src/main.rs:12`。应由“数字位置后紧跟冒号”语法切分；不得用“遇任意冒号就停”，否则 URL、盘符和 ADS 都受伤。
- **[无风险] Rust/GCC 常规位置**：`--> src/main.rs:12:5`、`D:\case\a.c:12:5: error` 在数字位置之后有空白，现有 `:line[:col]` 可唯一解析；应保留。
- **[无风险] Python traceback 常规形式**：`File "D:\Program Files\app.py", line 3, in f` 由 ASCII 双引号声明完整范围，逗号在闭引号外；若目标存在，应只画引号内路径，不含引号与逗号。
- **[指错风险] PowerShell 未引号空格路径**：`At D:\Program Files\app.ps1:12 char:3` 是“空格截出真实目录”最高优先级红测，不能因为 `D:\Program` 已获 yes 判决就画线。
- **[无风险] PowerShell provider 前缀**：`Microsoft.PowerShell.Core\FileSystem::D:\case\a.ps1:12` 中真正盘符路径可从 `D:` 开始独立解析，provider 限定符不应进入目标。
- **[不认风险] 非文件 provider**：`HKLM:\Software\Vendor`、`Cert:\CurrentUser\My` 不是本地文件路径；即使某个 cwd 下存在同名相对目录也不得降格解析其中的尾片段。
- **[指错风险] npm 的未引号 `path` 行**：`npm ERR! path D:\Program Files\nodejs\node_modules\x` 与 PowerShell 同类；存在性不是“第一个空白就是目标末端”的证据。
- **[无风险] Node stack 常规形式**：`at f (D:\case\app.js:10:2)` 应解析括号内的路径与位置，右括号不入范围。
- **[不认风险] 虚拟模块 scheme**：`webpack://app/src/main.ts:7:2`、`node:internal/modules/cjs/loader:123:4` 不是普通本地路径；没有显式映射就整体不画文件下划线。
- **[指错风险] git diff 的合成前缀**：`--- a/src/main.rs`、`+++ b/src/main.rs` 中 `a/`、`b/` 是 diff 命名空间；若 cwd 恰有真实 `a/src/main.rs`，按普通相对路径画线会指向错误文件。没有 diff 上下文剥前缀规则时，应宁可不认；不能仅因磁盘碰巧存在就认。
- **[不认风险] git rename 花括号压缩**：`src/{old => new}/main.rs` 不是一个屏幕字面路径。必须在明确 diff rename 语法下展开为两个候选，否则不画线；不能把空白前的 `src/{old` 或后的 `new` 当路径。
- **[不认风险] git C 风格八进制转义**：`"a/\346\226\207.txt"` 是 git 的 quoted path 编码；普通路径词法不应直接拿反斜线数字串探盘。若将来支持，必须完整按 git `core.quotePath` 规则解码。

### 3. URL、file URI 与路径扫描器的重叠

- **[指错风险] URL query/fragment 内嵌相对路径**：`https://host:8080/open?next=docs/a.md#L2` 必须由 URL 候选独占整个 span。若路径扫描器又从 `=` 后识别 `docs/a.md`，而 cwd 中该文件存在，就会在一个 URL 内制造第二个、语义不同的点击目标。
- **[指错风险] URL query 内嵌盘符路径**：`http://localhost:3000/?file=D:\case\a.txt` 同理；`D:` 前的 `=` 是合法候选边界，当前绝对路径扫描有机会命中。必须先做 scheme span 屏蔽/仲裁，不能靠后加入的 link 覆盖顺序碰运气。
- **[无风险] 带端口、路径、query、fragment 的 HTTP URL**：`https://host:8080/a/b?q=x%2Fy#frag` 应作为一个 URL，端口冒号不是路径位置后缀，query 中的 `/` 也不是相对路径证据。
- **[无风险] IPv6 authority**：`https://[2001:db8::1]:8443/a?q=b` 的方括号和多个冒号属于 authority；不得交给路径 `:line:col` 逻辑。
- **[指错风险] 括号嵌套 URL**：`see (https://host/a_(b)?x=(c)).` 应按 URL 语法与成对括号裁边；简单地剥所有尾 `)` 会把 URL 截短，简单地全保留又会吞掉 prose 右括号。至少要把“内部配对一个、外部多一个”列为红测。
- **[指错风险] URL 的应用硬换行**：上行 `https://host:8080/api/us` 恰落末列、下行 `ers?id=7` 时，上行是可工作的错误短 URL。方案虽不跨行重拼 URL，但必须让“末列截断门”同样覆盖 inferred URL；否则最危险的旧问题原样保留。
- **[不认风险] URL 不跨应用换行重拼**：即使两行拼接看似合法，也没有磁盘 witness，且两个独立 URL/日志行可拼成第三个合法 URL；首版应压住上行截断候选、但不生成跨行 URL 链接。
- **[无风险] 本地 file URI**：`file:///D:/case/a%20b.txt` 与 `file://localhost/D:/case/a.txt` 仍按既有裁决解码并探测本地目标；百分号解码只能发生一次。
- **[不认风险] remote/POSIX file URI**：`file://server/share/a.txt`、`file:///etc/passwd` 按既有 Windows 裁决继续不认；不得退化为其中某个 `share/a.txt` 相对候选。
- **[不认风险] file URI 的 query 与位置后缀组合**：`file:///D:/case/a.txt:12?raw=1` 必须先定义 query 是否属于 URI、`:12` 是否仍是打印位置；在规则未定前不画线，不能一会儿先剥 query、一会儿先拆行号而得到不同目标。
- **[指错风险] 百分号双解码**：`file:///D:/case/a%2520b.txt` 只能解成包含字面 `%20` 的文件名；若两层解码会错误打开 `a b.txt`。

### 4. 环境变量、家目录、盘符语义和 Windows 名字空间

- **[指错风险] POSIX 环境变量尾片段**：`$HOME/docs/a.md` 中 `HOME/docs/a.md` 满足裸相对路径形状；若 cwd 恰有该路径，存在性会把变量表达式误指到 cwd。看到紧邻的 `$`、`${...}`、`%...%`、`$env:` 等扩展前缀时，应整体屏蔽而不是从变量名内部重新开候选。
- **[不认风险] Windows 环境变量**：`%USERPROFILE%\docs\a.md`、`%TEMP%\x.log` 在没有可信展开上下文时不认；不要使用 BetterTerminal 自身环境替输出进程展开，因为二者可能不同。
- **[不认风险] PowerShell 环境变量**：`$env:USERPROFILE\docs\a.md`、`${env:USERPROFILE}\docs\a.md` 不认；冒号后的尾片段也不得作为新相对候选。
- **[不认风险] 波浪号**：`~\docs\a.md`、`~/docs/a.md` 不认，除非将来绑定到输出行所属 shell 的明确 home；不能用宿主进程 home 猜。
- **[不认风险] 盘符相对路径**：`C:foo\bar.txt` 是“C 盘当前目录相对”，不是 `C:\foo\bar.txt`，也不是 session cwd 相对。现有不认是正确的保守行为；若支持，必须拥有每盘 cwd，不能补一个反斜线。
- **[不认风险] 裸 UNC**：`\\server\share\dir\a.txt` 按 DESIGN 既有边界继续不探测，避免网络 I/O 与凭据泄漏；也必须阻止从 `server\share\dir\a.txt` 内部降格为 cwd 相对路径。
- **[不认风险] 长路径与设备前缀**：`\\?\D:\case\a.txt`、`\\?\UNC\server\share\a.txt`、`\\.\PIPE\name` 不属于当前裸路径合同；不得跳过前缀后链接内层 `D:\...`。其中 named pipe 更绝不能以文件 URI 打开。
- **[无风险] 大小写差异**：`d:\CASE\Src\MAIN.rs` 在常规 Windows 卷上可解析到同一目标；大小写不同最多造成账本重复探测，不应改变点击目标。测试需同时覆盖大小写敏感目录/卷的可能配置，不能在词法层强制 lowercase。
- **[无风险] 普通 Win32 尾点归一**：既有表的 `D:\x\a.md.` 在非扩展名字空间下由 Windows 归一到 `a.md`，目标并未改变，保留现裁决；但 UI 应明确这是现有兼容裁决，不可套到 `\\?\`。
- **[指错风险] 尾空格/尾点与扩展名字空间混用**：`"\\?\D:\case\a.txt "` 可以表示与 `a.txt` 不同的真实名字；若先丢前缀或 trim 再探测，会打开错误文件。当前整体不认最安全。
- **[指错风险] 数字 ADS 与行号歧义**：`D:\case\a.txt:12` 既可表示第 12 行，也可表示名为 `12` 的 NTFS alternate data stream。现合同把正整数尾缀裁为位置，必须明确“数字 ADS 不支持”；不能先探测 ADS 后在不同机器上改变语义。非数字 ADS 也不宜转 file URI，除非打开端确有一致合同。
- **[不认风险] DOS 设备名**：`D:\case\CON`、`D:\case\NUL.txt` 可能触发设备语义而非普通文件；仅凭 metadata 成功不足以承诺 Explorer/编辑器会打开同一对象，应列为拒绝或专门规范。

### 5. 应用硬换行、cell 边界与跨行误拼

- **[不认风险] 合法路径恰好填满一行**：完整 `D:\case\src\main.rs` 恰落最后一个 visual cell 时，与“被应用换行截断”在单行局部不可区分；末列门会牺牲它。这是可接受的安全降级，但必须有精确红测并在 resize 后重算。
- **[指错风险] verified 上半截**：骨架样例 `D:\WINDOWS\system` / `32\...` 中上半截真实存在。末列门必须在发起/消费存在性判决之前压住该 span；不能让账本中的旧 yes 绕过门。
- **[无风险] 五门中的“上下半截均未验证”应保留**：它会导致上例不重拼，但配合末列门，结果是安全的不认，不是指错。若为了修复该例放宽第五门，会重新允许“两个真实相邻日志项碰巧拼成第三个真实路径”。
- **[指错风险] “下行同缩进”不能作为首版入口**：两条同级 stack frame、诊断列表或目录列表通常同缩进；去掉下行缩进后可能拼出一个真实路径。首版只允许下行内容从 visual column 0 开始；“与上行同缩进”应拒绝，除非未来有生产者特定 continuation 标记。
- **[指错风险] 两个 peer 行可碰巧拼成真实路径**：上行末列 `D:\case\src`、下行第 0 列 `\main.rs` 拼成真实文件，但输出可能本来是两个独立项。第五门会因上半目录已验证而拒绝，正是必要防线。
- **[不认风险] 下半截自身也是有效相对路径**：若下行第 0 列 `src/main.rs` 自己在 cwd 可验证，五门应拒绝重拼；宁可留下两个独立正确候选中的下行，也不制造第三个目标。
- **[无风险] 位置后缀跨行**：`D:\case\src\main.rs:` / `12:5` 只有在拼接后同一词法器恰好产出“完整 span + 有效位置”、目标存在且两半各自未验证时才可重拼。
- **[不认风险] 带 continuation 装饰的换行**：下行以 `... `、`↪ `、四空格等装饰开头时，普通拼接不应猜着剥；首版不认。任何装饰剥除都应另立生产者语法。
- **[指错风险] 双宽字符与末列判定**：必须按 terminal cell 的占用判断“到最后一列”，而不是 UTF-8 byte、Unicode scalar 或 grapheme 数；宽字符落在倒数两格时尤其容易 off-by-one，造成门漏放错误前缀。
- **[无风险] DEC soft-wrap 与应用 CR/LF 分开**：soft-wrap 已由逻辑行重组，不能再走应用重拼而重复/错位；新门只接收“真实行边界 + visual 末列”的候选。
- **[指错风险] 滚动/resize 后陈旧坐标**：候选在 80 列时末列、resize 到 120 列后不再末列，或反之。门和跨行 span 必须随 viewport generation 作废重算，不能复用旧 cell 范围。

### 6. cwd、存在性与判决账本

- **[指错风险] scrollback 相对路径改绑新 cwd**：同一屏幕旧行 `src/main.rs` 若在 `cd` 后按“当前 cwd”重算，可能从 `D:\repoA\src\main.rs` 改指 `D:\repoB\src\main.rs`。识别请求、yes/no 回执和点击目标都必须携带该行识别时的 resolution base/generation。
- **[指错风险] OSC 7/spawn cwd 滞后**：子程序内部改变 cwd 却未及时发 OSC 7 时，打印的相对路径可能在旧 cwd 下也恰好存在。存在性不能证明语义正确；这是既有推断合同的残余风险，测试至少要确保 cwd 更新后旧回执不能投给新 base。
- **[不认风险] negative verdict 陈旧**：第一次输出时文件尚未生成，账本记 no；稍后构建生成同一路径，永久复用 no 会漏认。账本应有 generation/失效策略，或新一轮候选允许重探。
- **[指错风险] positive verdict 陈旧与 TOCTOU**：yes 后文件被删除、替换为 junction/symlink 或同名新文件，点击可能打开另一个对象。至少在点击前复核存在性/类型；高风险网络重解析点不应只信历史 yes。
- **[指错风险] 截断门之后仍消费旧 yes**：候选先以普通单行路径入账，随后 viewport 发现它位于应用硬换行边界；旧 yes 必须由候选 generation/span key 隔离，不能仅按 `PathBuf` 反向点亮被截断 span。
- **[不认风险] 大小写与等价路径造成重复账本项**：`D:\case\a`、`d:\CASE\a`、`D:\case\.\a` 可能是同一目标。重复探测只是性能/抖动问题；不要用不安全的字符串 canonicalization 合并，更不能跨 cwd 合并相对键。

## 四件方案评注

### 第一件：行末截断门

- **[无风险] 结论：方向成立，而且是安全性的主门。** 只要 inferred path 或 inferred URL 的候选 span 占到物理行最后一个 visual cell，就先标为 `edge-suspect`，不得进入“可点击 yes”状态。该门必须高于/早于存在性账本的 yes；OSC 8 明示链接不属于 inferred gate。
- **[指错风险] 必须补写 URL 也受门控。** 方案第二件说 URL 不重拼是对的，但若第一件只用于磁盘路径，`https://host/us` 这种真实短前缀仍会兑现错误 URL。
- **[不认风险] 点名待议：合法目标刚好占满整行。** 无额外 producer 信号时无法可靠区分，裁决应是“压住，不画线”；不要用“存在”放行，因为截断前缀正可能存在。可通过 hover 提示“行末疑似截断”降低困惑，但不能以提示代替安全门。
- **[无风险] 边界定义。** “末列”指候选最后一个 terminal cell 等于当时行宽减一；候选后若还有闭括号、中文标点或空白 cell 中的实际打印字符，则不是末列截断候选。resize、reflow、行内容 mutation 后 generation 失效。

### 第二件：五门应用硬换行重拼

- **[无风险] 结论：五门可作为极保守首版，且第五门不能为骨架样例 A 放宽。** A 最终“不画线”是正确安全结果；“修好指错”不等于必须恢复点击。
- **[指错风险] 点名待议：只选下行从第 0 列开始，不选“与上行同缩进”。** 同缩进是 peer 诊断/stack frame 的常态，不是 continuation 证据。未来若支持缩进，必须绑定明确生产者 marker，并单独威胁建模。
- **[无风险] 第三门要写成精确断言。** 拼接文本交回同一词法器后，必须恰好得到一个覆盖两片全部非装饰字符的候选；不能只要求“其中出现一个候选”，否则 query、尾标点或第二个路径可被静默遗弃。
- **[无风险] 第四门应验证词法拆出的磁盘目标，而非带 `:line:col` 的显示串。** 回执要携带拼接 generation、两行 id、两段 cell span、resolution base 与目标；任一变化即丢弃。
- **[不认风险] URL 不重拼维持。** 但应加入“上行 URL edge-suspect 不画线、下行照自身规则另判”的明确行为。
- **[指错风险] 反例。** 两个相邻 peer 行 `D:\case\src` 与 `\main.rs` 若去掉第五门或允许 verified 上半截，恰可拼成真实文件；磁盘 witness 并不能证明作者原意。

### 第三件：引号内列表

- **[指错风险] 结论：不能采用“整串不存在就按 `;` fallback”这一全局规则。** Windows 文件名允许分号；`"D:\case\real;D:\case\missing"` 可能表达一个不存在的单路径，而第一段碰巧存在。把第一段画线就是把“不存在”升级成错误承诺。
- **[指错风险] 点名待议：也不能把 `;` 变成所有引号内的普通终止符。** 这会破坏合法分号文件名、URL path parameter/query、命令参数和日志消息；还会改变 DESIGN 中“引号声明范围”的核心裁决。
- **[无风险] 建议裁决：新增显式 `path-list` 模式，而不是修改普通 path token。** 只有存在列表上下文证据（例如同一字段的 `PATH=`/`PSModulePath=`/已知“搜索路径”标签）且值语法完整时，外层范围内才按 Windows 列表语法拆段；每段独立去合法的段级引号、独立解析、独立探测、独立画线，分号本身永不画线。
- **[指错风险] 列表模式还必须定义空段、变量段和相对段。** `A;;B` 的空段可能代表当前目录，`%SystemRoot%` 取决于输出进程环境，相对段取决于生产者 cwd；首版应只链接可独立、字面、盘符根路径的段，其他段不认。
- **[不认风险] 仅支持外层双引号仍会漏掉常见未引号赋值。** `PATH=D:\A;D:\B` 和 `"D:\Program Files\A";D:\B` 更常见；若没有安全的字段级 parser，宁可先只修有明确标签的形式，不要全局改分号。

### 第四件：组合对抗矩阵

- **[无风险] 结论：矩阵必要，但骨架目前只列维度，不足以防“存在性碰巧给错目标背书”。** 每个形态至少要在三套 fixture 下跑：完整目标存在；仅危险前缀存在；前缀和拼接目标同时存在。
- **[指错风险] 测试 oracle 不能只断言“有/无下划线”。** 必须同时断言显示 span、归一前的 printed target、解析后 `PathBuf`/URL、行列、resolution base、请求 generation，以及点击路由；否则“有线但指错”会被绿色测试掩盖。
- **[无风险] 必加状态维度。** unknown→pending→yes/no、旧回执晚到、cwd 变化、resize/reflow、scrollback 再现、文件由不存在变存在/反向变化，都要进入矩阵。
- **[无风险] 必加所有权维度。** HTTP(S) URL span、file URI span、OSC 8 span 与裸路径 span 需要互斥/优先级断言；尤其 query 中的 `docs/a.md` 和 `D:\...` 不得生成嵌套文件链接。
- **[无风险] 必加 cell 维度。** 最后一列、倒数一列、候选后一个闭括号、候选后一个全角标点、宽字符占最后两格、80→120 resize、soft-wrap 与真实 CR/LF 必须分开。

## 边界表改判

23 行表目前只描述字符串，不描述 viewport 宽度与物理换行类型。因此，**在候选不触及最后一个 visual cell 的前提下，23 行的既有文字判决都不应因本方案改变**；新门应给表增加 placement 轴，而不是悄悄改写普通行语义。把各行原文布置为“候选末 cell = 行末 cell”后，改判如下。

- **[不认风险] 条件改判为 `edge-suspect / 不画线`：第 1、2、10、11、13、15、16、18、20、23 行。** 这些行的已识别 path/file URI 在原形态中可一直延伸到行末；若又恰好占满 viewport，就必须由第一门压住。移到行内后恢复原判。
- **[不认风险] 第 12 行的 HTTP URL 也必须条件改判。** `https://host:8080/img/x.png` 若占满最后一格则不画 inferred URL；同一行的“relative 识别为 none”保持。若不改第 12 行，方案仍保留已知的错误短 URL 漏洞。
- **[无风险] 第 3、5、6、7、17、22 行不改。** 第 3 行有闭引号证明范围完整，其余各行的候选后仍有右括号/全角或中文标点；即使整条显示行的最后字符落末列，路径候选本身没有占到末列，所以不是截断证据。
- **[无风险] 第 4、8、9、14、19、21 行不改。** 它们原本就无文件下划线；末列门不会把 none 变成候选。
- **[无风险] 第二件重拼不直接改任何现有单行条目。** 应在表后新增双物理行组：安全重拼、verified 上半截拒绝、下半截非第 0 列拒绝、同缩进拒绝、soft-wrap 不重复重拼、URL 只压不拼。
- **[无风险] 第三件不改第 3 行普通引号路径。** 另加带分号的三行：无列表上下文的引号串不拆；明确 `PSModulePath=` 上下文逐段；合法分号单文件即使某段存在也不拆。
- **[无风险] 建议给 23 行每个“应链接”条目复制三种 placement。** `interior` 保持原判，`edge + hard newline` 压住，`edge + DEC soft-wrap` 先逻辑重组后按完整 token 判；这是比只改文字期望更可执行的表结构。

## 用例清单

以下用例的“期望”均指最终稳定状态；每条测试应同时断言 span 和实际 target。除特别说明外，cwd 为 `D:\case`。fixture 可按每条独立建立，避免不同用例互相提供“碰巧存在”的 witness。

### A. 真实前缀、引号与位置语法

1. **[指错风险]** 输入原文：`At D:\Program Files\Tool\run.ps1:12 char:3`；fixture：`D:\Program` 与完整脚本均存在；期望：不得链接 `D:\Program`，在未支持未引号空格路径前整段不画线；理由：空白截成的真实目录不是作者目标。
2. **[指错风险]** 输入原文：`npm ERR! path D:\Program Files\nodejs\node_modules\x`；fixture：`D:\Program` 存在；期望：不得链接前缀；理由：npm 常见输出会稳定复现空格截断。
3. **[指错风险]** 输入原文：`'D:\Program Files\Tool\run.ps1'`；期望：不得链接 `D:\Program`；理由：单引号当前不声明完整范围。
4. **[指错风险]** 输入原文：`“D:\Program Files\Tool\run.ps1”`；期望：不得链接 `D:\Program`；理由：弯引号不能只处理右端而放任空白截断。
5. **[无风险]** 输入原文：`File "D:\Program Files\app.py", line 3, in f`；fixture：脚本存在；期望：仅 `D:\Program Files\app.py` 画线并打开该文件第 3 行（若 traceback parser 负责行号）或文件本身，绝不含引号/逗号；理由：ASCII 引号给出可靠 extent。
6. **[不认风险]** 输入原文：`{"file":"D:\\case\\src\\main.rs"}`；fixture：解码后的文件存在；期望：普通裸路径 pass 不画线；理由：这是序列化文本，不能猜一次反转义。
7. **[不认风险]** 输入原文：`D:\case\src\main.cpp(12,34): error C2143`；fixture：文件存在；期望：支持括号位置后只画路径并附 `(12,34)`；尚未实现该显式语法时整项不画线，绝不画某个前缀；理由：MSVC 高频形式。
8. **[不认风险]** 输入原文：`src/app.ts(7,19): error TS2322`；fixture：文件存在；期望：同上，目标为 `D:\case\src\app.ts`；理由：相对路径也要共享括号位置语法。
9. **[不认风险]** 输入原文：`at Foo in D:\case\Foo.cs:line 42`；fixture：文件存在；期望：仅在显式支持 `:line N` 后链接文件第 42 行，否则不画；理由：不能把 `:line` 当文件名探盘。
10. **[不认风险]** 输入原文：`src/main.rs:12:let x = 1`；fixture：文件存在；期望：显式 grep 语法下链接 `src/main.rs` 第 12 行，不把正文并入目标；理由：数字位置不在 token 尾。
11. **[无风险]** 输入原文：`--> src/main.rs:12:5`；fixture：文件存在；期望：`src/main.rs:12:5` 整个显示范围可点击，目标为文件第 12 行第 5 列；理由：现有数字尾缀无歧义。
12. **[无风险]** 输入原文：`at f (D:\case\app.js:10:2)`；fixture：文件存在；期望：右括号不入 span，目标行为第 10 行第 2 列；理由：Node stack 常见安全边界。
13. **[无风险]** 输入原文：`Microsoft.PowerShell.Core\FileSystem::D:\case\a.ps1:12`；fixture：文件存在；期望：只链接 `D:\case\a.ps1:12`；理由：provider 前缀不是目标的一部分。
14. **[不认风险]** 输入原文：`HKLM:\Software\Vendor`；期望：无文件下划线；理由：注册表 provider 不是磁盘路径。
15. **[不认风险]** 输入原文：`Cert:\CurrentUser\My\ABCDEF`；期望：无文件下划线；理由：证书 provider 不能降格为 cwd 相对路径。

### B. URL、file URI 与 span 所有权

16. **[无风险]** 输入原文：`https://host:8080/a/b?q=x%2Fy#frag`；期望：整串一个 URL target；理由：端口、query、fragment 都由 URL 语法拥有。
17. **[无风险]** 输入原文：`https://[2001:db8::1]:8443/a?q=b`；期望：整串一个 URL；理由：IPv6 冒号不是行列后缀。
18. **[指错风险]** 输入原文：`https://host:8080/open?next=docs/a.md#L2`；fixture：`D:\case\docs\a.md` 存在；期望：只有整 URL 链接，`docs/a.md` 没有嵌套文件链接；理由：query 参数不属于 cwd。
19. **[指错风险]** 输入原文：`http://localhost:3000/?file=D:\case\a.txt`；fixture：文件存在；期望：只有整 URL 链接，盘符子串不另建链接；理由：scheme span 必须独占。
20. **[指错风险]** 输入原文：`see (https://host/a_(b)?x=(c)).`；期望：URL 保留内部配对括号，排除 prose 的最后一个 `)` 与句点；理由：粗暴尾裁会打开错误短 URL。
21. **[无风险]** 输入原文：`file:///D:/case/a%20b.txt`；fixture：`D:\case\a b.txt` 存在；期望：整串 file URI 画线，目标只解码一次；理由：既有本地 URI 合同。
22. **[无风险]** 输入原文：`file://localhost/D:/case/a.txt`；fixture：文件存在；期望：链接本地文件；理由：localhost authority 已获准。
23. **[不认风险]** 输入原文：`file://server/share/a.txt`；期望：无文件下划线，包括 `share/a.txt` 子串；理由：remote file URI 按既有裁决拒绝。
24. **[不认风险]** 输入原文：`file:///etc/passwd`；期望：无文件下划线；理由：Windows 上的 POSIX file URI 不映射。
25. **[指错风险]** 输入原文：`file:///D:/case/a%2520b.txt`；fixture：`a%20b.txt` 与 `a b.txt` 均存在；期望：只打开前者；理由：禁止百分号双解码。
26. **[不认风险]** 输入原文：`file:///D:/case/a.txt:12?raw=1`；期望：规则未明确前不画线；理由：必须先统一 URI query 与打印位置的解析次序。

### C. 变量、UNC、长路径、盘符和 Windows 边角

27. **[指错风险]** 输入原文：`$HOME/docs/a.md`；fixture：`D:\case\HOME\docs\a.md` 存在；期望：无文件下划线；理由：不得从变量表达式内部重开 cwd 相对候选。
28. **[不认风险]** 输入原文：`${HOME}/docs/a.md`；期望：无文件下划线；理由：没有输出进程 home 的可信绑定。
29. **[不认风险]** 输入原文：`%USERPROFILE%\docs\a.md`；期望：无文件下划线；理由：不能使用宿主环境代展开。
30. **[不认风险]** 输入原文：`$env:USERPROFILE\docs\a.md`；期望：无文件下划线；理由：PowerShell 变量值与 session 推断 cwd 不等价。
31. **[不认风险]** 输入原文：`~\docs\a.md`；期望：无文件下划线；理由：未建立 shell-specific home 合同。
32. **[不认风险]** 输入原文：`C:foo\bar.txt`；fixture：C 盘当前目录与 session cwd 故意不同；期望：无文件下划线；理由：盘符相对路径不能补根或按普通 cwd 解。
33. **[不认风险]** 输入原文：`\\server\share\dir\a.txt`；期望：无文件下划线，且不探网络；理由：裸 UNC 在既有合同外。
34. **[不认风险]** 输入原文：`\\?\D:\case\a.txt`；fixture：文件存在；期望：无文件下划线，也不链接内层 `D:\case\a.txt`；理由：长路径名字空间不能被剥掉。
35. **[不认风险]** 输入原文：`\\?\UNC\server\share\a.txt`；期望：无文件下划线、不探网络；理由：长路径 UNC 仍有网络与前缀语义。
36. **[不认风险]** 输入原文：`\\.\PIPE\build-daemon`；期望：无文件下划线；理由：设备对象不是文件 URI。
37. **[无风险]** 输入原文：`d:\CASE\SRC\MAIN.rs`；fixture：普通大小写不敏感卷上对应文件存在；期望：打开同一文件，不改变显示大小写；理由：大小写折叠属于文件系统而非词法器。
38. **[无风险]** 输入原文：`D:\case\a.txt.`；fixture：普通 Win32 路径 `a.txt` 存在；期望：维持既有表裁决，显示 span 含点而目标归一到同一文件；理由：这是已接受的 Win32 尾点兼容。
39. **[指错风险]** 输入原文：`"\\?\D:\case\a.txt "`；fixture：扩展名字空间下尾空格文件与普通 `a.txt` 都存在；期望：当前整体不认；理由：trim/剥前缀会指向不同对象。
40. **[指错风险]** 输入原文：`D:\case\a.txt:12`；fixture：base 文件和名为 `12` 的 ADS 均存在；期望：按既有合同恒定解释为 base 文件第 12 行，并明确数字 ADS 不支持；理由：不能让探测结果改变同一文本的语义。
41. **[不认风险]** 输入原文：`D:\case\CON`；期望：在设备名策略明确前无下划线；理由：metadata 与最终打开端可能指向不同对象类型。

### D. git 与虚拟 scheme

42. **[指错风险]** 输入原文：`--- a/src/main.rs`；fixture：真实工作树文件是 `src/main.rs`，同时碰巧存在 `D:\case\a\src\main.rs`；期望：无普通相对文件链接，或仅在明确 diff parser 下映射到 `src/main.rs`；理由：`a/` 是合成前缀。
43. **[指错风险]** 输入原文：`+++ b/src/main.rs`；fixture 同上；期望同上；理由：`b/` 不是工作树目录证据。
44. **[不认风险]** 输入原文：`src/{old => new}/main.rs`；期望：普通路径 pass 无链接；理由：这是 git rename 压缩语法而非单一路径。
45. **[不认风险]** 输入原文：`"a/\346\226\207.txt"`；期望：普通路径 pass 无链接；理由：需要完整 git quoted-path 解码才可承诺。
46. **[不认风险]** 输入原文：`webpack://app/src/main.ts:7:2`；fixture：cwd 恰有 `app\src\main.ts`；期望：无本地文件下划线；理由：虚拟 scheme 不能因本地碰撞被认领。
47. **[不认风险]** 输入原文：`node:internal/modules/cjs/loader:123:4`；fixture：cwd 恰有 `internal\modules\cjs\loader`；期望：无本地文件下划线；理由：Node 内部模块不是 cwd 文件。

### E. 引号列表

48. **[指错风险]** 输入原文：`"D:\case\real;D:\case\missing"`；fixture：`D:\case\real` 存在，整串与第二段不存在；期望：无链接；理由：无列表上下文时分号可能属于单一路径，不能 fallback 拆段。
49. **[无风险]** 输入原文：`PSModulePath="D:\mods\A;D:\mods\B"`；fixture：A、B 均存在且整串不存在；期望：在显式列表模式下分别只给两段画线，分号和外引号不画；理由：字段提供 Windows path-list 证据。
50. **[不认风险]** 输入原文：`PATH=D:\bin;;%SystemRoot%\System32;.\tools`；期望：首版不链接空段、变量段和相对段；若支持字面绝对段，仅 `D:\bin` 可独立探测；理由：各段解析 base/环境不同。
51. **[无风险]** 输入原文：`PSModulePath="D:\Program Files\Mods";D:\mods\B`；fixture：两段存在；期望：字段级列表 parser 正确处理段级引号并分别链接；理由：不能用“第一对双引号就是整个列表”的简化。
52. **[指错风险]** 输入原文：`ARG="https://host/a;b?next=docs/a.md"`；fixture：cwd 有 `docs\a.md`；期望：只有整 URL 链接，不按分号拆路径也不嵌套相对链接；理由：分号是合法 URL 内容。

### F. 物理行边界、重拼与异步账本

53. **[不认风险]** 输入原文：单行 `D:\case\src\main.rs`，候选最后 cell 恰为 viewport 第 79 列且随后是应用 CR/LF；fixture：文件存在；期望：`edge-suspect`，无下划线；理由：完整恰满行与截断前缀不可区分。
54. **[无风险]** 输入原文：同一字符串结束于第 78 列，第 79 列未打印；fixture：文件存在；期望：正常链接；理由：候选没有触及最后 visual cell。
55. **[无风险]** 输入原文：`(D:\case\src\main.rs)` 且右括号占第 79 列；fixture：文件存在；期望：路径正常链接；理由：末列是 prose delimiter，不是候选末 cell。
56. **[指错风险]** 输入为两个物理行（80 列，真实 CR/LF）：

    ```text
    D:\WINDOWS\system
    32\WindowsPowerShell\v1.0\Modules
    ```

    fixture：上半目录与拼接目录都存在；期望：上半不画线、两行不重拼；理由：第五门拒绝已验证半片，第一门负责消除错误前缀。
57. **[无风险]** 输入为两个物理行（上行候选触末列，下行从第 0 列）：

    ```text
    D:\case\very\long\pa
    th\file.rs:12:3
    ```

    fixture：只有拼接后的文件存在，两半均无 yes；期望：五门全过后得到一个跨行显示 span，target 为完整文件第 12 行第 3 列；理由：这是安全重拼正例。
58. **[指错风险]** 输入为两个同缩进物理行：

    ```text
        D:\case\very\long\pa
        th\file.rs
    ```

    fixture：只有去掉第二行缩进后的拼接目标存在；期望：不重拼；理由：同缩进是 peer 行常态，不是 continuation 证据。
59. **[指错风险]** 输入为两个物理行：

    ```text
    D:\case\src
    \main.rs
    ```

    fixture：上半目录与拼接文件都存在；期望：不重拼，上半因 edge-suspect 也不画；理由：磁盘同时存在不能替代作者意图，第五门必须拒绝。
60. **[不认风险]** 输入为两个物理行：

    ```text
    D:\case\prefix-
    src/main.rs
    ```

    fixture：下半相对文件存在；期望：不重拼，下半按自身普通规则可独立链接；理由：已验证下半片阻止制造第三个目标。
61. **[指错风险]** 输入为两个物理行：

    ```text
    https://host:8080/api/us
    ers?id=7
    ```

    上行 URL 触末列；期望：上行不画 URL、两行不重拼，下行无独立 URL；理由：URL 无 witness，只能压住不能猜拼。
62. **[无风险]** 输入为一次 DEC soft-wrap 的逻辑行 `D:\case\very\long\path\file.rs:12`；fixture：文件存在；期望：先由既有逻辑行重组后只生成一个链接，不进入应用重拼；理由：soft-wrap 已有明确 provenance。
63. **[指错风险]** 输入原文：80 列下候选触末列并发出探测，随后 resize 到 120 列，旧 yes 晚到；期望：旧回执不能点亮旧 span，新 generation 重新判；理由：cell 坐标与截断属性已经改变。
64. **[指错风险]** 输入原文：旧 scrollback 行 `src/main.rs` 在 cwd=`D:\repoA` 识别，随后 cwd 变为 `D:\repoB`，两处文件都存在；期望：旧行 target 保持 repoA，repoA 回执不能投给 repoB generation；理由：相对路径必须绑定识别时 base。
65. **[不认风险]** 输入原文：`generated/out.js` 第一次探测不存在，稍后构建生成该文件并再次打印同样文本；期望：第二次候选允许新探测并最终链接；理由：negative ledger 不能永久陈旧。
66. **[指错风险]** 输入原文：`D:\case\out\app.exe` 已获 yes，随后文件被替换为 junction/reparse target 后点击；期望：点击前复核，不能只信历史 yes；理由：positive verdict 不是对象身份保证。
67. **[指错风险]** 输入原文：上行截断候选 `D:\WINDOWS\system` 已在旧普通 pass 中缓存 yes，随后启用 edge gate；期望：账本 yes 不能绕过 span/generation gate；理由：`PathBuf -> bool` 不能单独决定是否点亮某段屏幕文本。

### G. 23 行表的 placement 回归

68. **[不认风险]** 输入原文：边界表第 1 行 `D:\Developer\BetterTerminal\README.md`，分别放在行内与恰末列；期望：行内链接、末列 hard-newline 压住；理由：同一词法结果必须由 placement 轴区分。
69. **[不认风险]** 输入原文：边界表第 12 行 `https://host:8080/img/x.png`，分别放在行内与恰末列；期望：行内 URL、末列 hard-newline 压住；理由：URL 不能漏过截断门。
70. **[无风险]** 输入原文：边界表第 5 行 `(D:\x\a.md)`，右括号恰末列；期望：路径仍链接；理由：候选本身未触末列。
71. **[无风险]** 输入原文：边界表第 17 行 `D:\x\a.md。`，中文句号恰末列；期望：路径照旧链接且句号不入 span；理由：末列有明确 prose 尾巴。
72. **[无风险]** 输入原文：边界表第 3 行 `"D:\a b\c.md"`，闭引号恰占末列；期望：普通引号路径保持原判；理由：闭引号已证明 extent 完整，列表加固和末列门都不应把它压掉。
73. **[指错风险]** 输入原文：边界表第 3 行的扩展 `"D:\a;b\c.md"`；fixture：完整分号文件不存在但 `D:\a` 存在；期望：无列表上下文时不拆、不链接 `D:\a`；理由：专门防止第三件引入新误指。

以上清单的最低安全门不是“更多输入能画线”，而是：任何新增识别都必须证明 **唯一 span、唯一语义所有者、唯一 resolution base、未过期判决、点击 target 与显示承诺一致**。无法证明时，`不认` 是合格结果；把存在的前缀、query 子串或另一 cwd 下的同名文件点亮，不是。
