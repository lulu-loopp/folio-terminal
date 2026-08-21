# Folio 工程约定

> 每个会话开工前读一遍。`docs/DESIGN.md` 是**做什么**的权威，这份是**怎么做**的权威。
>
> **规则分三级，来源如实标注**——因为本文件自己要求"注释必须说真话"：
>
> - **【事故】** 这个项目真的踩过、在 `docs/reviews/` 里有案可查。违反过一次就付过一次返工。
> - **【预防】** M0 新立的原则，**没有**对应事故。它们是判断，不是血泪，可以讨论。
> - **【提示】** 审查时的参考，不是硬门。

## 零、最高原则

### 【事故】以规格和 upstream 契约为准推导，不要照复现步骤做局部修补

排第一，因为它造成过两次返工，一次比一次严重：

- 审核方给的一条复现步骤**本身就是错的**（称"APC 载荷里的 `ESC[3J` 会清空历史"，但按 DEC STD 070，载荷里的 ESC 本就终止 APC，那个 ED3 是真命令）。照它修 → 改坏 VT 状态机 → **未终止的 DCS/APC 永久卡死终端，连 RIS 都救不回**，比原 bug 严重得多。
- 另一次：复现步骤只用 `row 0`，锚点 rebase 就只修了 row 0，其他行静默漂移。

**正确做法**：回到 DESIGN.md / DEC STD 070 / upstream 源码推导出**正确行为**，修那个。修完复现步骤仍不过 → **说出来**，很可能是步骤错了。

第三轮 Codex 正是这么做的（面对那条错步骤，选择回到标准推导，让 OSC/APC 归于同一模型），这是它能通过的主要原因。

## 一、通用解

### 【事故】不要启发式

真实案例：曾用"前后网格切片相等"猜 resize 移除了哪些行——重复行（进度条、分隔线）就歧义。正确解是让 alacritty 直接上报。

**当"猜"能通过测试时，它仍然是错的**：它只是恰好没被你的样本证伪。

### 【预防】校验放在系统边界

PTY 字节流、用户输入、外部 API、配置文件是边界，要校验；内部模块之间靠类型系统。

来源是项目所有者的全局偏好（不在本仓库，Codex 看不到，所以写在这里），**不是审核事故**。**但当前代码与它冲突**：`bt-viewport:116`、`bt-transcript:165` 等公开构造器直接 `assert!` 调用方参数——那既不是边界，也和第四节的 panic 原则打架。M0 决定：改 `NonZero*` / `Result`，还是承认它们是边界。

## 二、vendor

### 【事故】`vendor/alacritty_terminal` 必须留在 `workspace.members`

**CI 用 `cargo metadata` 检查**（不是 grep——manifest 里这个路径出现两次，members 和 `[patch.crates-io]`，grep 会被 patch 行骗过，把 member 删了照样过）。

理由：vendor 的 180 个 upstream 测试是**我们补丁没破坏 VT 语义的关键回归防线**。它不在 members 里时 `cargo test --workspace` 根本不跑它——一个真实回归就是这样溜过两轮审核的（`tests::intermediate_reset_on_dcs_exit` 挂了没人知道）。

同时 CI 检查：路径指向 `vendor/`、版本精确等于 `0.26.0`（依赖也写 `=0.26.0`——否则 crates.io 上的 0.26.x 会 out-resolve 掉本地 patch，静默把捕获钩子从构建里拿掉）。

### 【事故】policy 不进 vendor，**也不进适配层**

什么该冻结、什么该丢弃是 `bt-term` 的判断；vendor 只如实上报发生了什么。三轮审核每轮都逐 hunk 核对过这条。

**但射程短了一格**：`TerminalAdapter::decorations_allowed()` / `mark_resize_quiescent()` 是**装饰调度策略**，却住在本该只做 alacritty 适配的 `TerminalAdapter` 里。policy 没进 vendor，但进了 **vendor 的门面**——R11' 想隔离的那层照样被污染了。

规则：**vendor 适配模块（`adapter.rs` / `cell_capture.rs`）不得依赖 `bt-doc` / `bt-detect` / `bt-viewport`**（CI 可 grep 它们的 `use`）。判据：适配层只回答"alacritty 发生了什么"，不回答"我们要拿它怎么办"。

### 【事故】upstream 的内存布局不得跨过适配层

冻结的转录里**不得**出现 upstream 的裸 bit / 裸 tag。现状违反：

