# Spike 03：数学渲染引擎选型

日期：2026-07-16
结论：**go-with-caveats**

## 结论

**选择 A（MiTeX + Typst）作为 M3 数学引擎，结论为 go-with-caveats。** A 已满足原生 SVG、有效语料 310/310 返回产物、关键结构视觉正确和进程级抢占隔离；锁定 M3 前必须修复 9 条同构自定义宏的 h/d 提取缺口。自动折行按任务书是交由人工权衡的 caveat，不是出局门槛。

- **A（MiTeX 0.2.4 → Typst 0.15 → SVG）入选，携带两项 caveat。** 310 条预期有效输入全部返回 SVG；视觉样本的分数、根号、伸缩定界符、大型运算符、矩阵和多行对齐结构正确。301/310 条有效输入得到 h/d；缺失的 9 条全部属于 30 条仅整数下标不同、结构完全相同的自定义宏，而其余 21 条能提取 h/d。这说明当前证据指向约 40 行 `find_math_metrics` frame 遍历的 adapter 缺口，而不是 Typst 根本没有基线能力；锁定 M3 前必须定位并修复，或明确根因位于 MiTeX/Typst/adapter 哪一层。长公式未观察到自动折行，这是需人工权衡的 caveat。release 二进制净增约 44.8 MiB。
- **B（ReX）本轮未充分评估，保留为纯 Rust 备胎。** 最活跃的 `KenyC/ReX` 固定 revision `aeccdba`：核心依赖和 parser probe 在 Rust 1.94.1 上通过；本轮只探测上游 Cairo SVG 示例 feature，它在本机 Windows MSVC 上因找不到系统 `pkg-config`/Cairo/GLib 而构建失败。该结果没有证明 Cairo 是唯一 SVG 路径，也没有评估 vcpkg/gvsbuild 提供 Cairo 或其他 ReX 后端。
- **C（rquickjs 0.12.1 + KaTeX 0.16.22）no-go 作为原生渲染路径。** 310/310 条有效输入都有 KaTeX h/d，release 净增约 3.0 MiB；但 KaTeX 给的是 HTML/MathML，本原型只能装进 SVG `foreignObject`，不是 wgpu 可直接消费的独立 SVG。它没有自动折行，且全语料 p95 为 344.7 ms。

进程隔离方法可行：32 MiB Job Object 硬上限，16 MiB private-bytes 监视阈值触发后由 `TerminateJobObject` 在 20 ms 内结束进程；50 ms 时间预算在 53 ms 观察到抢占。它证明了“超时即抢占”，不是任务返回后看表。

## 交付物

- 独立 workspace：`spikes/03-math-engine/`
- 语料：`corpus/math-expressions.jsonl`，360 条
- A/C benchmark 与 Windows Job Object 探针：`src/bin/math-bench.rs`
- ReX core probe：`rex-probe/`
- ReX SVG 失败 probe：`rex-svg-probe/`
- 原始数据：`docs/spikes/artifacts/03-math-engine.json`
- 六类并排证据：`docs/spikes/artifacts/03-visual/comparison.png`、对应 SVG 与 HTML
- 一键测试：`cargo test --manifest-path spikes/03-math-engine/Cargo.toml --locked`

主 workspace 未添加成员或依赖；`vendor/alacritty_terminal` 仍在根 workspace members。

## 语料与方法

语料固定为 360 条 JSONL：分数 40、根号 40、伸缩定界符 30、大型运算符 40、矩阵 40、AMS 多行对齐 40、Unicode math 30、自定义宏 30、畸形输入 30、恶意输入 20、超大表达式 20。310 条标为预期有效，50 条为畸形/恶意。

两条可运行路径使用同一批输入、同一台机器、release 构建串行测量。host 在进入引擎前限制输入为 64 KiB、嵌套深度 256，并拒绝文件/网络命令和直接递归宏。路径内分别关闭 Typst 的文件/包/网络 resolver，以及 QuickJS 的文件、进程、socket、`fetch`、`require` 和 module host binding。

`p50/p95` 包含成功、语法失败和 host 拒绝的全部 360 次调用；因此是端到端工作负载分布，不是只挑成功样本。内存由独立一次渲染进程的 Win32 peak working set 记录。二进制增量以 130,048 B 的同 profile baseline executable 相减。

