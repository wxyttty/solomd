# Directory Structure（solomd-core）

> solomd-core 的模块组织。完整 crate 拓扑见 frontend/directory-structure.md。

---

## 活跃代码

```
gpui/                          # GPUI 工作区（独立 cargo workspace）
├── Cargo.toml                 # 三方锁步 pin + 工具链注释（改依赖先读）
├── rust-toolchain.toml        # 1.97.1 稳定版（禁 nightly）
├── crates/
│   ├── core/                  # solomd-core：后端业务库（UI 无关）
│   │   └── src/
│   │       ├── deps.rs        # EventSink / CorePaths / ChannelSink（唯一接缝）
│   │       ├── ai_proxy.rs    # AI 流式（14 provider）
│   │       ├── agent_run.rs / agent_tools.rs / trace.rs
│   │       ├── github_sync.rs / git_history.rs / crypto.rs
│   │       ├── rag.rs / workspace_index.rs / search.rs
│   │       ├── recipes.rs / recipe_runner.rs
│   │       ├── cjk_proofread.rs / convert.rs / integrations.rs / cost_meter.rs
│   │       └── ollama.rs / pricing.rs / ai_keystore.rs
│   ├── app/                   # solomd 二进制（GPUI）
│   │   └── src/
│   │       ├── main.rs        # 入口：vault 参数、键位、主题恢复
│   │       ├── workspace.rs   # DockArea + 标签注册 + ⌘P + 右栏分页 + 外部监听
│   │       ├── note_panel.rs  # 笔记标签（编辑器 + ⌘S + 粘贴 + 校对）
│   │       ├── file_tree.rs   # 文件树（OpenNote 事件）
│   │       ├── ai_panel.rs / keys_panel.rs / search_panel.rs
│   │       ├── sync_panel.rs / recipes_panel.rs
│   │       ├── theme.rs       # SyntaxStyle 明/暗暖色板
│   │       ├── images.rs / mermaid.rs  # 块级 providers 宿主侧
│   │       └── vault.rs       # fs 操作 + mtime 轮询
│   └── spike/                 # P0 实验室（性能基线回归用，四面板入口）
└── board-webview/             # 白板前端（tldraw runtime + esbuild → 单文件）
```

## 迁移对应关系（旧 → 新）

| 旧（Tauri） | 新（GPUI） |
|---|---|
| `app/src-tauri/src/*.rs` | `gpui/crates/core/src/*.rs`（同名模块） |
| `app/src-tauri/src/commands.rs` | 壳废弃；GPUI 直接调 core 函数 |
| `app/src/lib/cm-live-*.ts` | zorite gpui-editor 内建 |
| `app/src/lib/tldraw-*.ts` | `gpui/board-webview/src/`（逐字拷贝） |
| `app/src/components/*.vue` | `gpui/crates/app/src/*_panel.rs` + workspace.rs |
| `app/src/styles/tokens.css` | `gpui/crates/app/src/theme.rs` |

## 约定

- 新后端功能 → core 加模块（UI 无关），UI 面板放 app
- 文件名与职责一一对应；不建 utils.rs 杂物抽屉
- spike 是实验区：验证过的增量才进 app
