use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::{stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::error::{BusinessError, BusinessResult, ResultToBusinessError};
use crate::log::{log_error, log_info};
use crate::network::Network;
use crate::utility::convert_bytes;

const MANIFEST_NAME: &str = "bootstrap-manifest.json";
const STATE_NAME: &str = "bootstrap-state.json";
const DOWNLOAD_ATTEMPTS: usize = 3;
const DOWNLOAD_CONCURRENCY: usize = 5;
const FILE_PIECE_LIMIT: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BootstrapManifest {
    schema: u32,
    #[serde(default = "default_true")]
    initialize: bool,
    files: Vec<BootstrapFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BootstrapFile {
    path: String,
    length: u64,
    sha256: String,
    #[serde(default)]
    external_source: Option<ExternalSource>,
    #[serde(default)]
    external_sources: Vec<ExternalSource>,
}

#[derive(Clone, Debug, Deserialize)]
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

    let mut completed_files = 0_usize;
    let mut pending = Vec::new();
    for (index, file) in manifest.files.iter().enumerate() {
        let target = checked_target(base_dir, &file.path).await?;
        if file_matches(&target, file.length, &file.sha256).await? {
            completed_bytes += file.length;
            completed_files += 1;
            continue;
        }

        let temp_path = temp_root.join(format!("{:04}.download", index));
        pending.push((index, file.clone(), target, temp_path));
    }

    update_progress(
        completed_bytes,
        total_bytes,
        #[cfg(target_os = "windows")]
        ui_cmd,
    )
    .await;

    let live_downloaded = Arc::new(AtomicU64::new(completed_bytes));

    #[cfg(target_os = "windows")]
    {
        ui_cmd
            .set_label(format!(
                "正在初始化整合包 ({completed_files}/{})",
                manifest.files.len()
            ))
            .await;
        ui_cmd
            .set_label_secondary(format!("并行下载，最多 {DOWNLOAD_CONCURRENCY} 线程"))
            .await;
    }

    let pending_count = pending.len();
    let permits = Arc::new(Semaphore::new(DOWNLOAD_CONCURRENCY));
    let mut downloads = stream::iter(pending.into_iter().enumerate().map(
        |(queue_index, (index, file, target, temp_path))| {
            let permits = Arc::clone(&permits);
            let live_downloaded = Arc::clone(&live_downloaded);
            let remaining_files = pending_count - queue_index;
            let max_segments =
                (DOWNLOAD_CONCURRENCY / remaining_files.min(DOWNLOAD_CONCURRENCY)).max(1);
            async move {
                download_verified(
                    network,
                    &file,
                    &temp_path,
                    &permits,
                    max_segments,
                    &live_downloaded,
                )
                .await?;
                install_verified_file(&temp_path, &target).await?;
                let name = target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&file.path)
                    .to_owned();
                Ok::<_, BusinessError>((index, file.length, name))
            }
        },
    ))
    .buffer_unordered(DOWNLOAD_CONCURRENCY);

    let mut progress_tick = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! {
            result = downloads.next() => {
                let Some(result) = result else { break };
                let (_index, _downloaded, name) = result?;
                completed_files += 1;
                #[cfg(target_os = "windows")]
                {
                    ui_cmd
                        .set_label(format!(
                            "正在初始化整合包 ({completed_files}/{})",
                            manifest.files.len()
                        ))
                        .await;
                    ui_cmd.set_label_secondary(name).await;
                }
            }
            _ = progress_tick.tick() => {
                update_progress(
                    live_downloaded.load(Ordering::Relaxed),
                    total_bytes,
                    #[cfg(target_os = "windows")]
                    ui_cmd,
                )
                .await;
            }
        }
    }
    update_progress(
        live_downloaded.load(Ordering::Relaxed),
        total_bytes,
        #[cfg(target_os = "windows")]
        ui_cmd,
    )
    .await;

    write_state(&state_path, &manifest_hash).await?;
    if let Err(error) = tokio::fs::remove_dir_all(&temp_root).await {
        if error.kind() != ErrorKind::NotFound {
            log_error(format!(
                "清理整合包初始化临时目录失败({temp_root:?})：{error:?}"
            ));
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
        let sources = file.sources();
        if file.length == 0
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || sources.is_empty()
            || sources.iter().any(|source| {
                !matches!(source.provider.as_str(), "modrinth" | "curseforge")
                    || !source.url.starts_with("https://")
            })
        {
            return Err(BusinessError::new(format!(
                "整合包初始化清单含有无效文件记录：{}",
                file.path
            )));
        }
    }
    Ok(())
}

