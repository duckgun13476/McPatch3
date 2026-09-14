use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_stream::StreamExt;

use crate::core::file_hash::calculate_sha256;
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

const MAX_FINGERPRINT_UPLOAD_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ChangePathRequest {
    path: String,
}

#[derive(Serialize)]
pub struct ChangePathResponse {
    path: String,
    removed: bool,
}

#[derive(Deserialize)]
pub struct HashDeletionRequest {
    sha256: String,
}

#[derive(Serialize)]
pub struct HashDeletionResponse {
    sha256: String,
    len: u64,
    name_hint: String,
    removed: bool,
}

pub async fn api_add_delete_file(
    State(state): State<WebState>,
    Json(payload): Json<ChangePathRequest>,
) -> Response {
    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    let path = match updated.add_forced_deletion(&payload.path) {
        Ok(path) => path,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(ChangePathResponse {
        path,
        removed: false,
    })
}

pub async fn api_remove_delete_file(
    State(state): State<WebState>,
    Json(payload): Json<ChangePathRequest>,
) -> Response {
    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    let removed = match updated.remove_forced_deletion(&payload.path) {
        Ok(removed) => removed,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(ChangePathResponse {
        path: payload.path,
        removed,
    })
}

pub async fn api_add_hash_deletion(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let name_hint = match headers
        .get("file-name")
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => match urlencoding::decode(value) {
            Ok(value) => value.into_owned(),
            Err(error) => {
                return PublicResponseBody::<()>::err(&format!("文件名解码失败: {error}"))
            }
        },
        None => return PublicResponseBody::<()>::err("缺少 file-name 请求头"),
    };
    if let Some(len) = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if len > MAX_FINGERPRINT_UPLOAD_BYTES {
            return PublicResponseBody::<()>::err("客户端删除指纹文件不能超过 1 GiB");
        }
    }

    let mut hasher = Sha256::new();
    let mut len = 0u64;
    let mut stream = body.into_data_stream();
    while let Some(frame) = stream.next().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return PublicResponseBody::<()>::err(&format!("读取上传文件失败: {error}"))
            }
        };
        len = len.saturating_add(frame.len() as u64);
        if len > MAX_FINGERPRINT_UPLOAD_BYTES {
            return PublicResponseBody::<()>::err("客户端删除指纹文件不能超过 1 GiB");
        }
        hasher.update(&frame);
    }
    let sha256 = format!("{:x}", hasher.finalize());

    let mods_dir = state.apppath.workspace_dir.join(".minecraft/mods");
    if mods_dir.exists() {
        let entries = match std::fs::read_dir(&mods_dir) {
            Ok(entries) => entries,
            Err(error) => {
                return PublicResponseBody::<()>::err(&format!(
                    "读取当前模组目录失败({mods_dir:?}): {error}"
                ))
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    return PublicResponseBody::<()>::err(&format!("读取当前模组条目失败: {error}"))
                }
            };
            let metadata = match entry.metadata() {
                Ok(metadata) if metadata.is_file() && metadata.len() == len => metadata,
                Ok(_) => continue,
                Err(error) => {
                    return PublicResponseBody::<()>::err(&format!(
                        "读取当前模组信息失败({:?}): {error}",
                        entry.path()
                    ))
                }
            };
            let _ = metadata;
            let mut file = match std::fs::File::open(entry.path()) {
                Ok(file) => file,
                Err(error) => {
                    return PublicResponseBody::<()>::err(&format!(
                        "打开当前模组失败({:?}): {error}",
                        entry.path()
                    ))
                }
            };
            if calculate_sha256(&mut file) == sha256 {
                return PublicResponseBody::<()>::err(
                    "该文件仍存在于当前服务器工作区，不能标记为客户端删除",
                );
            }
        }
    }

    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    if let Err(error) = updated.add_hash_deletion(&sha256, len, &name_hint) {
        return PublicResponseBody::<()>::err(&error);
    }
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(HashDeletionResponse {
        sha256,
        len,
        name_hint,
        removed: false,
    })
}

pub async fn api_remove_hash_deletion(
    State(state): State<WebState>,
    Json(payload): Json<HashDeletionRequest>,
) -> Response {
    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    let removed = match updated.remove_hash_deletion(&payload.sha256) {
        Ok(removed) => removed,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(HashDeletionResponse {
        sha256: payload.sha256,
        len: 0,
        name_hint: String::new(),
        removed,
    })
}
