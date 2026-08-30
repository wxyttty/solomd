# gpui — SoloMD GPUI 迁移工作区

Tauri → GPUI 迁移的 Phase 0 spike 与后续桌面端主工程。计划与决策见
[docs/gpui-migration-plan.md](../docs/gpui-migration-plan.md)。

## 依赖策略

zed@`f66ed39`（2026-08-29 gpui-component tip lockfile 实测 rev）+ gpui-component@`0e2fb7a`
+ zorite@`7915aad`，**后两者已 vendor 进本仓库**（`crates/gpui-component/` 五 crate、
`crates/zorite/` 三 crate，均为 path 依赖 + workspace 继承）。

- 为什么 vendor：git 依赖只有 spec 完全一致才会去重；zorite 仓库把整条链钉死在
  旧 rev（其 main tip 至 2026-08-29 仍钉 zed@1d217ee），gpc tip 又改成浮 pin
  （不写 zed rev），两者都无法与本工作区的 pin 去重。vendor 后 pin 决定权
  收回本仓库，升级不再等上游。
- 升级舞步（`gpui/Cargo.toml` 注释同步维护）：
  1. 取 gpui-component 新 tip，从其 Cargo.lock 读实际编译的 zed rev；
  2. 改根清单 `gpui`/`gpui_platform`/`gpui_macros`/`sum-tree` 的 rev；
  3. 更新 vendored gpc/zorite 源（照上游 diff）并修 API churn。
  实测（2026-08-29）：1505b14→0e2fb7a 共 226 commits，我们全量修复成本 =
  zorite 3 处 `paint_image` 新参数 + app/spike 十余处 dock/Input 重构
  （Panel trait 拆为 BasePanel+Panel、DockItem/TabPanel→DockLayout builder、
  InputState 多行改 TextareaState、DockSkin::dock_area）。
- 本机只有 CLT 无 Xcode，离线 metal 编译不可用 → `runtime_shaders` feature 把
  shader 编译挪到运行时（首次启动略慢）。

历史遗留：`Cargo.lock.pre-zorite.bak` 是切换前的锁文件备份（zed@693140a 组合）。

## 运行

```sh
cd gpui
cargo run -p spike -- dock       # 检查点① Dock + 文件树 + 笔记视图（默认）
cargo run -p spike -- markdown   # 检查点② TextView 渲染样例笔记
cargo run -p spike -- editor     # 检查点③ 大文件性能 harness（stdout 看 [perf]）
cargo run -p solomd-app -- _vault  # P2 应用骨架：文件树 + 多标签 + ⌘S/⌘N（日常记笔记）
cargo run -p spike -- wys        # P1 zorite gpui-editor 集成 + 5MB 性能闸门（stdout 看 [perf-zorite]）
cargo run -p spike -- webview    # 检查点④ tldraw 白板快照回写闭环（stdout 看 [ipc]/[fence]）
```

> 注：原 `live` 面板（自研 caret-reveal v2，tree-sitter-md 驱动，含单元测试）已从构建移除——
> 其职责由 gpui-editor 原生接管；代码保留在 git 历史。tree-sitter 依赖同步移除。

P4 起白板为原生 gpui-whiteboard（`crates/zorite/gpui-whiteboard`），app 不再使用
board-webview；该目录仅供 spike 检查点④（`cargo run -p spike -- webview`）复用，
tldraw 版本锁定 4.5.12（勿随意升级）。

## P0 检查点状态

| # | 检查点 | 状态 |
|---|---|---|
| ① | Dock + 文件树骨架 | 🟩 编译 + 启动冒烟通过；拖拽分割/标签交互待人工目检 |
| ② | TextView 渲染样例 | 🟩 编译 + 启动冒烟通过；渲染正确性待人工目检 |
| ③ | Editor 大文件性能实测 | 🟩 **5.0MB / 20.9 万行 → 载入 41ms**（debug）；手感待人工目检 |
| ④ | wry 挂 tldraw runtime 快照回写闭环 | 🟩 **全链路自动验证通过**：真实 tldraw@4.5.12 画板 → store 序列化 2095B 回传 → Rust 重写围栏（134→1981B），info 行逐字节保留 ✓ |

