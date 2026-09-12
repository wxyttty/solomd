<!-- MINI-TRELLIS:START -->
# Mini-Trellis 工作约定

本仓库使用 mini-trellis 薄壳工作流（skills：shape / verify / finish）。

## 三档模式（接到开发类请求先定档；Direct 是默认）

- **Direct**：明确、局部、低风险 → 直接实现，完成后 verify。零提问、零任务文件。
- **Shaped**：存在 1~5 个影响方向的决定（用户可见行为 / API 契约 / 数据模型 / 安全边界 / 不可逆架构 / 范围）→ 先 shape，产出 `.mini/tasks/current.md`，再实现、verify。
- **Structured**：跨会话 / 多个独立可交付物 / 大迁移 → shape 后升级为 spec + tickets（to-spec / to-tickets 类技能），不在单会话里硬扛。

判定权在模型；用户说"正式点 / 走流程"即升档，说"直接做"即降档。

## 提问预算（shape 内执行）

只问同时满足：无法从代码库推断 · 非 agent 可自主决定的实现细节 · 猜错会返工或改变上述六类边界 · 对本任务 blocking。
预算：tiny 0 / normal 3 / high-risk 5 次用户决策；耗尽后采用推荐答案写入 TASK.md 的 Assumptions 段继续开工。
唯一例外：destructive / security / data-loss 必须问。

## 两条铁律

1. 改代码前：查看最近的同类实现、相关测试、CONTEXT.md 与相关 ADR；不问仓库能回答的问题。
2. 报告完成前：verify（对照 TASK.md 的 Acceptance + lint / typecheck / tests）必须先过。

## 持久化唯一标准

只有"再发现成本 > 维护成本"的知识才落盘：词汇 → CONTEXT.md，不变量 → 本文件，难逆转决策 → docs/adr/。其余留在代码、测试与 git history。
<!-- MINI-TRELLIS:END -->

# SoloMD 编码规范与不变量

> 桌面端 = `gpui/` 独立 cargo workspace（solomd-core 后端 + solomd-app GPUI 应用 + spike 实验室）。旧 Tauri/Vue（`app/`）保留为参照与白板资产来源，不再演进。词汇表见 `CONTEXT.md`。

## 架构铁律

- **core 不认识 UI**：`gpui/crates/core/` 禁止依赖 gpui/tauri/任何 UI crate；与外界唯一接缝是 `core/src/deps.rs` 的 `EventSink`（事件出口）+ `CorePaths`（路径注入）。禁止 core 内 `std::process::Command::new("open")` 打开 URL/文件（属 UI 职责；`ollama.rs:402` 是历史例外且 GPUI 版已无调用方，勿新增）；禁止用全局单例传本应注入的上下文（`RECIPES_STATE` 是历史迁移产物，新代码用参数注入）。
- **core 内 spawn 一律走 `crate::rt`**（`core/src/rt.rs`）：宿主是 GPUI executor（无 tokio 上下文），直调 `tokio::spawn` / `tokio::task::spawn_blocking` 会 panic（sync 面板、AI 流式都中过招）。UI 侧桥接 tokio 依赖 future：`cx.spawn(async move { solomd_core::rt::spawn(async { … }).await })`（`ai_panel.rs:301` 同款）。纯同步 CPU 密集活（git2/rusqlite/pandoc）优先 `_inner` 函数 + `background_executor().spawn`（`history_panel.rs` 全套同款）。
- **事件是通知不是请求**：需要返回值走函数返回值。core 内模块零横向依赖——跨模块触发用事件解耦，组合层（UI）监听后再调目标模块。例：`git_auto_commit` 发 `git-committed` 事件，recipes 的 on-commit 由组合层监听后调 `recipe_runner::dispatch_on_commit`（`core/src/git_history.rs:438`）。topic 用短形式（原 `solomd://ai-chunk` → `ai-chunk`），payload 为 JSON。
- **文件名与职责一一对应**；不建 utils.rs 杂物抽屉；新后端功能进 core 模块（UI 无关），UI 面板放 app。

