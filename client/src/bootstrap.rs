use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{BusinessError, BusinessResult, ResultToBusinessError};
use crate::log::{log_info, log_error};
use crate::network::Network;
use crate::utility::convert_bytes;

const MANIFEST_NAME: &str = "bootstrap-manifest.json";
const STATE_NAME: &str = "bootstrap-state.json";
const DOWNLOAD_ATTEMPTS: usize = 3;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BootstrapManifest {
    schema: u32,
    #[serde(default = "default_true")]
    initialize: bool,
    files: Vec<BootstrapFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BootstrapFile {
    path: String,
    length: u64,
    sha256: String,
    external_source: ExternalSource,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ExternalSource {
    provider: String,
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct BootstrapState {
    initialize: bool,
    manifest_sha256: String,
}

fn default_true() -> bool {
    true
}

pub fn initialization_requested(exe_dir: &Path) -> bool {
    let manifest_bytes = match std::fs::read(exe_dir.join(MANIFEST_NAME)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let manifest = match serde_json::from_slice::<BootstrapManifest>(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(_) => return true,
    };
    if !manifest.initialize {
        return false;
    }
    let manifest_hash = sha256_bytes(&manifest_bytes);
    let state = std::fs::read(exe_dir.join(STATE_NAME))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BootstrapState>(&bytes).ok());
    !state.is_some_and(|state| {
        !state.initialize && state.manifest_sha256.eq_ignore_ascii_case(&manifest_hash)
    })
}

pub async fn initialize(
    exe_dir: &Path,
    base_dir: &Path,
    network: &Network<'_>,
    #[cfg(target_os = "windows")] ui_cmd: &crate::ui::main_ui::MainUiCommand,
) -> BusinessResult<()> {
    let manifest_path = exe_dir.join(MANIFEST_NAME);
    let manifest_bytes = match tokio::fs::read(&manifest_path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(BusinessError::new(format!(
                "读取整合包初始化清单失败({manifest_path:?})，原因：{error:?}"
            )))
        }
    };
    let manifest_hash = sha256_bytes(&manifest_bytes);
    let manifest: BootstrapManifest = serde_json::from_slice(&manifest_bytes)
        .be(|error| format!("整合包初始化清单解析失败({manifest_path:?})，原因：{error}"))?;
    validate_manifest(&manifest)?;
    if !manifest.initialize {
        return Ok(());
    }

    let state_path = exe_dir.join(STATE_NAME);
    if state_matches(&state_path, &manifest_hash).await {
        log_info("整合包初始化清单已经完整应用");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    ui_cmd.set_visible(true).await;

    let total_bytes = manifest.files.iter().map(|file| file.length).sum::<u64>();
    let mut completed_bytes = 0_u64;
    #[cfg(target_os = "windows")]
    ui_cmd.set_transfer(0, total_bytes).await;

    let temp_root = base_dir.join(".mcpatch-bootstrap");
    tokio::fs::create_dir_all(&temp_root)
        .await
        .be(|error| format!("创建整合包初始化临时目录失败({temp_root:?})，原因：{error:?}"))?;

    for (index, file) in manifest.files.iter().enumerate() {
        let target = checked_target(base_dir, &file.path).await?;
        #[cfg(target_os = "windows")]
        {
            ui_cmd
                .set_label(format!(
                    "正在初始化整合包 ({}/{})",
                    index + 1,
                    manifest.files.len()
                ))
                .await;
            ui_cmd
                .set_label_secondary(
                    target
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&file.path)
                        .to_owned(),
                )
                .await;
        }

        if file_matches(&target, file.length, &file.sha256).await? {
            completed_bytes += file.length;
            update_progress(
                completed_bytes,
                total_bytes,
                #[cfg(target_os = "windows")]
                ui_cmd,
            )
            .await;
            continue;
        }

        let temp_path = temp_root.join(format!("{:04}.download", index));
        download_verified(network, file, &temp_path).await?;
        install_verified_file(&temp_path, &target).await?;
        completed_bytes += file.length;
        update_progress(
            completed_bytes,
            total_bytes,
            #[cfg(target_os = "windows")]
            ui_cmd,
        )
        .await;
    }

    write_state(&state_path, &manifest_hash).await?;
    if let Err(error) = tokio::fs::remove_dir_all(&temp_root).await {
        if error.kind() != ErrorKind::NotFound {
            log_error(format!("清理整合包初始化临时目录失败({temp_root:?})：{error:?}"));
        }
    }
    log_info(format!(
        "整合包初始化完成：{} 个文件，{}",
        manifest.files.len(),
        convert_bytes(total_bytes)
    ));
    Ok(())
}

async fn update_progress(
    completed: u64,
    total: u64,
    #[cfg(target_os = "windows")] ui_cmd: &crate::ui::main_ui::MainUiCommand,
) {
    #[cfg(target_os = "windows")]
    {
        let progress = if total == 0 {
            1000
        } else {
            ((completed as f64 / total as f64) * 1000.0) as u32
        };
        ui_cmd.set_progress(progress.min(1000)).await;
        ui_cmd.set_transfer(completed, total).await;
    }
}

async fn state_matches(path: &Path, manifest_hash: &str) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    serde_json::from_slice::<BootstrapState>(&bytes)
        .map(|state| !state.initialize && state.manifest_sha256.eq_ignore_ascii_case(manifest_hash))
        .unwrap_or(false)
}

fn validate_manifest(manifest: &BootstrapManifest) -> BusinessResult<()> {
    if manifest.schema != 1 {
        return Err(BusinessError::new(format!(
            "不支持的整合包初始化清单版本：{}",
            manifest.schema
        )));
    }
    if manifest.files.len() > 2_000 {
        return Err(BusinessError::new("整合包初始化清单文件数量超过安全上限"));
    }
    for file in &manifest.files {
        validate_relative_mod_path(&file.path)?;
        if file.length == 0
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !matches!(file.external_source.provider.as_str(), "modrinth" | "curseforge")
            || !file.external_source.url.starts_with("https://")
        {
            return Err(BusinessError::new(format!(
                "整合包初始化清单含有无效文件记录：{}",
                file.path
            )));
        }
    }
    Ok(())
}

fn validate_relative_mod_path(path: &str) -> BusinessResult<()> {
    let normalized = path.replace('\\', "/");
    let parsed = Path::new(&normalized);
    let components = parsed.components().collect::<Vec<_>>();
    let safe = components.len() == 3
        && components[0] == Component::Normal(".minecraft".as_ref())
        && components[1] == Component::Normal("mods".as_ref())
        && matches!(components[2], Component::Normal(_))
        && normalized.to_ascii_lowercase().ends_with(".jar");
    if !safe {
        return Err(BusinessError::new(format!(
            "拒绝不安全的整合包初始化路径：{path}"
        )));
    }
    Ok(())
}

async fn checked_target(base_dir: &Path, path: &str) -> BusinessResult<PathBuf> {
    validate_relative_mod_path(path)?;
    let canonical_base = tokio::fs::canonicalize(base_dir)
        .await
        .be(|error| format!("核验整合包根目录失败({base_dir:?})，原因：{error:?}"))?;
    let mods_dir = base_dir.join(".minecraft").join("mods");
    tokio::fs::create_dir_all(&mods_dir)
        .await
        .be(|error| format!("创建整合包模组目录失败({mods_dir:?})，原因：{error:?}"))?;
    let canonical_mods = tokio::fs::canonicalize(&mods_dir)
        .await
        .be(|error| format!("核验整合包模组目录失败({mods_dir:?})，原因：{error:?}"))?;
    if !canonical_mods.starts_with(&canonical_base) {
        return Err(BusinessError::new("拒绝越出整合包根目录的初始化模组路径"));
    }
    Ok(base_dir.join(path.replace('\\', "/")))
}

async fn file_matches(path: &Path, expected_len: u64, expected_sha: &str) -> BusinessResult<bool> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(BusinessError::new(format!(
                "读取初始化目标失败({path:?})，原因：{error:?}"
            )))
        }
    };
    if !metadata.is_file() || metadata.len() != expected_len {
        return Ok(false);
    }
    Ok(sha256_file(path).await?.eq_ignore_ascii_case(expected_sha))
}

