# M1：宽度正确性交付报告

> M1.5 已补足彩色 emoji 与 fallback em 归一；当前字形验收和字体资产策略见 `M1.5-glyph-quality.md`。本文保留 M1 当时的宽度证据与历史边界。

## 结论与证据边界

M1 的代码与自动化门完成：单一 width oracle 默认把 East Asian Ambiguous 判窄；DEC private mode 2027 开启 UAX #29 extended grapheme clustering；emoji/VS/ZWJ/flag 簇宽最多 2 cells；grid 继续产出既有 `WIDE_CHAR` lead + spacer，renderer 对 committed 内容没有新增宽度特判。

ConPTY 侦察决定 **2027 默认关闭**。程序发 `CSI ? 2027 h` 后，BetterTerminal grid 才走聚类档；但经 PowerShell/ConPTY 的屏幕输出不是 payload 的透明通道，不能再用 shell `echo` 的闭合竖线反推 grid 宽度。宽度视觉验收改由 `BT_PROBE_INPUT` 直接把受控字节送入 Term；ConPTY 路径只验稳定性与协商透传。

真实 BetterTerminal 窗口的最终肉眼裁决仍由人工完成；自动化覆盖原始 ConPTY 字节捕获、同流 grid 回放、光标、lead/spacer、renderer 输入和 resize 不变量。

## 改动与行为测试

| 改动 | 行为 | 自动化证据 |
|---|---|---|
| `bt-unicode` 单一 oracle | `unicode-width 0.2` 字符串规则处理 VS15/16、emoji modifier、ZWJ、RI；每簇钳 2；ambiguous 默认 narrow；保留唯一 `AmbiguousWidth` policy seam 给 P2-7 | `emoji_variation_and_clusters_follow_one_clamped_oracle`；`ambiguous_defaults_narrow_and_has_one_future_configuration_seam` |
| UAX #29 streaming 边界 | `unicode-segmentation::GraphemeCursor` 使用当前簇完整上下文处理 GB11 与 GB12/13；状态跨任意 PTY 字节切块保留 | `uax29_extension_state_covers_combining_emoji_zwj_and_flags`；`recorded_width_corpus_drives_legacy_and_mode_2027_oracles_byte_by_byte` |
| mode 2027 协商 | `CSI ? 2027 $ p` 回 reset `CSI ? 2027 ; 2 $ y` / set `... ; 1 $ y`；DECSET/DECRST 开关聚类 | `dec_mode_2027_query_set_and_reset_use_standard_decrqm_semantics` |
| vendor 写入 seam | 簇文本存 lead cell，spacer 沿用 alacritty flags；晚到 VS/RI 导致 1↔2 时原子重写，右边界不拆簇；DECAWM 关闭时无法容纳的 1→2 保持单格并维持 pending-wrap；insert mode 不吞尾部 | `grapheme_mode_clusters_the_m1_width_matrix_while_legacy_mode_stays_compatible`；`late_vs_and_flag_width_changes_rewrite_atomically_at_the_right_margin`；`late_wide_grapheme_at_right_margin_stays_narrow_without_decawm`；`late_narrow_grapheme_at_right_margin_shrinks_without_decawm`；`late_cluster_width_changes_preserve_insert_mode_tail_cells` |
| 混排与 resize | cluster+CJK+ASCII 只产生既有 lead/spacer；完成簇在 reflow 后仍是同一 lead/spacer pair；resize 若插在两个 codepoint 之间会重新锚定进行中的簇 | `mixed_clusters_wrap_as_an_indivisible_wide_lead_and_spacer`；`resize_never_separates_cluster_text_from_its_wide_spacer`；`resize_between_codepoints_reanchors_the_in_progress_cluster`；`replayed_resize_keeps_a_completed_cluster_in_one_lead_spacer_pair` |
| renderer / IME | committed renderer 继续只消费 cell flags；preedit 改用同一 oracle，提交前后 caret 不跳 | `preedit_uses_the_same_cluster_oracle_as_committed_cells`；β 的 `mixed_cjk_ascii_wide_slots_use_exact_terminal_cell_origins`、`cursor_on_either_half_of_a_wide_cell_covers_both_cells` 继续通过 |
| corpus 裁决 | `cjk-width-cases.json` 将 ambiguous 来源从 pending 改为 M1 narrow；同一 22-case corpus 同时钉 legacy 与 2027 两列 | `recorded_width_corpus_drives_legacy_and_mode_2027_oracles_byte_by_byte` |
| 模式压力复现 | `2027h → DECAWM off → 行尾 VS16 晚变宽 → DECAWM on → 2027l` 后，legacy family 仍占 8 格且内容存在 | `decawm_margin_pressure_then_decrst_2027_cannot_consume_legacy_text` |
| 实机字节回放 | probe 运行旧验收的等价 PowerShell 序列，打印完整原始流并把同一流喂给 `DualPlaneSession`；混排与 ruler 的闭合列均为 8，旧脚本的空 family 行按字节原样重现 | `bt-conpty-width-probe` 内建断言 |
| 直灌视觉入口 | `BT_PROBE_INPUT=<file>` 时不启动 ConPTY，文件字节直接进入 Term；左右两栏 fixture 在一屏并列 legacy/2027 全矩阵 | `scripts/dev/width-probe-input.vt` 与配套说明 |