## 依赖与工具链

- **三方锁步**：zed + gpui-component + zorite 必须同一 zed rev（当前 zed@f66ed399 ← gpc@0e2fb7a ← zorite@7915aad，后两者已 vendor 为 path 依赖）。pin 只在 `gpui/Cargo.toml` 一处；升级舞步见 `gpui/README.md` 依赖策略节。只改一处 rev = 锁步断裂 → 双 gpui 图编译失败。
- **工具链 1.97.1 稳定版**（`gpui/rust-toolchain.toml`），禁 nightly（pathfinder_simd SIMD intrinsics 编译失败）。
- 本机无 Xcode：`gpui_platform` 必须带 `runtime_shaders` feature。
- **写 gpui-component API 前对照 vendored 源码真实签名**，不凭记忆/文档：`rg -n "pub fn xxx" gpui/crates/gpui-component/{base,ui}/src/…`。

## GPUI 已踩坑（勿重复）

- **Panel trait 拆两层**：`BasePanel`（ui crate 里 `pub use gpui_base::dock::Panel as BasePanel`：panel_name/set_active/on_removed）+ `Panel`（ui：title 等表现层），实现要拆成两个 impl 块（`note_panel/mod.rs:350,372` 同款）；把 BasePanel 方法写进 `impl Panel` 会 E0407。
- **DockItem/TabPanel 已删**：用 `DockSkin::dock_area(id, ver, window, cx)` 建区、`DockLayout::tabs().panel(entity)` 组布局、`area.set_center/set_dock/set_dock_size`、`area.add_panel(panel, placement, …)`（自动激活）。已开面板再激活 = `remove_panel` + `add_panel`（remove 触发 on_removed，注意宿主侧列表同步，`workspace.rs:325`）。
- **InputState 单行 / TextareaState 多行是两个类型**（`InputBaseState<InputMode>` vs `<TextareaMode>`）：多行用 `TextareaState::new(...).auto_grow(4,16)` + `Textarea::new(&state)`（`recipes_panel.rs:78,81`），没有 `InputState::multi_line(true)`。
- `AsyncWindowContext` **没有 `notify()`**——用 `update_in(cx, |panel, window, cx| { …; window.refresh(); })`（`sync_panel.rs:74` 同款；window 是闭包参数，外层方法的 window 不能 move 进 async 块）。
- `Window::paint_image` 7 参（bounds/image_bounds/corner_radii/data/frame_index/grayscale，zed f66ed39 签名）：可见区 = bounds∩image_bounds；旧行为等价于前两者相同。
- `InputState::set_value("", window, cx)` 要 window；`InputState::cursor()` 不要（`gpui-component/base/src/input/base/state.rs:830,2074`）；`Context::spawn(async move |this, cx|)` 里 `this.upgrade()` 返回 **Option**。
- gpui-component 无 prelude：`FluentBuilder` 从 `gpui::prelude` 导入（`keys_panel.rs:6`）；`on_click`/`overflow_y_scroll` 需要元素先 `.id()`（`StatefulInteractiveElement`）。
- **绘制顺序 = child 顺序：浮层必须是最后一个 child**，否则被后画的不透明区域盖住（⌘P 事故，`workspace.rs:946` 注释）。
- **`Root::new(...).bg(具体色)` 会以 refinement 覆盖主题每帧刷新** → 切主题残影。创建 Root 不传 bg（正确：`app/src/main.rs:106`；反例勿抄：`spike/src/main.rs:107`）。
- 自绘元素必须每帧读 `cx.theme()`，禁止硬编码色值（工具栏白底白字事故）；hover 高亮用 `cx.theme().accent.opacity(0.12)`（深浅色通用）。

## 异步后台 → UI（泵模式）

