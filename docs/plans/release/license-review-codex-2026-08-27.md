# 许可与合规四条推荐——Codex 只读审阅(2026-08-27)

审阅对象:发布前许可四条推荐(协议维持双许可/版权行/贡献条款/商标说明)与二进制 zip 合规专项(a)–(e)。审阅由 Codex(gpt-5,high)在只读模式完成,未改任何文件。落地单据见 `legal-notes-2026-08-27.md` 与门 1 整改提交。

只读审阅完成，未修改任何文件；最终 `git status --short` 为空。以下不是正式法律意见，但可作为发布前整改清单。

## 1. 维持 MIT OR Apache-2.0

结论：无问题，但建议改写理由。

该选择不会堵死商业化：

- 可以出售签名版、官方二进制、技术支持、订阅服务和手机远程服务。
- MIT/Apache-2.0 都不要求公开服务端源码，也不禁止闭源附加组件。
- 作者可对自己今后独立创作的代码采用其他许可。
- 官方签名、品牌、更新渠道和云服务仍可形成商业差异。

但“作者作为版权人不受限”只适用于作者自己的代码，不适用于 vendored 代码和未来贡献者代码。既有 MIT/Apache 版本一旦发布便不能撤回；第三方依赖仍须满足各自许可。

外部贡献进入后：

- 当前双许可足以让 Folio 在商业闭源发行物中使用贡献代码，但贡献者仍保有版权和继续使用代码的权利。
- 若将来想把整个代码库“排他地”改成 BSL/专有许可，并删除贡献者代码原有的 MIT/Apache 权利，则必须取得全部相关贡献者同意或版权转让。
- DCO 只证明来源，不转让版权；不做 DCO 并不会改善或恶化未来换许可能力。
- GPLv3/AGPLv3 形式的后续发行在技术上可能兼容现有许可，但不能令旧版本失去 MIT/Apache 权利。

建议把理由改为：

> Folio 保持 MIT OR Apache-2.0。该许可允许商业发行、闭源附加功能和托管服务；既有开源版本及第三方、贡献者代码仍受原许可约束。官方品牌、签名、服务和发行渠道可独立商业化。

## 2. 版权行改为 Weiyi Shi and Folio contributors

结论：无问题，前提是 Weiyi Shi 确实拥有初始代码版权，且不存在雇主、学校或委托合同中的权利归属问题。

仓库历史中人类提交作者只有 Weiyi Shi；AI 的 `Co-Authored-By` trailer 不应被当成版权主体。当前年份 2026 也与最早提交年份一致。

直接修改：

- `LICENSE-MIT:3`：

  `Copyright (c) 2026 Weiyi Shi and Folio contributors`

- `LICENSE-APACHE:190` 附录应用声明：

  `Copyright 2026 Weiyi Shi and Folio contributors`

注意：

- “Folio contributors”不会产生版权转让，只是集合性署名。
- Apache 正文不要改动，只改附录中的应用声明。
- 若以后版权转让给公司，应再更新所有权说明；公司成立本身不会自动取得个人版权。

## 3. CONTRIBUTING 加 inbound 双许可条款，不做 CLA/DCO

结论：方向无问题，但不能把原句孤立加入。

当前 [CONTRIBUTING.md](CONTRIBUTING.md) 前文没有明确的许可先行段落，因此单独写 `dual licensed as above` 会出现“above”无明确指代的风险。

建议直接加入完整章节：

```markdown
## Licensing contributions

Folio is licensed under either the MIT License or the Apache License,
Version 2.0, at the recipient's option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms
or conditions.

Do not submit code, assets, or other material that you do not have the
right to license. Identify any third-party material and its license in
the pull request.
```

不做 CLA/DCO 对目前的小型开源项目可以接受。需要明确其代价：以后若要取得排他版权或彻底更换贡献代码许可，必须逐一联系贡献者。若未来公司贡献或大规模外部贡献明显增加，再评估 CLA 即可。

## 4. 商标说明

结论：有实质风险，建议在 v0.1 公开前处理，而不是“商业化前”才处理。

主要问题：

