# 总体结论

**建议退回修改，暂时不能作为 M0 的工程基线。**

规范的大方向正确，核心教训多数确实来自三轮审核；但目前有三个实质性问题：

1. `[workspace.lints]` **完全没有生效**。
2. `cargo fmt --all -- --check` **会因 vendor 大面积失败**。
3. vendor 成员 CI 检查 **实际上不起作用**。

此外，`CONVENTIONS.md` 把若干通用偏好包装成“项目已付过代价的教训”，有夸大和张冠李戴。

---

# 1. 规范来源核对

| 条款 | 审核记录支持 | 结论 |
|---|---|---|
| §0 以规格/upstream 为准，不按复现步骤打补丁 | APC/DCS 错误复现、row 0 特判都有直接证据 | **成立，是最重要的规则** |
| §1 禁止启发式、硬编码测试样例 | `infer_removed_top`、`live_row=0`、裸字节嗅探均是直接案例 | **成立** |
| §1 “只在系统边界校验，内部完全信任类型系统” | 三轮 review 没有这条教训；当前 checkout 也没有所引用的根 `CLAUDE.md` | **来源不实且过度绝对** |
| §2 vendor 必须是 workspace member | 第二轮漏跑 upstream 测试、第三轮 180 tests 关闭回归 | **完全成立** |
| §2 “180 tests 是唯一证明” | 它们是重要回归证据，但不是数学意义的证明，也不是唯一证据 | **措辞夸大，应改为关键回归防线** |
| §2 vendor 只上报事件、policy 留在 bt-term | 第二、三轮明确核对 | **成立** |
| §2 永远只准改 `src/term/mod.rs` | 当前补丁恰好如此，但未来 upstream 变化可能要求调整 | **应改为默认边界，越界需专项审核，不宜永久绝对禁止** |
| §3.1 门测试从 VT 字节跑完整链路 | 首轮“六座孤岛”是直接教训 | **成立；应限定为 gate/integration tests，不是否定单元测试** |
| §3.2 任意 0/默认值都必须补对偶 | row 0 事故真实，但“任意参数”范围过宽 | **规律成立，规则过度**。应限定为会改变控制流、命名空间、边界或生命周期的参数 |
| §3.3 断言必须有失败机制 | 分包测试恒真有直接证据 | **成立** |
| §3.4 绝对禁止所有 placeholder/ignore | review 只记录“当前为 0”，没有 placeholder 导致事故 | **不符合“每条都付过代价”的声明**。产品代码禁止 `todo!`/`unimplemented!` 合理；`#[ignore]` 可要求 issue、原因和期限，而非绝对禁止 |
| §4.1 1218 行、五职责、约 400 行阈值 | 三份指定 review 未提 1218/400；当前文件已是 1314 行，测试从 679 行开始 | **代码味道真实，来源声明不实；400 是经验值，不是项目证据** |
| §4.2 命名常量 | 18px 硬编码、100,000 spike 配额有真实背景 | **部分成立** |
| §4.2 “裸字面量仅允许 0/1/2” | review 没有支持，而且协议 tag、颜色编码、测试尺寸天然需要字面量 | **错误，建议删除** |
| §4.3 newtype | review 有重复版本类型、生命周期双定义，但没有行/列/高度混用事故 | **方向合理，但“newtype 免费”不准确**；它只有零运行时成本，仍有 API/转换/序列化成本 |
| §4.4 panic=数据损失 | 产品风险成立，但不是三轮审核里的事故 | **可作为新产品原则，但不能声称来自既有教训** |
| §5 注释必须真实 | IL/DL 假注释是直接案例 | **核心成立**；“只写 why、不写 what”过于绝对，复杂算法和协议映射仍需要说明 what |
| §5 每个公共 API 都链接 DESIGN 小节 | review 没要求，很多工具 API也没有一一对应规格 | **过度**。只要求规格承载型 API 链接 DESIGN |
| §6 偏离申请 | 首轮三项未披露偏离是直接案例 | **成立** |
| §7 报告纪律 | 首轮误报三门通过、后续撤回是直接案例 | **成立** |
| §8 工具链固定、不得靠抑制换干净 | 固定原则合理，第三轮记录零抑制 | **原则成立；“绝不 allow”过度，应允许局部 `#[expect(..., reason = "...")]`** |

