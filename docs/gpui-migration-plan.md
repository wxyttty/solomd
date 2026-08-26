# SoloMD GPUI 迁移计划

> 决策日期：2026-08-25 · 状态：Phase 0 进行中 · 配套决策记录：[Agent Note](../.agents/notes/proposed/architecture/2026-08-25-tauri-to-gpui-migration.md)

## 1. 目标

把桌面端从 Tauri 2 + Vue 3 + CodeMirror 6 迁移到 GPUI（Zed）+ gpui-component（Longbridge），换取原生性能与低资源占用：去掉 WebView 常驻内存、消除 JS↔Rust IPC 序列化边界、GPU 直绘。

本仓库为**自用 fork**，范围按个人需求裁剪。

## 2. 范围决定（已确认）

| 决定 | 内容 |
|---|---|
| ✅ 白板保留 | tldraw 白板经 gpui-wry（Wry webview）独立窗口承载，现有前端资产大部分复用 |
| ❌ 移动端不做 | 无 Android/iOS 负担 |
| ❌ Vim mode 不做 | 排除对应重写成本 |
| ❌ 数学公式不做 | KaTeX/RaTeX 管线整体移除；块级活渲染只剩图片行、GFM 表格、mermaid 围栏 |
| ⚠️ mermaid 暂保留 | 用 ariel-rs + resvg 渲染为位图；若实际不用可随时再砍 |

## 3. 技术选型依据（2026-08-25 检索）

### 选定

- **GPUI**（zed main，git 依赖）：Zed 已于 2026-04 底发 1.0，框架经大规模生产验证。
- **gpui-component**（longbridge，git 依赖，HEAD）：Apache-2.0；60+ 组件、Dock 布局、Tree-sitter 编辑器、原生 Markdown/HTML TextView、虚拟表格、i18n、CJK 良好；Longbridge Pro 生产在用；提交频率约每日多个（截至 2026-08-25），markdown 性能与 IME 持续打磨。
- **gpui-wry**（gpui-component 的 webview crate，包名 `gpui-wry`，基于 `lb-wry` 0.53.3）：承载白板。macOS/Windows 官方支持；Linux 路径 issue #765 已于 2026-08-24 关闭但仍属 experimental。
- **ariel-rs** v0.4（MIT）：纯 Rust Mermaid 渲染，28 种图类型，对 Mermaid JS 做过视觉回归。输出 SVG → resvg 栅格化 → GPUI image。

### 关键约束事实

1. **crates.io 发布管道停摆**：`gpui` 最新 0.2.2 停在 2025-10-22，早于 platform 拆分（zed#49277）；因 `gpui_macros` 等 `publish = false`，当前版本无人能发布（zed discussion #55271）。→ **必须用 git 依赖**。
2. **锁版本策略**：不自行钉 rev——gpui-component 以裸 git 引用 zed，下游若单独钉 rev 会与它的解析冲突。正确做法是与其一致用裸 git 依赖，靠**提交 Cargo.lock** 冻结整图解析结果，升级走集中 `cargo update` 窗口。
3. **API pre-1.0 churn**：跟随 zed main 意味着周期性 breakage，需以升级窗口集中消化。
4. **数学公式现状**：gpui-component TextView 对 math 仅渲染为代码样式文本（PR #2482 实质未实现排版）；本项目已砍公式，无影响。
5. **GPUI overlay 盖在原生 webview 之上未解决**（gpui-component #2626）→ 白板必须走**独立窗口**，恰好匹配现有全屏 overlay 交互。

## 4. 功能映射

| SoloMD 功能 | GPUI 方案 | 风险 |
|---|---|---|
| Tabs / split panes | gpui-component Dock | 低 |
| 文件树/大纲/命令面板/设置 | gpui-component 组件 + 自绘 | 低 |
| Markdown 只读渲染 | TextView（原生 MD） | 低 |
| Mermaid 图 | ariel-rs → resvg → image | 中 |
| **WYSIWYG 活编辑** | gpui-component Editor + 自研 caret-reveal 装饰层 | **最高（唯一攻关点）** |
| tldraw 白板 | gpui-wry 独立窗口 + 复用 tldraw-runtime/tldraw-markdown 层 | 中 |
| Hunspell/CJK 校对、编码检测 | hunspell-rs、cjk_proofread.rs、encoding_rs | 低 |
| Pandoc 导出 | pandoc.rs shell-out 不变 | 低 |
| Agent Panel / recipes / trace | 自绘 UI + 后端进程内直调 | 低 |
| i18n | 字符串迁入 Rust 表（按需砍语言） | 机械量 |