## P2 进度（crates/app，二进制 solomd）

| 项 | 状态 |
|---|---|
| M1 应用骨架 | 🟩 编译+冒烟通过：Vault（参数/缺省 `_vault`，空库自动建 welcome.md）+ FileTreePanel（点击发 OpenNote）+ NotePanel（gpui-editor + ⌘S + 脏态 • + Tab 关闭同步）+ Workspace（TabPanel 动态开关，activate 走 DockItem::active_index 侧通道）|
| 已修坑 | Vault read/write 未 resolve 相对路径（文件落 CWD）；TabPanel::set_active_ix 私有；panels 字段 pub(crate)（自维护有序 tabs 列表）|
| 交互补充（用户反馈修复） | 🟩 图片粘贴移植进 NotePanel（⌘V 位图/文件 → `_assets/`）；PreviewImage/OpenLink 事件接线（系统看图器/浏览器）；⌘W 关标签；最后标签关闭 → 自动回 welcome（dock 要求恒有面板，其余标签永远有 ✕）|
| 锁版交互代价 | 1505b14 无标签右键菜单/中键关闭（上游后续版本才有）；标签栏右侧 **⋯ 按钮**含 Close 菜单可用 |
| 外部变更监听 | 🟩 **完成并验证**：2s mtime 轮询（零依赖）；干净标签自动重载（`[sync]` 日志）、脏标签编辑器版本优先；文件树外部新增文件自动刷新。修复根因：Vault root 规范化为绝对路径使 resolve 幂等（曾因双重 join 产生 `_vault/_vault/` 嵌套、⌘S 写错位置） |
| ⌘P 笔记切换面板 | 🟩 自建浮层（1505b14 无 CommandPalette 组件）：半透明遮罩 + 居中卡片，输入过滤（大小写不敏感子串）+ 点击/Enter 打开首个匹配 + Esc/点遮罩关闭；Enter 走 InputEvent::PressEnter（subscribe_in） |
| 设置 + 深色主题 | 🟩 工具栏 ☀/🌙 切换：全局 Theme::change + 所有打开编辑器 SyntaxStyle 重涂（warm_dark 色板取自 tokens.css dark 段）+ 持久化 `<vault>/.solomd/settings.json`（SoloMD 目录约定）+ 启动恢复 |
| 后端迁移批次1（solomd-core） | 🟩 **9,100 行去 Tauri 化编译全绿**：EventSink/CorePaths 注入层 + ai_proxy/ai_keystore/ollama/agent_run/agent_tools/crypto/git_history/cost_meter/pricing/search/trace（原 164 command 壳全拆，emit→sink，spawn_blocking→tokio）；git_auto_commit 的 recipes 内联触发改为 git-committed 事件（组合层触发）|
| AI 面板 v1 | 🟩 最小聊天面：deepseek 硬编码 + 流式事件泵（channel+80ms 轮询，白板 IPC 同款）+ key 环 prime；provider 设置 UI 与工具调用卡片属 P3 |
| 后端迁移批次2 | 🟩 **+4.6K 行全绿（core 共 12,913 行 / 16 模块）**：github_sync（纯去壳）、convert（Pandoc 之外的 docx/xlsx/pdf/html 全格式，补 chardetng/encoding_rs/quick-xml/calamine/zip/csv/htmd/pdf-extract 依赖）、integrations（MCP 路径/Claude Desktop 配置，AppHandle→dirs::home_dir）、watcher（notify 防抖监听，emit→sink）；dev_bridge 暂缓（eval 驱动 Vue UI 无 GPUI 等价物，P3 重设计） |
| 后端迁移批次3 | 🟩 **+4.4K 行全绿（core 封顶：21 模块 / 17,324 行）**：recipes（零耦合直搬）、cjk_proofread（纯去壳）、recipe_runner（RecipesState→Lazy 全局、fire/run_recipe/cron_loop 全链 sink 化）、workspace_index（IndexCtx{sink,data_dir} 注入，缓存迁 data_dir）；补 chrono/cron/globset/hex 依赖。**后端迁移完成**（砍 capture/rest/dev_bridge 已按决策排除） |
| AI key 管理 | 🟩 Keys 右栏页：13 家 provider chips（● 已存/○ 未存）+ key 输入保存/清除；钥匙串与 Tauri 版互通（service="solomd"/user="ai-{provider}"，同机零迁移）；AI 助手/Keys 右栏分页切换 |
| 搜索右栏页 | 🟩 语义搜索（rag sqlite）优先、全文兜底（search_in_dir）；结果卡片（文件名+相关度/行号+摘要）；一键重建索引 |
| 同步右栏页 | 🟩 GitHub token 存钥匙串、remote 关联（github_link_workspace）、状态查询（ahead/behind/dirty）、⬆推送/⬇拉取（冲突列表展示） |
| Recipes 右栏页 | 🟩 列表（name/触发/摘要）+ ▶ 立即运行（recipes_run_now 全链 sink 事件泵）；空态引导写 YAML |
| 中文校对 | 🟩 NotePanel 工具行「中文校对」按钮：cjk_proofread 全量检测 → 编辑器波浪线诊断 + 计数提示 |
| tokio runtime 修复 | 🟩 **发现并修复迁移遗留的正确性缺口**：`tauri::async_runtime` → `tokio::task::spawn_blocking` 直译有隐含前提（Tauri 自带全局 runtime）；GPUI executor 线程上直调会 panic（sync 面板按钮/AI reqwest 流式均为潜伏崩溃）。新增 `core/rt.rs` 进程级 runtime（`rt::spawn_blocking`/`rt::spawn`，Handle 可任意线程调用），core 全部 ~40 处替换；宿主桥接模式 = `rt::spawn(fut).await`（JoinHandle 可在 GPUI executor 上 await）。core 测试套件修复编译阻塞（tempfile dev-dep、edition-2024 unsafe set_var）后 131/133 通过（2 个失败为迁移前遗留 fixture 缺失，与本修复无关） |
| 历史右栏页 | 🟩 当前笔记 per-file 提交列表（sha/相对时间/作者）+ 展开内联 diff（+/−着色）+ 恢复此版本（回写磁盘，干净标签 2s 轮询自动重载）+ 初始化 Git 仓库 + 提交当前文件；走 `git_history::*_inner` + background_executor |
| Pandoc 导出 | 🟩 工具栏「导出」浮层菜单（pandoc_detect 首开探测 + 版本/缺失提示）：DOCX/HTML/EPUB/ODT/RTF/LaTeX → `<vault>/exports/<stem>.<ext>`（core/pandoc.rs 自 src-tauri 去壳移植）；导出含未保存编辑（读编辑器缓冲） |
| AI 面板 v2 | 🟩 provider/model/base_url 设置区（10 家 provider chips + 输入行，持久化 settings.json）+ ai_verify_key 测试连接（rt 桥接）+ 多轮对话（history 累积）+「附带当前笔记」上下文开关 + 每条回复复制/插入笔记；key 仍在 Keys 页管理 |
| 活跃笔记跟踪 | 🟩 NotePanel::set_active 激活回调 → Workspace::note_activated（active_note/active_panel）→ 推送历史/AI/信息面板 |
| 信息右栏页（大纲/反链/标签） | 🟩 三合一内部 tab：大纲 = 活跃缓冲标题解析（跳过围栏）+ 点击跳光标；反链 = workspace_index（lazy init + watcher 增量 + index-updated 事件泵），来源文件+行号+上下文，点击跳行；标签 = 全库 tag 计数，展开看文件列表 |
| Agent 工具调用卡片 | 🟩 ai-panel 泵消费 ai-tool-call/ai-tool-result 事件：折叠卡（🔧/…/⚠ 状态），展开看参数与结果；工具续写文本自动开新块（saw_tools 区分 done 定稿策略）。core 侧 tools=None 默认发只读工具集（list_notes/read_note/search/get_backlinks/list_tags/get_outline/autogit_log/autogit_diff） |
| Recipes 可视化编辑 | 🟩 表单编辑：名称/触发 chips（手动/保存/提交/定时/打标签，按触发显隐字段）/提示词/provider/model/base_url/tools/写开关/write_cap；save_recipe 生成 canonical YAML（编辑回写源文件，改名保留原 slug），支持新建/删除 |
| 白板窗口（历史，P4 已原生替代） | 🟩 spike 检查点④产品化进 app：NotePanel「白板」按钮（无围栏自动文末建围栏）→ gpui-wry 独立窗口（Root + 标题栏）→ tldraw 防抖快照持续回写围栏体（内容去重 + info 行逐字节不变量校验，不过关不落盘）→ 落盘即存（脏态不受影响）。围栏解析升级：反引号长度自适应 |
| 原生菜单 | 🟩 macOS 菜单栏：SoloMD(退出)/文件(新建 ⌘N/保存 ⌘S/关闭标签 ⌘W)/编辑(撤销/重做/剪切/拷贝/粘贴/全选，直派 gpui_editor 动作)/查看(⌘P 面板/⌘⇧T 主题)；ToggleTheme 提为全局 action |
| 单实例 | 🟩 固定端口占位（127.0.0.1:45917），第二实例 bind 失败即退出 |
| dev_bridge v3（GPUI 重设计） | 🟩 debug-only 原生命令 API（替代 Vue 时代的 JS eval 通道）：GET /ping、GET /state、POST /open_note、POST /run_recipe；Bearer token，`dev-bridge.port`/`dev-bridge.token` 写 `~/Library/Application Support/solomd-gpui/`；std 线程 TCP（不经 rt），UI 动作经 channel 由 Workspace 泵执行。冒烟：ping/state/open_note 全链路 + 鉴权拒绝 ✓ |
| P3 待做 | 反链/大纲/标签面板 ✅、recipes 可视化编辑 ✅、Agent 工具卡片 ✅、白板 ✅、原生菜单/单实例 ✅、dev_bridge ✅ —— 剩余：打包签名（P4）、性能基线对照 |