- `bt-transcript:33` `flags: u16` 直接存 `cell.flags.bits()` ——**硬编码了 alacritty 的 bit 布局**
- `bt-transcript:34-35` 颜色是 `encode_color` 手打的 `0x01/0x02/0x03` tag，**全 workspace 没有解码器、没有测试**

§2 的"升级前 diff `shrink_lines`"**盖不到这里**：upstream 重排 `Flags` 的位，测试全绿，而历史的含义静默改变。

规则：转录层的每个编码类型必须是 **bt-transcript 自己定义**的（自己的 enum / bitflags），翻译和显式的位映射留在 `bt-term`。这样 upstream 改布局是**编译失败**而不是静默漂移。

**"只写数据"是可以 grep 的坏味道：任何 `encode_*` 没有配套的 `decode_*` + round-trip 测试，就是。**

### 【提示】补丁默认只在 `src/term/mod.rs`

当前补丁面：单文件 15 hunk +109/-4。越界不是禁止，但**需要专项说明**（upstream 结构变化可能逼你换地方）。每次改 vendor 更新报告里的补丁面统计。

### 【事故】升级 alacritty 前强制 diff `grid/resize.rs::shrink_lines`

`Term::resize` 镜像了它的公式。224 个测试能兜住行为，**兜不住上游公式静默漂移**。

## 三、测试

### 【事故】门测试从字节跑完整链路

首次交付把"零件级单测"误报成"三门全过"：24 个测试全是手工构造入参调一个函数，六个 crate **从未合并运行过**。规格的价值在协议的**接缝**上，而接缝正是没被测的部分。

**限于 gate / 集成测试**（G1/G2/G3 必须从"喂 VT 字节"到"装饰记录状态"，中间不许手工构造）。**这不否定单元测试**——零件级测试有它的价值，只是不能冒充门。

### 【事故】默认值会掩盖 bug —— 这是一个**族**，不是一个案例

所有锚点测试都用 `live_anchor(0, _)`，而 **row 0 恰好是唯一被旧代码覆盖的行**。于是"锚点从不 rebase"这个 BLOCKER 躲过了两轮审核和全部测试。

`live_anchor(0,_)` 被抓到，是因为它是**参数**。同族的另外三个躲过了三轮审核，因为它们是**字段默认值**：

| 恒定的默认值 | 掩盖了什么 |
|---|---|
| `live_anchor()` 的 `GridGeneration(1)` | 恒等于 session 初始 generation → 锚点 generation 永远"匹配"。配合"从不比较"，**generation 逻辑整个写反也不会红** |
| `LayoutKey` 的 `dpi/font/theme = 1000/1/1` | 4 个字段只有 1 个被测过。**把 LayoutKey 换成只剩 `width_cells` 的 struct，全部测试照样绿** |
| `CapturedRow::plain()` 的默认样式 | bt-term 所有测试只比 `.text` → `alacritty Cell → CapturedCell` 的翻译**从未从 VT 字节验证过**，样式是只写数据 |

规则：

1. **参数**：当它会改变控制流、命名空间、边界或生命周期时，取默认值/首个值的测试必须补非默认值的对偶用例。
2. **多字段的键**：每个字段都必须有一个能让它**单独失效**的测试。
3. **只写字段 = 死规格**：规格要求携带、但全 workspace 从未被读过的字段（现状：锚点 `generation`、`HistoryEntry.source`），**要么接线，要么从 DESIGN 里删**。每次 review 人工过一遍"新增字段有没有消费者"——`dead_code` 盖不到 pub 字段。

写测试时反问：**这个取值是不是恰好走了最简单的那条路径？**

### 【事故】断言要能失败——**射程覆盖构建配置本身**

分包不变性测试曾是**恒真**的：`feed()` 内部逐字节 `advance`，分包边界在构造上不可观测，那个断言**永远不可能红**。

**同一个 bug 已经出现三次，每次换个马甲**：

1. 恒真的分包断言
2. vendor 里那句"IL/DL 绝不触发"的假注释（测试恰好绕开了那个 bug）
3. **本规范的首版**：写了一大堆 lint，但 6 个 crate 都没写 `[lints] workspace = true`，**一条都没生效**，`clippy -D warnings` 是个永远绿的空门

所以这条规则的射程不止测试：**任何声称在挡住什么的东西，都必须先证明它会红。**

