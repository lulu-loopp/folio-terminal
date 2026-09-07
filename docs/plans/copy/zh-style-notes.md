# 中文文案风格参考

> 研究日期：2026-09-07。用于 README.zh-CN.md 及后续中文 UI 文案。
> 综合原生中文文档观察与 `docs/plans/copy/user-facing-copy-guide.md` 中的 Folio 文案指南。

## 写作规则

1. **先说结果，再说方法。** 开头讲用户看到什么或能做什么，实现细节放后面或省略。
2. **一句一件事。** 每句 15–35 字，超过 40 字必须拆分。操作、前提、解释不挤在一句里。
3. **省略不必要的主语。** 中文常省"你""它"等主语，不反复写"Folio 会……""你可以……"。
4. **用直接动词，不用名词化。** 写"配置通知"而非"进行通知的配置"，写"写入文件"而非"执行写入操作"。
5. **不写防御性否定句。** 除非省略会导致误操作，否则不写"不会做 X""没有做 Y"。保留必要限制（如功能缺少某依赖时的提示），删除"其余不受影响""不做多余操作"式安慰句。
6. **不解释内部机制。** 不写渲染器名称、线程模型、注册表细节，除非用户需要手动操作它们。库名不出现在面向用户的文案中。
7. **用熟悉的界面词汇，保持一致。** 标签页、窗格、预览窗格、文件列、右键菜单、搜索面板、设置、快捷键、配置。不在"窗格/面板/区域"之间换来换去。
8. **英文术语保持原文。** 产品名、路径、命令、环境变量、快捷键保持原样。中文与拉丁字母/数字之间加半角空格。
9. **中文标点。** 用全角，。；：？！和（），用、连接短并列项。引用界面文字用加粗而非引号。
10. **段落结构跟中文习惯走。** 可以先条件再动作，用"即可""则""若"连接，不必跟英文主谓宾顺序。

## 来源与观察

### 参考页面

| 来源 | URL | 用途 |
| --- | --- | --- |
| Vue 中文文档（原生维护） | https://cn.vuejs.org/guide/introduction.html | 技术概念介绍的写法 |
| Vue 快速开始 | https://cn.vuejs.org/guide/quick-start.html | 安装步骤、前提条件 |
| 腾讯云 CVM 快速入门 | https://cloud.tencent.com/document/product/213/2936 | 产品功能描述、操作步骤 |
| 阿里云 ECS 快速入门 | https://help.aliyun.com/zh/ecs/getting-started/quick-start | 步骤式引导、称呼方式 |
| uTools 文档 | https://www.u-tools.cn/docs/guide/about-uTools.html | 国产工具功能介绍 |
| Windows Terminal 概述 zh-cn | https://learn.microsoft.com/zh-cn/windows/terminal/ | 终端产品中文术语 |
| Windows Terminal 交互设置 zh-cn | https://learn.microsoft.com/zh-cn/windows/terminal/customize-settings/interaction | 设置项描述的中文写法 |
| 少数派 WT 自定义 | https://sspai.com/post/59380 | 原生中文科技写作 |

### 句式特征

**开篇方式：** 原生中文技术文档以产品定义或功能结果开篇，不做长背景铺垫。

- "Vue 是一款用于构建用户界面的 JavaScript 框架。"（cn.vuejs.org）
- "uTools 是新一代的效率工具平台，它采用底座平台 + 插件应用的创新形式。"（u-tools.cn）
- "Windows 终端是你喜欢的命令行 shell 的新式主机应用程序。"（learn.microsoft.com zh-cn）

**操作步骤：** 祈使句为主，条件在前，动作在后。

- "确保你安装了最新版本的 Node.js，并且你的当前工作目录正是打算创建项目的目录。"（Vue）
- "在命令行中运行以下命令（不要带上 `$` 符号）："（Vue）
- "单击**立即购买**，并付费完成后，即完成了云服务器的购买。"（腾讯云）

**功能描述：** 直接说能力，用"支持""可以"或省略主语的短句。

- "可以在 Windows 终端中使用命令行接口运行任何应用程序。"（Microsoft zh-cn）
- "通过中文、拼音或拼音首字母快速启动软件。"（uTools）
- "在任何窗口，你都可以使用快捷键 `Alt + 空格键` 呼出 uTools。"（uTools）

**读者称呼：** 开发者工具用"你"，云服务用"您"。很多时候省略。

**并列项：** 短项用顿号（、），长项用逗号分句。

- "多个选项卡、窗格、Unicode 和 UTF-8 字符支持、GPU 加速文本呈现引擎"（Microsoft zh-cn）

**英文混排：** 产品名和技术术语保持英文，前后加空格。

- "如果不确定是否要开启某个功能，你可以直接按下回车键选择 `No`。"（Vue）
- "这一指令将会安装并执行 create-vue，它是 Vue 官方的项目脚手架工具。"（Vue）

### 术语参照

| 英文 | Windows Terminal zh-cn | Folio 用语 |
| --- | --- | --- |
| terminal | 终端 | 终端 |
| tab | 选项卡 | 标签页 |
| pane | 窗格 | 窗格 |
| profile | 配置文件 | 配置 |
| shortcut / keybinding | 快捷键/键盘快捷方式 | 快捷键 |
| context menu | 上下文菜单/右键菜单 | 右键菜单 |
| command palette | 命令面板 | 搜索面板 |
| clipboard | 剪贴板 | 剪贴板 |
| settings | 设置 | 设置 |
| notification | 通知 | 通知 |
| focus | 焦点 | 焦点 |
| preview pane | — | 预览窗格 |
| file list / files column | — | 文件列 |
