use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::response::Response;
use serde::Serialize;

use crate::core::file_hash::calculate_sha256;
use crate::task::pack::UPDATER_SOURCE_PATH;
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

const MAX_UPDATER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Serialize)]
pub struct UpdaterSourceStatus {
    exists: bool,
    size: u64,
    sha256: String,
}

pub async fn api_status(State(state): State<WebState>) -> Response {
    match updater_status(&state.apppath.workspace_dir.join(UPDATER_SOURCE_PATH)) {
        Ok(status) => PublicResponseBody::ok(status),
        Err(error) => PublicResponseBody::<UpdaterSourceStatus>::err(&error),
    }
}

pub async fn api_upload(
    State(state): State<WebState>,
    body: Body,
) -> Response {
    let bytes = match to_bytes(body, MAX_UPDATER_BYTES + 1).await {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_UPDATER_BYTES => bytes,
        Ok(_) => return PublicResponseBody::<UpdaterSourceStatus>::err("EXE 不能为空或超过 64 MiB"),
        Err(_) => return PublicResponseBody::<UpdaterSourceStatus>::err("EXE 超过 64 MiB"),
    };
    if !is_windows_pe(&bytes) {
        return PublicResponseBody::<UpdaterSourceStatus>::err("文件不是有效的 Windows PE 可执行程序");
    }

    let target = state.apppath.workspace_dir.join(UPDATER_SOURCE_PATH);
    if let Some(parent) = target.parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            return PublicResponseBody::<UpdaterSourceStatus>::err(&format!("创建更新器目录失败：{error}"));
        }
    }
    if let Err(error) = atomic_write(&target, &bytes).await {
        return PublicResponseBody::<UpdaterSourceStatus>::err(&format!("保存更新器失败：{error}"));
    }
    state.status.lock().await.invalidate();

    match updater_status(&target) {
        Ok(status) => PublicResponseBody::ok(status),
        Err(error) => PublicResponseBody::<UpdaterSourceStatus>::err(&error),
    }
}

fn updater_status(path: &std::path::Path) -> Result<UpdaterSourceStatus, String> {
    if !path.exists() {
        return Ok(UpdaterSourceStatus { exists: false, size: 0, sha256: String::new() });
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("读取更新器失败：{error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("更新器源必须是普通文件".to_owned());
    }
    let sha256 = calculate_sha256(
        &mut std::fs::File::open(path).map_err(|error| format!("打开更新器失败：{error}"))?,
    );
    Ok(UpdaterSourceStatus { exists: true, size: metadata.len(), sha256 })
}

fn is_windows_pe(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    offset.checked_add(4).is_some_and(|end| end <= bytes.len() && &bytes[offset..end] == b"PE\0\0")
}

async fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    tokio::fs::write(&temporary, data).await?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    tokio::fs::rename(temporary, path).await
}

#[cfg(test)]
mod tests {
    use super::is_windows_pe;

    #[test]
    fn accepts_pe_and_rejects_extension_only_files() {
        let mut pe = vec![0u8; 0x84];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&(0x80u32).to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        assert!(is_windows_pe(&pe));
        assert!(!is_windows_pe(b"MZ fake exe"));
        assert!(!is_windows_pe(b"not an exe"));
    }
}
