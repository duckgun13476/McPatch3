use std::collections::HashMap;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::core::data::version_meta::ExternalSource;

const MODRINTH_ENDPOINT: &str = "https://api.modrinth.com/v2/version_files";
const HASH_BATCH_SIZE: usize = 100;
const FINGERPRINT_BATCH_SIZE: usize = 1_000;
const MAX_FILES: usize = 2_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BootstrapResolveRequest {
    pub schema: u32,
    pub files: Vec<BootstrapCandidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BootstrapCandidate {
    pub path: String,
    pub length: u64,
    pub sha256: String,
    pub sha1: String,
    pub curseforge_fingerprint: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BootstrapResolveResponse {
    pub schema: u32,
    pub files: Vec<BootstrapResolvedFile>,
    pub unmatched: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BootstrapResolvedFile {
    pub path: String,
    pub length: u64,
    pub sha256: String,
    pub external_source: ExternalSource,
}

#[derive(Serialize)]
struct ModrinthRequest<'a> {
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

#[derive(Serialize)]
struct CurseForgeRequest {
    fingerprints: Vec<u32>,
}

#[derive(Deserialize)]
struct CurseForgeResponse {
    data: CurseForgeMatches,
}

#[derive(Deserialize)]
struct CurseForgeMatches {
    #[serde(rename = "exactMatches")]
    exact_matches: Vec<CurseForgeMatch>,
}

#[derive(Deserialize)]
struct CurseForgeMatch {
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

pub fn resolve_from_stdio(config: &Config) -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("failed to read resolver input: {error}"))?;
    let request: BootstrapResolveRequest =
        serde_json::from_str(&input).map_err(|error| format!("invalid resolver input: {error}"))?;
    let response = resolve_external_sources(request, config)?;
    serde_json::to_writer(std::io::stdout(), &response)
        .map_err(|error| format!("failed to write resolver output: {error}"))?;
    std::io::stdout()
        .write_all(b"\n")
        .map_err(|error| format!("failed to finish resolver output: {error}"))?;
    Ok(())
}

pub fn resolve_external_sources(
    request: BootstrapResolveRequest,
    config: &Config,
) -> Result<BootstrapResolveResponse, String> {
    validate_request(&request)?;
    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|error| format!("failed to create resolver HTTP client: {error}"))?;
    let mut resolved = HashMap::<String, ExternalSource>::new();
    if config.modrinth.enabled {
        resolve_modrinth(&client, &request.files, &mut resolved)?;
    }
    if config.curseforge.enabled && !config.curseforge.api_key.trim().is_empty() {
        resolve_curseforge(&client, &request.files, &mut resolved, config)?;
    }

    let mut files = Vec::new();
    let mut unmatched = Vec::new();
    for candidate in request.files {
        match resolved.remove(&candidate.path) {
            Some(external_source) => files.push(BootstrapResolvedFile {
                path: candidate.path,
                length: candidate.length,
                sha256: candidate.sha256,
                external_source,
            }),
            None => unmatched.push(candidate.path),
        }
    }
    Ok(BootstrapResolveResponse {
        schema: 1,
        files,
        unmatched,
    })
}

fn validate_request(request: &BootstrapResolveRequest) -> Result<(), String> {
    if request.schema != 1 {
        return Err(format!(
            "unsupported bootstrap resolver schema: {}",
            request.schema
        ));
    }
    if request.files.len() > MAX_FILES {
        return Err(format!(
            "too many bootstrap candidates: {} > {MAX_FILES}",
            request.files.len()
        ));
    }
    for file in &request.files {
        let normalized = file.path.replace('\\', "/");
        if !normalized.starts_with(".minecraft/mods/")
            || !normalized.ends_with(".jar")
            || normalized.contains("/../")
            || normalized.contains("//")
        {
            return Err(format!("unsafe bootstrap candidate path: {}", file.path));
        }
        if file.length == 0
            || file.sha1.len() != 40
            || !file.sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!(
                "invalid bootstrap candidate fingerprint: {}",
                file.path
            ));
        }
    }
    Ok(())
}