因此，[CONVENTIONS.md:4](D:/Developer/BetterTerminal/CONVENTIONS.md:4) 的“每一条都是已经付过代价的教训”不成立。建议把规则明确分成：

- 事故后形成的硬规则；
- M0 新采用的预防性原则；
- 非强制的代码审查提示。

## 已出现但规范没覆盖的坏味道

- [bt-term:548](D:/Developer/BetterTerminal/crates/bt-term/src/lib.rs:548) 忽略 `transition()` 的布尔返回值，非法状态转移可能静默通过。应返回 `Result`、加 `#[must_use]`，或至少 `debug_assert!`。
- 多个公开构造器直接 `assert!` 用户参数，例如 [bt-viewport:116](D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:116) 和 [bt-transcript:165](D:/Developer/BetterTerminal/crates/bt-transcript/src/lib.rs:165)。这与“panic 是数据丢失”冲突；应使用 `NonZero*` 或 `Result`。
- `unwrap_used` 没覆盖 `expect()`、`panic!()`、`assert!()`、`unreachable!()`；`indexing_slicing` 也不检查字符串切片。当前 panic 政策存在明显绕过路径。
- 约有 162 个公开类型/函数/方法声明，但只有 23 行 `///` 注释；规范要求公共 API 文档，却没有 `missing_docs` lint。
- [DualPlaneSession](D:/Developer/BetterTerminal/crates/bt-term/src/lib.rs:296) 明写是 “M-1 protocol harness”，却已经是公开产品 API。M0 应决定它是正式 actor 核心还是 test-support，不能默认让 spike harness 演化成产品接口。
- Edition 2024 的虚拟 workspace 仍使用 [resolver = "2"](D:/Developer/BetterTerminal/Cargo.toml:2)。建议改为 resolver 3，使依赖解析考虑 `rust-version`；这是 Edition 2024 的推荐行为。[Rust Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/cargo-resolver.html)

---

# 2. Lint 是否过度

## `unwrap_used`

**产品代码 deny 合理，测试代码 deny 不合理。**

静态统计第一方代码有：

- 59 次 `.unwrap()`：约 6 次产品代码、53 次测试代码。
- 5 次 `.expect()`：其中约 2 次产品代码。

因此当前规则真正启用后，测试会出现大量无收益改写。测试中的 unwrap 通常表达“fixture/setup 不成立就立即失败”，是合理语义。

建议：

```toml
# clippy.toml
allow-unwrap-in-tests = true
allow-indexing-slicing-in-tests = true
```

Clippy 官方支持这两个配置，默认都是 `false`。[Clippy lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html)

产品代码应同时考虑 `expect_used`，否则把 `unwrap()` 换成 `expect()` 就绕过了政策。

## `indexing_slicing`

**全局 deny 是错的。**

现有产品代码至少有约 20 个天然索引点，包括：

- Fenwick 树的五处核心操作：[bt-viewport:44](D:/Developer/BetterTerminal/crates/bt-viewport/src/lib.rs:44)。
- alacritty 的 typed grid 访问。
- 已经由长度条件保护的命令、resize plan、input plan 索引。

Fenwick/网格代码把所有索引改成 `.get().ok_or(...)`，往往只会制造噪音和额外分支，并不会改善已由算法不变量保证的安全性。

正确做法是：

- 不可信字节、用户下标、公共 API 输入：必须 checked access。
- 算法内部且不变量清晰：允许索引，使用局部 `#[expect(clippy::indexing_slicing, reason = "...")]`。
- 初期设为 advisory warn，不应全局 deny。
- 测试中允许索引。

## 约 400 行阈值

**拍脑袋，但作为提示可以保留。**

当前 `bt-term/lib.rs` 是 1314 行，其中生产部分约 678 行、测试约 636 行。真正问题是职责耦合，不是总行数。建议删除具体 400，改成：

> 当一个模块同时拥有多个独立生命周期/状态机、修改一个规格概念需要跨越多个不相关区域，或测试夹具超过实现可读性时，按 DESIGN 概念边界拆分。

## 四个点名 lint

