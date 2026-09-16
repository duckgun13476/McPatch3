use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::UNIX_EPOCH;

use axum::extract::State;
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::core::data::index_file::IndexFile;
use crate::core::data::version_meta::{ClientHashDeletion, FileChange, VersionMeta};
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

#[derive(Deserialize)]
pub struct RequestBody {
    label: String,
}

#[derive(Serialize)]
pub struct ResponseBody {
    label: String,
    filename: String,
    package_hash: String,
    archive_size: u64,
    payload_size: u64,
    change_logs: String,
    counts: ChangeCounts,
    changes: Vec<HistoryChange>,
}

#[derive(Default, Serialize)]
pub struct ChangeCounts {
    total: usize,
    files: usize,
    deletions: usize,
    moves: usize,
    directories: usize,
    hash_deletions: usize,
}

#[derive(Serialize)]
pub struct HistoryChange {
    operation: &'static str,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
    hash: Option<String>,
    len: Option<u64>,
    modified: Option<u64>,
    external_provider: Option<String>,
}

pub async fn api_version_history(
    State(state): State<WebState>,
    Json(payload): Json<RequestBody>,
) -> Response {
    let label = payload.label.trim().to_owned();
    if label.is_empty() || label.len() > 240 || label.chars().any(char::is_control) {
        return PublicResponseBody::<()>::err("版本号无效");
    }

    let apppath = state.apppath.clone();
    let result = tokio::task::spawn_blocking(move || {
        catch_unwind(AssertUnwindSafe(|| {
            let index_file = IndexFile::load_from_file(&apppath.index_file);
            let (index, meta) = index_file
                .read_meta(&apppath.public_dir, &label)
                .ok_or_else(|| "找不到指定版本".to_owned())?;
            let archive_size = index.archive_size.unwrap_or_else(|| {
                std::fs::metadata(apppath.public_dir.join(&index.filename))
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
            });
            Ok::<_, String>(history_response(
                index.filename,
                index.hash,
                archive_size,
                meta,
            ))
        }))
        .map_err(|_| "更新包元数据读取失败，请先校验历史更新包".to_owned())?
    })
    .await;

    match result {
        Ok(Ok(response)) => PublicResponseBody::ok(response),
        Ok(Err(error)) => PublicResponseBody::<()>::err(&error),
        Err(error) => PublicResponseBody::<()>::err(&format!("读取历史失败: {error}")),
    }
}

fn history_response(
    filename: String,
    package_hash: String,
    archive_size: u64,
    meta: VersionMeta,
) -> ResponseBody {
    let mut counts = ChangeCounts::default();
    let mut changes = meta
        .changes
        .iter()
        .map(|change| history_change(change, &mut counts))
        .collect::<Vec<_>>();
    for deletion in &meta.client_hash_deletions {
        changes.push(hash_deletion(deletion, &mut counts));
    }
    counts.total = changes.len();
    let payload_size = changes
        .iter()
        .filter(|change| change.operation == "update-file")
        .filter_map(|change| change.len)
        .sum();

    ResponseBody {
        label: meta.label,
        filename,
        package_hash,
        archive_size,
        payload_size,
        change_logs: meta.logs,
        counts,
        changes,
    }
}

fn history_change(change: &FileChange, counts: &mut ChangeCounts) -> HistoryChange {
    match change {
        FileChange::CreateFolder { path } => {
            counts.directories += 1;
            simple_change("create-directory", path)
        }
        FileChange::UpdateFile {
            path,
            hash,
            len,
            modified,
            external_source,
            ..
        } => {
            counts.files += 1;
            HistoryChange {
                operation: "update-file",
                path: Some(path.clone()),
                from: None,
                to: None,
                hash: Some(hash.clone()),
                len: Some(*len),
                modified: modified
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|duration| duration.as_secs()),
                external_provider: external_source
                    .as_ref()
                    .map(|source| source.provider.clone()),
            }
        }
        FileChange::DeleteFolder { path } => {
            counts.directories += 1;
            simple_change("delete-directory", path)
        }
        FileChange::DeleteFile { path } => {
            counts.deletions += 1;
            simple_change("delete-file", path)
        }
        FileChange::MoveFile { from, to } => {
            counts.moves += 1;
            HistoryChange {
                operation: "move-file",
                path: None,
                from: Some(from.clone()),
                to: Some(to.clone()),
                hash: None,
                len: None,
                modified: None,
                external_provider: None,
            }
        }
    }
}

fn simple_change(operation: &'static str, path: &str) -> HistoryChange {
    HistoryChange {
        operation,
        path: Some(path.to_owned()),
        from: None,
        to: None,
        hash: None,
        len: None,
        modified: None,
        external_provider: None,
    }
}

fn hash_deletion(deletion: &ClientHashDeletion, counts: &mut ChangeCounts) -> HistoryChange {
    counts.hash_deletions += 1;
    HistoryChange {
        operation: "delete-file-by-hash",
        path: Some(deletion.path.clone()),
        from: None,
        to: None,
        hash: Some(deletion.sha256.clone()),
        len: Some(deletion.len),
        modified: None,
        external_provider: None,
    }
}

#[cfg(test)]
mod tests {
    use super::history_response;
    use crate::core::data::version_meta::{ClientHashDeletion, FileChange, VersionMeta};
    use std::collections::LinkedList;
    use std::time::UNIX_EPOCH;

    #[test]
    fn preserves_every_historical_operation_and_count() {
        let changes = LinkedList::from([
            FileChange::CreateFolder { path: "a".into() },
            FileChange::UpdateFile {
                path: "a/new.jar".into(),
                hash: "hash".into(),
                len: 42,
                modified: UNIX_EPOCH,
                offset: 0,
                external_source: None,
            },
            FileChange::MoveFile {
                from: "a/old".into(),
                to: "a/new".into(),
            },
            FileChange::DeleteFile {
                path: "a/gone".into(),
            },
            FileChange::DeleteFolder {
                path: "empty".into(),
            },
        ]);
        let meta = VersionMeta::new(
            "v1".into(),
            "notes".into(),
            changes,
            vec![ClientHashDeletion {
                path: "legacy.jar".into(),
                sha256: "a".repeat(64),
                len: 9,
            }],
        );

        let response = history_response("v1.tar".into(), "package".into(), 100, meta);
        assert_eq!(response.counts.total, 6);
        assert_eq!(response.counts.files, 1);
        assert_eq!(response.counts.deletions, 1);
        assert_eq!(response.counts.moves, 1);
        assert_eq!(response.counts.directories, 2);
        assert_eq!(response.counts.hash_deletions, 1);
        assert_eq!(response.payload_size, 42);
        assert_eq!(response.changes[5].operation, "delete-file-by-hash");
    }
}