## P1 进度

| 项 | 状态 |
|---|---|
| **底座切换决策** | 🟩 **采纳 zorite/gpui-editor（GPL-3.0）做笔记编辑器**：调研了全部下游候选后确认其能力独占（caret-reveal + 可变行高标题 + 块级 providers 图片/mermaid/transclusion + 表格单元格编辑 + wikilink/tag/属性面板 + spell-check 接口 + SelectionChanged 事件）；双栈架构由 zorite 本身验证；Orion-Editor 已死排除、iamnbutler/gpui-editor 为纯代码编辑器不满足 |
| 5MB 大文件硬闸门 | 🟩 **91ms 载入 / 20.9万行（debug）**，进程稳定；滚动手感待目检 + release 复测 |
| SyntaxStyle 主题化（warm design system） | 🟩 **完成**：`warm_theme.rs` 全量映射 25 个字段（品牌橙 #ff9f40 链接/高亮、--md-code 红棕行内代码、暖金 tag、alert 五色语义近似、popover 用 bg-elev/border）；`set_markdown_style` 即开即用，不在载入热路径 |
| 目检模式 | 🟩 `wys` 默认小样例（标题/粗斜体/删除线/代码/链接/引用/任务列表/围栏）；`SPIKE_BIG=1 cargo run -p spike -- wys` 切 5MB 性能复测 |
| Block providers 接线 | 🟩 **完成**：图片 provider（同步解码 + 1024px 缩图 + BGRA 转换 + 防炸弹 limits，相对路径基于工作区根）；mermaid provider 采用 **zed 同款 mermaid-rs-renderer**（替代原计划的 ariel-rs+resvg，zorite 生产验证）→ gpui SvgRenderer 栅格化，off-thread 状态机驱动；demo 含真实图片与 flowchart |
| EditorEvent 消费 | 🟩 PreviewImage（系统看图器）/ OpenLink（系统浏览器）/ OpenWikiLink（日志占位）已接线 |
| 图片粘贴 | 🟩 宿主侧 ⌘V handler（zorite 同款双键位模式）：剪贴板位图/拷贝文件 → 存 `_assets/` → 插入 markdown；无图时 propagate 给文本粘贴 |
| WYSIWYG 开关 | 🟩 工具栏按钮：clear/set_markdown_style 切换源码模式（官方 sanctioned 的图片源码编辑方式） |
| 交互语义澄清 | 独立图片行 = **Word 式原子对象**：光标只停左右两侧、不显示源码（by design）、⌫ 整图删除、右键菜单删除、右下角柄拖拽缩放；行内图片（与文字混排）= 缩略图可点击预览、光标可穿越、光标进入显源码 |
| 疑似上游 bug | 独立图片行上左右方向键无法从一侧穿越到另一侧（zorite demo 也未覆盖此场景）；待向 zorite 报告；临时用 WYSIWYG 开关编辑源码 |
| 中文 IME 组词验证 | ⬜ 待人工 |
| 已知边界 | gpui-editor auto-grow 无内滚动（宿主负责滚动容器）；gpui-component 回退两个月损失部分修复；**中文斜体不可见**：CJK 字体无斜体字面且 GPUI 不做合成倾斜（拉丁文正常，机制已验证），浏览器会合成而原生不会——如需补偿可 fork 给 italic run 叠色相偏移 |

