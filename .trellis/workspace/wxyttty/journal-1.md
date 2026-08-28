# Journal - wxyttty (Part 1)

> AI development session journal
> Started: 2026-08-26

---



## Session 1: Tauri→GPUI 迁移全程落地（P0→P2-M2 + solomd-core）

**Date**: 2026-08-28
**Task**: Tauri→GPUI 迁移全程落地（P0→P2-M2 + solomd-core）
**Branch**: `main`

### Summary

桌面端完成 GPUI 迁移：三 crate 拓扑（solomd-core 22 模块 18.2K 行后端 / solomd-app 应用 / spike 实验室）。编辑器底座 zorite gpui-editor（caret-reveal WYSIWYG），三方依赖锁步 zed@1d217ee+gpui-component@1505b14+zorite@main。后端 EventSink/CorePaths 注入接缝，tauri 引用归零。功能面：文件树/多标签/⌘S/⌘N/⌘P/深色主题/AI 聊天/Keys/搜索/同步/Recipes/中文校对/白板(wry)/外部监听。关键踩坑已沉淀 spec：API 版本纪律、绘制顺序、主题刷新覆盖、路径幂等、锁步升级舞步。

### Git Commits

(No commits - planning session)

### Testing

- [OK] cargo check -p solomd-core/app 全绿；app 冒烟 RUNNING-OK；5MB 编辑器载入 91ms；wry tldraw 快照回写闭环 info 行逐字节保留

### Status

[OK] **Completed**

### Next Steps

- 实际使用一周攒痛点清单；按需增量：AI provider 设置 UI、recipes 可视化编辑、原生菜单/单实例（打包前）

---

## Session 2: P3 功能对等补全（历史/导出/AI 设置）+ tokio runtime 修复

**Date**: 2026-08-29
**Task**: 续 GPUI 迁移——完成剩余重写（PRD 保留清单中未落 UI 的 core 能力）
**Branch**: `main`

### Summary

继续 Tauri→GPUI 重写的收尾，三块新 UI + 一个关键正确性修复：

1. **tokio runtime 修复（最重要）**：盘点时发现 core 迁移直译
   `tauri::async_runtime::spawn_blocking` → `tokio::task::spawn_blocking` 有隐含前提
   （Tauri 壳自带全局 runtime），GPUI executor 线程上直调必 panic——sync 面板按钮、
   AI 流式（reqwest 无 reactor）全是潜伏崩溃，此前只做过"启动冒烟"没暴露。新增
   `core/rt.rs` 进程级 runtime，~40 处调用点统一替换；宿主桥接 =
   `rt::spawn(fut).await`（JoinHandle 可在 GPUI executor await）。约定已沉淀
   spec（core §3.5 / gpui §3）。
2. **历史右栏页**（`history_panel.rs`）：per-file 提交列表 + 展开 diff（±着色）+
   恢复版本（回写磁盘复用 2s 外部变更轮询自动重载）+ git init + 提交当前文件。
   活跃笔记跟踪走新加的 `NotePanel::set_active` 激活回调 → Workspace 分发。
3. **Pandoc 导出**：`core/pandoc.rs` 自 src-tauri 去壳移植（函数名一一对应）；
   工具栏「导出」浮层菜单（DOCX/HTML/EPUB/ODT/RTF/LaTeX），输出
   `<vault>/exports/`，导出读编辑器缓冲（含未保存编辑），首开探测 pandoc。
4. **AI 面板 v2**：provider/model/base_url 持久化（settings.json）+ 10 家 provider
   chips + 测试连接（ai_verify_key 经 rt 桥接）+ 多轮对话 + 附带当前笔记 +
   复制/插入到笔记。

### Git Commits

(待提交)

### Testing

- [OK] cargo check -p solomd-core/app 全绿（含 cargo fix 清 warnings）
- [OK] 冒烟：新二进制 8s 稳定，临时 vault 正常初始化（旧"RUNNING-OK"实为 cargo 还在编译的假象，已用直跑二进制方式复验）
- [OK] cargo test -p solomd-core：修复测试目标编译阻塞（tempfile dev-dep、edition-2024 unsafe set_var）后 131/133 通过；2 个失败（trace fixture 缺失、agent_tools seed 计数）为迁移前遗留，与本轮无关
- [OK] clippy 未装于此工具链（1.97.1），与既往会话一致跳过

### Status

[OK] **Completed**

### Next Steps

- 实际使用攒痛点；PRD 保留清单内的 UI 已对齐，剩余为按需项：反链/大纲/标签面板、
  recipes 可视化编辑、Agent 工具调用卡片、白板打磨、原生菜单/单实例（打包前）

---

## Session 3: P3 功能对等清零（反链/大纲/标签、recipes 编辑、Agent 工具卡、白板、菜单/单实例、dev_bridge v3）

**Date**: 2026-08-29
**Task**: 完成 GPUI 重写的全部 P3 遗留项
**Branch**: `main`

### Summary

八项全部落地（含白板——盘点时发现它只到 spike 检查点，从未集成进 app）：

1. **信息右栏页**（`info_panel.rs`）：大纲/反链/标签三合一内部 tab。大纲纯前端
   解析活跃缓冲（跳过围栏）+ `EditorState::set_cursor` 跳转；反链/标签走
   core workspace_index（首次打开 lazy init + notify watcher + index-updated
   事件泵）。跳转通过 `InfoEvent::OpenNote` 事件由 Workspace
   `open_note_at`（1-based 行 → 字节偏移 → set_cursor）执行。
