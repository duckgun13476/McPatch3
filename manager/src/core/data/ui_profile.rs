use std::path::Path;

use serde::{Deserialize, Serialize};

pub const ICON_ASSET_PATH: &str = "assets/updater.png";
pub const BACKGROUND_ASSET_PATH: &str = "assets/updater-background.png";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiProfile {
    pub schema: u8,
    pub headline: String,
    pub subtitle: String,
    pub footer: String,
    #[serde(default)]
    pub headline_color: String,
    #[serde(default)]
    pub subtitle_color: String,
    #[serde(default)]
    pub footer_color: String,
    #[serde(default)]
    pub traffic_color: String,
    pub launch_label_offset_x: i8,
    pub icon: String,
    pub background_image: String,
    pub stages: StageLabels,
    pub theme: ThemeColors,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct StageLabels {
    pub prepare: String,
    pub checking: String,
    pub downloading: String,
    pub applying: String,
    pub completed: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ThemeColors {
    pub accent: String,
    pub accent_hover: String,
    pub accent_soft: String,
    pub background: String,
    pub surface: String,
    pub log_background: String,
    pub text: String,
    pub muted: String,
    pub border: String,
}

impl Default for StageLabels {
    fn default() -> Self {
        Self {
            prepare: "正在准备".to_owned(),
            checking: "正在检查".to_owned(),
            downloading: "正在下载".to_owned(),
            applying: "正在应用".to_owned(),
            completed: "更新完成".to_owned(),
        }
    }
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            accent: "#147d67".to_owned(),
            accent_hover: "#106b59".to_owned(),
            accent_soft: "#dff2eb".to_owned(),
            background: "#f4f7f6".to_owned(),
            surface: "#ffffff".to_owned(),
            log_background: "#f7faf9".to_owned(),
            text: "#16332d".to_owned(),
            muted: "#648078".to_owned(),
            border: "#e2ebe8".to_owned(),
        }
    }
}

impl Default for UiProfile {
    fn default() -> Self {
        Self {
            schema: 1,
            headline: "自动更新器".to_owned(),
            subtitle: "安全检查并应用客户端更新".to_owned(),
            footer: "请保持此窗口开启，完成后将自动启动客户端。".to_owned(),
            headline_color: ThemeColors::default().text,
            subtitle_color: ThemeColors::default().muted,
            footer_color: ThemeColors::default().muted,
            traffic_color: ThemeColors::default().text,
            launch_label_offset_x: 2,
            icon: String::new(),
            background_image: String::new(),
            stages: StageLabels::default(),
            theme: ThemeColors::default(),
        }
    }
}

impl UiProfile {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("读取个性化配置失败：{error}"))?;
        let profile = serde_json::from_str::<Self>(&text)
            .map_err(|error| format!("个性化配置格式错误：{error}"))?;
        profile.validate()
    }

    pub fn validate(mut self) -> Result<Self, String> {
        if self.schema != 1 {
            return Err("不支持的个性化配置版本".to_owned());
        }
        for value in [&self.headline, &self.subtitle, &self.footer] {
            if !valid_text(value) {
                return Err("显示文字不能为空、换行或超过 240 字节".to_owned());
            }
        }
        if self.headline_color.is_empty() {
            self.headline_color = self.theme.text.clone();
        }
        if self.subtitle_color.is_empty() {
            self.subtitle_color = self.theme.muted.clone();
        }
        if self.footer_color.is_empty() {
            self.footer_color = self.theme.muted.clone();
        }
        if self.traffic_color.is_empty() {
            self.traffic_color = self.theme.text.clone();
        }
        for value in [
            &self.stages.prepare,
            &self.stages.checking,
            &self.stages.downloading,
            &self.stages.applying,
            &self.stages.completed,
        ] {
            if !valid_text(value) {
                return Err("阶段文字不能为空、换行或超过 240 字节".to_owned());
            }
        }
        for color in [
            &self.theme.accent,
            &self.theme.accent_hover,
            &self.theme.accent_soft,
            &self.theme.background,
            &self.theme.surface,
            &self.theme.log_background,
            &self.theme.text,
            &self.theme.muted,
            &self.theme.border,
            &self.headline_color,
            &self.subtitle_color,
            &self.footer_color,
            &self.traffic_color,
        ] {
            if !valid_color(color) {
                return Err("颜色必须使用 #RRGGBB 格式".to_owned());
            }
        }
        if !valid_managed_asset(&self.icon, ICON_ASSET_PATH)
            || !valid_managed_asset(&self.background_image, BACKGROUND_ASSET_PATH)
        {
            return Err("图片路径不属于个性化资源目录".to_owned());
        }
        self.launch_label_offset_x = self.launch_label_offset_x.clamp(-24, 24);
        Ok(self)
    }
}

pub fn ensure_ui_profile(path: &Path) -> std::io::Result<()> {
    let profile = if path.exists() {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str::<UiProfile>(&text)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
            .validate()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
    } else {
        UiProfile::default()
    };

    let data = serde_json::to_vec_pretty(&profile)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, data)?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(temporary, path)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 240 && !value.contains(['\r', '\n'])
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_managed_asset(value: &str, expected: &str) -> bool {
    value.is_empty() || value == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_old_profile_without_background() {
        let profile: UiProfile = serde_json::from_str(
            r##"{"schema":1,"headline":"A","subtitle":"B","footer":"C","icon":"assets/updater.png","theme":{"accent":"#147d67","text":"#112233","muted":"#445566"}}"##,
        )
        .unwrap();
        let profile = profile.validate().unwrap();
        assert!(profile.background_image.is_empty());
        assert_eq!(profile.icon, ICON_ASSET_PATH);
        assert_eq!(profile.headline_color, "#112233");
        assert_eq!(profile.subtitle_color, "#445566");
        assert_eq!(profile.footer_color, "#445566");
        assert_eq!(profile.traffic_color, "#112233");
    }

    #[test]
    fn ensure_migrates_old_profile_with_new_default_fields() {
        let path = std::env::temp_dir().join(format!(
            "mcupdate-ui-profile-migration-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r##"{"schema":1,"headline":"A","subtitle":"B","footer":"C","icon":"","backgroundImage":"","theme":{"accent":"#147d67","text":"#112233","muted":"#445566"}}"##,
        )
        .unwrap();

        ensure_ui_profile(&path).unwrap();

        let stored = std::fs::read_to_string(&path).unwrap();
        let migrated: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(migrated["trafficColor"], "#112233");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_unmanaged_asset_paths() {
        let mut profile = UiProfile::default();
        profile.background_image = "../secret.png".to_owned();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn validates_colors_and_clamps_offset() {
        let mut profile = UiProfile::default();
        profile.launch_label_offset_x = 100;
        assert_eq!(profile.validate().unwrap().launch_label_offset_x, 24);

        let mut profile = UiProfile::default();
        profile.theme.accent = "red".to_owned();
        assert!(profile.validate().is_err());
    }
}
