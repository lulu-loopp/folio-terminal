<!-- zh pending opus46 -->
<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio——能排版公式的 Windows 和 macOS 终端，并提示哪个 agent 在等你。
       图中终端窗格运行了一条命令并输出文件内容，其中的展示公式——e
       的负 x 平方在整条实数轴上的积分等于根号 π——以排版形式显示在
       输出中，位于下一行输入上方。">
</picture>

[![License: MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%20or%20Apache--2.0-green)](#许可证)
[![Build](https://github.com/lulu-loopp/folio-terminal/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/lulu-loopp/folio-terminal/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/lulu-loopp/folio-terminal?include_prereleases&label=release&color=blue)](https://github.com/lulu-loopp/folio-terminal/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/lulu-loopp/folio-terminal/total?label=downloads&color=pink)](https://github.com/lulu-loopp/folio-terminal/releases)

Folio 是一个开源的 Windows 和 macOS 终端。公式在命令输出中原位排版，文件在终端旁预览，窗口自由布局，agent 等待时标签页亮灯提醒。

[English](README.md) · [快捷键](docs/shortcuts.md) ·
[安全](SECURITY.md) · [更新记录](CHANGELOG.md)

## 安装

<!-- zh: pending — 由原「下载」节的中文句子拼成，语序待审 -->
**Windows**——从[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载压缩包，解压到任意目录，运行 `folio.exe`。无需安装，压缩包中的文件需放在同一文件夹中。系统要求：**Windows 10 1809 及以上，64 位**。

<!-- zh: pending -->
**macOS** — take the DMG from the same page and drag **Folio** to Applications.
Needs an Apple silicon Mac running macOS 14 or newer. Or, with Homebrew:

```sh
brew install --cask lulu-loopp/folio/folio
```

<!-- winget: add when live -->

其余内容见 [`docs/install.zh-CN.md`](docs/install.zh-CN.md)：压缩包里有什么、初次启动询问什么，以及系统弹出提示时该怎么办。 <!-- zh: pending -->

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

<!-- zh: pending — 每条由 docs/features.zh-CN.md 各节的首句删减而来，删减处待审 -->
- [终端中的 LaTeX 排版](docs/features.zh-CN.md#终端中的-latex-排版)——命令输出中的 `$…$` 和 `$$…$$` 在打印所在行排版。
- [终端旁的预览：文件、PDF、视频、网页](docs/features.zh-CN.md#终端旁的预览文件pdf视频网页)——PDF 逐页翻阅，视频播放，网页带地址栏。
- [在阅读的地方编辑 Markdown](docs/features.zh-CN.md#在阅读的地方编辑-markdown)——用的还是阅读时的字体，文件其余内容逐字节保持原样。
- [窗格、标签页，自由移动](docs/features.zh-CN.md#窗格标签页自由移动)——拆分、拖出、浮动，内部的会话保持运行。
- [快捷终端](docs/features.zh-CN.md#快捷终端)——``Win+` ``（Mac 上是 ``⌃` ``）调出终端覆盖在屏幕上方。
- [搜索一切](docs/features.zh-CN.md#搜索一切)——操作、窗格、命令、文件和设置项，一个搜索框全查。
- [Windows 集成](docs/features.zh-CN.md#windows-集成)、[macOS integration](docs/features.zh-CN.md#macos-integration) 和 [Visual Studio Code](docs/features.zh-CN.md#visual-studio-code)。
- [English and Chinese](docs/features.zh-CN.md#english-and-chinese) — every string in both, switched from one row in Settings. <!-- zh: pending -->

## 隐私

<!-- zh: pending — 由原「隐私」两段并成一段，另加末句「设置、配置和会话都留在本机」，措辞待审 -->
Folio 不发送遥测、分析数据或崩溃报告。会访问网络的只有两项：在预览窗格中打开的网页，以及更新检查——每天最多一次（所有窗口合计），向 `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` 发送一次 `GET` 请求，不携带版本号、标识符或查询参数，可在设置 > 通用 > **检查更新**关闭，或在 `settings.json` 中设置 `"update_check": false`。设置、配置和会话都留在本机。各文件的具体内容见 [`docs/PRIVACY.md`](docs/PRIVACY.md)。

## 许可证

<!-- zh: pending — 由原「许可证」两段并成一句，措辞待审 -->
MIT 或 Apache-2.0，任选其一；所有依赖的许可证及其要求的声明见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)，Folio 名称和标识不在授权范围内，见 [`TRADEMARK.md`](TRADEMARK.md)。

## 构建与贡献

<!-- zh: pending — 原句的后半改为「其中指向」，措辞待审 -->
从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)，其中指向 [`CONTRIBUTING.md`](CONTRIBUTING.md) 和 [`SECURITY.md`](SECURITY.md)。