vendor 补丁面只有 `alacritty_terminal::Term` 的输入/模式状态与一条到 `bt-unicode` 的依赖；没有改 grid、resize 或 renderer 结构。`cargo metadata --no-deps --format-version 1` 继续把根目录列为 workspace root，并列出 `vendor/alacritty_terminal` member。

## 渲染层整列定位与 overhang 政策

Committed text 的最终 pen-x 由 grid 列唯一决定：第 `n` 格的起笔位置是 `padding + n * cell_width`。每个非空窄格独立整形为局部 `x ≈ 0` 的 buffer，再由该格的绝对 `TextArea.left` 放到 grid 列；宽 lead 则独立整形并放入绝对列上的两格槽位。格与格之间的 ligature/kerning 被有意禁用，因为终端 grid 已经确定每格的起笔与推进，跨格 shaping 会让字体度量改写终端语义。

变更行仍以 `row_needs_reshaping` 作为粗 damage 门；门内的窄格整形结果由 renderer 级缓存按 `(text, bold, italic)` 复用。列与颜色不进 key：列只影响 `TextArea.left`，颜色由每格的 `default_color` 提供，二者都不改变 glyph 与 advance。同文本跨列、跨行、跨帧共享同一个 buffer；缓存使用 1024 项访问时钟 LRU，满额淘汰最久未使用项，scale/字号变化时与行缓存一起清空。因而 80 个相同 ASCII 格的脏行仍建立 80 个轻量放置实例，但只在首次未命中时 shape 1 次，后续行和帧均命中；成本按不同 `(text, bold, italic)` 的数量而不是非空格数增长。

窄格字形允许整行 overhang，不按单格裁剪，以免切掉 italic、重音和字体本身的左右 bearing；其起笔仍固定在整数格轴上。唯一例外是宽槽：宽 CJK/emoji buffer 按 `[left, left + 2 * cell_width]` 双格裁剪，防止缺字字体产生的多枚 `.notdef` 墨迹污染后续格。背景、光标和 damage 始终按 grid cell 计算。

跨层核实：legacy 模式关闭 grapheme clustering，vendor 对 RI 逐 scalar 调用宽度 oracle；单个 RI 宽 1，所以真实 US flag 发成两个普通窄 lead、没有 spacer，走窄格绝对定位路径；2027 模式才把 RI 对聚成一个两格 lead + spacer，走宽槽路径。

## 人工验收异常的最终三因定案