fn resolve_modrinth(
    client: &reqwest::blocking::Client,
    candidates: &[BootstrapCandidate],
    resolved: &mut HashMap<String, ExternalSource>,
) -> Result<(), String> {
    let mut versions = HashMap::<String, ModrinthVersion>::new();
    for batch in candidates.chunks(HASH_BATCH_SIZE) {
        let response = client
            .post(MODRINTH_ENDPOINT)
            .json(&ModrinthRequest {
                hashes: batch
                    .iter()
                    .map(|candidate| candidate.sha1.as_str())
                    .collect(),
                algorithm: "sha1",
            })
            .send()
            .map_err(|error| format!("Modrinth bootstrap lookup failed: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Modrinth bootstrap lookup returned an error: {error}"))?
            .json::<HashMap<String, ModrinthVersion>>()
            .map_err(|error| format!("invalid Modrinth bootstrap response: {error}"))?;
        versions.extend(response);
    }
    for candidate in candidates {
        let Some(version) = versions.get(&candidate.sha1) else {
            continue;
        };
        let Some(file) = version.files.iter().find(|file| {
            file.size == candidate.length
                && file
                    .hashes
                    .get("sha1")
                    .is_some_and(|hash| hash.eq_ignore_ascii_case(&candidate.sha1))
        }) else {
            continue;
        };
        resolved.insert(
            candidate.path.clone(),
            ExternalSource {
                provider: "modrinth".to_owned(),
                url: file.url.clone(),
                fallback_to_mcpatch: false,
            },
        );
    }
    Ok(())
}

fn resolve_curseforge(
    client: &reqwest::blocking::Client,
    candidates: &[BootstrapCandidate],
    resolved: &mut HashMap<String, ExternalSource>,
    config: &Config,
) -> Result<(), String> {
    let unresolved = candidates
        .iter()
        .filter(|candidate| !resolved.contains_key(&candidate.path))
        .collect::<Vec<_>>();
    for batch in unresolved.chunks(FINGERPRINT_BATCH_SIZE) {
        let response = client
            .post(format!(
                "https://api.curseforge.com/v1/fingerprints/{}",
                config.curseforge.game_id
            ))
            .header("Accept", "application/json")
            .header("x-api-key", &config.curseforge.api_key)
            .json(&CurseForgeRequest {
                fingerprints: batch
                    .iter()
                    .map(|candidate| candidate.curseforge_fingerprint)
                    .collect(),
            })
            .send()
            .map_err(|error| format!("CurseForge bootstrap lookup failed: {error}"))?
            .error_for_status()
            .map_err(|error| format!("CurseForge bootstrap lookup returned an error: {error}"))?
            .json::<CurseForgeResponse>()
            .map_err(|error| format!("invalid CurseForge bootstrap response: {error}"))?;
        let matches = response
            .data
            .exact_matches
            .into_iter()
            .map(|matched| (matched.file.file_fingerprint as u32, matched.file))
            .collect::<HashMap<_, _>>();
        for candidate in batch {
            let Some(file) = matches.get(&candidate.curseforge_fingerprint) else {
                continue;
            };
            if file.file_length != candidate.length {
                continue;
            }
            resolved.insert(
                candidate.path.clone(),
                ExternalSource {
                    provider: "curseforge".to_owned(),
                    url: curseforge_cdn_url(
                        &config.curseforge.cdn_base_url,
                        file.id,
                        &file.file_name,
                    ),
                    fallback_to_mcpatch: false,
                },
            );
        }
    }
    Ok(())
}

fn curseforge_cdn_url(base: &str, file_id: u64, filename: &str) -> String {
    format!(
        "{}/{}/{}/{}",
        base.trim_end_matches('/'),
        file_id / 1_000,
        file_id % 1_000,
        urlencoding::encode(filename)
    )
}

#[cfg(test)]
mod tests {
    use super::{validate_request, BootstrapCandidate, BootstrapResolveRequest, MAX_FILES};

    fn candidate(path: &str) -> BootstrapCandidate {
        BootstrapCandidate {
            path: path.to_owned(),
            length: 1,
            sha256: "a".repeat(64),
            sha1: "b".repeat(40),
            curseforge_fingerprint: 42,
        }
    }

    #[test]
    fn accepts_only_bounded_mod_jar_inventory() {
        assert!(validate_request(&BootstrapResolveRequest {
            schema: 1,
            files: vec![candidate(".minecraft/mods/example.jar")]
        })
        .is_ok());
        assert!(validate_request(&BootstrapResolveRequest {
            schema: 1,
            files: vec![candidate(".minecraft/config/example.jar")]
        })
        .is_err());
        assert!(validate_request(&BootstrapResolveRequest {
            schema: 1,
            files: vec![candidate(".minecraft/mods/../secret.jar")]
        })
        .is_err());
        assert!(validate_request(&BootstrapResolveRequest {
            schema: 1,
            files: vec![candidate(".minecraft/mods/a.jar"); MAX_FILES + 1]
        })
        .is_err());
    }
}