④ 的实现：`board-webview/` 内逐字拷贝 app 的 `tldraw-runtime.ts`/`tldraw-board.ts`/
`tldraw-markdown.ts`（零改动），入口 `src/main.ts` 做 IPC 胶水；esbuild 打包为单文件
iife（1.9MB JS + 77KB CSS），经 `include_str!` 嵌入二进制后由 `with_html` 加载。
Rust 侧 ipc 回调只投 channel，GPUI 实体内的异步泵消费消息并驱动闭环。
待办（P1）：移植 `fenceLengthForSnapshot` 反引号长度自适应与容错属性解析；
快照防抖写回策略；关闭窗口时销毁 webview 释放内存的验证。

## P4 进度

| 项 | 状态 |
|---|---|
| 白板原生化 | 🟩 vendor zorite@2dab499 gpui-whiteboard（零 churn）；tldraw webview 从 app 白板路径移除，围栏 `​```tldraw` → `​```whiteboard`（Scene JSON）；ChangeFn 每手势回写落盘 |
| 数学渲染 | 🟩 vendor zorite ratex-gpui（RaTeX 引擎，纯 Rust、KaTeX 字体内嵌）；`gpui_editor::math_sources` + `inline_math_sources` 开窗热身，off-thread `render_latex` → MathStore 缓存（MermaidStore 同款管线），editor `set_block_math_provider` + `set_block_math_em`（行内 `$…$` 复用块级光栅缩放显示） |
| 笔记滚动容器 | 🟩 NotePanel 编辑器包 `track_scroll` + `overflow_y_scroll`（此前编辑器无滚动宿主）；`set_scroll_compensator` 滚动锚定——异步块在视口上方渲染完成改变高度时吸收偏移，阅读位置不动 |
| 打字机模式 | 🟩 工具行「打字机」开关（Settings.typewriter 持久化）；SelectionChanged → caret_screen_bounds/window_top/content_height 换算光标内容 y → 滚动容器居中（视口高按窗口高扣 72px chrome 估算；vendored gpui-editor 加 `window_top`/`content_height` 两个访问器） |
| 结构化公式编辑 | 🟩 note_panel 拆目录 + math_edit.rs 会话模块（zorite app/math.rs 移植）：光标进入公式（EditMath）→ MathEditor 塞进编辑器 editing 槽位（块级全宽空隙/行内原位覆盖）→ blur/方向键越界提交 → `replace_range` 回写（可撤销）+ 落盘 + 单条公式热身重渲；右键公式复制 LaTeX（v1，对齐/导出菜单后置）；简化项：RTL 出向、公式上方无内容守卫未移植 |
| 待人工 | 数学：排版保真度对照（生僻宏/化学式 mhchem 类预期缺）、中文 IME 下公式附近编辑、**MathEditor 结构化编辑全流程**（进入/编辑/回写/撤销/重渲）；打字机：滚动跟随手感、拖拽选区时的居中策略（现版会随选区移动）；滚动容器：长文滚动手感 + 滚动条 UI（现为裸滚动，无 thumb） |

## 性能基线记录

| 场景 | 数据 | 备注 |
|---|---|---|
| Editor 载入 5.0MB / 209,078 行 markdown | 41ms | debug 构建，release 更快；生成内容耗时 4ms 另计 |
| 空载内存 / 冷启动 / 滚动帧率 | 待测 | 与 Tauri 版同场景对比是 P0 收尾的 go/no-go 输入 |
