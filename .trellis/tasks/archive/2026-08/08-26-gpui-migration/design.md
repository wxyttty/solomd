# 技术设计：GPUI 迁移架构（已落地形态）

## 三 crate 拓扑

```
gpui/
├── crates/core/     solomd-core  — 后端业务库（UI 无关，22 模块 18.2K 行）
├── crates/app/      solomd       — GPUI 桌面应用（二进制）
├── crates/spike/    spike        — P0 实验室（保留：性能基线/回归验证用）
└── board-webview/   白板前端（tldraw runtime，esbuild 打包 → include_str! 进二进制）
```

## 依赖锁步（关键约束）

三方 git 依赖必须统一在同一 zed rev 上（git 依赖仅 spec 完全一致才去重）：

```
zed@1d217ee  ←  gpui-component@1505b14（其内部清单钉的就是 1d217ee）
             ←  zorite@main（gpui-editor 所在，workspace 钉同一 rev）
```

升级舞步：跟 zorite 的根 Cargo.toml 注释走——它升级 gpui-component pin 后我们跟随。
本机无 Xcode（仅 CLT），离线 metal 编译不可用 → `runtime_shaders` feature 必开。
工具链锁稳定版 1.97.1（nightly 会因 pathfinder_simd 内部 intrinsics 变动编译失败）。

## solomd-core 接缝设计

原 tauri AppHandle 的三类用途全部收敛为两个注入 trait：

- **EventSink**（+ EventSinkExt::emit_json 扩展 trait 保 dyn 兼容）：
  替代 `app.emit("solomd://topic", payload)`；UI 侧实现 = mpsc channel + 实体内
  异步泵（80–200ms 轮询），此模式已在三处复用（白板 IPC / AI 面板 / recipes）。
  `Arc<dyn EventSink>` 自带转发 impl。
- **CorePaths / IndexCtx**：替代 `app_config_dir` / `app_data_dir`。
- **Lazy 全局**：RecipesState（原 tauri manage）。
- **事件解耦**：git_auto_commit 不再内联触发 recipes，改发 `git-committed` 事件
  由组合层监听。

## 编辑器底座：zorite gpui-editor（GPL-3.0）

选型理由：唯一具备 caret-reveal + 可变行高标题 + 块级 providers（图片/mermaid/
transclusion）+ 表格单元格编辑 + wikilink/属性面板 + spell-check 接口的 GPUI 编辑器；
zorite 本身（Logseq 风格笔记）验证了"gpui-component 做外壳 + gpui-editor 做笔记"的
双栈架构。gpui-component 的 Editor 装饰仅限样式，无 WidgetType 等价物——这是选型
的分水岭。

宿主侧三件套（全部 zorite 同款模式）：
- SyntaxStyle：明/暗两套暖色板（tokens.css 映射），set/clear 即 WYSIWYG 开关
- 块级 providers：image（同步解码+BGRA+1024px 缩图）、mermaid（mermaid-rs-renderer
  → SvgRenderer，off-thread 状态机 Loading/Ready/Failed）
- 事件泵：EditorEvent（Changed/PreviewImage/OpenLink/OpenWikiLink）→ SoloMD 导航语义

## 白板：Wry webview 独立窗口

tldraw 前端层（tldraw-runtime/board/markdown 三个 framework-free 文件）零改动复用，
esbuild 打包 1.9MB iife 经 include_str! + with_html 加载；快照回写走 wry IPC
（board→host postMessage / host→board evaluate_script），Rust 侧按 tldraw-markdown
字节规则重写围栏（info 行逐字节保留已验证）。

## 已知边界（记录在案）

- 中文斜体不可见：CJK 字体无斜体字面且 GPUI 不合成倾斜（拉丁正常）
- 独立图片行光标无法左右穿越：疑似 zorite 上游 bug（其 demo 亦未覆盖）
- gpui-wry experimental：白板走独立窗口规避 overlay 未解问题；Linux webkitgtk
  路径 2026-08 刚修通但未深测
- gpui-component 回退至 1505b14：无标签右键菜单/中键关闭（上游后续才有）
- crates.io 发布停摆：全部 git 依赖 + Cargo.lock 冻结，升级=集中窗口
