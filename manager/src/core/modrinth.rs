use std::collections::HashMap;
use std::collections::LinkedList;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use sha1::Digest;

use crate::config::modrinth_config::ModrinthConfig;
use crate::core::data::version_meta::ExternalSource;
use crate::core::data::version_meta::FileChange;

const HASH_BATCH_SIZE: usize = 1_000;
const VERSION_FILES_ENDPOINT: &str = "https://api.modrinth.com/v2/version_files";

#[derive(Clone)]
struct Candidate {
    path: String,
    len: u64,
    sha1: String,
}

#[derive(Serialize)]
struct VersionFilesRequest<'a> {
    hashes: Vec<&'a str>,
    algorithm: &'static str,
}

#[derive(Deserialize)]
struct ModrinthVersion {
    files: Vec<ModrinthFile>,
}

#[derive(Deserialize)]
struct ModrinthFile {
    hashes: HashMap<String, String>,
    url: String,
    size: u64,
}

/// 优先从 Modrinth 的公开 SHA-1 索引匹配客户端下载源。
///
/// 只有长度和 SHA-1 同时相符的文件才会被采用。未命中的变更保持空 source，
/// 后续可由 CurseForge 或原 mcpatch 分片继续处理。
pub fn attach_external_sources(
    changes: &mut LinkedList<FileChange>,
    workspace_dir: &Path,
    config: &ModrinthConfig,
) -> Result<usize, String> {
    if !config.enabled {
        return Ok(0);
    }

    let mut candidates = Vec::new();
    for change in changes.iter() {
        let FileChange::UpdateFile {
            path,
            len,
            external_source,
            ..
        } = change
        else {
            continue;
        };
        if external_source.is_some() || !is_client_mod_jar(path) {
            continue;
        }

        let file = workspace_dir.join(path);
        let metadata = std::fs::metadata(&file)
            .map_err(|error| format!("无法读取 Modrinth 候选文件 {}: {}", file.display(), error))?;
        if metadata.len() != *len {
            return Err(format!(
                "Modrinth 候选文件长度在扫描中变化: {}",
                file.display()
            ));
        }
        candidates.push(Candidate {
            path: path.clone(),
            len: *len,
            sha1: sha1_file(&file)?,
        });
    }
    if candidates.is_empty() {
        return Ok(0);
    }

    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|error| format!("无法创建 Modrinth HTTP 客户端: {}", error))?;
    let mut versions = HashMap::<String, ModrinthVersion>::new();
    for batch in candidates.chunks(HASH_BATCH_SIZE) {
        let response = client
            .post(VERSION_FILES_ENDPOINT)
            .json(&VersionFilesRequest {
                hashes: batch
                    .iter()
                    .map(|candidate| candidate.sha1.as_str())
                    .collect(),
                algorithm: "sha1",
            })
            .send()
            .map_err(|error| format!("Modrinth SHA-1 查询失败: {}", error))?
            .error_for_status()
            .map_err(|error| format!("Modrinth SHA-1 查询返回失败状态: {}", error))?
            .json::<HashMap<String, ModrinthVersion>>()
            .map_err(|error| format!("无法解析 Modrinth SHA-1 查询响应: {}", error))?;
        versions.extend(response);
    }

    let mut resolved = HashMap::new();
    for candidate in candidates {
        let Some(version) = versions.get(&candidate.sha1) else {
            continue;
        };
        let Some(file) = version.files.iter().find(|file| {
            file.size == candidate.len
                && file
                    .hashes
                    .get("sha1")
                    .is_some_and(|hash| hash.eq_ignore_ascii_case(&candidate.sha1))
        }) else {
            continue;
        };
        resolved.insert(
            candidate.path,
            ExternalSource {
                provider: "modrinth".to_owned(),
                url: file.url.clone(),
                fallback_to_mcpatch: config.fallback_to_mcpatch,
            },
        );
    }

    let mut attached = 0;
    for change in changes.iter_mut() {
        let FileChange::UpdateFile {
            path,
            external_source,
            ..
        } = change
        else {
            continue;
        };
        if let Some(source) = resolved.remove(path) {
            *external_source = Some(source);
            attached += 1;
        }
    }
    Ok(attached)
}

fn is_client_mod_jar(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.starts_with(".minecraft/mods/") && normalized.ends_with(".jar")
}

fn sha1_file(path: &Path) -> Result<String, String> {
    let mut input = std::fs::File::open(path)
        .map_err(|error| format!("无法打开 Modrinth 候选文件 {}: {}", path.display(), error))?;
    let mut digest = sha1::Sha1::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("无法计算 Modrinth SHA-1 {}: {}", path.display(), error))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::is_client_mod_jar;

    #[test]
    fn only_accepts_client_mod_jars() {
        assert!(is_client_mod_jar(".minecraft/mods/example.jar"));
        assert!(!is_client_mod_jar(".minecraft/config/example.jar"));
        assert!(!is_client_mod_jar(".minecraft/mods/example.zip"));
    }
}
