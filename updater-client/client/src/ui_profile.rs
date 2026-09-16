use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::log::log_warning;
use crate::network::Network;

const PROFILE_PATH: &str = "ui-profile.json";
const CACHE_FILE: &str = "ui-profile-cache.json";
const MAX_TEXT_BYTES: usize = 240;
const MAX_ICON_BYTES: usize = 3 * 1024 * 1024;
const MAX_ICON_DATA_URL_BYTES: usize = 4 * 1024 * 1024;
const MAX_BACKGROUND_BYTES: usize = 8 * 1024 * 1024;
const MAX_BACKGROUND_DATA_URL_BYTES: usize = 11 * 1024 * 1024;

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
    pub icon_data_url: String,
    pub background_image: String,
    pub background_data_url: String,
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
            icon_data_url: data_url(include_bytes!("../app-icon.png")).unwrap_or_default(),
            background_image: String::new(),
            background_data_url: String::new(),
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
    let cached = load_cached(working_dir).await.unwrap_or_default();
    let text = network
        .request_text(PROFILE_PATH, 0..0, "updater UI profile")
        .await
        .ok()?;
    let mut profile = validate_profile(serde_json::from_str::<UiProfile>(&text).ok()?)?;

    if !profile.icon.is_empty() {
        profile.icon_data_url = cached.icon_data_url;
        match network
            .request_bytes(&profile.icon, "updater UI icon", MAX_ICON_BYTES)
            .await
        {
            Ok(icon) => match data_url(&icon) {
                Some(encoded) if encoded.len() <= MAX_ICON_DATA_URL_BYTES => {
                    profile.icon_data_url = encoded;
                }
                Some(_) => log_warning("远端更新器图标编码后超过 4 MiB，继续使用缓存图标"),
                None => log_warning("远端更新器图标格式不受支持，继续使用缓存图标"),
            },
            Err(error) => {
                log_warning(format!(
                    "远端更新器图标加载失败，继续使用缓存图标：{}",
                    error.reason
                ));
            }
        }
    }

    if !profile.background_image.is_empty() {
        profile.background_data_url = cached.background_data_url;
        match network
            .request_bytes(
                &profile.background_image,
                "updater UI background",
                MAX_BACKGROUND_BYTES,
            )
            .await
        {
            Ok(background) => match data_url(&background) {
                Some(encoded) if encoded.len() <= MAX_BACKGROUND_DATA_URL_BYTES => {
                    profile.background_data_url = encoded;
                }
                Some(_) => log_warning("远端更新器背景图编码后超过 11 MiB，继续使用缓存背景图"),
                None => log_warning("远端更新器背景图格式不受支持，继续使用缓存背景图"),
            },
            Err(error) => log_warning(format!(
                "远端更新器背景图加载失败，继续使用缓存背景图：{}",
                error.reason
            )),
        }
    }

    let profile = validate_profile(profile)?;
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
    if profile.headline_color.is_empty() {
        profile.headline_color = profile.theme.text.clone();
    }
    if profile.subtitle_color.is_empty() {
        profile.subtitle_color = profile.theme.muted.clone();
    }
    if profile.footer_color.is_empty() {
        profile.footer_color = profile.theme.muted.clone();
    }
    if profile.traffic_color.is_empty() {
        profile.traffic_color = profile.theme.text.clone();
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
        &profile.headline_color,
        &profile.subtitle_color,
        &profile.footer_color,
        &profile.traffic_color,
    ] {
        if !valid_color(color) {
            return None;
        }
    }
    if !profile.icon.is_empty() && !safe_asset_path(&profile.icon) {
        return None;
    }
    if !profile.background_image.is_empty() && !safe_asset_path(&profile.background_image) {
        return None;
    }
    if !profile.icon_data_url.is_empty()
        && (!profile.icon_data_url.starts_with("data:image/")
            || profile.icon_data_url.len() > MAX_ICON_DATA_URL_BYTES)
    {
        return None;
    }
    if !profile.background_data_url.is_empty()
        && (!profile.background_data_url.starts_with("data:image/")
            || profile.background_data_url.len() > MAX_BACKGROUND_DATA_URL_BYTES)
    {
        return None;
    }
    profile.launch_label_offset_x = profile.launch_label_offset_x.clamp(-24, 24);
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
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_icon_paths() {
        assert!(safe_asset_path("assets/updater.png"));
        assert!(!safe_asset_path("../updater.png"));
        assert!(!safe_asset_path("https://example.invalid/icon.png"));

        let mut profile = UiProfile::default();
        profile.background_image = "../background.png".to_owned();
        assert!(validate_profile(profile).is_none());
    }

    #[test]
    fn accepts_png_data_url() {
        assert!(data_url(b"\x89PNG\r\n\x1a\nrest")
            .unwrap()
            .starts_with("data:image/png"));
    }

    #[test]
    fn accepts_animated_gif_data_url_even_with_a_png_asset_path() {
        assert!(data_url(b"GIF89arest")
            .unwrap()
            .starts_with("data:image/gif"));
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
    fn old_profile_uses_existing_text_colors_for_brand_copy() {
        let profile: UiProfile = serde_json::from_str(
            r##"{"schema":1,"headline":"A","subtitle":"B","footer":"C","theme":{"text":"#112233","muted":"#445566"}}"##,
        )
        .unwrap();
        let profile = validate_profile(profile).unwrap();
        assert_eq!(profile.headline_color, "#112233");
        assert_eq!(profile.subtitle_color, "#445566");
        assert_eq!(profile.footer_color, "#445566");
        assert_eq!(profile.traffic_color, "#112233");
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

    #[test]
    fn rejects_oversized_cached_icon_instead_of_truncating_it() {
        let mut profile = UiProfile::default();
        profile.icon_data_url = format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_ICON_DATA_URL_BYTES)
        );
        assert!(validate_profile(profile).is_none());
    }
}
