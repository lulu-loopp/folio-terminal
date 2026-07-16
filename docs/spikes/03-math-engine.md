# Spike 03：数学渲染引擎选型

日期：2026-07-16
结论：**no-go**

## 结论

三条路径目前没有一条同时满足 BetterTerminal 的原生 SVG、基线 h/d、Windows 可部署性和窄宽公式可读性要求，不能据此锁定 M3 引擎。

- **A（MiTeX 0.2.4 → Typst 0.15 → SVG）最接近可用，建议作为后续首选调查对象，但本轮仍是 no-go。** 310 条预期有效输入全部返回 SVG；视觉样本的分数、根号、伸缩定界符、大型运算符、矩阵和多行对齐结构正确。可是只有 301/310 条有效输入得到 h/d，9 条自定义宏虽返回 SVG 却没有可用基线；这项按任务定义是一票否决信息。长公式在 480pt 与 160pt 下高度完全相同，没有自动折行。release 二进制净增约 44.8 MiB。
- **B（ReX）no-go。** 最活跃的 `KenyC/ReX` 固定 revision `aeccdba`：核心依赖和 parser probe 在 Rust 1.94.1 上通过；真正的 SVG feature 链依赖 Cairo/GLib 与外部 OpenType MATH 字体，在本机 Windows MSVC 上因找不到系统 `pkg-config`/Cairo/GLib 而无法构建，因此按规则停止，不伪造成功率或视觉结果。
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

A 的 9 个有效 h/d 缺失全部来自 `custom-macro`；另外 18 个畸形输入返回了无基线产物，不能把“返回 SVG”误当作可用渲染。C 的高 p95 主要受超大表达式尾部影响；这进一步说明它必须在可抢占 worker 内运行，不能驻留 UI 线程。

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

A 从 Typst frame baseline 推导 ascent/descent；C 从 KaTeX `.strut` 的 `height=h+d` 与 `vertical-align=-d` 解析。A 对 9 条有效输入拿不到 frame baseline，所以本轮不能用“总图片高度”猜测补齐。

## 选型建议与降级预案

本轮不批准锁定引擎。建议 M3 前增加一个范围受控的后续裁决，而不是现在进入产品实现：

1. 优先修 A 的自定义宏 h/d 缺失，并确认这是 MiTeX 输出、Typst frame 结构还是 adapter 提取错误；必须把 310/310 有效样本 h/d 作为门。
2. 用统一字号重跑视觉图库，并调查 Typst math 在何种输入结构下才能进行关系符优先的断行；当前裸 math block 没有发生断行。
3. 若 A 仍不满足 h/d，则不选 C 冒充 SVG。C 只保留为进程隔离的 HTML/离屏浏览器研究备选，其无折行与高尾延迟必须保留。
4. 按 R10 降级：M3 只保留块级数学检测与源码展示，或使用 `tui-math` 风格字符网格排版；横滚块可作为显式低质量 fallback，但不能称为自动折行。

由于三条路径全 no-go，这会改变 M3 的技术方案/排期；本报告只提出裁决与降级选项，**没有自行修改 DESIGN.md 或进入 M0/M3**。

## 测试与反向验证

主 probe `cargo test` 4 passed，`cargo clippy --all-targets -- -D warnings` 通过。测试覆盖语料数量/类别、安全上限、KaTeX h/d 提取和失败计数。

反向证据包括：最初缺少 MiTeX Typst scope 时 A probe 确实编译后运行失败；补入 upstream MiTeX Typst assets 后才产出 SVG。ReX core 的错误私有 API 调用先编译失败，改用公开 `rex::parser::parse` 后才通过。ReX SVG probe 仍稳定失败在 `cairo-sys-rs`/`glib-sys` 的系统依赖发现阶段，因此它是一扇会红的门，而不是只检查 Cargo.toml 存在。

## 遗留风险

1. A 的 9 个有效宏样本缺 h/d 尚未根因定位；这是阻塞项，不是统计噪声。
2. 两条可运行路径都未观察到窄宽自动折行；超长公式仍需横滚或降级。
3. 视觉抽查只有六类各一条，尚无像素差异阈值或跨字体平台基线。
4. ReX 只证明 core/parser 可编译；没有 Cairo/GLib 和 MATH font 的 Windows 可部署路径，性能与 h/d 均未知。
5. QuickJS 的 344.7 ms p95 与 in-process elapsed watchdog 不能提供抢占；生产若继续调查必须始终放在 Job worker 中。
6. Job spawn/assign 正常开销 p95 35.4 ms，不适合每公式启动一次；需要常驻、可轮换的受限 worker 池，同时保持 generation 取消。
7. 当前只测 Windows x86_64 MSVC 和一台机器；二进制/LTO、字体集合、低端 CPU 均会改变数值。

## 偏离申请

无。路径 B 因 SVG feature 链在目标环境无法构建，按任务规则如实停止；路径 C 无法生成原生独立 SVG，未把 HTML `foreignObject` 包装冒充达标。三条路径均未满足选择门，因此结论按真实证据标为 no-go。