async fn download_verified(
    network: &Network<'_>,
    file: &BootstrapFile,
    temp_path: &Path,
) -> BusinessResult<()> {
    let mut last_error = String::from("没有可用下载源");
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let request = network
            .request_external_file(
                &file.external_source.url,
                &format!("bootstrap {}", file.path),
            )
            .await;
        let (reported_len, mut stream) = match request {
            Ok(result) => result,
            Err(error) => {
                last_error = error.reason;
                continue;
            }
        };
        if reported_len != file.length {
            last_error = format!("CDN 长度不符：预期 {}，实际 {reported_len}", file.length);
            continue;
        }
        let mut output = tokio::fs::File::create(temp_path)
            .await
            .be(|error| format!("创建初始化下载文件失败({temp_path:?})，原因：{error:?}"))?;
        let mut written = 0_u64;
        let mut oversized = false;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = stream
                .read(&mut buffer)
                .await
                .be(|error| format!("读取初始化下载流失败({})，原因：{error:?}", file.path))?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > file.length {
                last_error = "CDN 返回内容超过清单长度".to_owned();
                oversized = true;
                break;
            }
            output
                .write_all(&buffer[..read])
                .await
                .be(|error| format!("写入初始化下载文件失败({temp_path:?})，原因：{error:?}"))?;
        }
        output
            .flush()
            .await
            .be(|error| format!("刷新初始化下载文件失败({temp_path:?})，原因：{error:?}"))?;
        drop(output);
        if !oversized
            && written == file.length
            && sha256_file(temp_path).await?.eq_ignore_ascii_case(&file.sha256)
        {
            log_info(format!(
                "初始化下载成功：{} via {} (attempt {attempt}/{DOWNLOAD_ATTEMPTS})",
                file.path, file.external_source.provider
            ));
            return Ok(());
        }
        if !oversized {
            last_error = format!("CDN 文件校验失败（第 {attempt}/{DOWNLOAD_ATTEMPTS} 次）");
        }
    }
    Err(BusinessError::new(format!(
        "整合包初始化下载失败({})，来源：{}，原因：{}",
        file.path, file.external_source.provider, last_error
    )))
}

