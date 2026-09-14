use std::rc::Weak;

use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_stream::StreamExt;

use crate::core::data::index_file::IndexFile;
use crate::core::data::pending_changes::normalize_client_path;
use crate::core::file_hash::calculate_sha256;
use crate::diff::abstract_file::AbstractFile;
use crate::diff::history_file::HistoryFile;
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

const MAX_FINGERPRINT_UPLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const HASH_DELETE_STAGING_DIR: &str = ".mcpatch-hash-delete-staging";

#[derive(Deserialize)]
pub struct ChangePathRequest {
    path: String,
}

#[derive(Serialize)]
pub struct ChangePathResponse {
    path: String,
    removed: bool,
}

#[derive(Serialize)]
pub struct HashDeletionResponse {
    path: String,
    sha256: String,
    len: u64,
    staged: bool,
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
    let path = match decode_path_header(&headers) {
        Ok(path) => path,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if state.apppath.workspace_dir.join(&path).exists() {
        return PublicResponseBody::<()>::err(
            "目标路径仍存在于当前工作区；请从变化预览中右键转换该新增项",
        );
    }
    if let Err(error) = require_path_absent_from_history(&state, &path) {
        return PublicResponseBody::<()>::err(&error);
    }
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

    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    if let Err(error) = updated.add_hash_deletion(&path, &sha256, len, None) {
        return PublicResponseBody::<()>::err(&error);
    }
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(HashDeletionResponse {
        path,
        sha256,
        len,
        staged: false,
        removed: false,
    })
}

pub async fn api_convert_add_to_hash_deletion(
    State(state): State<WebState>,
    Json(payload): Json<ChangePathRequest>,
) -> Response {
    let path = match normalize_client_path(&payload.path) {
        Ok(path) => path,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if let Err(error) = require_path_absent_from_history(&state, &path) {
        return PublicResponseBody::<()>::err(&error);
    }

    let source = state.apppath.workspace_dir.join(&path);
    let metadata = match std::fs::symlink_metadata(&source) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        Ok(_) => return PublicResponseBody::<()>::err("新增项不是普通文件"),
        Err(error) => return PublicResponseBody::<()>::err(&format!("读取新增文件失败: {error}")),
    };
    let len = metadata.len();
    let mut file = match std::fs::File::open(&source) {
        Ok(file) => file,
        Err(error) => return PublicResponseBody::<()>::err(&format!("打开新增文件失败: {error}")),
    };
    let sha256 = calculate_sha256(&mut file);
    let staged_file = format!("{:x}.bin", Sha256::digest(path.as_bytes()));
    let staging_dir = state.apppath.working_dir.join(HASH_DELETE_STAGING_DIR);
    let staged_path = staging_dir.join(&staged_file);
    if staged_path.exists() {
        return PublicResponseBody::<()>::err("该新增项已有未完成的哈希删除暂存文件");
    }
    if let Err(error) = std::fs::create_dir_all(&staging_dir) {
        return PublicResponseBody::<()>::err(&format!("创建哈希删除暂存目录失败: {error}"));
    }
    if let Err(error) = std::fs::rename(&source, &staged_path) {
        return PublicResponseBody::<()>::err(&format!("暂存新增文件失败: {error}"));
    }

    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    if let Err(error) = updated.add_hash_deletion(&path, &sha256, len, Some(staged_file.clone())) {
        let _ = std::fs::rename(&staged_path, &source);
        return PublicResponseBody::<()>::err(&error);
    }
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        let _ = std::fs::rename(&staged_path, &source);
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(HashDeletionResponse {
        path,
        sha256,
        len,
        staged: true,
        removed: false,
    })
}

pub async fn api_remove_hash_deletion(
    State(state): State<WebState>,
    Json(payload): Json<ChangePathRequest>,
) -> Response {
    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    let removed = match updated.remove_hash_deletion(&payload.path) {
        Ok(removed) => removed,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    let Some(removed) = removed else {
        return PublicResponseBody::ok(HashDeletionResponse {
            path: payload.path,
            sha256: String::new(),
            len: 0,
            staged: false,
            removed: false,
        });
    };

    let mut restored = None;
    if let Some(staged_file) = &removed.staged_file {
        let staged_path = state
            .apppath
            .working_dir
            .join(HASH_DELETE_STAGING_DIR)
            .join(staged_file);
        let target = state.apppath.workspace_dir.join(&removed.path);
        if target.exists() {
            return PublicResponseBody::<()>::err("撤销失败：工作区目标路径已被占用");
        }
        let mut file = match std::fs::File::open(&staged_path) {
            Ok(file) => file,
            Err(error) => {
                return PublicResponseBody::<()>::err(&format!("读取暂存文件失败: {error}"))
            }
        };
        let actual_len = file
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(u64::MAX);
        if actual_len != removed.len || calculate_sha256(&mut file) != removed.sha256 {
            return PublicResponseBody::<()>::err("撤销失败：暂存文件内容已变化");
        }
        if let Some(parent) = target.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return PublicResponseBody::<()>::err(&format!("恢复目标目录失败: {error}"));
            }
        }
        if let Err(error) = std::fs::rename(&staged_path, &target) {
            return PublicResponseBody::<()>::err(&format!("恢复新增文件失败: {error}"));
        }
        restored = Some((target, staged_path));
    }

    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        if let Some((target, staged_path)) = restored {
            let _ = std::fs::rename(target, staged_path);
        }
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(HashDeletionResponse {
        path: removed.path,
        sha256: removed.sha256,
        len: removed.len,
        staged: removed.staged_file.is_some(),
        removed: true,
    })
}

fn decode_path_header(headers: &HeaderMap) -> Result<String, String> {
    let value = headers
        .get("file-path")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "缺少 file-path 请求头".to_owned())?;
    let decoded = urlencoding::decode(value).map_err(|error| format!("路径解码失败: {error}"))?;
    normalize_client_path(&decoded)
}

fn require_path_absent_from_history(state: &WebState, path: &str) -> Result<(), String> {
    let index = IndexFile::load_from_file(&state.apppath.index_file);
    let mut history = HistoryFile::new_dir("workspace_root", Weak::new());
    for (_index, meta) in index.read_all_metas(&state.apppath.public_dir) {
        history.replay_operations(&meta);
    }
    if history.find(path).is_some() {
        Err("该路径属于已发布文件，应使用普通删除而不是哈希删除".to_owned())
    } else {
        Ok(())
    }
}