- CI 有一个**反向测试**：往 crate 里种一个 `todo!()`，clippy 必须红。护栏加进来的那天就要证明它会响。
- 写完断言问一句：**什么情况下它会红？** 答不上来就是假测试。
- 写完 CI job 问一句：**它挡过什么？** 答不上来就是空门。

### 【事故】驱动真实子进程的测试，超时按"孩子静默多久"算，不按墙钟总额

（2026-08-20，分支 `test-env-immunity`；技术细节见 `docs/DESIGN.md` §7.1.6c-3b 尾部。）

`bt-app` 的 `real_powershell_input_reaches_a_viewport_owned_frame` 与 `bt-pty` 的 `sidecar_resize_keeps_history_navigation_on_a_clean_prompt_line` 反复"随机"挂，一天里两个 agent 各自撞到。它们的等待全是**总额**：5s 等一行、10s 从头管到尾。这种超时量的不是被测的东西，是**这台机器当时有多忙**——闲时 32 次 0 失败，把二十四个自旋进程压满这台二十四线程的机器，同一支脚本立刻 8/8、13/16 全塌。改好之后把改前改后的两个测试二进制并排放进同一段负载里交替跑：**旧臂 7/8 挂，新臂 0/8**，新臂最慢一次熬到 48 秒才绿——那 48 秒正是"同样的字节，只是间隔更远"长的样子，也正是任何一个总额都买不到的东西。

规则：

1. **超时问"孩子停了没有"，不问"过去了多久"**：每读到一个字节预算就重置，另设一个远高于任何诚实开销的绝对天花板兜住"一直说话但永远说不到"。饿着的机器交付同样的字节，只是间隔更远，所以这个判据对负载免疫；真挂的孩子仍然在原来的秒数里红。
2. **不许把数字调大**。那买到的绿是把每一次真挂的代价一起乘上去，等于把不稳定藏起来。
3. **超时要报它等到了什么**：等了多久、其中静默多久、读了多少字节、握手到没到、屏幕长什么样。只说"失败"的超时会把下一个人送去查错的方向（这次就送错了：真凶是负载，被告是一条 env）。
4. **`sleep` 不是同步原语**。"写下去、等 100ms、假设孩子已经在跑了"和"让孩子睡 2 秒、趁它睡完这段活"都是钟：负载一压就先后颠倒，而且**改变的是被测到的东西**，不只是耗时。改成孩子自己说话（打一个标记出来）或探针自己开闩（等一个本进程故意还没建的文件）。

### 【事故】A/B 必须在同一段时间里交替，先跑完一组再跑另一组等于把负载当结论

同上一条的同一天。"带 `export` 跑全量会挂、单独不带 `export` 重跑就绿"这句观察里，`export` 与"全量 vs 单跑"两个变量是**捆在一起**的，而后者意味着 1856 个测试用 24 条线程一起抢机器。交替 A/B（两臂在同一窗口内轮流、每 rep 互换先后手）当场判 `export` 无罪：闲时两臂各 16 次 0 失败、时长不可分辨；加载后**两臂一起塌**。

顺带的裁决：**`export PSModulePath=...` 从测试纪律里退役**。它从来救的不是测试，是 `crates/bt-pty/build.rs` 里那支 Windows PowerShell 5.1 的模块解析；那条路已改成由 build.rs 自己声明模块路径，谁在外面导出什么都不再有影响。留着这行没有坏处也没有作用，但一条"必须记得导出否则构建神秘失败"的纪律，正是这次误判的全部燃料。

### 【预防】产品代码不留占位符

`todo!()` / `unimplemented!()` 由 clippy deny（当前为 0，**没有**因它出过事故——这是预防，不是教训）。做不完就如实写 no-go。

`#[ignore]` 由 CI 检查（问测试框架 `--ignored --list`，不是 grep 源码）。要 ignore 就写明理由和期限。

## 四、代码结构

### 【提示】按概念边界拆模块，不是按行数

`bt-term/src/lib.rs` 1314 行（实现 ~678 + 测试 ~636），只有一个 `mod tests` 边界，里面挤着五个职责（适配层 / 捕获钩子 / 冻结管线 / 生命周期事件表 / session actor）。

**没有行数阈值**（之前写的"400 行"是我拍的，删掉了）。判据是：

> 当一个模块同时拥有多个独立生命周期/状态机、改一个规格概念要跨越多个不相关区域、或测试夹具压过实现可读性时 → 按 DESIGN.md 的**概念边界**拆。

