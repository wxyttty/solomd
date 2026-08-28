# Component Guidelines（GPUI 右栏页 / Dock 面板）

> 两种可复用 UI 形态的标准写法。参考实现：sync_panel.rs（右栏页）、
> note_panel.rs（Dock 面板）。

---

## 右栏页（新增一页 checklist）

1. `workspace.rs`：`RightTab` 加变体 + `Option<Entity<XxxPanel>>` 字段
2. 工具栏按钮：点击切换 tab（再点收起）+ 懒创建
3. render 右栏 match 加分支（body 400px、border_l_1、overflow_y_scrollbar）
4. 异步动作走 update_in 泵：`update_in(cx, |panel, window, cx| { …; window.refresh(); })`
   —— AsyncWindowContext 无 notify，refresh 在闭包内（window 是闭包参数，外层
   方法的 window 不能 move 进 async 块）

## Dock 面板（1505b14 单 trait）

```rust
impl Panel for XxxPanel {
    fn panel_name(&self) -> &'static str { "stable-id" }  // 永不改
}
impl EventEmitter<PanelEvent> for XxxPanel {}
impl Focusable for XxxPanel { … }
```

- `title()` 返回标签文案（脏态 • 前缀在这里拼）
- `set_active(true)` 里聚焦编辑器；`on_removed` 回调同步宿主注册表
- 关闭同步：TabPanel UI 关闭走 `on_removed`；宿主记录用
  `Box<dyn Fn(&mut App)>` 回调（Context deref 到 App 传入）

## 表格/列表渲染

- `results: Vec<AnyElement>` + `.into_any_element()` push，最后 `.children(vec)`
- 可点击行：`div().id(...)` + `.on_click(cx.listener(...))`，hover 用
  `cx.theme().accent.opacity(0.12)`（深浅色通用）
- 滚动容器：`.id(...)` + `.overflow_y_scroll()`（gpui）或
  `overflow_y_scrollbar()`（gpui-component，需 scroll::ScrollableElement）

## 禁止

- 面板字段存 `Rc<RefCell>`（跨 spawn 不 Send）
- 按钮态用 `.disabled()`（1505b14 无此 API）——条件分支换整个按钮构造
- 在 render 里做重活（文件树 walk、索引扫描）——只做轻量读取
