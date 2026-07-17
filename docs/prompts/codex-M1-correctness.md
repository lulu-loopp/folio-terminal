# M1:宽度正确性攻坚(grapheme 聚类 + ambiguous width)

> 前置:M0-α(`81c7dd9`)、M0-β(`5712448`)已关片。本片解决 spike 04 立案、M0 明确推迟的两个裁决项。**两个裁决已定**(业界调研 2026-07-17):① ambiguous width **默认窄**,判定收敛到单一 width oracle,配置项留给 P2-7;② grapheme 聚类**要上**:UAX #29 状态机 + mode 2027 协商,单簇宽度钳制 2 格(wcwidth 0.8 共识),默认档由 ConPTY 侦察结果决定(见下)。

## 一、这一片「做完」长什么样

1. `echo 👨‍👩‍👧‍👦` 占 **2 格**(现在是 8 格——WT #900 本体),`👍🏽` 占 2 格(现在 4),`é`(e+combining)占 1 格;VS16 把窄 emoji 变宽(☂︎→☂️)、VS15 反之;国旗序列 2 格;
2. `echo ☆` 等 ambiguous 字符占 1 格(默认窄),判定集中在一个可配置的 oracle 函数,有注释说明 P2-7 时如何公开;
3. 程序 `CSI ? 2027 $p` 能查询、DECSET/DECRST 2027 能开关聚类模式,回报语义按 contour 的 terminal-unicode-core 规范;
4. 混排行(cluster+CJK+ASCII)在真窗口里占格与光标全部正确——β 片的宽字符渲染路径(lead+spacer、双格光标、字体回退)直接消费新的宽度判定,渲染层零特判。

## 二、关键侦察项(动手前先查,结论写进交付报告)

**ConPTY 按什么宽度假设记账?** 我们坐在 ConPTY 后面,它对光标位置有自己的簿记(WT 1.22 自家 conhost 已支持 grapheme 三档测量)。侦察:向真实 ConPTY 喂 ZWJ 序列,观察它转发的字节/光标控制序列如何处理;结论决定**默认档**——若 ConPTY 与 wcwidth 一致,默认 wcwidth 兼容、2027 协商才开聚类(Ghostty 的姿势,避免 fish/tmux 式光标错位);若 ConPTY 已按 grapheme(新 Windows 11 conhost 可能),默认可以直接聚类。不许拍脑袋,用字节证据说话。

## 三、实现要点

- **聚类发生在写入路径**(字符落格时),有状态(前一码点+当前码点+状态整数),用成熟库(unicode-segmentation)实现 UAX #29 边界判定;
- **vendored alacritty 网格**:现有 zerowidth 附加机制(宽 0 字符挂到前一 cell)是聚类落格的现成锚点;若需 vendor 补丁,遵守 α 立的纪律——**补丁面最小化 + 行为测试钉死**(参照 R11' 的处理);
- **width oracle 单点化**:所有「这个簇/字符占几格」的判定走同一个函数(含 ambiguous 表、VS15/16、ZWJ 簇钳 2),bt-term 与渲染层共用,禁止两处各算各的;
- **渲染**:簇文本落进 lead cell,渲染层照 β 的宽字机制画;**彩色 emoji 字形渲染不在本片强求**——若 Segoe UI Emoji 加进固定回退链 + cosmic-text 能直接出彩色字形则顺手做,若有兔子洞(COLR/CPAL 支持问题)则如实记录、宽度正确先行,豆腐块可接受但占格必须对;
- **语料弹药**:spike 01 语料 `bt-corpus` 里有 cjk-width / emoji-vs16 / zwj-family 案例;G1 回放矩阵风格的字节驱动测试覆盖:ZWJ 家庭、肤色修饰、VS15/16、combining、国旗、ambiguous 样本、聚类模式开/关两档、resize 跨簇行为(簇不得被拆到两行各一半)。

## 四、明确不做

- BiDi、竖排;字体级 emoji 分色渲染的完备性(见上,占格正确优先);
- ambiguous 配置项 UI(判定函数留好口子即可,P2-7 落地);
- 富内容/数学(M3);分屏/标签(M2);
- 上游 wcwidth/终端生态的兼容性洗白工作(mode 2027 本身就是那个答案)。

## 五、硬规矩(沿用)

- DESIGN.md 为权威;两个裁决(默认窄、聚类+2027+钳2)已定,认为有问题写偏离申请,不许静默改;
- 声明「已实现」必须指出行为测试;ConPTY 侦察必须有字节证据;
- `cargo test --workspace --locked`、clippy `-D warnings`、fmt 全绿;vendor 补丁若有,`cargo metadata` 自证仍在根 workspace;
- 做完停下等审核。

## 六、交付

每项改动:做了什么、哪个测试验证。ConPTY 侦察报告(字节证据+默认档决定)。手动验收清单(真窗口里敲哪些序列、看什么)。若 winit/ConPTY 有本片新坑,如实记录。