拆的依据是规格里的概念——这样改规格时才知道该动哪个文件。

**M0 注意**：先确认正式的 actor / API 边界再拆，别为 spike harness 做一次性重构。

### 【事故】规格钉死的值必须是命名常量

`PARSE_QUANTUM` / `SUBPIXELS_PER_PX` / `DEFAULT_STAGING_QUOTA` 是对的：看名字就知道对应 DESIGN.md 哪一条。文档注释写明出处（`/// DESIGN.md §1.3`）。

**没有"裸字面量只准 0/1/2"这条**（之前写了，是错的——协议 tag、颜色编码、测试尺寸天然需要字面量）。判据是**这个值是否承载规格语义**。

**单位常量住在最底层 crate**：`SUBPIXELS_PER_PX` 现在住在 `bt-viewport`，而 `bt-detect:169` 不依赖它，只能写裸 `1024`——**同一个量两个定义**，而测试却用命名常量断言，看起来一切正常。同一个量出现两个定义 = 回归。

### 【事故】`SPIKE_` 前缀是欠条

`SPIKE_CELL_HEIGHT_SUBPIXELS` 标记"这个值是 spike 里拍的，M0 要按真实数据定"。**M0 结束前 `SPIKE_` 必须清零**。

**`DEFAULT_FROZEN_QUOTA = 100_000` 也是 spike 参数却没有前缀**——M0 一并处理：要么按实测内存定，要么变配置项。

### 【预防】类型承载语义

行号、列号、grapheme offset、subpixel 高度不是同一种 usize。newtype **运行时零成本**（但有 API / 转换 / 序列化成本，不是"免费"）。

审核记录里有"版本类型被复制成两套""`SourceLifecycle` 双定义"，但**没有**行列混用的事故——所以这是预防。

### 【预防】panic = 数据丢失

终端里 panic 会带走用户的 scrollback。这是产品风险判断，**不是既有事故**。

- 库用 `thiserror`；二进制用 `anyhow`。
- `unreachable!()` 只在类型系统证明不可达时用，写明为什么。
- 相关 lint 见 §8 的晋升阶梯——**现在还没开**，因为产品代码里还有约 6 处 `unwrap`。

## 五、注释

### 【事故】注释必须说真话，或者不写

vendor 里曾有 `// IL/DL … deliberately never emit`——**与实际行为完全相反**，而现有测试恰好用了个参数绕开那个 bug。假注释比没有注释危险：它让后来的人不去查。

改行为时同步改注释。发现注释撒谎 → 当场修。

### 【提示】优先写为什么

约束、取舍、规格出处优先于"是什么"。但**复杂算法和协议映射仍然需要说明"是什么"**——Fenwick 树的索引推导、VT 事件到我们事件的映射，读者看代码看不出来。

**规格承载型的公共 API** 链到 DESIGN.md 对应小节。工具型 API 不强求（之前写"每个公共 API 都要链"，过度了）。

现状：约 162 个公开声明 vs 23 行 `///`。`missing_docs` 见 §8 阶梯。

## 六、【事故】偏离申请

规格与实现冲突时**不要自行偏离**。报告里开一节：

```markdown
## 偏离申请
- 规格条款：DESIGN.md §X.Y "……"
- 实际做法：……
- 理由：……
- 替代方案与取舍：……
```

代码按原规格实现或留 TODO 注释（**不是 `todo!()`**），等裁决。

曾发生过：报告写"无待裁决的语义偏离"，但代码里逐字节解析（违反 §1.3 的 256 KiB quantum）、`redetect` 不重建装饰意图（§3.3 明文禁止）、`SourceLifecycle` 有两个矛盾定义。**代码偏离了但报告说没偏离**——这比偏离本身更严重，它让审核失去意义。

## 七、【事故】报告纪律

- 结论必须有据：go / no-go / go-with-caveats，每条都要有可复现的实测数据。
- **不许把"函数写了且自测通过"表述为"门通过"**。
- **声明"已修复"必须指出哪个测试在修复前会红**。答不上来的"已修复"按未修复处理——这是"断言要能失败"用在流程上。
  > 反例：某次 signoff 建议"删掉 `bt-doc:331` 的死赋值"，但删完 `Tombstoned` 就成了首轮骂过的"幽灵变体"（从未被构造的枚举分支）——一个 MINOR 换成一个已判定为 MAJOR 的模式。**说明那条建议没有被任何测试驱动。**