impl BootstrapFile {
    fn sources(&self) -> Vec<&ExternalSource> {
        let mut sources = Vec::new();
        for source in &self.external_sources {
            if !sources
                .iter()
                .any(|existing: &&ExternalSource| existing.url == source.url)
            {
                sources.push(source);
            }
        }
        if let Some(source) = &self.external_source {
            if !sources.iter().any(|existing| existing.url == source.url) {
                sources.push(source);
            }
        }
        sources
    }
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
    permits: &Arc<Semaphore>,
    max_segments: usize,
    live_downloaded: &Arc<AtomicU64>,
) -> BusinessResult<()> {
    let mut failures = Vec::new();
    for source in file.sources() {
        let mut last_error = String::from("没有发起下载");
        for attempt in 1..=DOWNLOAD_ATTEMPTS {
            match download_segmented(
                network,
                file,
                source,
                temp_path,
                permits,
                max_segments,
                live_downloaded,
            )
            .await
            {
                Ok(segments)
                    if sha256_file(temp_path)
                        .await?
                        .eq_ignore_ascii_case(&file.sha256) =>
                {
                    log_info(format!(
                        "初始化下载成功：{} via {} (attempt {attempt}/{DOWNLOAD_ATTEMPTS}, segments={segments})",
                        file.path, source.provider,
                    ));
                    return Ok(());
                }
                Ok(_) => {
                    live_downloaded.fetch_sub(file.length, Ordering::Relaxed);
                    last_error =
                        format!("CDN 文件 SHA-256 校验失败（第 {attempt}/{DOWNLOAD_ATTEMPTS} 次）")
                }
                Err(error) => last_error = error.reason,
            }
        }
        failures.push(format!("{}: {}", source.provider, last_error));
        log_error(format!(
            "初始化下载源失败，准备切换：{} via {}",
            file.path, source.provider
        ));
    }
    Err(BusinessError::new(format!(
        "整合包初始化下载失败({})，所有来源均失败：{}",
        file.path,
        failures.join("；")
    )))
}

