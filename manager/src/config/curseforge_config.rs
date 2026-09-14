use serde::Deserialize;
use serde::Serialize;

/// CurseForge 外部下载源配置。
///
/// API Key 只驻留在管理端。客户端只接收已解析的公共 CDN 地址，且仍会按
/// mcpatch 元数据校验文件，失败时可回退到 tar 分片。
#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "kebab-case")]
pub struct CurseForgeConfig {
    /// 是否在 pack 时解析 `.minecraft/mods` 下的 jar。
    pub enabled: bool,

    /// CurseForge for Studios API Key，禁止写入更新元数据。
    pub api_key: String,

    /// Minecraft 在 CurseForge API 中的 game id。
    pub game_id: u32,

    /// 客户端外部下载失败后是否允许回退 mcpatch tar。
    pub fallback_to_mcpatch: bool,

    /// PCL 使用的公共 CurseForge CDN 根地址。
    pub cdn_base_url: String,
}

impl Default for CurseForgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            game_id: 432,
            fallback_to_mcpatch: true,
            cdn_base_url: "https://mediafilez.forgecdn.net/files".to_owned(),
        }
    }
}