- **放宽性能或正确性门之前必须先做同机同条件的 HEAD / 工作树对照**，把命令与原始数字写进报告并证明退化不存在；没有这组证据就不许把失败归因于 flaky，也不许抬预算，只能保留门并如实报告退化。
- **no-go 是有价值的产出**。如实的 no-go 会被接受；虚假的 go 会被打回重做，还要连报告一起改。
- 保留完整审计记录（误报、返工、blocker 都在案），不要事后美化。

## 八、工具链与 lint 阶梯

`rust-toolchain.toml` **钉死完整版本**（`1.85.1`，不是"1.85"那个会浮动的系列）。CI 不指定 channel，以该文件为唯一来源——clippy 的 lint 集随版本变，不钉死的话"clippy 干净"只是关于某台机器某一天的陈述。

### 现在就 gate 的（`Cargo.toml` 的 `[workspace.lints]`，全树今天真的过）

`unsafe_code` · `clippy::todo` · `clippy::unimplemented` · `clippy::dbg_macro`

⚠️ **成员 crate 必须写 `[lints] workspace = true`**——cargo 不自动继承。缺这行整张表就是摆设（这正是本规范首版的真实状态：写了一大堆 lint，一条都没生效）。**vendor 不继承**：它是 upstream 的代码，按 upstream 的标准。

⚠️ CI 跑 `-D warnings`，所以 **manifest 里没有"advisory"这回事**——写 `warn` 等于写 `deny`。要观察就先别放进 manifest。

### 晋升阶梯（欠条，M0 逐项清）

| lint | 挡在前面的债 | 备注 |
|---|---|---|
| `clippy::unwrap_used` + `expect_used` | 产品代码约 6 处 `unwrap`、2 处 `expect` | `clippy.toml` 已配 `allow-*-in-tests`——测试里的 unwrap 是合理语义（fixture 不成立就立即失败），53 处不必改写 |
| `clippy::indexing_slicing` | 产品代码约 20 处天然索引（Fenwick 树、typed grid） | **不要全局 deny**：算法不变量保证的索引改成 `.get().ok_or()` 只是噪音。规则应是：不可信字节 / 用户下标 / 公共 API 入参必须 checked；算法内部用局部 `#[expect(..., reason = "...")]` |
| `missing_docs` | 162 个公开声明 vs 23 行 `///` | 比 `doc_markdown` 有价值得多（后者只管排版，不管缺不缺） |
| `unreachable_pub` | 未统计 | 收益高噪音低，模块拆分后尤其有价值 |
| `missing_debug_implementations` | `TerminalAdapter`、`DualPlaneSession` 会命中 | 只用于第一方 |

**不采用**：`print_stdout`（`bt-replay` 的 `println` 是 CLI 的正常输出，不是调试残留）、`needless_pass_by_value`（pedantic，会逼着改所有权和公开 API，收益偏低）。

### 【提示】`#[allow]` / `#[expect]`

不要用来绕过本文件的规则。局部豁免用 **`#[expect(lint, reason = "…")]`**（比 `allow` 好：豁免不再需要时会自己报错），理由写清楚。

## 九、M0 开工前必须先还的债

按两轮工程审核（`docs/reviews/claude-review-conventions.md`）：

1. **工程基线**（本次已做）：lint 继承（6 个 crate 的 `[lints] workspace = true`）、CI 的反向测试、第一方/vendor 分治、fmt 只查第一方、`cargo metadata` 版的 vendor 守卫、精确工具链与 `=0.26.0`、`--locked`、resolver 3、`clippy.toml` 的测试豁免。
2. **护栏得先证明是活的**：本地跑一次 `cargo clippy --workspace --all-targets -- -D warnings`，确认它**真的跑起来过**（审核方本地 rustup 拉 clippy 组件失败，怀疑它在这个 workspace 上从未成功跑过）。
3. **小型 hygiene commit**：
   - G2/G3 的 `live_anchor(0, _)` 至少各改一个非 0 行
   - `bt-term:548` 忽略了 `transition()` 的返回值（非法状态转移会静默通过——应返回 `Result` 或加 `#[must_use]`）
   - `bt-transcript:299` 的**撒谎注释**（把 staging 锚点作废归因于 generation bump，但根本没有代码检查 anchor generation；真正作废靠的是 `delete_transaction` 的显式 flag）
   - **`bt-doc:331` 的死赋值不要直接删**（signoff 的建议）——删完 `Tombstoned` 就成了幽灵变体。**先决定 tombstone 是不是 `HistoryEntry` 的状态**：代码的真实模型是「entry 在 = Frozen；entry 没了且 id 在 `tombstones` = Tombstoned」。按这个模型该删的是整个 `HistoryEntry.source` 字段。
