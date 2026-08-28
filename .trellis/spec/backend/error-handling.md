# Error Handling（solomd-core）

- 对外 API（UI 直接调用的 pub fn）：`Result<T, String>`，错误消息中文化可直显
- 内层工具函数：`anyhow::Result` / `Option`，入口处 `.map_err(|e| e.to_string())`
- 检测类函数不可失败（如 cjk proofread）：返回 `Vec` 空 vec，不包 Result
- IO/解码失败不 panic：`let _ =` 或 `.ok()` 吞掉并打日志；写盘失败必须 eprintln 留痕
- 解码类加资源上限（image 512MB limits 防解压炸弹）
- spawn_blocking 的 join 错误统一 `.map_err(|e| format!("join: {e}"))?`
