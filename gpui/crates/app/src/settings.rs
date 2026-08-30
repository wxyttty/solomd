//! settings.rs — 用户设置持久化（沿用 SoloMD 的 `.solomd/` 目录约定）。
//!
//! 存 `<vault>/.solomd/settings.json`，随 vault 走；损坏/缺失时静默用默认值。

use std::path::Path;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
pub struct Settings {
    /// "light" | "dark"（serde 默认表达，避免引入 ThemeMode 类型依赖）。
    #[serde(default)]
    pub dark: bool,
    /// AI provider id（ai_proxy::resolve_provider 的别名也接受）。
    #[serde(default)]
    pub ai_provider: String,
    /// 模型名（provider 默认模型之外的覆盖）。
    #[serde(default)]
    pub ai_model: String,
    /// 自定义 OpenAI 兼容 / Ollama 地址（空 = 用 provider 默认）。
    #[serde(default)]
    pub ai_base_url: String,
    /// 打字机模式（光标行垂直居中）。
    #[serde(default)]
    pub typewriter: bool,
}

/// AI 设置缺省值（Settings derive Default 走不到 String 字段的业务默认）。
impl Settings {
    pub fn ai_provider_or_default(&self) -> String {
        if self.ai_provider.is_empty() {
            "deepseek".into()
        } else {
            self.ai_provider.clone()
        }
    }

    pub fn ai_model_or_default(&self) -> String {
        if self.ai_model.is_empty() {
            "deepseek-chat".into()
        } else {
            self.ai_model.clone()
        }
    }
}

impl Settings {
    pub fn load(vault_root: &Path) -> Self {
        let path = settings_path(vault_root);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, vault_root: &Path) {
        let path = settings_path(vault_root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    eprintln!("[settings] 保存失败: {e}");
                }
            }
            Err(e) => eprintln!("[settings] 序列化失败: {e}"),
        }
    }
}

fn settings_path(vault_root: &Path) -> std::path::PathBuf {
    vault_root.join(".solomd").join("settings.json")
}
