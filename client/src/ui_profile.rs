use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::network::Network;

const PROFILE_PATH: &str = "ui-profile.json";
const CACHE_FILE: &str = "ui-profile-cache.json";
const MAX_TEXT_BYTES: usize = 240;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiProfile {
    pub schema: u8,
    pub headline: String,
    pub subtitle: String,
    pub footer: String,
    pub launch_label_offset_x: i8,
    pub icon: String,
    pub icon_data_url: String,
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
            launch_label_offset_x: 2,
            icon: String::new(),
            icon_data_url: data_url(include_bytes!("../app-icon.png")).unwrap_or_default(),
            stages: StageLabels::default(),
            theme: ThemeColors::default(),
        }
    }
}

impl UiProfile {
    pub fn stage_label(&self, key: &str) -> &str {
        match key {
            "checking" => &self.stages.checking,
            "downloading" => &self.stages.downloading,
            "applying" => &self.stages.applying,
            "completed" => &self.stages.completed,
            _ => &self.stages.prepare,
        }
    }
}

pub async fn load_cached(working_dir: &Path) -> Option<UiProfile> {
    let text = tokio::fs::read_to_string(working_dir.join(CACHE_FILE))
        .await
        .ok()?;
    let profile = serde_json::from_str::<UiProfile>(&text).ok()?;
    validate_profile(profile)
}

pub async fn refresh(network: &mut Network<'_>, working_dir: &Path) -> Option<UiProfile> {
    let text = network
        .request_text(PROFILE_PATH, 0..0, "updater UI profile")
        .await
        .ok()?;
    let mut profile = validate_profile(serde_json::from_str::<UiProfile>(&text).ok()?)?;

    if !profile.icon.is_empty() {
        if let Ok(icon) = network
            .request_bytes(&profile.icon, "updater UI icon")
            .await
        {
            profile.icon_data_url = data_url(&icon).unwrap_or_default();
        }
    }

    let serialized = serde_json::to_vec(&profile).ok()?;
    let _ = tokio::fs::write(working_dir.join(CACHE_FILE), serialized).await;
    Some(profile)
}

fn validate_profile(mut profile: UiProfile) -> Option<UiProfile> {
    if profile.schema != 1
        || !valid_text(&profile.headline)
        || !valid_text(&profile.subtitle)
        || !valid_text(&profile.footer)
    {
        return None;
    }
    for value in [
        &profile.stages.prepare,
        &profile.stages.checking,
        &profile.stages.downloading,
        &profile.stages.applying,
        &profile.stages.completed,
    ] {
        if !valid_text(value) {
            return None;
        }
    }
    for color in [
        &profile.theme.accent,
        &profile.theme.accent_hover,
        &profile.theme.accent_soft,
        &profile.theme.background,
        &profile.theme.surface,
        &profile.theme.log_background,
        &profile.theme.text,
        &profile.theme.muted,
        &profile.theme.border,
    ] {
        if !valid_color(color) {
            return None;
        }
    }
    if !profile.icon.is_empty() && !safe_asset_path(&profile.icon) {
        return None;
    }
    if !profile.icon_data_url.is_empty() && !profile.icon_data_url.starts_with("data:image/") {
        return None;
    }
    profile.launch_label_offset_x = profile.launch_label_offset_x.clamp(-24, 24);
    profile.icon_data_url.truncate(1_400_000);
    Some(profile)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_TEXT_BYTES && !value.contains(['\r', '\n'])
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn safe_asset_path(path: &str) -> bool {
    !path.starts_with('/')
        && !path.contains('\\')
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && ["png", "jpg", "jpeg", "webp"]
            .iter()
            .any(|ext| path.ends_with(&format!(".{ext}")))
}

fn data_url(bytes: &[u8]) -> Option<String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        return None;
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

pub(crate) fn decode_icon_data_url(value: &str) -> Option<Vec<u8>> {
    let (header, encoded) = value.split_once(',')?;
    if !matches!(
        header,
        "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
    ) {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    if bytes.len() > 1_048_576 {
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_icon_paths() {
        assert!(safe_asset_path("assets/updater.png"));
        assert!(!safe_asset_path("../updater.png"));
        assert!(!safe_asset_path("https://example.invalid/icon.png"));
    }

    #[test]
    fn accepts_png_data_url() {
        assert!(data_url(b"\x89PNG\r\n\x1a\nrest")
            .unwrap()
            .starts_with("data:image/png"));
    }

    #[test]
    fn decodes_only_supported_icon_data_urls() {
        let png = data_url(b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert_eq!(
            decode_icon_data_url(&png).unwrap(),
            b"\x89PNG\r\n\x1a\nrest"
        );
        assert!(decode_icon_data_url("data:text/html;base64,PHNjcmlwdD4=").is_none());
    }

    #[test]
    fn accepts_only_six_digit_theme_colors() {
        assert!(valid_color("#147d67"));
        assert!(valid_color("#AABBCC"));
        assert!(!valid_color("147d67"));
        assert!(!valid_color("#abcd"));
        assert!(!valid_color("red"));
    }

    #[test]
    fn clamps_launch_label_offset_to_safe_layout_range() {
        let mut profile = UiProfile::default();
        profile.launch_label_offset_x = 100;
        assert_eq!(validate_profile(profile).unwrap().launch_label_offset_x, 24);

        let mut profile = UiProfile::default();
        profile.launch_label_offset_x = -100;
        assert_eq!(
            validate_profile(profile).unwrap().launch_label_offset_x,
            -24
        );
    }
}
