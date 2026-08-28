# zorite gpui-editor 集成规范

> 笔记编辑器底座：`gpui-editor`（zorite，GPL-3.0，git 依赖）。
> 版本锁步与升级舞步见 gpui/Cargo.toml 注释。

---

## 1. 创建与 WYSIWYG 开关

```rust
let mut ed = EditorState::new(window, cx).with_text(content);
ed.set_markdown_style(warm(dark, mono_family), cx);  // 装上即活渲染
ed.clear_markdown_style(cx);                          // 全量源码模式（运行时可切）
```

- `set_markdown_style` 不在载入热路径上（5MB/91ms 数据不受影响）
- SyntaxStyle 全量 25 字段构造，无 Default——色值改 `theme.rs`（映射 tokens.css）

## 2. 事件订阅（EditorEvent）

| 事件 | SoloMD 语义 |
|---|---|
| `Changed` | 脏态标记 •；注意程序性 `set_text` 不发 |
| `PreviewImage(src)` | 左键点行内图片 → 系统看图器（**不放置光标**，设计如此） |
| `OpenLink` / `OpenWikiLink` | 系统浏览器 / 笔记跳转 |
| `SelectionChanged` | 有此事件（gpui-component 没有），无需轮询 |
| `EditMath` / `EditProperties` | 未用（公式/属性面板未接） |

交互语义速记：独立图片行 = Word 式原子对象（光标只停左右、⌫ 整删、右下角柄缩放、
不显源码）；行内图片缩略图可点击预览、光标可穿越、进入显源码。

## 3. 块级 providers（宿主职责）

- `set_block_image_provider`：同步解码 + HashMap 缓存 + 1024px 缩图 + RGBA→BGRA；
  带 512MB 解码 limits 防解压炸弹
- `set_block_mermaid_provider`：只读缓存；渲染 = mermaid-rs-renderer → SVG →
  `cx.svg_renderer().render_single_frame`，off-thread 状态机
  Loading/Ready/Failed + `begin()/finish()` 占坑幂等；完成后 `window.refresh()`
  （仅 cx.notify 不够——编辑器缓存行布局会陈旧）
- 内容提取用 `gpui_editor::mermaid_sources(content)`（保证缓存键与编辑器查找一致）

## 4. 中文校对（diagnostics）

```rust
let issues = solomd_core::cjk_proofread::proofread(&text);
ed.set_diagnostics(issues.map(|i| Diagnostic { range: i.col_start..i.col_end }), cx);
```

右键波浪线出建议菜单（crate 内建）。

## 5. 已知边界（勿重复踩）

- **中文斜体不可见**：CJK 无斜体字面 + GPUI 不合成倾斜；拉丁正常。补偿方案（fork
  给 italic run 叠色相）未启用
- **独立图片行光标不能左右穿越**：疑似上游 bug，zorite demo 亦未覆盖；源码编辑用
  WYSIWYG 开关
- 编辑器 auto-grow 无内滚动：宿主负责滚动容器
- 事件 `Changed` 仅用户编辑触发；IME 组词保护由 crate 内建（我们不再做冻结层）

## 6. 依赖纪律

- 只用 zorite 公开 API；其内部（element.rs/lib.rs 私有项）不 hack
- tldraw 白板走 board-webview（wry）路线，与本 crate 无关——见 gpui/README.md
