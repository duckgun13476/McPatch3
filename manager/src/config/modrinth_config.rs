use serde::Deserialize;
use serde::Serialize;

/// Modrinth 公开哈希查询与外部下载源配置。
#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "kebab-case")]
pub struct ModrinthConfig {
    /// 是否在 pack 时优先解析 `.minecraft/mods` 下的 jar。
    pub enabled: bool,

    /// 客户端外部下载失败后是否允许回退 mcpatch tar。
    pub fallback_to_mcpatch: bool,
}

impl Default for ModrinthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            fallback_to_mcpatch: true,
        }
    }
}