## 支撑数据

| 指标 | A：MiTeX + Typst | B：ReX | C：QuickJS + KaTeX |
|---|---:|---:|---:|
| 预期有效返回产物 | 310/310 | 未进入语料门 | 310/310 |
| 有效输入同时有 h/d | 301/310（97.1%） | 未测 | 310/310（100%） |
| 全部成功调用 | 328/360 | 未测 | 310/360 |
| host 安全拒绝 | 20 | 未测 | 20 |
| p50 | 20.605 ms | 未测 | 3.989 ms |
| p95 | 35.077 ms | 未测 | 344.743 ms |
| 独立进程 peak working set | 19.0 MB | 未测 | 9.7 MB |
| 相对 baseline 的 peak 增量 | 11.7 MB | 未测 | 5.0 MB |
| release executable | 47,080,448 B | 未产出 | 3,247,616 B |
| release 净增量 | 46,950,400 B | 未产出 | 3,117,568 B |
| 原生独立 SVG | 是 | SVG 链构建失败 | 否，仅 `foreignObject` HTML |
| 自动数学折行 | 未观察到 | 未测 | 否 |

A 的 9 个有效 h/d 缺失全部来自 `custom-macro`。语料核对显示 30 条宏的结构完全相同、只差整数下标，其中 21 条能提取 h/d、9 条不能；这类时灵时不灵的结果是 `find_math_metrics` adapter 必修 bug 的证据，不是引擎缺少基线能力的证据。另外 18 个畸形输入返回无基线产物，不纳入有效输入的 h/d 判断。C 的高 p95 主要受超大表达式尾部影响；这进一步说明它必须在可抢占 worker 内运行，不能驻留 UI 线程。

### 进程级隔离

| 项目 | 结果 |
|---|---:|
| Job Object process memory hard limit | 32 MiB |
| host private-bytes kill threshold | 16 MiB |
| 内存压力终止 | 通过；观测 17,469,440 B，20 ms |
| 超时预算 | 50 ms |
| 超时终止 | 通过；53 ms |
| 正常 spawn + assign + exit | p50 20.246 ms；p95 35.379 ms（n=10） |

硬内存 limit 本身的 Windows 语义是阻止继续分配，不等于自动终止。因此原型同时监视 child private bytes，并用 Job handle 结束整个进程组；报告不会把“设置了 limit”冒充“已经 kill”。

## 视觉质量抽查

`comparison.png` 是同一批六类公式的 A 产物与 KaTeX reference 实际浏览器截图，不是手工排版的示意图。

1. 分数：两者分子/分母居中、分数线完整；A 默认字号明显更小。
2. 根号：三次根号指标和根线覆盖正确；字形与 KaTeX 不同但结构一致。
3. `left-right`：两者括号均按内部分式拉伸；KaTeX 定界符更粗、更高。
4. 大型运算符：求和上下限、积分上下限位置正确；A 的整体墨色和尺寸偏小。
5. 矩阵：两行两列与伸缩括号正确；未见元素碰撞。
6. AMS align：等号列保持对齐；A 的行距更紧、字号明显小于 reference。

字号差异来自原型 adapter 的 12pt 与 comparison 页的 KaTeX 20px 配置，说明生产适配器必须统一 em/pt，而不是据此断言引擎排版错误。结构性项目均通过目视抽查，但这里只覆盖六条代表样本，不能替代 310 条的视觉回归图库。

## 自动折行与 h/d

A 的同一条 40 项长公式在 480pt 和 160pt 下均为 14.128pt 高，SVG 内容横向溢出；没有在顶层关系符或二元运算符断开，也没有续行缩进。C 的 KaTeX 输出同样 nowrap，宽度参数不参与布局。B 未能进入 SVG 布局阶段，不能声称有或没有折行能力。

按任务书，“不能断的路径不等于出局”，因此自动折行只作为选型 caveat：C 明确继承 KaTeX 的 nowrap 限制；A（Typst）的其他布局方式与 B（ReX）的折行能力需后续单独评估。若未来另行评估 MathJax-SVG，它属于新的引擎路径，不能拿来补写本轮 C 的能力。折行代价最终交由人工权衡。

