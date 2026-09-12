# ADR-0005：编辑器底座选 zorite/gpui-editor（GPL-3.0）

- 状态：已采纳（2026-08-25，P1）
- 关联：`docs/gpui-migration-plan.md` §3/§7、`gpui/README.md` P1 进度表、`gpui/crates/zorite/gpui-editor/`

## 背景

WYSIWYG 活编辑是整个迁移的最高风险点（原计划在 gpui-component Editor 上自研 caret-reveal 装饰层，spike 的 `live_md.rs` 就是该路线的产物）。P1 前调研了全部下游候选。

## 决策

采纳 zorite/gpui-editor 做笔记编辑器底座（git 依赖 @7915aad，2026-08-29 起 vendor 进 `crates/zorite/`）。

## 真实权衡

- **能力独占**（调研结论，其他候选均不满足）：caret-reveal WYSIWYG + 可变行高标题 + 块级 providers（图片/mermaid/transclusion）+ 表格单元格编辑 + wikilink/tag/属性面板 + spell-check 接口 + SelectionChanged 事件。
- **排除的候选**：Orion-Editor（已死）；iamnbutler/gpui-editor（纯代码编辑器）；gpui-component Editor + 自研 caret-reveal（`live_md.rs` 路线，风险最高且重复造轮子——该实验代码已从构建移除，保留在 git 历史）。
- **许可证代价**：GPL-3.0。自用不分发，可接受；许可证文件保留在 `crates/zorite/LICENSE-zorite`。若未来商业化需整体换底座或谈许可。
- **双栈验证**：zorite 本身就是 GPUI + 自有编辑器的双栈架构在生产运行，证明该路线可行。

## 后果

- 5MB/20.9 万行 debug 载入 91ms 过闸门；WYSIWYG 开关（`set_markdown_style` / `clear_markdown_style`）成为编辑围栏源码的官方途径。
- 只用 zorite 公开 API，不 hack 其内部（不变量已入 AGENTS.md）；升级随三方锁步（ADR-0002）。
- 已知边界随底座而来：中文斜体不可见、独立图片行光标穿越疑似上游 bug、auto-grow 无内滚动（宿主负责滚动容器）。
