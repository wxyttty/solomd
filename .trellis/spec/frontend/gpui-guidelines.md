# GPUI 应用开发规范（crates/app）

> 适用：`gpui/crates/app`。框架版本与依赖锁步见 `gpui/Cargo.toml` 注释——**改依赖前先读它**。

---

## 1. 依赖锁步（最高约束，2026-08-29 起 vendor 模式）

zed + gpui-component + zorite（gpui-editor）必须落在同一个 zed rev。2026-08-29 起
**gpc 与 zorite 已 vendor 进本仓库**（`crates/gpui-component/`、`crates/zorite/`，
path 依赖），pin 只存在于 `gpui/Cargo.toml` 一处：

```
zed@f66ed399 ← vendored gpc@0e2fb7a ← vendored zorite@7915aad
```

- 升级舞步见 `gpui/README.md` 依赖策略节（gpc tip lockfile 定 zed rev → 改根清单 → 修 churn）
- 本机无 Xcode：`gpui_platform` 必须带 `runtime_shaders` feature
- 工具链 1.97.1 稳定版（`rust-toolchain.toml`），禁 nightly

## 2. API 版本纪律（f66ed39 / 0e2fb7a 时代）

写代码前**必须对照本地 vendored 源码的真实签名**，不凭记忆/文档：

```bash
D=gpui/crates/gpui-component   # vendored 源码即真相
rg -n "pub fn xxx" $D/base/src/... $D/ui/src/...
```

已踩过的坑（勿重复）：
- **Panel trait 拆成两层**：`BasePanel`（gpui_base，panel_name/set_active/on_removed）
  + `Panel`（ui，title/toolbar_buttons 等表现层）。实现要拆成两个 impl 块；
  把 BasePanel 方法写进 `impl Panel` 会 E0407
- **DockItem/TabPanel 已删**：改用 `DockSkin::dock_area(id, ver, window, cx)` 建区
  （无 skin 不画 chrome）、`DockLayout::tabs().panel(entity)` 组布局、
  `area.set_center/set_dock/set_dock_size`、`area.add_panel(panel, placement, …)`
  （自动激活）。已开面板的再激活 = `remove_panel` + `add_panel`
  （remove 会触发 on_removed 回调，注意宿主侧列表同步 + pending 标记复位）
- **InputState 单行 / TextareaState 多行是两个类型**
  （`InputBaseState<InputMode>` vs `<TextareaMode>`）：多行用
  `TextareaState::new(...).auto_grow(4,16)` + `Textarea::new(&state)`，
  不再有 `InputState::multi_line(true)`
- `Window::paint_image` 6 参：`(bounds, image_bounds, corners, img, frame, gray)`，
  可见区 = bounds∩image_bounds；旧行为等价于前两者相同
- `InputState::set_value("", window, cx)` 要 window；`cursor()` 不要
- `Context::spawn(async move |this, cx|)` 里 `this.upgrade()` 返回 **Option**
- `AsyncWindowContext` **没有** `notify()`——用 `update_in(cx, |panel, window, cx| … window.refresh())`
- gpui-component 无 `prelude`：`FluentBuilder` 从 `gpui::prelude` 导入；
  `ScrollableElement` 从 `gpui_component::scroll` 导入；`on_click`/`overflow_y_scroll`
  需要 `StatefulInteractiveElement`（元素先 `.id()`）
- GPUI 绘制顺序 = child 顺序：**浮层必须是最后一个 child**，否则被后画的不透明区域盖住
  （⌘P 面板踩过）
- `Root::new(...).bg(具体色)` 会以 refinement 覆盖主题每帧刷新的背景 → 切主题残影。
  创建 Root 不传 bg

## 3. 实体与事件泵模式

异步后台结果回到 UI 的标准形态（三处复用，新代码照抄）：

```rust
// core 侧：EventSink → mpsc channel
let (tx, rx) = std::sync::mpsc::channel();
let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(tx));
// UI 侧：实体内 spawn 轮询泵
cx.spawn(async move |this, cx| loop {
    cx.background_executor().timer(Duration::from_millis(80..200)).await;
    let Some(e) = this.upgrade() else { break };
    e.update(cx, |panel, cx| { /* drain rx → 改状态 → cx.notify() */ });
}).detach();
```

注意：`Rc<RefCell<…>>` 不 Send，不能 move 进 `background_executor().spawn`；
跨线程只传 Send 数据（String/Arc<RenderImage>），在主线程泵里落缓存。

tokio 依赖的 core 调用（reqwest 流式等）不能直接在 GPUI executor 上 await：
经 `solomd_core::rt::spawn(fut).await` 桥接（JoinHandle 可在任意 executor
await，任务在进程级 runtime 内跑；详见 backend/solomd-core-guidelines §3.5）。
纯同步 CPU 活走 `_inner` + `background_executor().spawn`（历史/导出/搜索同款）。

## 4. 主题

- 全局切换：`Theme::change(ThemeMode::Dark/Light, Some(window), cx)`
- 自绘元素**必须**每帧读 `cx.theme()`（bg/text_color/border），禁止硬编码色值
  （历史 bug：工具栏白底白字）
- 编辑器 SyntaxStyle 重涂广播：Workspace::apply_editor_theme 遍历打开的面板
- 持久化：`<vault>/.solomd/settings.json`，启动时窗口创建前应用

## 5. 右栏页模式（AI/Keys/搜索/同步/Recipes）

新增一页的标准动作：
1. `RightTab` 枚举加变体；Workspace 加 `Option<Entity<XxxPanel>>` 字段
2. 工具栏按钮：点击切换 `right_tab`（再点收起），懒创建面板
3. render 的右栏 match 加分支；body 宽 400px、`border_l_1`
4. 异步动作用 §3 泵模式或 `update_in(cx, |panel, window, cx| … window.refresh())`

## 6. 中文

全部用户可见字符串中文硬编码。无 i18n 层（自用决策 2026-08-26）。
