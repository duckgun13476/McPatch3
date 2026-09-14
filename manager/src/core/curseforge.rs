use std::collections::HashMap;
use std::collections::LinkedList;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

use crate::config::curseforge_config::CurseForgeConfig;
use crate::core::data::version_meta::ExternalSource;
use crate::core::data::version_meta::FileChange;

const FINGERPRINT_BATCH_SIZE: usize = 1_000;

#[derive(Clone)]
struct Candidate {
    path: String,
    len: u64,
    fingerprint: u32,
}

#[derive(Serialize)]
struct FingerprintRequest {
    fingerprints: Vec<u32>,
}

#[derive(Deserialize)]
struct FingerprintResponse {
    data: FingerprintMatches,
}

#[derive(Deserialize)]
struct FingerprintMatches {
    #[serde(rename = "exactMatches")]
    exact_matches: Vec<CurseForgeMatch>,
}

#[derive(Deserialize)]
struct CurseForgeMatch {
    // The actual downloadable-file metadata is nested under `file`.
    file: CurseForgeFile,
}

#[derive(Deserialize)]
struct CurseForgeFile {
    id: u64,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "fileLength")]
    file_length: u64,
    #[serde(rename = "fileFingerprint")]
    file_fingerprint: i64,
}

/// 为可确认来源的客户端 mod 填充可选外部下载地址。
///
/// 失败时返回错误给 pack 调用方决定是否降级。调用方会把非严格模式降级为
/// 原有 mcpatch 包；此函数本身不会删除或省略 tar 中的文件。
pub fn attach_external_sources(
    changes: &mut LinkedList<FileChange>,
    workspace_dir: &Path,
    config: &CurseForgeConfig,
) -> Result<usize, String> {
    if !config.enabled {
        return Ok(0);
    }

    if config.api_key.trim().is_empty() {
        return Err("CurseForge 外部下载已启用，但 api-key 为空".to_owned());
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
        if external_source.is_some() {
            continue;
        }

        if !is_client_mod_jar(path) {
            continue;
        }

        let file = workspace_dir.join(path);
        let bytes = std::fs::read(&file).map_err(|error| {
            format!("无法读取 CurseForge 候选文件 {}: {}", file.display(), error)
        })?;

        if bytes.len() as u64 != *len {
            return Err(format!(
                "CurseForge 候选文件长度在扫描中变化: {}",
                file.display()
            ));
        }

        candidates.push(Candidate {
            path: path.clone(),
            len: *len,
            fingerprint: murmur2_hash32(&bytes, 1),
        });
    }

    if candidates.is_empty() {
        return Ok(0);
    }

    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|error| format!("无法创建 CurseForge HTTP 客户端: {}", error))?;
    let endpoint = format!(
        "https://api.curseforge.com/v1/fingerprints/{}",
        config.game_id
    );
    let mut matches = HashMap::<u32, CurseForgeFile>::new();

    for batch in candidates.chunks(FINGERPRINT_BATCH_SIZE) {
        let request = FingerprintRequest {
            fingerprints: batch
                .iter()
                .map(|candidate| candidate.fingerprint)
                .collect(),
        };
        let response = client
            .post(&endpoint)
            .header("Accept", "application/json")
            .header("x-api-key", &config.api_key)
            .json(&request)
            .send()
            .map_err(|error| format!("CurseForge fingerprint 查询失败: {}", error))?
            .error_for_status()
            .map_err(|error| format!("CurseForge fingerprint 查询返回失败状态: {}", error))?
            .json::<FingerprintResponse>()
            .map_err(|error| format!("无法解析 CurseForge fingerprint 响应: {}", error))?;

        for matched in response.data.exact_matches {
            matches.insert(matched.file.file_fingerprint as u32, matched.file);
        }
    }

    let mut resolved = HashMap::new();
    for candidate in candidates {
        let Some(file) = matches.get(&candidate.fingerprint) else {
            continue;
        };

        // fingerprint 命中仍须校验长度，避免 API 异常响应把不同对象挂到同一条目。
        if file.file_length != candidate.len {
            continue;
        }

        resolved.insert(
            candidate.path,
            ExternalSource {
                provider: "curseforge".to_owned(),
                url: curseforge_cdn_url(&config.cdn_base_url, file.id, &file.file_name),
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

fn curseforge_cdn_url(base: &str, file_id: u64, filename: &str) -> String {
    let base = base.trim_end_matches('/');
    format!(
        "{}/{}/{}/{}",
        base,
        file_id / 1_000,
        file_id % 1_000,
        urlencoding::encode(filename)
    )
}

/// CurseForge 的 Minecraft 文件指纹：忽略空白字节后的 MurmurHash2 32-bit，seed=1。
fn murmur2_hash32(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;

    let data = data
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, b'\t' | b'\n' | b'\r' | b' '))
        .collect::<Vec<_>>();
    let mut length = data.len();
    let mut index = 0;
    let mut hash = seed ^ length as u32;

    while length >= 4 {
        let mut k = u32::from_le_bytes(data[index..index + 4].try_into().unwrap());
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        hash = hash.wrapping_mul(M);
        hash ^= k;
        index += 4;
        length -= 4;
    }

    match length {
        3 => {
            hash ^= (data[index + 2] as u32) << 16;
            hash ^= (data[index + 1] as u32) << 8;
            hash ^= data[index] as u32;
            hash = hash.wrapping_mul(M);
        }
        2 => {
            hash ^= (data[index + 1] as u32) << 8;
            hash ^= data[index] as u32;
            hash = hash.wrapping_mul(M);
        }
        1 => {
            hash ^= data[index] as u32;
            hash = hash.wrapping_mul(M);
        }
        _ => (),
    }

    hash ^= hash >> 13;
    hash = hash.wrapping_mul(M);
    hash ^ (hash >> 15)
}

#[cfg(test)]
mod tests {
    use super::curseforge_cdn_url;
    use super::FingerprintResponse;
    use super::murmur2_hash32;

    #[test]
    fn builds_pcl_compatible_curseforge_cdn_url() {
        assert_eq!(
            curseforge_cdn_url(
                "https://mediafilez.forgecdn.net/files",
                3424504,
                "example file.jar"
            ),
            "https://mediafilez.forgecdn.net/files/3424/504/example%20file.jar",
        );
    }

    #[test]
    fn murmur2_is_stable_for_curseforge_seed() {
        assert_eq!(murmur2_hash32(b"hello", 1), 0xa631_918e);
        assert_eq!(murmur2_hash32(b"h e\nl\tl\ro", 1), 0xa631_918e);
    }

    #[test]
    fn reads_nested_curseforge_exact_match_file() {
        let response: FingerprintResponse = serde_json::from_str(
            r#"{"data":{"exactMatches":[{"id":123,"file":{"id":456,"fileName":"example.jar","fileLength":789,"fileFingerprint":-42}}]}}"#,
        )
        .unwrap();

        let matched = &response.data.exact_matches[0].file;
        assert_eq!(matched.id, 456);
        assert_eq!(matched.file_fingerprint, -42);
    }
}