- `missing_debug_implementations`：第一方库设 warn 可以；诊断价值尚可，但 wrapper 的内部类型不支持 Debug 时会有噪音。当前至少 `TerminalAdapter`、`DualPlaneSession` 会命中。不要应用到 vendor。
- `unreachable_pub`：收益高、噪音低，模块拆分后很有价值；可以在清零后升 deny。
- `needless_pass_by_value`：pedantic，可能要求改变所有权和公开 API，收益偏低。建议只做 advisory，不进硬门。
- `doc_markdown`：只是文档排版，不会检查缺文档。当前优先级远低于 `missing_docs`，不应阻塞 M0。

另外，`print_stdout` 不应全局启用：`bt-replay` 的 [println](D:/Developer/BetterTerminal/crates/bt-corpus/src/bin/bt-replay.rs:29) 是 CLI 的正常输出，不是调试残留。

---

# 3. 可执行性实测

## Clippy

原样运行了：

```text
cargo clippy --workspace --all-targets -- -D warnings
```

但固定的 1.85 工具链未安装，rustup 在只读环境中无法写入用户目录；改用已安装的 1.94.1 后，Cargo 又无法创建 `target/debug/.cargo-lock`。因此这次无法诚实给出固定 1.85 的完整编译诊断数量。

不过可以确定更严重的问题：**新增 lint 当前一个也没生效。**

6 个第一方 manifest 都缺：

```toml
[lints]
workspace = true
```

