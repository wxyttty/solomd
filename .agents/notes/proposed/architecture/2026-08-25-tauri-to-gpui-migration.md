# Agent Note: Tauri → GPUI 桌面迁移（自用 fork 双轨收敛为单轨桌面）

Status: proposed

## Problem

SoloMD 上游是 Tauri 2 + Vue 3 + CodeMirror 6（前端 ~88K 行 TS/Vue，后端 ~27K 行 Rust）。本仓库为自用 fork，WebView 常驻内存与 JS↔Rust IPC 边界带来不必要的资源开销；希望迁移到 GPUI 获得原生性能，但需先厘清生态可行性、砍掉个人不需要的功能、并把唯一的高风险点（WYSIWYG 活编辑重写）隔离成可控闸门。

## Proposal

采用 **GPUI（zed main）+ gpui-component（Longbridge HEAD）** 重写桌面端；后端 27K 行 Rust 去 Tauri 化改为进程内直调。范围裁剪：不做移动端、不做 vim、不做数学公式（KaTeX 管线移除）；保留白板（经 gpui-wry 独立窗口承载，复用现有 tldraw 前端层）与 mermaid（ariel-rs + resvg）。

依赖锁定策略：两个上游都用裸 git 依赖（不自行钉 rev），靠提交 Cargo.lock 冻结解析图，升级走集中窗口。完整计划见 [gpui-migration-plan.md](../../../docs/gpui-migration-plan.md)。

## 关键取舍

- **为何 gpui-component 而非裸 GPUI**：Dock/组件/i18n/CJK 开箱即用且生产验证（Longbridge Pro），避免自建设计系统。
- **为何白板走 webview 而非原生重写**：tldraw-runtime.ts 已把 React/tldraw 隔离在单文件后暴露框架无关接口，复用成本远低于原生 canvas 重写；GPUI overlay 盖 webview 未解决（#2626）→ 独立窗口恰好匹配现有全屏交互。
- **为何砍公式**：可见未来不需要；同时 TextView 的 math 本就只是代码样式文本（PR #2482 实质未实现），省去 RaTeX 适配风险。

## Alternatives considered

- **维持 Tauri**：无性能收益，放弃。
- **混合壳（GPUI 壳 + WebView 编辑区）**：落地快但内存收益打折，仅作 WYSIWYG 攻关失败的退路。
- **等待 GPUI mobile 成熟再统一迁**：时间不可控，且本 fork 无移动需求，无意义。

## Acceptance criteria

Phase 0 四个验证点全部通过（详见计划文档 §6 P0 行）：① Dock+文件树骨架可跑；② TextView 正确渲染含表格/图片/代码块的样例笔记；③ Editor 打开大文件的输入/滚动延迟与空载内存有实测数据且优于 Tauri 版同场景；④ wry 窗口加载现有 tldraw runtime 并完成一次"打开围栏→编辑→快照回写"闭环。任一点失败则回退混合壳方案或终止迁移。

## Risks

- WYSIWYG caret-reveal 装饰层在 gpui-component Editor 模型上实现受阻（🔴 唯一攻关点，P1 内设 IME 与块级两道检查点）。
- 上游 API churn 与 crates.io 发布停摆常态化 → 锁 lockfile、集中升级。
- gpui-wry experimental → Tauri 旧版长期保留作白板兜底。
