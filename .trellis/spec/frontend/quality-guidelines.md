# Quality Guidelines（GPUI 时代，前后端通用）

> 冒烟优先、证据驱动的自用项目纪律。

---

## 每次改动的验证底线

```sh
cd gpui
cargo check -p solomd-core && cargo check -p solomd-app   # 类型检查
cargo build -p solomd-app                                  # 构建
(./target/debug/solomd _vault > solomd-app.log 2>&1 & SPID=$!; sleep 10;
 kill -0 $SPID && { echo RUNNING-OK; kill $SPID; })        # 冒烟：存活无 panic
```

- "已完成/已修复"必须有编译/运行证据；不能跑的部分明说"未验证"
- 日志 grep：`rg "panic|error" solomd-app.log`（macOS 键盘布局噪音/IMK 日志是正常噪音）

## 禁止模式

| 禁止 | 原因 |
|---|---|
| core 内引 gpui/tauri | 破坏 UI 无关性（见 backend/solomd-core-guidelines.md §1） |
| 硬编码 UI 色值 | 切主题残影（工具栏白底白字事故） |
| 夜间凭记忆写 gpui-component API | 1505b14 与 HEAD/文档差异大，先查本地 checkout 签名 |
| `Rc<RefCell>` 进后台线程 | 非 Send；跨线程只传 Send 数据，主线程泵落缓存 |
| 浮层非最后一个 child | 被不透明后绘区域盖住（⌘P 事故） |
| Root::new 传 bg | refinement 覆盖主题每帧刷新（切主题残影事故） |
| 对未 resolve 的路径二次 resolve | Vault 已改绝对 root 幂等；曾产生 `_vault/_vault/` 写错位置 |
| 升级只改一处 rev | 三方锁步断裂 → 双 gpui 图编译失败 |
| nightly 构建 | pathfinder_simd SIMD intrinsics 失败 |

## 已知边界（改到相关代码时先读）

frontend/gpui-guidelines.md 与 editor-guidelines.md 的"已知边界"两节；
docs/gpui-migration-plan.md 风险登记表。

## 变更记录纪律

非平凡决策（选型/砍功能/踩坑修复）落两处：
1. `gpui/README.md` 对应表格行（现场）
2. `.trellis/spec/` 对应规范文件（沉淀）