```rust
// core 侧：EventSink → mpsc channel
let (tx, rx) = std::sync::mpsc::channel();
let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(tx));
// UI 侧：实体内 spawn 轮询泵（workspace.rs / ai_panel.rs / info_panel.rs / recipes_panel.rs 四处复用）
cx.spawn(async move |this, cx| loop {
    cx.background_executor().timer(Duration::from_millis(80..300)).await;
    let Some(e) = this.upgrade() else { break };
    e.update(cx, |panel, cx| { /* drain rx → 改状态 → cx.notify() */ });
}).detach();
```

- `Rc<RefCell<…>>` 不 Send，不能 move 进 `background_executor().spawn`；跨线程只传 Send 数据（String/Arc<RenderImage>），主线程泵里落缓存。
- mermaid/数学离线程渲染完成后要 `cx.refresh_windows()`（仅 `cx.notify()` 不够——编辑器缓存行布局会陈旧，`note_panel/mod.rs:125`）。

## solomd-core 约定

- 对外 API（UI 直调 pub fn）返回 `Result<T, String>`（错误中文可直显 UI）；内层用 `anyhow`/`Option`，入口 `.map_err(|e| e.to_string())`。检测类不可失败函数返回 `Vec`（如 `cjk_proofread::proofread`）。
- 不 panic：解码/IO 失败返回 Err/None；图片解码带 512MB limits 防解压炸弹（`app/src/images.rs:64`）；`spawn_blocking` join 错误统一 `.map_err(|e| format!("join: {e}"))?`。
- rusqlite bundled（免系统依赖），库文件放 `<workspace>/.solomd/`；连接即用即开不池化；嵌入计算在 `spawn_blocking`；schema 变更走 `rag.rs` 的 `index_version` 字段判断重建。
- 日志：core 内 `eprintln!`，前缀模块名打头（`[ipc]` `[fence]` `[mermaid]`；UI 泵类 `[sync]` `[poll]`）——stdout 可 grep 是硬要求。只记关键路径与失败；循环内高频事件（每 chunk）不打。
- 从上游 tauri 壳同步代码时保持模块名/函数名一一对应（`app/src-tauri/src/*.rs` ↔ `gpui/crates/core/src/*.rs`），方便对照 diff；只搬业务逻辑，command 壳/emit/AppHandle 按接缝模式重写（改写表见 `core/src/lib.rs` 头注释）。

## zorite gpui-editor 集成

- 只用 zorite 公开 API，不 hack 其内部私有项。围栏内容提取用 `gpui_editor::mermaid_sources` / `math_sources` / `inline_math_sources`（保证缓存键与编辑器查找一致）。
- `EditorEvent::Changed` 仅用户编辑触发（程序性 `set_text` 不发）；`SelectionChanged` 是 gpui-editor 独有事件（gpui-component 没有），无需轮询光标。
- 已知边界：中文斜体不可见（CJK 无斜体字面 + GPUI 不合成倾斜，拉丁正常）；独立图片行光标不能左右穿越（疑似上游 bug，源码编辑用 WYSIWYG 开关）；编辑器 auto-grow 无内滚动，宿主负责滚动容器（`note_panel/mod.rs:462` track_scroll）。
- 交互语义：独立图片行 = Word 式原子对象（光标只停左右、⌫ 整删、右下角柄缩放、不显源码）；行内图片缩略图可点击预览、光标可穿越、进入显源码。

## 验证底线（每次改动）

```sh
cd gpui
cargo check -p solomd-core && cargo check -p solomd-app
cargo build -p solomd-app
(./target/debug/solomd _vault > solomd-app.log 2>&1 & SPID=$!; sleep 10;
 kill -0 $SPID && { echo RUNNING-OK; kill $SPID; })   # 冒烟：存活无 panic
```

- "已完成/已修复"必须有编译/运行证据；不能跑的部分明说"未验证"。日志 grep `rg "panic|error" solomd-app.log`（macOS 键盘布局/IMK 噪音属正常）。
- 对未 resolve 的路径禁止二次 resolve：Vault root 已规范化为绝对路径、resolve 幂等；曾因双重 join 产生 `_vault/_vault/` 写错位置（`vault.rs:77` 注释）。
