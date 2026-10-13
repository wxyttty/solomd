# 国内社区 · 发布稿

用你自己的账号发，同一天不要在多个平台发一模一样的文字，评论区要回复。

## V2EX · 「分享创造」节点

**标题：** 做了一个开源的 Markdown 编辑器 SoloMD：Typora 式实时预览，五个平台都能用，大部分代码由 Claude Code 写

```
SoloMD 是一个免费开源（MIT）的 Markdown 编辑器，基于 Tauri 2 + Vue 3 + CodeMirror 6。

- Typora 式实时预览：光标离开的那一行，Markdown 标记会自动隐藏；也可以切换分栏或源码视图
- KaTeX 公式、Mermaid / PlantUML 图表、tldraw 白板
- 双向链接、反向链接、知识图谱，笔记就是一个文件夹里的普通 .md 文件
- 导出 PDF（文字版 / 图片版）、Word、独立 HTML（图表内嵌，离线可看）
- Windows / macOS / Linux / iOS / Android，Windows 安装包 13 MB
- Windows 上用系统原生文本框编辑，搜狗等输入法不吞字、不重复标点
- 内置 MCP 服务，Claude / Cursor 可以直接读你的笔记（默认只读）

说个实话：这个项目的代码、测试、发版，甚至 GitHub issue 的回复，大部分都是 Claude Code 写的，我负责方向和把关，README 开头就写明了。

下载（国内走 Gitee 镜像更快）：https://solomd.app
源码：https://github.com/zhitongblog/solomd

欢迎拍砖，尤其是从 Typora、Obsidian 转过来的朋友。
```

说明：应用默认开启匿名使用统计（不收集文件内容和 IP，设置里一键关闭），被问到时如实回答。

## 少数派 · Matrix 投稿

少数派 Matrix 要的是长文，不是广告。建议选题：

> **《让 AI 写一个 Markdown 编辑器，并把它上架四个应用商店：一个人 + Claude Code 的五个月》**

提纲：
1. 为什么还要再做一个 Markdown 编辑器（Typora 收费、Obsidian 不开源、手机端缺位）
2. 分工：人定方向和把关，Claude Code 负责写代码、测试、发版、回复 issue
3. 三个真实的坑：Windows 输入法吞字、iOS 27 启动崩溃被拒、Android 16 KB 内存页
4. 它现在能做什么（配图）
5. 这种做法的边界：它会犯哪些错，靠什么兜底（CLI + MCP 自测、先复现再修）

需要的话我可以按这个提纲写出全文，配好截图。

## 小众软件 · 发现频道

发现频道由用户推荐，填软件名、官网、一句话介绍：

- 名称：SoloMD
- 官网：https://solomd.app
- 一句话：免费开源的 Typora 式 Markdown 编辑器，支持 Windows / macOS / Linux / iOS / Android，Windows 安装包 13 MB
- 介绍：沿用 V2EX 稿的功能列表，去掉「由 Claude Code 编写」那一段（小众软件的读者更关心软件本身）
