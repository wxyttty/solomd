# ADR-0002：vendor gpui-component 与 zorite，收回框架 rev 决定权

- 状态：已采纳（2026-08-29）
- 关联：`gpui/Cargo.toml` 依赖策略注释、`gpui/README.md` 依赖策略节、提交 d48fffe / e8bea89

## 背景

三方依赖（本仓库 / gpui-component / zorite）必须统一在同一个 zed rev 上——git 依赖只有 spec 完全一致才会去重。原策略（裸 git 依赖 + Cargo.lock 冻结）被两头卡死：

- zorite 仓库把整条链钉死在旧 rev（其 main tip 至 2026-08-29 仍钉 zed@1d217ee + gpc@1505b14，升级舞步它自己一直没跳）；
- gpui-component tip 改成浮 pin（manifest 不写 zed rev）。

结果：本仓库被锁在两个月前的框架版本上，gpc 1505b14→0e2fb7a 之间 226 commits（ui/ ±6 万行，Input 大重构）无法跟进。

## 决策

把 zorite 3 个 crate（gpui-editor 等，GPL-3.0，`crates/zorite/`）和 gpui-component 5 个 crate（Apache-2.0，`crates/gpui-component/`）vendor 进本仓库，全部改 path 依赖。zed rev 的 pin 只存在于 `gpui/Cargo.toml` 一处。

## 真实权衡

- **得到**：pin 决定权收回本仓库，升级不等上游；升级舞步简化为「取 gpc 新 tip → 从其 Cargo.lock 读实际 zed rev → 改根清单两行 → 修 vendored crate + app 的 API churn」。
- **代价**：~18 万行 vendored 代码进仓库（gpc 6.3MB + zorite 2.4MB）；上游修复需手动同步；zorite 的 GPL-3.0 传染面扩大到 whiteboard（自用项目可接受，许可证文件保留在 `crates/zorite/LICENSE-zorite`）。
- **替代方案否决**：继续裸 git 依赖 = 永久锁死旧 rev；fork 上游仓库改 pin = 双份维护成本高于 vendor。
- **实测参考**（2026-08-29）：gpc tip 0e2fb7a ≈ zed@f66ed39；zorite 三 crate 零 gpc 依赖，vendor churn 为零。

## 后果

- 依赖升级从「被上游卡」变为「自己跳锁步舞」；升级时必须三方一起动，只改一处 rev = 双 gpui 依赖图编译失败（不变量已入 AGENTS.md）。
- wry 必须用 longbridge 的 lb-wry fork（0.53.3），版本与 gpui-wry 内部声明一致。
- vendored gpc 清单继承其 root lints 配置（`gpui/Cargo.toml`，照抄勿删）。
