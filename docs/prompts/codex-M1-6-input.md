# M1.6 输入 QoL 片 kickoff

现状:bt-app(M0 骨架,经 M1/M1.5 演进)键盘输入只有可打印字符、Backspace、Enter、空格;无方向键、Home/End/Delete、无粘贴。这是日用第一痛点。本片补齐基础编辑键与粘贴,不做选区/复制(选区是独立大片)。

## 交付项

### 1. 按键编码(winit KeyEvent → PTY 字节)

- 方向键:普通模式 CSI A/B/C/D;**DECCKM(application cursor keys,CSI ? 1 h/l)开启时 SS3 O A/B/C/D**——模式状态从 vendor Term 读(alacritty TermMode 已有 APP_CURSOR),不要自己另记一份。
- Home/End:CSI H / CSI F(DECCKM 下 SS3 H / SS3 F);Delete:CSI 3 ~;PageUp/PageDown:CSI 5 ~ / 6 ~;Insert:CSI 2 ~。
- 修饰键组合(xterm 编码):Ctrl/Shift/Alt + 方向/Home/End/Delete 按 `CSI 1;{mod} {final}` 与 `CSI 3;{mod} ~` 系列(mod = 1+Shift·1+Alt·2+Ctrl·4),至少覆盖 Ctrl+←/→(词跳)与 Shift+方向;完整矩阵按 xterm 规范做,不要只做清单里这两个。
- Tab/BackTab:Tab 字节 0x09 应已通;Shift+Tab → CSI Z,补上。
- Alt+可打印字符 → ESC 前缀(meta escape)。
- **IME 交互红线**:组字进行中(winit Ime::Preedit 非空)方向键/Delete 等一律交给 IME,不进 PTY——现有 IME 管线里的组字状态就是判据;组字外照常。此项必须有明确代码路径与注释,M0-β 的 IME 成果不得回退。

### 2. Ctrl+V 粘贴(与 Shift+Insert 同路)

- 剪贴板文本获取用 winit/arboard(依赖选择你定,写明理由;不要引重量级)。
- 换行规范化:粘贴文本中的 CRLF 与 LF 一律转 CR(终端粘贴约定)。
- **bracketed paste(DECSET 2004)**:应用开启时用 ESC [ 200 ~ … ESC [ 201 ~ 包裹,且**粘贴内容内出现的 ESC [ 201 ~ 序列必须被剥除**(粘贴注入攻击的标准防御);模式状态同样从 vendor Term 读(TermMode::BRACKETED_PASTE)。
- 大粘贴走现有 PTY 写入通道的背压机制,不一次性无界写入;控制字符(除 CR/Tab)默认过滤并在文档记载政策(参照 xterm/WT 的 paste 安全实践)。
- Ctrl+V 与「未来快捷键审计」的关系:本片先绑 Ctrl+V+Shift+Insert,冲突(vim 里 Ctrl+V 块选)记入 P2-7 快捷键审计待统一,文档注明这是临时绑定。

### 3. 测试与验收配套

- 编码矩阵单测:每个键 × DECCKM 开/关 × 修饰键组合 → 断言字节序列;粘贴规范化(CRLF→CR、201~ 剥除、bracketed 开/关)单测。放 bt-app 或抽独立模块按你架构判断。
- 人工验收脚本(给用户,**必须分页 Read-Host、输出紧凑**——用户明确反馈过输出太长看不到):PowerShell 里验方向键翻历史(PSReadLine)、Home/End/Delete 行编辑、Ctrl+V 粘贴多行命令、`vim`(如可得)或 `less` 里方向键翻页(DECCKM 路径)。注明每步期望。
- 已知边界如实记录:注入键进不了 winit(ui-probe 的已知限制),故按键路径自动化验收受限,以单测矩阵 + 人工脚本为准。

## 门禁

cargo test --workspace --locked;cargo clippy --workspace --all-targets --locked -- -D warnings;cargo fmt 一方 crate;vendor 182 与 bt-render 35 不降(本片不应动渲染;若动 vendor 需说明理由并保测试计数)。完成停下等审。