A 从 Typst frame baseline 推导 ascent/descent；C 从 KaTeX `.strut` 的 `height=h+d` 与 `vertical-align=-d` 解析。A 对 9 条有效输入拿不到 frame baseline，所以本轮不能用“总图片高度”猜测补齐。

## 选型建议与降级预案

本轮选择 A 作为 M3 引擎，但带条件交接；这不授权现在进入产品实现：

1. **必修 caveat：**锁定 M3 前定位并修复 `find_math_metrics` 对 9 条同构宏 frame 的 h/d 提取，或用证据明确根因位于 MiTeX、Typst 或 adapter 哪一层；310/310 有效样本 h/d 仍是修复验收门。
2. **权衡 caveat：**当前 A 裸 math block 没有自动折行。用统一字号扩展视觉图库，并单独评估 Typst 关系符优先断行；是否接受横滚由人工裁决，不作为本轮否决 A 的理由。
3. ReX 保留为本轮未充分评估的纯 Rust 备胎；若 A 的必修 caveat 最终证明不可修，再评估 vcpkg/gvsbuild Cairo 与非 Cairo 后端，而不是沿用“Windows 无可部署路径”的过度结论。
4. C 只保留为进程隔离的 HTML/离屏浏览器研究方向；不能把 `foreignObject` 冒充原生 SVG，其无折行与高尾延迟必须保留。
5. 若后续裁决推翻 A，再按 R10 降级：M3 只保留块级数学检测与源码展示，或使用 `tui-math` 风格字符网格排版；横滚块可作为显式低质量 fallback，但不能称为自动折行。

本报告把 A 交接为 M3 的 go-with-caveats 选择，并保留 ReX/C 与 R10 降级路径；**没有修改 DESIGN.md，也没有进入 M0/M3**。

## 测试与反向验证

主 probe `cargo test` 4 passed，`cargo clippy --all-targets -- -D warnings` 通过。测试覆盖语料数量/类别、安全上限、KaTeX h/d 提取和失败计数。

反向证据包括：最初缺少 MiTeX Typst scope 时 A probe 确实编译后运行失败；补入 upstream MiTeX Typst assets 后才产出 SVG。ReX core 的错误私有 API 调用先编译失败，改用公开 `rex::parser::parse` 后才通过。ReX SVG probe 仍稳定失败在 `cairo-sys-rs`/`glib-sys` 的系统依赖发现阶段，因此它是一扇会红的门，而不是只检查 Cargo.toml 存在。

## 遗留风险

1. A 的 9 个同构有效宏样本缺 h/d 尚未根因定位；这是锁定 M3 前必须修复或定责的 adapter caveat，不是引擎能力的一票否决，也不是统计噪声。
2. 两条可运行路径都未观察到窄宽自动折行；超长公式仍需横滚或降级。这是任务书明确交由人工权衡的 caveat，不是本轮选型门槛。
3. 视觉抽查只有六类各一条，尚无像素差异阈值或跨字体平台基线。
4. ReX 只证明 core/parser 可编译；本轮只探了 Cairo SVG 示例 feature 并失败，未证明它是唯一 SVG 路径，也未评估 vcpkg/gvsbuild Cairo 或非 Cairo 后端，性能与 h/d 均未知。
5. QuickJS 的 344.7 ms p95 与 in-process elapsed watchdog 不能提供抢占；生产若继续调查必须始终放在 Job worker 中。
6. Job spawn/assign 正常开销 p95 35.4 ms，不适合每公式启动一次；需要常驻、可轮换的受限 worker 池，同时保持 generation 取消。
7. 当前只测 Windows x86_64 MSVC 和一台机器；二进制/LTO、字体集合、低端 CPU 均会改变数值。

## 偏离申请

无。路径 B 的已测 Cairo SVG feature 链在目标环境无法构建，按任务规则如实停止，但不外推为 ReX 没有其他可部署路径；路径 C 无法生成原生独立 SVG，未把 HTML `foreignObject` 包装冒充达标。A 的 9 条 h/d 缺口按同构语料证据归为必修 adapter caveat，自动折行按任务书归为人工权衡项，因此结论修正为 go-with-caveats。
