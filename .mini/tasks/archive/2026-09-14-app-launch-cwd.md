done: 2026-09-14

# Goal
双击 /Applications/Solomd.app 能正常启动并打开用户实际的笔记库（gpui/_vault），
不再因 cwd 依赖而闪退或在错误位置创建 vault。

# Scope
1. `main.rs`：缺省 vault 路径不再用相对 `_vault`（cwd 相关），改为
   `~/.config/solomd-gpui/last-vault` 记忆 + 缺省 `~/Documents/Solomd`；
   CLI 传参仍最高优先。
2. `workspace.rs:209`：ImageStore 解析根从 `current_dir()` 改为 `vault.root`。
3. `note_panel/mod.rs`：粘贴图片（位图 + Finder 文件两条路径）落盘到
   `vault.root/_assets/`，不再写 cwd。
4. `make-dmg.sh` 不变（bundle 结构无改动）。

# Non-goals
- 不做多 vault 切换 UI / 文件夹选择器（后续任务）。
- 不迁移历史误建数据（/Applications/Solomd.app/_vault、/tmp/_vault 由用户手动清理，
  本次给出指引）。
- 不改 core crate。

# Acceptance
- [x] `cargo check -p solomd-app` 通过（无新增 warning，基线 7 条 unused 为既有）
- [x] release 构建后以 cwd=/ 启动：进程存活 ≥8s，vault 落在 ~/Documents/Solomd（首启）
- [x] `open /Applications/Solomd.app`（Finder 双击等价）：进程存活，bundle 内无 _vault
- [x] 粘贴图片写入 `<vault>/_assets/`（代码路径已改为 vault.root，运行验证见限制）
- [x] 笔记内相对图片路径按 vault root 解析（ImageStore 根 = vault.root）

# Decisions
- 缺省 vault = `~/Documents/Solomd`（桌面笔记应用惯例，Finder 可见），
  记忆文件 `~/.config/solomd-gpui/last-vault`（复用 CorePaths::default_paths 的
  config_dir 约定，与 AI key 同目录）。
- 优先级：CLI 参数 > last-vault 记忆 > ~/Documents/Solomd 缺省。

# Assumptions
- 用户现有数据在 `/Users/wxy/code/solomd/gpui/_vault`（开发期 cwd 恰好是 gpui/）。
  修复后首次启动会打开 `~/Documents/Solomd`（空库）；要继续用旧库，
  用 `open /Applications/Solomd.app --args /Users/wxy/code/solomd/gpui/_vault`
  一次，之后 last-vault 记忆接管。推荐依据：不猜用户数据位置，显式一次迁移。

# Verify
```sh
cd gpui
cargo check -p solomd-app
cargo build --release -p solomd-app
# cwd=/ 模拟 Finder：
python3 -c "import subprocess,time; p=subprocess.Popen(['target/release/solomd'],cwd='/'); time.sleep(8); print('alive' if p.poll() is None else f'dead rc={p.poll()}')"
```