1. 脚本层：旧 `width-acceptance.ps1` 把码点 helper 命名为 `cp`，撞上 Windows PowerShell 5.1 的 `Copy-Item` alias，造成 family payload 实际缺失；现已改名 `CodePoints`。
2. 渲染层：旧主行把窄字符连续 shaping，并用字体 advance 推进后续 glyph；`☆` 等窄 fallback 的实际 advance 不同，误差会传给本行后续 glyph，而 grid 的 lead/spacer 与闭合列记账本身正确。现由逐格独立整形和绝对列定位隔离 advance，renderer 级缓存消除重复 shaping。
3. 传输层：PowerShell Console API / ConPTY 会先按自己的 screen model 记账，再以 CUP、擦除和重绘合成 VT；它不是 payload 的透明通道。shell 标尺只适合观察 ConPTY 兼容行为，宽度像素裁决必须使用 `BT_PROBE_INPUT` 直灌 fixture。

## ConPTY 侦察

环境：Windows `10.0.26200.8875`；`System32/conhost.exe` FileVersion/ProductVersion `10.0.26100.1`；Windows PowerShell `5.1.26100.8875`。可复现探针：

```text
cargo run -p bt-pty --bin bt-conpty-width-probe --locked
```

探针通过真实 `portable-pty` native ConPTY 启动 PowerShell，先回答启动时 ConPTY 发来的 DSR `ESC [ 6 n`，然后以 Console API 写入每个序列并读取 `Console.CursorLeft`。宽度流完整打印为 `width_raw_hex` / `width_raw_debug`，旧验收等价序列打印为 `acceptance_raw_hex` / `acceptance_raw_debug`，摘要为：

```text
legacy-family=11       mode2027-family=11
legacy-skin-tone=4     mode2027-skin-tone=4
legacy-combining=2     mode2027-combining=2
legacy-umbrella-vs15=2 mode2027-umbrella-vs15=2
legacy-umbrella-vs16=2 mode2027-umbrella-vs16=2
legacy-flag=4          mode2027-flag=4
legacy-ambiguous-star=1 mode2027-ambiguous-star=1
```

宽度套件的字节证据：family 的 UTF-8 原样出现为 `f0 9f 91 a8 e2 80 8d f0 9f 91 a9 e2 80 8d f0 9f 91 a7 e2 80 8d f0 9f 91 a6`；DECSET/DECRST 2027 也原样出现，分别为 `1b 5b 3f 32 30 32 37 68` 与 `1b 5b 3f 32 30 32 37 6c`。也就是说 ConPTY **转发 payload 与 2027 协商序列，但自身 Console cursor 簿记没有切到 grapheme**；family=11 还显示它比 scalar wcwidth 的 8 更接近 UTF-16 code-unit 兼容记账。

二轮 `||` 定案：旧 `width-acceptance.ps1` 把码点 helper 命名为 `cp`，与 Windows PowerShell 5.1 的 `cp → Copy-Item` alias 冲突。真实原始流在 `CSI ? 2027 l` 后只有 `7c 7c`，没有 family UTF-8；同流回放自然也显示 `||`。因此异常 3 是验收脚本 bug，不是 Term 丢字符，也没有 vendor 状态机修复。重写脚本已改名为 `CodePoints`。独立的纯 VT 压力回归证明相同模式切换后 family 内容保留且按 legacy 占 8 格。

混排定案：等价序列的原始流含连续的 `|A☆中│Ｂ|` 与 `|#######|`；同流回放中两行闭合 ASCII `|` 都落在零基第 8 列。异常 2 不是 grid 宽度错误。该次捕获在混排行内部没有 CUP，所以也不能把截图差异精确归因成“这一行被 CUP 移了两格”；它只证明 shell 标尺不是可靠的 grid oracle。宽度像素裁决改走直灌 fixture。

默认档决定：关闭 2027。若 BetterTerminal 默认直接聚类，未协商且依赖 ConPTY/legacy 记账的程序会立刻产生另一种光标分歧；显式协商的程序则能让转发后的 BetterTerminal grid 开启聚类。

## ConPTY 中介下的视觉现实

PowerShell 的 Console API 先更新 ConPTY/console 自己的屏幕模型，ConPTY 再合成 VT；本机原始流可见 `CSI 2 J`、`CSI H`、`CSI K` 与 `CSI row ; col H` 的整屏擦除、绝对定位和重画，故它不是 shell 字符串的透明传输。双方列记账不一致时，后续重画位置由 ConPTY 的 CUP 坐标主导；终端可能只能忠实显示一个由另一套宽度规则排好位置的字节流。间隙或闭合线错位不能单独证明 BetterTerminal grid 算错。

