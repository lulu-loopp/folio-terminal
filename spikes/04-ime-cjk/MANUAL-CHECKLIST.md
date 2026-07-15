# Task 04 人工 IME 验收清单

这一步必须由真人操作实体键盘并肉眼观察候选框；`SendInput`、宏或自动化按键不能替代。

## 准备

1. 打开 PowerShell，进入 `spikes/04-ime-cjk`。
2. 每家输入法单独运行一次：

   ```powershell
   ./run-manual.ps1 -ImeName "Microsoft Pinyin" -LogName microsoft-pinyin
   ./run-manual.ps1 -ImeName "WeChat Input 2.1.0.36" -LogName wechat-input
   ./run-manual.ps1 -ImeName "Sogou Pinyin <version>" -LogName sogou-pinyin
   ```

3. 蓝框是每次传给 `set_ime_cursor_area` 的物理像素区域。窗口关闭后审计器必须返回成功；若失败，保留日志，不要重跑覆盖。

## 每家输入法都执行的项目

| # | 操作 | 窗口中应看到 | 结构化日志应有 | 失败判据 |
|---|---|---|---|---|
| 1 | 切到中文全拼，实体键盘输入 `zhongwen`，选“中文” | 黄色 preedit 随输入变化，提交后进入 Committed | 非空 `ime_preedit`，清空 preedit，非空 `ime_commit` | 只有 KeyboardInput、无 Preedit；提交重复/丢字/乱码 |
| 2 | 输入拼音后用左右键移动预编辑光标/选词 | 黄色串和 byte cursor range 更新 | cursor range 始终落在 UTF-8 边界内 | 范围越界、候选选择与高亮位置不同步 |
| 3 | 输入一半后按 Backspace 两次 | preedit 缩短，不删除已提交文本 | 连续 `ime_preedit`，没有错误 commit | Backspace 穿透到 committed；preedit 不更新 |
| 4 | 输入一半后按 Esc | preedit 清空，committed 不变 | 空 `ime_preedit` 或 `ime_disabled`，无被取消文本的 commit | 被取消文本仍提交；窗口把 Esc 当退出 |
| 5 | 按 Shift 或该 IME 的中英切换键，分别输入 `abc` 与 `中文` | 英文直接输入、中文仍走 Preedit/Commit | 英文 KeyboardInput text；中文 IME 事件 | 切换后 IME 永久失效、字符重复 |
| 6 | 按 F2 移动蓝框，再开始一次拼音 | 候选框出现在新蓝框附近 | 至少两个不同 area 的 `set_ime_cursor_area` | 候选框仍停在旧位置或屏幕角落 |
| 7 | 按 F3 开启鼠标跟随，开始拼音并移动鼠标 | 蓝框和候选框跟随鼠标 | `mouse_tracking` area 连续变化；每帧记录相同 area/preedit | 蓝框动但候选框不动；候选框闪到错误 DPI 坐标 |
| 8 | preedit 尚未提交时切换到另一家 IME，再切回来 | 不崩溃、不残留幽灵 preedit；后续仍可输入 | Disabled/Enabled 或 preedit 清理序列 | preedit 卡住、丢焦点、后续无事件 |
| 9 | 使用双拼方案输入一个词；若该 IME 未配置双拼，记 N/A 并写原因 | 与该方案在记事本中的行为一致 | Preedit/Commit 完整 | 只写“应该可用”而未实测 |
| 10 | 把窗口拖到不同 DPI 显示器（若只有一个显示器记 N/A），再输入 | 蓝框、preedit、候选框仍重合 | `scale_factor_changed` 后新的 area 和 frame | 候选框按旧 DPI 偏移 |

## 肉眼结果回填

复制下表三次，每家一份，并附对应 `.jsonl`：

| 输入法 / 版本 | 全拼 | 双拼 | Backspace | Esc | 中英切换 | 中途换 IME | F2 跟随 | F3 动态跟随 | DPI | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|
|  | PASS/FAIL | PASS/FAIL/N/A | PASS/FAIL | PASS/FAIL | PASS/FAIL | PASS/FAIL | PASS/FAIL | PASS/FAIL | PASS/FAIL/N/A |  |

任何一项 FAIL 都不要自行解释为“输入法问题”；保留日志和复现动作，交回后再依据 winit/IMM32/TSF 契约判定。