async fn download_segmented(
    network: &Network<'_>,
    file: &BootstrapFile,
    source: &ExternalSource,
    temp_path: &Path,
    permits: &Arc<Semaphore>,
    max_segments: usize,
    live_downloaded: &Arc<AtomicU64>,
) -> BusinessResult<usize> {
    let segment_count = segment_count(file.length, max_segments);
    let segment_size = file.length.div_ceil(segment_count as u64);
    let parts = (0..segment_count)
        .filter_map(|index| {
            let start = index as u64 * segment_size;
            (start < file.length).then(|| {
                let end = (start + segment_size).min(file.length);
                (
                    index,
                    start..end,
                    temp_path.with_extension(format!("part{index}")),
                )
            })
        })
        .collect::<Vec<_>>();

    let attempt_downloaded = Arc::new(AtomicU64::new(0));
    let download_result =
        stream::iter(parts.iter().cloned().map(|(index, range, part_path)| {
            let live_downloaded = Arc::clone(live_downloaded);
            let attempt_downloaded = Arc::clone(&attempt_downloaded);
            async move {
                let _permit = permits
                    .acquire()
                    .await
                    .map_err(|_| BusinessError::new("初始化下载线程池已关闭"))?;
                let expected = range.end - range.start;
                let desc = format!(
                    "bootstrap segment {}/{} {}",
                    index + 1,
                    segment_count,
                    file.path
                );
                let (reported, mut input) = if segment_count == 1 {
                    network.request_external_file(&source.url, &desc).await?
                } else {
                    network
                        .request_external_file_range(&source.url, range, &desc)
                        .await?
                };
                if reported != expected {
                    return Err(BusinessError::new(format!(
                        "CDN 分片长度不符：分片 {} 预期 {expected}，实际 {reported}",
                        index + 1
                    )));
                }
                let mut output = tokio::fs::File::create(&part_path)
                    .await
                    .be(|error| format!("创建初始化分片失败({part_path:?})，原因：{error:?}"))?;
                let mut copied = 0_u64;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let read = input.read(&mut buffer).await.be(|error| {
                        format!("读取初始化分片失败({part_path:?})，原因：{error:?}")
                    })?;
                    if read == 0 {
                        break;
                    }
                    output.write_all(&buffer[..read]).await.be(|error| {
                        format!("写入初始化分片失败({part_path:?})，原因：{error:?}")
                    })?;
                    copied += read as u64;
                    live_downloaded.fetch_add(read as u64, Ordering::Relaxed);
                    attempt_downloaded.fetch_add(read as u64, Ordering::Relaxed);
                }
                output
                    .flush()
                    .await
                    .be(|error| format!("刷新初始化分片失败({part_path:?})，原因：{error:?}"))?;
                if copied != expected {
                    return Err(BusinessError::new(format!(
                        "CDN 分片内容不完整：分片 {} 预期 {expected}，实际 {copied}",
                        index + 1
                    )));
                }
                Ok::<_, BusinessError>(())
            }
        }))
        .buffer_unordered(segment_count)
        .try_collect::<Vec<_>>()
        .await;
    if let Err(error) = download_result {
        live_downloaded.fetch_sub(
            attempt_downloaded.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        return Err(error);
    }

    let mut output = tokio::fs::File::create(temp_path)
        .await
        .be(|error| format!("创建初始化合并文件失败({temp_path:?})，原因：{error:?}"))?;
    for (_, _, part_path) in &parts {
        let mut input = tokio::fs::File::open(part_path)
            .await
            .be(|error| format!("打开初始化分片失败({part_path:?})，原因：{error:?}"))?;
        tokio::io::copy(&mut input, &mut output)
            .await
            .be(|error| format!("合并初始化分片失败({part_path:?})，原因：{error:?}"))?;
    }
    output
        .flush()
        .await
        .be(|error| format!("刷新初始化合并文件失败({temp_path:?})，原因：{error:?}"))?;
    drop(output);
    for (_, _, part_path) in parts {
        let _ = tokio::fs::remove_file(part_path).await;
    }
    let actual = tokio::fs::metadata(temp_path)
        .await
        .be(|error| format!("读取初始化合并文件失败({temp_path:?})，原因：{error:?}"))?
        .len();
    if actual != file.length {
        return Err(BusinessError::new(format!(
            "CDN 合并长度不符：预期 {}，实际 {actual}",
            file.length
        )));
    }
    Ok(segment_count)
}

fn segment_count(length: u64, max_segments: usize) -> usize {
    length
        .div_ceil(FILE_PIECE_LIMIT)
        .min(max_segments.max(1) as u64)
        .max(1) as usize
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
    use super::{
        segment_count, sha256_bytes, validate_manifest, validate_relative_mod_path, BootstrapFile,
        BootstrapManifest, ExternalSource, FILE_PIECE_LIMIT,
    };

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
                external_source: Some(ExternalSource {
                    provider: "modrinth".to_owned(),
                    url: "https://cdn.example/example.jar".to_owned(),
                }),
                external_sources: Vec::new(),
            }],
        };
        assert!(validate_manifest(&valid).is_ok());
    }

    #[test]
    fn allocates_segments_only_for_large_files_and_available_threads() {
        assert_eq!(segment_count(FILE_PIECE_LIMIT - 1, 5), 1);
        assert_eq!(segment_count(FILE_PIECE_LIMIT, 5), 1);
        assert_eq!(segment_count(FILE_PIECE_LIMIT + 1, 5), 2);
        assert_eq!(segment_count(FILE_PIECE_LIMIT * 20, 5), 5);
        assert_eq!(segment_count(FILE_PIECE_LIMIT * 20, 2), 2);
    }
}