1. FOLIO Technologies, Inc. 的美国申请是序列号 `97445151`，2022-06-06 申请；公开商标数据库目前显示其于 2023-07-17 因未答复而放弃，并不是有效注册。不能只写“有 2022 年申请”而省略状态。[申请资料](https://uspto.report/TM/97445151/APP20220609114618)、[状态记录](https://furm.com/trademarks/folio-97445151)。最终应由律师通过 USPTO TSDR 再核验；USPTO 也指定 TSDR 为状态查询渠道。[USPTO 指引](https://www.uspto.gov/trademarks/apply/check-status-view-documents)

2. “Folio”在软件领域高度拥挤，已经存在 Windows 桌面软件和长期运营的 FOLIO 开源软件平台，例如现有的 [Windows Folio 应用](https://github.com/kkauff/folio) 和 [FOLIO Library Services Platform](https://docs.folio.org/docs/)。因此商标检索不能只查这一件已放弃申请。

3. “fork 须改名”过于绝对。它可能错误限制真实说明、未修改再分发以及法律允许的指称性使用。

4. “图标不在开源许可范围内”会混淆版权与商标。如果构建产物必须复制、嵌入该图标，却完全不授予版权许可，可能令正常编译和未修改再分发也无法进行。更稳妥的做法是保留图像版权许可，但明确不授予其作为来源标识使用的商标权。

建议：

- 在公开 v0.1 前做美国及预期市场的名称检索，至少覆盖软件、终端、远程访问和 SaaS 服务。
- 不要把第三方的 `97445151` 申请写进 README 商标政策；应放在内部法律风险记录。
- 未完成检索前不要使用 `®`；是否使用 `™`也应在检索后决定。
- 建议商标条款：

```markdown
## Trademarks

The MIT and Apache-2.0 licenses grant copyright and patent permissions
only. They do not grant permission to use the Folio name, logos, or
other source-identifying marks in a way that suggests affiliation,
sponsorship, or endorsement.

Modified distributions must use a different product name and branding.
Truthful, nominative references to Folio and notices required by
applicable licenses or law are permitted.
```

## 专项核查

### (a) alacritty_terminal Apache-2.0 §4

仓库内：满足。

- vendored 许可证与 crates.io 0.26.0 原版哈希完全一致。
- 逐字节比较得到 23 个差异文件，23 个都在文件前 12 行内具有醒目的修改声明。
- `CHANGES-FOLIO.md:10` 完整索引这些修改。
- 上游 crate 确实没有 `NOTICE`，所以 §4(d) 没有上游 NOTICE 要传播。

Zip：满足。

- `package.ps1:82` 带根 `LICENSE-APACHE`，已满足 §4(a) 的“提供一份本许可证”；不要求再放一个不同文件名的 Alacritty LICENSE。
- `THIRD-PARTY-NOTICES.md` 明确说明 vendored、modified、无上游 NOTICE，并再次包含 Apache-2.0 全文。
- 二进制包不必包含 `CHANGES-FOLIO.md` 或 23 个源文件。

Apache §4 的原始要求可参见[官方许可证文本](https://www.apache.org/licenses/LICENSE-2.0.html)。

### (b) 微软 conpty.dll / OpenConsole.exe

部分满足，但有一个残余风险。

已满足的部分：

- 两个 `.nupkg` 的 `.nuspec` 均声明 MIT 和 Microsoft copyright。
- 仓库中的两个 Microsoft MIT 文本哈希一致。
- Zip 中的 `THIRD-PARTY-NOTICES.md` 包含 Microsoft 版权行和 MIT 全文。
- 官方 NuGet 页面也把该包标为 MIT。[NuGet 包页](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY/)

残余风险：

Microsoft Terminal 仓库本身有一份包含 `jsoncpp`、`fmt`、WIL 等项目的 [`NOTICE.md`](https://github.com/microsoft/terminal/blob/main/NOTICE.md)，但 NuGet 包没有携带它，也没有给出源码 commit。仅凭 `.nuspec` 的 MIT 表达式，不能证明 `OpenConsole.exe` 静态包含的所有第三方组件署名均已覆盖。

直接改法：

- 将与二进制版本 `1.25.260710002-preview` 对应的 Windows Terminal tag `v1.25.1322.0` 的 `NOTICE.md` 原样固定，例如保存为 `licenses/microsoft-terminal-NOTICE-v1.25.1322.0.md`。
- 由 `bundled-assets.md` 将其全文纳入 `THIRD-PARTY-NOTICES.md`。
- 记录 URL、tag、SHA-256，并加入漂移检查。全文多带几项比漏掉静态链接组件安全。

### (c) 字体与 Material 图标

字体：Zip 已满足。

Noto 字体原始字节嵌入 exe；OFL 全文、Google copyright、字体内部 2022 copyright 信息均进入 `THIRD-PARTY-NOTICES.md`，zip 会带该文件。

Material 图标：二进制 zip 基本满足；源码仓有一个保守整改项。

- Zip 中已有来源、Apache-2.0 全文和修改说明。
- 但实际路径位于 `marks.rs:3153`，文件附近没有直接的 Material 来源及修改声明。Apache §4(b)要求修改文件带醒目声明。

建议在 gear 常量附近加入类似注释：

```rust
// Adapted from Google's Material Design "settings" icon.
// Modified for currentColor and Folio's rendering sizes.
// Apache-2.0; see THIRD-PARTY-NOTICES.md.
```

### (d) 二进制 zip 应带什么

当前七文件结构基本正确：

- `folio.exe`
- `conpty.dll`
- `OpenConsole.exe`
- `README.md`
- `LICENSE-MIT`
- `LICENSE-APACHE`
- `THIRD-PARTY-NOTICES.md`

不需要为每个第三方组件提供单独文件，只要合并通知可读、完整且随包分发。`SHA256SUMS.txt` 和 SBOM 作为 release sibling asset 可以，不是许可证义务。

但目前 `target/release-package` 不存在，因此本次只能确认脚本逻辑，不能确认某个实际 zip。建议在 release workflow 的 `archive` 前直接运行：

```powershell
./scripts/check-notices.ps1
./scripts/check-vendor-notices.ps1
```

当前普通 CI 会运行它们，但 release workflow 与 CI 是两个独立 workflow；不应让发布草稿依赖另一工作流恰好成功。

### (e) Cargo.toml 元数据

许可字段一致，无问题。

- `Cargo.toml:36` 是标准 SPDX 表达式 `MIT OR Apache-2.0`。
- 所有一方 workspace crate 都通过 `license.workspace = true` 继承。
- 两个 vendored crate 保留各自 Apache-2.0/MIT 声明。
- Cargo 官方也以 `MIT OR Apache-2.0` 作为合法示例。[Cargo Manifest Reference](https://doc.rust-lang.org/cargo/reference/manifest.html)

`authors` 和 `repository` 当前均缺失：

- `authors` 是可选且已被 Cargo 标记为 deprecated；缺失不构成许可不一致，也不建议把它当作版权证明。
- `repository` 可选，但公开发布前应补真实 GitHub URL。当前仓库没有配置 remote，因此本次无法验证应填的 URL。
- 如果不准备向 crates.io 发布内部 crate，建议给一方 crate 加 `publish = false`，防止误发布；若准备发布，则补 `repository.workspace = true`、README、description 等元数据。

## 其他遗漏项

1. `option-ext` 的 MPL-2.0 源码可得性目前依赖 crates.io URL。链接当前可用，但发布者应确保其持续可得。建议把精确的 `option-ext-0.2.0.crate` 作为 GitHub Release asset 一并发布并纳入 `SHA256SUMS.txt`。

2. 发布前应做一次权利链确认：作者是否受雇佣、学校政策、赞助或委托合同约束；这比版权行写法本身更重要。

3. 未来手机远程服务上线时，需另行处理隐私政策、服务条款、数据处理/跨境传输、未成年人、消费者订阅和安全事件响应。这些不是当前纯本地 v0.1 的开源许可阻塞项。

综合结论：第 1、2 条可采纳；第 3 条补足明确前文和第三方材料规则后采纳；第 4 条必须重写，并把商标检索提前到 v0.1 公开发布前。当前最需要补的许可证证据是 Microsoft Terminal 对应版本的完整 `NOTICE.md`，其次是 Material gear 源文件附近的 §4(b) 修改声明。
