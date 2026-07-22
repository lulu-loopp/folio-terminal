# M1.9r 任务书:还原 Claude Code 吃掉的数学环境换行 `\\`(带开关)

档位 high(踩「忠实渲染 vs 补救坏输入」边界,规则须确定性、非启发式)。
基线 = **当前工作树**(`efd2587` + 未提交的 M1.9q alt 装饰改动)。
**本片只改 `crates/bt-detect/`(+ 必要的 options 传递),严禁碰 `session.rs` 的 alt 装饰/
重绘逻辑(那是进行中的 M1.9q)。**

## 根因(协调者已用 dump 字节铁证)

Claude Code 的 markdown 把数学环境换行符 `\\`(两个反斜杠)**吃成单个 `\`** 输出到终端。
录制 `.tmp-repaint-capture/cc-topbot.vt` 证:`\begin{aligned}` 块里双反斜杠字节 `5c5c`
出现 **0 次**、单 `\` 有 29 个;CC 实发 `... &= 0 \`(单反斜杠)+ `\x1b[K\r\x1b[1B`
(绝对定位换行)写下一行。`aligned`/`matrix`/`cases`/`gather` 靠 `\\` 分行,收到单 `\`
→ 公式引擎无换行命令 → **所有方程挤成一行重叠**(用户实拍 image229)。我们是忠实渲染
CC 的坏输出,但结果对所有复杂多行公式都不可用。

## 目标

在**数学环境内**,把被 CC 吃掉的 `\\` 还原回来,让多行公式正确分行渲染。
**必须带开关**(用户明确要求):CC 迟早会修此 bug,届时它正常发 `\\`,我们的还原就会
反噬成 bug——所以要能一键关掉恢复忠实渲染。

## 确定性还原规则(这是语法还原,不是概率启发式)

依据:LaTeX 数学环境内,一个逻辑行**行尾**出现裸单 `\`(其后是行结束、非字母命令、
非第二个 `\`)是**语法非法**的——唯一合法来源就是被吃掉一个反斜杠的 `\\`。据此:

- **仅作用于**:`is_math_environment`(`bt-detect/src/lib.rs:448`)判定为真的环境内
  (aligned/alignedat/matrix 全家/cases/array/gather/gathered/split 等);**环境外(含
  普通 `$$…$$`、`\[…\]`、行内、散文)一律不改**。
- **仅作用于逻辑行行尾**:`joined_range`(:978)按 `\n` 拼的每个逻辑行(= 硬换行;
  软折行 `continues`(:179)已在上游合并,**不在软折行拼接处还原**)。
- **命中条件**:该逻辑行 `trim_end` 尾部空白后,以**恰好一个** `\` 结尾
  (即倒数第二字符不是 `\`,排除已是 `\\`),还原为 `\\`。
- **幂等 & 不误伤**:已是 `\\` 的不动;`\` 后跟命令字母的(如行中 `\nabla`,本就不在
  行尾)不动;环境的最后一行(其后无内容、本不需 `\\`)按 LaTeX 允许尾随 `\\` 处理——
  但更稳妥是**只对"后面还有同环境逻辑行"的行还原**(末行不加),请按此实现避免多余 `\\`。

## 开关(用户核心要求)

- 加配置项(建议挂现有 `MathLayoutOptions` 或检测 options,由 session/app 传入 detect):
  `restore_stripped_environment_newlines: bool`,**默认 `true`**(当前 CC 有 bug)。
- `false` 时:行为**逐字节等同基线**(不做任何还原)——用变异回归锁死这一点。
- 命名与默认值写清注释:CC 修复 markdown 转义后应置 `false` 恢复忠实渲染。

## 硬约束(不可回退)

- 非数学环境源码**一字节不改**;
- M1.9k 两条红线(错配/CJK 散文)、九类误报守卫、CommonMark 代码上下文守卫全绿;
- M1.9o 三条 repaint 回归、M1.9m 几何 全绿;
- 不碰 `session.rs` alt 逻辑、vendor/glyphon;
- bt-detect 保持纯函数可单测,开关经参数传入(不要全局状态/env 读取)。

## 回归(各配变异)

1. `$$\begin{aligned} … &= 0 \ (单) … \end{aligned}$$`(逐行尾单 `\`)→ 还原后每逻辑行
   以 `\\` 结尾、KaTeX/引擎正确分多行;**变异:开关 off → 保持单 `\`(不还原)**;
2. 正常已含 `\\` 的 aligned 源码 → 幂等**不变**(不出现 `\\\`);
3. 环境**外**行尾单 `\`(如散文 `foo \` 或 `$$x \$$`)→ **不动**;
4. matrix/cases 同 ① 一条;
5. **真实验证**:用 `.tmp-repaint-capture/cc-topbot.vt` 经 repaint oracle 回放,
   `\begin{aligned}` 麦克斯韦块还原后源码含 `\\`(记录 before/after 源码片段);
6. M1.9k 两条红线变异仍红、M1.9o 三条 repaint 仍绿。

## 门禁

全 workspace `--locked --offline`、310 语料、G1/G2/G3、clippy `-D warnings`、fmt、
adapter boundary;vendor/glyphon 零改。数字实跑抄写、附原始片段;第一遍失败如实报。
(注:`bt-pty` 的 `real_conpty_child_receives_color_environment…` 今日因宿主 ConPTY
不稳 flaky,与本片无关——若它失败,单独标注、重跑确认,勿改本片归因。)

## 交付(写进 output 文件)

还原接入点 file:line、开关接入点、确定性规则的语法依据陈述、真实 dump before/after
源码片段、新增回归(含开关 off 变异)、门禁数字。停下等审,不提交。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M8B2ZEM1UsvgLCidRXEpvR