## 5. 白板架构

```
┌─ GPUI 主窗口 ──────────────┐      ┌─ Wry webview 独立窗口 ─────────┐
│ ```tldraw 围栏 → 占位芯片   │ 打开 │ 复用 tldraw-runtime.ts         │
│ （board id / 尺寸元数据）    │ ───→ │ (mountBoard/BoardHandle 不变)  │
│                            │      │ 编辑 → 快照 JSON                │
│ 快照回写：Rust 侧按          │ ←─── │                                │
│ tldraw-markdown 字节规则     │ IPC  │ wry IPC（evaluate_script 下行 / │
│ 重写围栏体                   │      │ ipc handler 上行）              │
└────────────────────────────┘      └────────────────────────────────┘
```

- `tldraw-markdown.ts`（围栏解析/序列化，framework-free）的不变量移植到 Rust 侧做缓冲区手术；
- `tldraw-runtime.ts`（React/tldraw 隔离层）原样进 webview；
- 缩略图 v1 为占位芯片，v2 再考虑快照预览。

## 6. 阶段计划

| 阶段 | 内容 | 工期 | 闸门 |
|---|---|---|---|
| **P0 Spike** | workspace 骨架（本目录）；四个验证点：① Dock+文件树骨架 ② TextView 渲染样例笔记 ③ Editor 大文件性能实测 ④ wry 窗口挂 tldraw runtime 完成快照回写闭环 | 2–3 周 | **四点均已编译/运行验证**：③ 实测 5MB/20.9万行载入 41ms（debug）；④ 真实 tldraw@4.5.12 画板快照回写闭环自动通过（info 行逐字节保留）。待人工目检 + Tauri 版对照数据后出正式 go/no-go |
| **P1 编辑器核心** | ✅ **完成（2026-08-25）**：zorite/gpui-editor 底座（5MB 91ms 闸门过）、SyntaxStyle 暖色主题、图片/mermaid providers、EditorEvent 导航、图片粘贴、WYSIWYG 开关；已知边界：中文斜体（CJK 无斜体字面）、独立图片行光标穿越（疑似上游 bug） | 4–6 周 | ✅ 用户目检确认 |
| **P2 应用骨架** | 🟨 **进行中（M1 已落地）**：新 crate `crates/app`（二进制 `solomd`）——vault 目录（参数/缺省 `_vault`）+ 文件树（点击打开/目录展开）+ 多标签编辑（TabPanel 动态开关，关闭同步注册表）+ ⌘S 保存（标题 • 脏态）+ ⌘N 新建 + 首启示例笔记 + 图片/mermaid providers 全量接入。修复：vault 路径 resolve 缺失。待做：外部变更监听、设置、命令面板、i18n、后端 27K 行去 Tauri 化 | 5–7 周 | 能日常记笔记（待用户验收） |
| **P2 应用骨架** | Dock/文件树/settings/命令面板/i18n/主题移植；后端 commands 去 Tauri 化改直调 | 5–7 周 | — |
| **P3 功能对等** | Agent Panel、recipes、图谱视图、搜索/反链/历史、同步设置、导出、白板打磨 | 8–12 周 | — |
| **P4 打包验收** | 签名公证、性能基线（冷启动/空载内存/大文件）、与 Tauri 版回归对照 | 2–3 周 | 基线达标 |

总量估计：单人 6–9 个月达到桌面功能对等。

## 7. 风险登记

| # | 风险 | 等级 | 对策 |
|---|---|---|---|
| 1 | WYSIWYG 在 gpui-component Editor 上受阻 | 🔴 | P1 双检查点；降级模式兜底 |
| 2 | gpui-wry experimental（Linux 边缘、overlay 未解） | 🟡 | 独立窗口模式；Tauri 旧版兜底开白板重文档 |
| 3 | 上游 API churn + 发布停摆常态化 | 🟡 | Cargo.lock 锁图；集中升级窗口；薄封装层隔离框架类型 |
| 4 | ariel-rs 单点依赖 | 🟢 | mermaid 渲染加适配接口，必要时退回源码显示 |

## 8. 证据索引

- zed discussion #55271（crates.io 停摆，2026-08 讨论）
- gpui-component README / commits（HEAD 至 2026-08-25）/ PR #2473 #2482 #2626 #2634 / issue #765
- gpui-mobile docs.rs（状态页）
- RaTeX README（本项目最终未采用——公式已砍）
- ariel-rs README v0.4
