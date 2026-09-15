use axum::extract::State;
use axum::response::Response;
use serde::Serialize;

use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

#[derive(Serialize)]
pub struct ResponseData {
    pub dev: String,
    pub used: u64,
    pub total: u64,
    pub workspace_used: u64,
    pub workspace_files: u64,
    pub workspace_path: String,
    pub public_used: u64,
    pub public_files: u64,
}

fn directory_stats(path: &std::path::Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut pending = vec![path.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    bytes = bytes.saturating_add(metadata.len());
                    files = files.saturating_add(1);
                }
            }
        }
    }

    (bytes, files)
}

pub async fn api_disk_info(State(state): State<WebState>) -> Response {
    #[allow(unused_mut)]
    let mut path = state
        .apppath
        .working_dir
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    #[cfg(target_os = "windows")]
    if path.starts_with(r"\\?\") {
        path = path[4..].to_owned();
    }

    let one_peta_bytes: u64 = 1 * 1024 * 1024 * 1024 * 1024 * 1024;
    let mut usages = (one_peta_bytes, one_peta_bytes, "none".to_owned());

    let disks = sysinfo::Disks::new_with_refreshed_list();

    for disk in disks.list() {
        let name = disk.name().to_str().unwrap().to_owned();
        let mount = disk.mount_point().to_str().unwrap().replace(r"\\", r"\");

        if path.starts_with(&mount) {
            let total = disk.total_space();
            let available = disk.available_space();

            usages = (total - available, total, name);
        }
    }

    let workspace_path = state
        .apppath
        .workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| state.apppath.workspace_dir.clone())
        .to_string_lossy()
        .to_string();
    let workspace_dir = state.apppath.workspace_dir.clone();
    let public_dir = state.apppath.public_dir.clone();
    let ((workspace_used, workspace_files), (public_used, public_files)) =
        tokio::task::spawn_blocking(move || {
            (
                directory_stats(&workspace_dir),
                directory_stats(&public_dir),
            )
        })
        .await
        .unwrap_or_default();

    PublicResponseBody::<ResponseData>::ok(ResponseData {
        used: usages.0,
        total: usages.1,
        dev: usages.2,
        workspace_used,
        workspace_files,
        workspace_path,
        public_used,
        public_files,
    })
}