async fn install_verified_file(temp: &Path, target: &Path) -> BusinessResult<()> {
    let backup = target.with_extension("jar.mcpatch-bootstrap-backup");
    if tokio::fs::try_exists(&backup).await.unwrap_or(false) {
        tokio::fs::remove_file(&backup)
            .await
            .be(|error| format!("清理旧初始化备份失败({backup:?})，原因：{error:?}"))?;
    }
    let had_target = tokio::fs::try_exists(target).await.unwrap_or(false);
    if had_target {
        tokio::fs::rename(target, &backup)
            .await
            .be(|error| format!("备份初始化目标失败({target:?})，原因：{error:?}"))?;
    }
    if let Err(error) = tokio::fs::rename(temp, target).await {
        if had_target {
            let _ = tokio::fs::rename(&backup, target).await;
        }
        return Err(BusinessError::new(format!(
            "安装初始化文件失败({temp:?} => {target:?})，原因：{error:?}"
        )));
    }
    if had_target {
        tokio::fs::remove_file(&backup)
            .await
            .be(|error| format!("清理初始化目标备份失败({backup:?})，原因：{error:?}"))?;
    }
    Ok(())
}

async fn write_state(path: &Path, manifest_hash: &str) -> BusinessResult<()> {
    let temp = path.with_extension("json.temp");
    let bytes = serde_json::to_vec_pretty(&BootstrapState {
        initialize: false,
        manifest_sha256: manifest_hash.to_owned(),
    })
    .be(|error| format!("序列化整合包初始化状态失败，原因：{error}"))?;
    tokio::fs::write(&temp, bytes)
        .await
        .be(|error| format!("写入整合包初始化状态失败({temp:?})，原因：{error:?}"))?;
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        tokio::fs::remove_file(path)
            .await
            .be(|error| format!("替换旧整合包初始化状态失败({path:?})，原因：{error:?}"))?;
    }
    tokio::fs::rename(&temp, path)
        .await
        .be(|error| format!("提交整合包初始化状态失败({temp:?} => {path:?})，原因：{error:?}"))?;
    Ok(())
}

async fn sha256_file(path: &Path) -> BusinessResult<String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .be(|error| format!("打开 SHA-256 校验文件失败({path:?})，原因：{error:?}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .be(|error| format!("读取 SHA-256 校验文件失败({path:?})，原因：{error:?}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::{sha256_bytes, validate_manifest, validate_relative_mod_path, BootstrapFile, BootstrapManifest, ExternalSource};

    #[test]
    fn restricts_bootstrap_targets_to_direct_mod_jars() {
        assert!(validate_relative_mod_path(".minecraft/mods/example.jar").is_ok());
        assert!(validate_relative_mod_path(".minecraft/mods/../config/secret.jar").is_err());
        assert!(validate_relative_mod_path(".minecraft/config/example.jar").is_err());
        assert!(validate_relative_mod_path("C:/mods/example.jar").is_err());
    }

    #[test]
    fn validates_provider_and_https_source() {
        let valid = BootstrapManifest {
            schema: 1,
            initialize: true,
            files: vec![BootstrapFile {
                path: ".minecraft/mods/example.jar".to_owned(),
                length: 3,
                sha256: sha256_bytes(b"abc"),
                external_source: ExternalSource {
                    provider: "modrinth".to_owned(),
                    url: "https://cdn.example/example.jar".to_owned(),
                },
            }],
        };
        assert!(validate_manifest(&valid).is_ok());
    }
}