本机实测记账如下；2027 开/关不改变 ConPTY 数值：

| 序列 | ConPTY cells | BetterTerminal legacy | BetterTerminal 2027 |
|---|---:|---:|---:|
| family ZWJ | 11 | 8 | 2 |
| thumbs + skin tone | 4 | 4 | 2 |
| `e` + combining acute | 2 | 1 | 1 |
| umbrella + VS15 | 2 | 1 | 1 |
| umbrella + VS16 | 2 | 1 | 2 |
| US flag | 4 | 2 | 2 |
| ambiguous `☆` | 1 | 1 | 1 |

这正是默认关闭 2027 的裁决依据：默认档优先保持未协商应用的 legacy 兼容面；支持 2027 的应用可显式开启聚类。宽度正确性必须绕过 ConPTY 验，ConPTY 端到端则如实接受由中介记账造成的间隙。

## 真窗口人工验收清单

宽度视觉正确性：按 `scripts/dev/width-probe-input.md` 设置 `BT_PROBE_INPUT` 并启动 release `bt-app`。应用不会启动 ConPTY，而会把 `.vt` 文件原始字节直接送入 Term。左右两栏分别是 legacy 与 2027；逐对确认内容行与 `#` ruler 的闭合 `|` 垂直对齐。该单屏覆盖 family、肤色、国旗、combining、混排和 VS15/16；豆腐块可接受。

ConPTY 端到端：清掉 `BT_PROBE_INPUT`，正常启动 shell，运行 `scripts/dev/width-acceptance.ps1`。脚本每段 `Read-Host` 分页。只判定两件事：

1. 默认 legacy 下记账不一致的串不崩溃、不造成后续残留错乱；间隙类差异按本节已知现实记录，不判 grid FAIL。
2. `CSI ? 2027 h/l` 原样透传，2027 + DECAWM 行尾晚变宽后仍能看到 `PANIC PASS`，复原后 family 内容存在，最终 `DONE` 可见。

## 已知边界

- M1 当时没有承诺彩色 emoji；该历史边界已由 M1.5 的显式 Noto/VS 路由与彩色光栅测试解除，见 `M1.5-glyph-quality.md`。
- 当前 ConPTY 自身即使收到并转发 2027，也不改变 Console API cursor 簿记。协商程序应以终端的 DECRQM/DSR 结果为准，不应把 ConPTY 的 `Console.CursorLeft` 当作 grapheme oracle。
- PowerShell/ConPTY 的 shell 标尺不再承担宽度视觉验收；双方记账不一致时，CUP 重画可制造间隙。`BT_PROBE_INPUT` fixture 才是受控视觉证据。
- `can_extend_grapheme` 只校验进行中簇的锚点、预期光标/换行状态与屏幕，不重新校验 lead 格当前内容。因此 CUP 精确移走再移回后到达的组合符仍可能按原进行中簇附着；该兼容语义本轮不改。
- resize 后的 `reanchor_grapheme_after_resize` 按簇内容搜索最近 lead；若屏幕上有多个同内容簇，锚点存在二义性。它只影响 resize 恰好发生在簇尚未完成的瞬间，本轮不引入额外 identity 机制。
- `cargo fmt --all -- --check` 会要求把整个上游 vendor 按本仓库 rustfmt 重排；沿用 M0 纪律，只对一方 crates 执行 fmt check，vendor 保持上游格式，M1 新 seam 由编译、clippy 和行为测试钉死。

## 性能后续项

- 2027 开启时，`extends_grapheme_cluster` 当前每个字符都会在 `crates/bt-unicode/src/lib.rs` 通过 `String::with_capacity` 分配候选缓冲，`extend_grapheme` 又以 `mem::take` 暂取状态。作为 opt-in 模式本轮可接受；后续可改用真正流式的 `GraphemeCursor` 状态，或复用候选缓冲，消除逐字符堆分配。