4. **决定 `DualPlaneSession` 的身份**：它自称 "M-1 protocol harness"，却已经是公开产品 API。M0 要么把它扶正为正式 actor 核心，要么降为 test-support——**不要让 spike harness 默认演化成产品接口**。
5. 之后才开始 M0 的产品功能。

### M0 中顺手做

- **`bt-term` 按概念边界拆**（677 实现行 / 5 职责）：`cell_capture.rs`（Cell→CapturedCell 翻译 + 位映射 + 解码器）、`adapter.rs`（vendor 门面，**禁止依赖 bt-doc/bt-detect/bt-viewport**）、`lifecycle.rs`（§3.1 事件表——**规格里是一张表，代码里就该是一张表**，现在埋在一个 44 行 match 里）、`scheduling.rs`（§1.3 容量契约 + resize epoch）、`session.rs`。`bt-doc` 拆 `anchor.rs`/`versions.rs`/`document.rs`。
- **样式管线的端到端断言**：从 VT 字节（`\x1b[1;31m` + OSC 8 + 宽字符）一路断到 `frozen()[0].styles`——现在这段翻译从字节出发从未被验证过。
- **`encode_color` / `flags` 换成 bt-transcript 自己的类型**（见 §2 的"内存布局不得跨过适配层"）。
- **`SPIKE_CELL_HEIGHT_SUBPIXELS` 清零**：M0 接真字体度量后由 cell metrics **注入为构造参数**（`ViewportProjection::new` 已经是注入式的，`DualPlaneSession` 跟上）。`DEFAULT_FROZEN_QUOTA = 100_000` 同样是 spike 参数却没前缀，一并处理。
- **`bt-record.rs:126` 的字节嗅探**：用 4 字节滑窗在原始流里找 `\x1b[6n`——**产品代码里被强制删掉的那种启发式，录制工具里还在**。而语料保真度是所有回放测试的地基。
- **门测试搬到 `tests/`**：G1/G2/G3 是跨 crate 契约门，DESIGN §9 说"任一不过 → 砍功能"——**一个不能按名字单独跑的门是不能执行的门**。搬到 `tests/` 还能**物理强制**"只准用公开 API"（`transcript_mut()` 降 `pub(crate)` 后，绕路直接编译不过）。
- **门测试一场景一函数**：`g1_staging_limits_split_and_live_controls_are_byte_driven` 一个函数塞了 6 个独立场景——失败时不知道哪条规格破了。

### 明确不做（避免过度工程）

- **不要为 `indexing_slicing` 改写 Fenwick 树**：教科书算法的裸索引是可读性最优解，套 `.get().ok_or()` 只是噪音 + 永不发生的错误路径（正好违反 CLAUDE.md）。**这是配置该让步的地方**，给模块级 `#[expect]` + 理由。
- **不要建 `bt-testkit` 共享测试辅助 crate**：现在只有 3 个各 ~10 行的 helper。而且**共享 fixture 正是"默认值掩盖 bug"的温床**（§3.2 整族问题的来源）。等第 4 次复制再说。
- **不要补全 serde**：15 个类型挂着 derive，**全 workspace 没有任何序列化调用**（corpus 是手写二进制格式）。而且在位布局修好前这个格式本来就不稳定——序列化它等于把 upstream 的内存布局写进磁盘。**该删依赖，不是补类型。**
- **不要给 `TranscriptStore`/`HistoryDocument` 抽 trait**：各只有一个实现，DESIGN 也没要求多实现。
- **不要现在为锚点 generation 造校验机制**：先决定它要不要。**在没有消费者时先造校验框架，只会得到第二个只写机制。**
- **不要批量给 163 个 pub 项补 `///`**：只给跨 crate 契约补（`AdapterEvent`、`ContentAnchor`、§3.1 的判定函数、配额语义）。给 getter 写 `///` 是噪音，还稀释真文档。
- **不要重开 `Term::resize` 镜像 `shrink_lines` 的议题**：signoff 已裁决为接受的取舍（让 grid 上报会扩大 vendor 面，方向相反）。护栏是"升级前强制 diff"。