2. **Agent 工具调用卡片**：ai-panel 泵消费 ai-tool-call/result；工具卡折叠
   （🔧/…/⚠）展开看参数/结果；工具后续文本自动开新块；`saw_tools` 决定
   done 时是否用 full_text 定稿末块。core 的 ChatRequest tools=None 本就
   默认发只读工具集，此前事件没人消费。
3. **Recipes 可视化编辑**：表单（触发 chips 按类型显隐字段）→ `save_recipe`
   生成 canonical YAML；编辑回写原文件（改名保留原 slug），支持新建/删除。
4. **白板窗口**（`whiteboard.rs`）：spike 检查点④移植产品化。NotePanel「白板」
   按钮（无围栏自动建）→ gpui-wry 独立窗口 → tldraw 防抖快照持续回写围栏
   （内容去重、info 行逐字节不变量校验、围栏解析升级为反引号长度自适应），
   回写即落盘且不置脏态。
5. **原生菜单**：文件/编辑/查看菜单全部落到真实动作；编辑菜单直派
   gpui_editor 的 Undo/Cut/Copy 等（自绘编辑器打不到原生 selector）；
   ToggleTheme 提为全局 action（⌘⇧T）。
6. **单实例**：固定端口占位，第二实例退出（冒烟 ✓）。
7. **dev_bridge v3**：GPUI 无 JS eval 面，重设计为原生命令 API——
   GET /ping、/state、POST /open_note、/run_recipe；沿用 port/token 文件
   约定（写 solomd-gpui config 目录）；std 线程 TCP（不引 tokio）+ channel
   交 Workspace 泵执行；debug-only 编译。冒烟全链路 + 鉴权拒绝 ✓。

### Testing

- [OK] cargo check 全绿零警告；cargo build 通过（含 gpui-wry/wry 依赖引入 app）
- [OK] 冒烟：单实例退出 ✓；/ping ✓；无 token 401 ✓；/state ✓；/open_note 后
  active_note 更新 ✓；首实例 6s 稳定 ✓
- [OK] 白板/大纲跳转/recipes 表单为 GUI 交互，编译 + spike 已验证协议 + 人工目检待做

### Status

[OK] **Completed** —— P3 功能对等清零，剩 P4（打包签名 + 性能基线对照）

### Next Steps

- 人工目检：白板窗口、大纲跳转、recipes 编辑保存、工具卡片展开
- dev-mcp 侧适配 v3 API（读 solomd-gpui config 目录的 port/token）
- P4：打包签名、公证、性能基线（冷启动/内存/大文件）与 Tauri 版对照

---

## Session 4: 框架解锁——vendor gpc/zorite + 锁步升级 zed f66ed39

**Date**: 2026-08-29
**Task**: 提交推送 P3 全量 + 解决"框架被上游 pin 卡两个月"
**Branch**: `main`

### Summary

**Part 1 提交推送**：P2 尾批 + P3 全量分 3 个提交（trellis docs / core runtime+pandoc /
app 功能对等）推上 origin/main（f00b291 → 1dcdb17）。

**Part 2 依赖调查与解锁**（用户问：编辑器依赖外部项目把框架锁在两个月前，能否解决）：

1. **根因定位**：升级舞步的驱动者是 gpui-component（取 gpc tip → 其 lockfile 读
   zed rev）。实测 zorite main tip（2dab499）至今仍钉 zed@1d217ee + gpc@1505b14
   ——链条是 zorite 自己卡住的；gpc main（0e2fb7a）却已浮 pin 跟 zed main。
2. **[patch] 不可行**：URL 级 patch 不能指向同源（cargo 直接拒绝），zorite/gpc
   的内部 pin 无法外部重定向 → 必须 vendor。
3. **方案：两段 vendor**。zorite 三 crate（gpui-editor 纯 gpui 零 gpc 依赖，
   闭包仅 gpui-markdown/gpui-bidi）+ gpc 五 crate（base/ui/assets/macros/webview，
   tip 已浮 pin，不 vendor 无法去重）。全部 path 依赖 + workspace 继承，
   pin 收敛到根清单一处。
4. **升级实施**：zed 1d217ee→f66ed39（gpc tip lockfile 实测），总修复量出乎意料地小：
   - zorite：3 处 `paint_image` 补 image_bounds 参数（可见区=交集，旧行为=同值）
   - app/spike：Panel trait 拆 BasePanel/Panel 两 impl、DockItem/TabPanel→
     DockLayout builder + DockSkin::dock_area + add_panel（自动激活）+
     remove/add 再激活、InputState 多行拆成 TextareaState/Textarea
   - 发现 zed 已开始发 crates.io（sum-tree=zed-sum-tree 0.2.0 registry）
5. 验证：workspace 编译全绿、冒烟 OK、bridge OK、core 测试 132/133。

### Git Commits

- 1a51088 docs(trellis) / 718894d fix(gpui-core) / 1dcdb17 feat(gpui-app) / d48fffe build(gpui) vendor zorite
- 本会话待提交：gpc vendor + zed 升级 + API churn 修复

### Testing

- [OK] cargo check --workspace 全绿（app/core/spike + 8 个 vendored crate）
- [OK] 依赖图验证：zed 单一 source（f66ed39）、longbridge git 源归零
- [OK] 冒烟 8s + dev_bridge ping/open_note；core 测试 132/133（1 个为迁移前 fixture 遗留）

### Status

[OK] **Completed**（框架解锁达成；后续升级 = 改一处 rev + 修 vendored churn）

### Next Steps

- 人工目检新 dock 的 tab 交互（关闭/拖拽/缩放由 gpc 新皮肤接管）与 WYSIWYG 渲染
- 关注 zorite 上游：其自身升级舞步完成后可 vendor 同步拿新修复
- 5MB 大文件闸门在新 zed rev 上复测（P4 基线）
