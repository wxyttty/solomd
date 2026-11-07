//! settings.rs — 用户设置持久化（沿用 SoloMD 的 `.solomd/` 目录约定）。
//!
//! 存 `<vault>/.solomd/settings.json`，随 vault 走；损坏/缺失时静默用默认值。

use std::path::Path;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
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
    /// AutoGit：保存（⌘S）后自动提交快照（上游 autoGitEnabled，默认关）。
    #[serde(default)]
    pub auto_git: bool,
    /// Agent 允许写工具（上游 agentAllowWrite，默认关——agent 默认只读）。
    #[serde(default)]
    pub agent_allow_write: bool,
    /// 标签标题取文档首个非空行（上游 tabTitleFromFirstLine，默认开）。
    /// UUID / 哈希文件名场景下比文件名更有用。
    #[serde(default = "default_true")]
    pub tab_title_from_first_line: bool,
    /// LanguageTool 语法检查（上游 ltMode 系列；默认关，opt-in）。
    /// mode: "local" | "premium" | "public"。
    #[serde(default = "default_lt_mode")]
    pub lt_mode: String,
    /// 本地服务地址（local 模式）。
    #[serde(default)]
    pub lt_base_url: String,
    /// Premium 账号邮箱（API key 走系统钥匙串，provider id = "languagetool"）。
    #[serde(default)]
    pub lt_username: String,
    /// 检查语言："auto" 或 LT 代码（如 de-DE）。
    #[serde(default = "default_lt_language")]
    pub lt_language: String,
    /// "default" | "picky"。
    #[serde(default = "default_lt_level")]
    pub lt_level: String,
    /// 保存前确认原文件已不存在（上游 #9 confirmSaveMissingFile，默认关）。
    #[serde(default)]
    pub confirm_save_missing_file: bool,
}

fn default_true() -> bool {
    true
}

fn default_lt_mode() -> String {
    "local".into()
}

fn default_lt_language() -> String {
    "auto".into()
}

fn default_lt_level() -> String {
    "default".into()
}

/// 手写 Default：`tab_title_from_first_line` 业务默认是 true，而
/// derive(Default) 只会给 bool 的 false（新库首次启动会与“旧配置缺字段”
/// 的 serde 默认值不一致）。
impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: false,
            ai_provider: String::new(),
            ai_model: String::new(),
            ai_base_url: String::new(),
            typewriter: false,
            auto_git: false,
            agent_allow_write: false,
            tab_title_from_first_line: true,
            lt_mode: default_lt_mode(),
            lt_base_url: String::new(),
            lt_username: String::new(),
            lt_language: default_lt_language(),
            lt_level: default_lt_level(),
            confirm_save_missing_file: false,
        }
    }
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
