<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio，Windows 和 macOS 终端，公式在命令输出中排版，agent 等待时标签页亮灯。
       图中一个终端窗格的命令输出里有一个展示公式，排版显示在下一条提示符上方。">
</picture>

[![License: MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%20or%20Apache--2.0-green)](#许可证)
[![Build](https://github.com/lulu-loopp/folio-terminal/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/lulu-loopp/folio-terminal/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/lulu-loopp/folio-terminal?include_prereleases&label=release&color=blue)](https://github.com/lulu-loopp/folio-terminal/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/lulu-loopp/folio-terminal/total?label=downloads&color=pink)](https://github.com/lulu-loopp/folio-terminal/releases)

Folio 是一个开源的 Windows 和 macOS 终端。公式在命令输出中原位排版，文件在终端旁预览，窗口自由布局，agent 等待时标签页亮灯提醒。

[English](README.md) · [快捷键](docs/shortcuts.md) ·
[安全](SECURITY.md) · [更新记录](CHANGELOG.md)

## 安装

**Windows**——从[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载压缩包，解压后运行 `folio.exe`。无需安装，文件保持在同一文件夹中即可。Windows 10 1809 及以上，64 位。

**macOS**——从同一页面下载 DMG，将 **Folio** 拖入 Applications。需要 Apple silicon Mac，macOS 14 及以上。也可以用 Homebrew：

```sh
brew install --cask lulu-loopp/folio/folio
```

<!-- winget: add when live -->

其余内容见 [`docs/install.zh-CN.md`](docs/install.zh-CN.md)：压缩包里有什么、初次启动询问什么，以及系统弹出提示时该怎么办。

## 功能

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="一个终端窗格显示一条命令的排版输出：段落中穿插行内公式，另有
       三个展示公式分占独立行。">
</picture>

*命令输出中的 LaTeX 在打印位置直接排版。*

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-markdown-dark.png">
  <img src="docs/screenshots/preview-markdown-light.png" width="100%"
       alt="终端旁的预览窗格中排版显示的 Markdown 文档：标题、表格、
       代码块和展示公式同时可见。">
</picture>

*文件内容可在终端旁直接查看，无需打开其他程序。*

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/settings-agents-dark.png">
  <img src="docs/screenshots/settings-agents-light.png" width="100%"
       alt="设置中的 Agent 页：Claude Code、Codex 和 GitHub Copilot CLI
       各一行，每行说明开关写入哪个文件，三个开关均关闭。">
</picture>

*[等待你操作的 agent](docs/features.zh-CN.md#为-agent-而设计) 会在标签页上标记，无需逐个检查。*

- [终端中的 LaTeX 排版](docs/features.zh-CN.md#终端中的-latex-排版)——命令输出中的 `$…$` 和 `$$…$$` 在打印所在行排版。
- [终端旁的预览](docs/features.zh-CN.md#终端旁的预览文件pdf视频网页)——PDF 逐页翻阅，视频播放，网页带地址栏。
- [在阅读的地方编辑 Markdown](docs/features.zh-CN.md#在阅读的地方编辑-markdown)——用阅读时的字体，文件其余内容逐字节保持原样。
- [窗格、标签页，自由移动](docs/features.zh-CN.md#窗格标签页自由移动)——拆分、拖出、浮动，内部的会话保持运行。
- [快捷终端](docs/features.zh-CN.md#快捷终端)——``Win+` ``（Mac 上是 ``⌃` ``）调出终端覆盖在屏幕上方。
- [搜索一切](docs/features.zh-CN.md#搜索一切)——操作、窗格、命令、文件和设置项，一个搜索框全查。
- [系统集成](docs/features.zh-CN.md#windows-集成)——资源管理器和 Finder 的右键菜单，以及 VS Code 的外部终端。
- [中英双语](docs/features.zh-CN.md#中英双语)——所有界面文字均有中英两种语言，在设置中一行切换。

## 隐私

Folio 不收集任何数据：没有遥测，没有统计，没有崩溃上报。联网的只有两件事——网页预览里打开的页面，以及更新检查，向 `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` 发一次 `GET`，不携带版本号和标识符，在**设置 > 通用 > 检查更新**或 `"update_check": false` 关闭。设置、配置和会话留在本机，不发往任何地方；[`docs/PRIVACY.md`](docs/PRIVACY.md) 列出每个文件的内容。

## 许可证

MIT 或 Apache-2.0，任选其一；[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) 收录所有依赖项的许可证，[`TRADEMARK.md`](TRADEMARK.md) 说明名称和标识的使用。

## 构建与贡献

从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)，其中指向 [`CONTRIBUTING.md`](CONTRIBUTING.md) 和 [`SECURITY.md`](SECURITY.md)。
