use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::core::data::ui_profile::{
    ThemeColors, UiProfile, BACKGROUND_ASSET_PATH, ICON_ASSET_PATH,
};
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileRequest {
    headline: String,
    subtitle: String,
    footer: String,
    headline_color: Option<String>,
    subtitle_color: Option<String>,
    footer_color: Option<String>,
    launch_label_offset_x: i8,
    theme: ThemeColors,
}

#[derive(Deserialize)]
pub struct ImageRequest {
    kind: String,
}

pub async fn api_get(State(state): State<WebState>) -> Response {
    let _guard = state.ui_profile.lock().await;
    match UiProfile::load(&state.apppath.ui_profile_file) {
        Ok(profile) => PublicResponseBody::ok(profile),
        Err(error) => PublicResponseBody::<UiProfile>::err(&error),
    }
}

pub async fn api_save(
    State(state): State<WebState>,
    Json(payload): Json<UpdateProfileRequest>,
) -> Response {
    let _guard = state.ui_profile.lock().await;
    let mut profile = match UiProfile::load(&state.apppath.ui_profile_file) {
        Ok(profile) => profile,
        Err(error) => return PublicResponseBody::<UiProfile>::err(&error),
    };
    profile.headline = payload.headline;
    profile.subtitle = payload.subtitle;
    profile.footer = payload.footer;
    if let Some(color) = payload.headline_color {
        profile.headline_color = color;
    }
    if let Some(color) = payload.subtitle_color {
        profile.subtitle_color = color;
    }
    if let Some(color) = payload.footer_color {
        profile.footer_color = color;
    }
    profile.launch_label_offset_x = payload.launch_label_offset_x;
    profile.theme = payload.theme;
    let profile = match profile.validate() {
        Ok(profile) => profile,
        Err(error) => return PublicResponseBody::<UiProfile>::err(&error),
    };
    if let Err(error) = save_profile(&state, &profile).await {
        return PublicResponseBody::<UiProfile>::err(&error);
    }
    PublicResponseBody::ok(profile)
}

pub async fn api_upload_image(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let kind = match headers
        .get("x-image-kind")
        .and_then(|value| value.to_str().ok())
    {
        Some(kind) => kind,
        None => return PublicResponseBody::<UiProfile>::err("缺少图片类型"),
    };
    let managed_path = match managed_asset_path(kind) {
        Some(path) => path,
        None => return PublicResponseBody::<UiProfile>::err("图片类型必须是 icon 或 background"),
    };
    let bytes = match to_bytes(body, MAX_IMAGE_BYTES + 1).await {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_IMAGE_BYTES => bytes,
        Ok(_) => return PublicResponseBody::<UiProfile>::err("图片不能为空或超过 8 MiB"),
        Err(_) => return PublicResponseBody::<UiProfile>::err("图片超过 8 MiB"),
    };
    if !supported_image(&bytes) {
        return PublicResponseBody::<UiProfile>::err("仅支持 PNG、GIF、JPEG 或 WebP 图片");
    }

    let _guard = state.ui_profile.lock().await;
    let asset_path = state.apppath.public_dir.join(managed_path);
    if let Some(parent) = asset_path.parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            return PublicResponseBody::<UiProfile>::err(&format!("创建图片目录失败：{error}"));
        }
    }
    if let Err(error) = atomic_write(&asset_path, &bytes).await {
        return PublicResponseBody::<UiProfile>::err(&format!("保存图片失败：{error}"));
    }

    let mut profile = match UiProfile::load(&state.apppath.ui_profile_file) {
        Ok(profile) => profile,
        Err(error) => return PublicResponseBody::<UiProfile>::err(&error),
    };
    if kind == "icon" {
        profile.icon = ICON_ASSET_PATH.to_owned();
    } else {
        profile.background_image = BACKGROUND_ASSET_PATH.to_owned();
    }
    if let Err(error) = save_profile(&state, &profile).await {
        return PublicResponseBody::<UiProfile>::err(&error);
    }
    PublicResponseBody::ok(profile)
}

pub async fn api_remove_image(
    State(state): State<WebState>,
    Json(payload): Json<ImageRequest>,
) -> Response {
    let managed_path = match managed_asset_path(&payload.kind) {
        Some(path) => path,
        None => return PublicResponseBody::<UiProfile>::err("图片类型必须是 icon 或 background"),
    };
    let _guard = state.ui_profile.lock().await;
    let mut profile = match UiProfile::load(&state.apppath.ui_profile_file) {
        Ok(profile) => profile,
        Err(error) => return PublicResponseBody::<UiProfile>::err(&error),
    };
    if payload.kind == "icon" {
        profile.icon.clear();
    } else {
        profile.background_image.clear();
    }
    if let Err(error) = save_profile(&state, &profile).await {
        return PublicResponseBody::<UiProfile>::err(&error);
    }
    let asset_path = state.apppath.public_dir.join(managed_path);
    if let Err(error) = tokio::fs::remove_file(&asset_path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            return PublicResponseBody::<UiProfile>::err(&format!("移除图片失败：{error}"));
        }
    }
    PublicResponseBody::ok(profile)
}

fn managed_asset_path(kind: &str) -> Option<&'static str> {
    match kind {
        "icon" => Some(ICON_ASSET_PATH),
        "background" => Some(BACKGROUND_ASSET_PATH),
        _ => None,
    }
}

fn supported_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

async fn save_profile(state: &WebState, profile: &UiProfile) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(profile)
        .map_err(|error| format!("序列化个性化配置失败：{error}"))?;
    atomic_write(&state.apppath.ui_profile_file, &data)
        .await
        .map_err(|error| format!("保存个性化配置失败：{error}"))
}

async fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    tokio::fs::write(&temporary, data).await?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    tokio::fs::rename(&temporary, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_managed_image_kinds_only() {
        assert_eq!(managed_asset_path("icon"), Some(ICON_ASSET_PATH));
        assert_eq!(
            managed_asset_path("background"),
            Some(BACKGROUND_ASSET_PATH)
        );
        assert_eq!(managed_asset_path("../../config"), None);
    }

    #[test]
    fn recognizes_supported_image_magic() {
        assert!(supported_image(b"\x89PNG\r\n\x1a\nrest"));
        assert!(supported_image(b"GIF89arest"));
        assert!(supported_image(b"RIFF0000WEBPrest"));
        assert!(!supported_image(b"<svg></svg>"));
    }
}
