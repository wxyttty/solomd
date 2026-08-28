# GPUI 桌面版迁移（Tauri→GPUI）记录与规范沉淀

## Goal

把 SoloMD 桌面端从 Tauri 2 + Vue3 + CodeMirror 6 迁移到 GPUI（Zed）+ zorite gpui-editor +
gpui-component，换取原生性能与低占用。本任务为**完成记录 + 规范沉淀**（工作已落地，
代码在 `gpui/` 目录：solomd-core + solomd-app + spike 实验室）。

## 背景与范围决策（用户确认）

- 本仓库为**自用 fork**：砍移动端、砍 vim、砍数学公式（KaTeX/RaTeX 管线）
- 保留：白板（tldraw 经 gpui-wry 独立窗口）、mermaid（mermaid-rs-renderer）、
  AI 全家桶、Git 历史 + GitHub 同步、语义搜索、recipes、cjk 校对、Pandoc 导出
- 砍除：capture_endpoint、rest_api、Android/Windows 平台层；dev_bridge 暂缓重设计
- i18n：仅中文硬编码，不做 i18n 层

## 完成状态（全部已验证）

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 spike | 四检查点（Dock/TextView/Editor 5MB 91ms/wry+tldraw 回写闭环） | ✅ |
| P1 编辑器 | zorite gpui-editor 底座 + SyntaxStyle 主题 + providers + EditorEvent | ✅ |
| P2-M1 | vault + 文件树 + 多标签 + ⌘S/⌘N + 外部变更监听 | ✅ |
| P2-M2 | ⌘P 切换 + 深色主题 + AI 聊天 + Keys + 搜索 + 同步 + Recipes + 中文校对 | ✅ |
| 后端 | solomd-core 22 模块 / 18.2K 行，tauri 引用归零 | ✅ |

## Acceptance Criteria

- [x] `gpui/crates/core` 编译全绿，无 tauri 代码依赖（仅注释提及历史）
- [x] `gpui/crates/app` 冒烟运行正常，日常记笔记闭环可用
- [x] GPUI 开发规范沉淀到 `.trellis/spec/backend/` 与 `.trellis/spec/frontend/`
- [x] 关键决策与踩坑记录在案（本目录 design.md + gpui/README.md + docs/gpui-migration-plan.md）

## Notes

- 详细计划与证据链：`docs/gpui-migration-plan.md`
- 运行/构建说明：`gpui/README.md`
- 决策记录：`.agents/notes/proposed/architecture/2026-08-25-tauri-to-gpui-migration.md`