例如 [bt-term/Cargo.toml](D:/Developer/BetterTerminal/crates/bt-term/Cargo.toml:1)。`cargo metadata --no-deps` 显示所有 package 的 `lints` 都为空。Cargo 明确要求成员显式继承 workspace lints。[Cargo workspace lints](https://doc.rust-lang.org/stable/cargo/reference/workspaces.html#the-lints-table)

因此：

- 当前 Clippy 即使通过，也只是默认 lint 通过。
- 第三轮 signoff 记录的 0 warning 与这一点一致。
- 修复继承后，至少会出现 59 个 unwrap 候选、约 36 个索引候选、2 个 missing-debug 候选和 1 个合法 stdout。
- **不要给 vendor 添加 `[lints] workspace=true`**；vendor 含 upstream unsafe、unwrap、索引，实现策略不应受第一方 restriction lints 控制。

## Rustfmt

使用本机 1.94.1 实测：

```text
cargo fmt --all -- --check
```

结果：**失败，303 个 diff hunk，涉及 22 个文件，全部在 vendor。**

第一方包单独检查：

```text
cargo fmt \
  --package bt-term --package bt-transcript --package bt-doc \
  --package bt-viewport --package bt-detect --package bt-corpus \
  -- --check
```

结果：**通过。**

所以问题不是第一方代码格式，而是根 `rustfmt.toml` 把 Edition 2024 默认风格强加给按 upstream 风格 vendored 的源码。CI 应只 fmt 第一方包，不应把整个 vendor 重排。

## 分阶段建议

当前 CI 使用 `-D warnings`，所以 manifest 里的 `"warn"` 最终也会被升成错误；现在所谓“先 warn”实际上仍是 deny。

建议：

- 立即 gate：默认 warnings、第一方 `unsafe_code`、`todo`、`unimplemented`、`dbg_macro`。
- 修掉 6 个产品 unwrap 后：产品代码启用 `unwrap_used`，并考虑 `expect_used`；测试显式豁免。
- 暂时 advisory：`indexing_slicing`、`missing_debug_implementations`、`unreachable_pub`、`needless_pass_by_value`、`doc_markdown`。
- 清零后再逐项晋升 gate；不要把 advisory lint 放进会被 `-D warnings` 升级的路径。

---

# 4. CI 有效性

## Vendor 检查完全无效

[ci.yml:27](D:/Developer/BetterTerminal/.github/workflows/ci.yml:27) 只搜索字符串。即使删除 members 中的 vendor，仍会匹配 [Cargo.toml:53](D:/Developer/BetterTerminal/Cargo.toml:53) 的 patch 路径。

我在内存中删除 member 行后重跑同一正则，仍有 1 个匹配。因此它不只是“可能被注释骗过”，而是**当前必然会被 patch 条目骗过**。

建议使用：

```powershell
$m = cargo metadata --format-version 1 --no-deps --locked | ConvertFrom-Json
$manifest = [IO.Path]::GetFullPath(
    "vendor/alacritty_terminal/Cargo.toml",
    $PWD.Path
)

$vendor = $m.packages |
    Where-Object {
        $_.name -eq "alacritty_terminal" -and
        [IO.Path]::GetFullPath($_.manifest_path) -eq $manifest
    }

if (-not $vendor -or $vendor.id -notin $m.workspace_members) {
    throw "vendored alacritty_terminal is not a workspace member"
}
```

`cargo metadata` 的 `workspace_members` 就是为这种机器检查准备的。[Cargo metadata](https://doc.rust-lang.org/stable/cargo/commands/cargo-metadata.html)

同时应检查：

- version 必须是 `0.26.0`；
- manifest 路径必须指向 `vendor/alacritty_terminal`；
- workspace dependency 改为 `=0.26.0`，避免未来 `0.26.x` 解析绕开本地 0.26.0 patch。

## Placeholder grep

当前第一方命中为 0，但规则并不可靠：

- 会误伤注释或字符串里的 `"todo!("`、`"#[ignore]"`。
- 漏掉 `todo !()`、`todo! {}`、`#[ ignore ]`、`#[ignore = "..."]`、`cfg_attr(..., ignore)`。
- 当前 Clippy lint 没继承，所以所谓“双重把关”实际只有 grep。

建议：

- `todo`/`unimplemented` 交给真正生效的 Clippy。
- ignored test 用 `cargo test --workspace -- --ignored --list` 检查是否存在，而不是扫描源码。
- 如果保留 grep，明确它只是辅助检查，并限定第一方源码。

## 工具链问题

[ci.yml:15](D:/Developer/BetterTerminal/.github/workflows/ci.yml:15) 安装的是最新 stable，而 [rust-toolchain.toml:5](D:/Developer/BetterTerminal/rust-toolchain.toml:5) 指向 1.85，两者语义冲突且重复。`dtolnay/rust-toolchain@stable` 明确选择 latest stable。[dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain#example-workflow)

建议统一为完整版本，例如 `1.85.1`，并让 workflow 与 toolchain 文件使用同一值。

## 还应补的关键项

- 所有 Cargo CI 命令增加 `--locked`。
- fmt 仅检查第一方包。
- metadata 检查 vendor 的成员身份、路径和精确版本。
- 增加 `permissions: contents: read` 和合理的 `timeout-minutes`。
- 修复 lint 继承后再保留 `clippy -D warnings`。
- 当前 `cargo test --workspace` 已覆盖 upstream 180 tests，不需要再复制一个相同 test job。

---

# 5. M0 债务优先级

| 债务 | 规范覆盖 | 建议 |
|---|---|---|
| vendor member + resize 版本护栏 | §2 覆盖，但 CI 实现无效，版本也非精确约束 | **M0 功能开发前必须修** |
| `bt-doc:331` 死赋值 | 未覆盖 | 第一笔 M0 hygiene commit 删除 |
| G1/G2/G3 gate 仍用 row 0 | §3.2 覆盖 | 第一笔 M0 commit 改；至少 G2/G3 用非 0，对 G1 补非 0 对偶 |
| `SPIKE_CELL_HEIGHT_SUBPIXELS` | §4.2 覆盖 | 可在 M0 内按真实字体度量替换，**M0 结束前清零** |
| `DEFAULT_FROZEN_QUOTA=100_000` | 实际也是 signoff 指出的 spike 参数，但没有 `SPIKE_` 前缀 | 规范覆盖不完整；M0 内测量后变配置/有依据默认值 |
| `bt-term/lib.rs` 1314 行 | §4.1 覆盖，但 400 阈值不可靠 | 不阻塞 M0 开始；在 M0 确认正式 actor/API 边界后拆，避免为 spike harness 做一次性重构 |
| `transition()` 返回值被忽略 | 未覆盖 | 与死赋值一起优先清理 |
| panic/lint 债务 | §4.4 覆盖不完整 | 先处理产品代码 unwrap/assert，再启用硬 lint |

建议的进入顺序：

1. **先修工程基线**：lint 继承、第一方/vendor 分治、fmt、metadata vendor guard、精确工具链和 alacritty 版本、`--locked`。
2. **做小型 hygiene commit**：死赋值、非 row-0 gate tests、ignored transition result。
3. **再开始 M0 产品功能**。
4. `bt-term` 拆分和 `SPIKE_` 参数实测可以随 M0 做，但不得拖到 M0 验收之后。

本次全程只读，没有修改任何文件。