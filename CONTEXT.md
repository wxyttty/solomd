# SoloMD 领域词汇表

> 纯词汇表：术语 → 含义。规范与不变量在 `AGENTS.md`，决策记录在 `docs/adr/`。

| 词汇 | 含义 |
|---|---|
| vault | 笔记库根目录（缺省 `_vault/`）；配置、索引、导出全部随 vault 走，落在 `<vault>/.solomd/` |
| EventSink / EventSinkExt | core → UI 的事件出口接缝 trait（`gpui/crates/core/src/deps.rs`）；Ext 独立成 trait 是为保 `dyn` 兼容 |
| ChannelSink / NullSink | EventSink 的两个实现：mpsc channel（UI 泵消费）/ 丢弃（dev_bridge 的 run_recipe 等不需要回调的场景，`workspace.rs:490`） |
| CorePaths | core 的路径注入（config_dir），替代原 tauri 的 `app_config_dir` |
| 泵（事件泵） | UI 实体内 `cx.spawn` 轮询 mpsc channel（timer 80~300ms + `try_recv` drain）把后台结果落回 UI 状态的标准模式 |
| 锁步 | zed + gpui-component + zorite 三方必须统一在同一个 zed rev 上，否则 git 依赖不去重、双 gpui 依赖图编译失败 |
| vendor | 把 git 依赖收进本仓库改 path 依赖（`crates/gpui-component/`、`crates/zorite/`），收回 rev 决定权 |
| spike | 实验室 crate（`gpui/crates/spike`）：Dock/markdown/editor/wys/webview 五面板入口，性能基线回归用；验证过的增量才进 app |
| provider | AI 供应商。词表归一见 `ai_proxy.rs::resolve_provider`：`local`→ollama，`llama.cpp`/`LM Studio`/`vLLM`/`custom`→openai-compat；key 管理 13 家 + ollama（keyless）共 14 家（`keys_panel.rs::PROVIDERS`）；AI 面板芯片 10 家（`ai_panel.rs::PROVIDERS`，另一套清单） |
| keyless provider | 无账号供应商（ollama、openai-compat），不强制 key、无 key 不发 Authorization 头（`ai_proxy.rs::is_keyless_provider`） |
| recipe | `<workspace>/.solomd/agents/*.yml` 声明式自动化配方：触发（手动/保存/提交/定时/打标签）+ 提示词 + provider + 工具集 |
| AutoGit | agent 运行的沙箱机制：每次跑在独立分支 `agent/<slug>/<run-id>` 上，写入不碰 main，用户点 Accept（`recipes_accept_run` → merge_branch_into_main）才合并 |
| write-cap | agent 单次运行的写操作配额，默认 5、硬上限 50（`recipes.rs::WRITE_CAP_MAX/DEFAULT`）；读工具免费 |
| 围栏（fence） | markdown 代码围栏；特指可执行的 ```mermaid / ```whiteboard 块，体 = 渲染源或 Scene JSON |
| 白板（whiteboard） | 原生白板独立窗口（`gpui-whiteboard` crate 承载，P4 替代 tldraw webview）；围栏体 = Scene JSON，每手势回写落盘（`whiteboard.rs`） |
| 脏态 • | 未保存标记，拼在标签 title 前缀（`note_panel/mod.rs:372` `fn title`） |
| WYSIWYG 开关 | `set_markdown_style`（活渲染）/ `clear_markdown_style`（源码模式）运行时切换；源码编辑图片/围栏的官方途径 |
| _inner 函数 | 同步纯函数形态（如 `git_auto_commit_inner`），供 GPUI `background_executor` 直调；异步薄入口包一层 rt/spawn |
| rt | core 的进程级 tokio runtime（`core/src/rt.rs`）：GPUI executor 无 tokio 上下文，core 内 spawn 一律走它 |
| RightTab（右栏页） | Workspace 右侧 400px 分页：AI / Keys / 搜索 / 同步 / Recipes / 历史 / 信息（`workspace.rs::RightTab` 枚举） |
| OpenNote | 面板 → Workspace 的打开笔记导航事件（`file_tree.rs::OpenNote`、`info_panel.rs::InfoEvent::OpenNote`） |
| _assets/ | 图片粘贴落盘目录（vault 根下），⌘V 位图/文件自动存入并插入 markdown |
| 钥匙串约定 | AI key 存系统钥匙串：service=`solomd`、user=`ai-{provider}`；与旧 Tauri 版同机互通 |
| 中文校对 | `cjk_proofread::proofread` 全量检测 → 编辑器波浪线 diagnostics（`note_panel/mod.rs`）；右键建议菜单需另接 `on_suggest` provider，目前未接 |
| 打字机模式 | 光标行垂直居中滚动（SelectionChanged 驱动，`settings.rs` `typewriter` 字段持久化） |
| 滚动锚定 | `set_scroll_compensator`：异步块（mermaid/数学）在视口上方渲染完成改变高度时，滚动容器吸收偏移、阅读位置不动 |
| 结构化公式编辑 | 光标进入公式（EditMath 事件）→ MathEditor 塞进编辑器 editing 槽位 → blur/越界提交回写（可撤销）+ 重渲（`note_panel/math_edit.rs`） |
| dev_bridge | debug-only 原生 HTTP 调试 API（ping/state/open_note/run_recipe，Bearer token），替代 Vue 时代的 JS eval 通道（`app/src/dev_bridge.rs`） |
| 单实例 | 固定端口 127.0.0.1:45917 占位，第二实例 bind 失败即退出（`single_instance.rs`） |
