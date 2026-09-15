use std::collections::{HashSet, LinkedList};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Weak;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_path::AppPath;
use crate::config::Config;
use crate::core::archive_tester::ArchiveTester;
use crate::core::curseforge::attach_external_sources;
use crate::core::data::index_file::{IndexFile, VersionIndex};
use crate::core::data::pending_changes::PendingChanges;
use crate::core::data::version_meta::{ClientHashDeletion, FileChange, VersionMeta};
use crate::core::data::version_meta_group::VersionMetaGroup;
use crate::core::file_hash::{calculate_hash, calculate_sha256};
use crate::core::modrinth::attach_external_sources as attach_modrinth_sources;
use crate::core::tar_writer::TarWriter;
use crate::diff::abstract_file::AbstractFile;
use crate::diff::diff::Diff;
use crate::diff::disk_file::DiskFile;
use crate::diff::history_file::HistoryFile;
use crate::web::log::Console;

#[derive(Clone, Serialize)]
pub struct PackChangePreview {
    pub id: String,
    pub operation: String,
    pub path: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub hash: Option<String>,
    pub len: Option<u64>,
    pub explicit: bool,
}

#[derive(Clone, Default, Serialize)]
pub struct PackChangeCounts {
    pub create_directory: usize,
    pub add_file: usize,
    pub update_file: usize,
    pub move_file: usize,
    pub delete_file: usize,
    pub delete_directory: usize,
    pub delete_by_hash: usize,
}

#[derive(Clone, Serialize)]
pub struct PackPreview {
    pub confirmation_required: bool,
    pub fingerprint: String,
    pub changes: Vec<PackChangePreview>,
    pub counts: PackChangeCounts,
}

pub struct PackPlan {
    pub preview: PackPreview,
    pub changes: Vec<FileChange>,
    pub pending_deletions: HashSet<String>,
    pub hash_deletions: Vec<ClientHashDeletion>,
    pub pending_hash_deletions: HashSet<String>,
    required_change_ids: HashSet<String>,
}

pub struct PackSelection {
    pub changes: Vec<FileChange>,
    pub hash_deletions: Vec<ClientHashDeletion>,
    pub emitted_pending_deletions: HashSet<String>,
    pub emitted_pending_hash_deletions: HashSet<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct OneShotHashDeletionManifest {
    schema: u8,
    deletions: Vec<ClientHashDeletion>,
}

const UPDATER_SOURCE_PATH: &str = ".minecraft/autoupdate/AutoUpdateClient.exe";
const UPDATER_STARTLIST_PATH: &str = ".minecraft/autoupdate/startlist.txt";
const UPDATER_BASE_NAME: &str = "AutoUpdateClient.exe";
const UPDATER_VERSION_PREFIX: &str = "AutoUpdateClient-";

struct PreparedUpdaterSelfUpdate {
    required_paths: HashSet<String>,
}

pub fn load_one_shot_hash_deletions(path: &Path) -> Result<Vec<ClientHashDeletion>, String> {
    let content = std::fs::read(path)
        .map_err(|error| format!("读取一次性哈希删除清单失败({path:?}): {error}"))?;
    let manifest: OneShotHashDeletionManifest = serde_json::from_slice(&content)
        .map_err(|error| format!("解析一次性哈希删除清单失败({path:?}): {error}"))?;
    if manifest.schema != 1 {
        return Err(format!(
            "不支持的一次性哈希删除清单版本: {}",
            manifest.schema
        ));
    }
    if manifest.deletions.is_empty() {
        return Err("一次性哈希删除清单不能为空".to_owned());
    }

    let mut normalized = PendingChanges::default();
    let mut seen = HashSet::new();
    for deletion in manifest.deletions {
        let path_key = deletion.path.replace('\\', "/").to_ascii_lowercase();
        if !seen.insert(path_key) {
            return Err(format!("一次性哈希删除清单包含重复路径: {}", deletion.path));
        }
        normalized.add_hash_deletion(&deletion.path, &deletion.sha256, deletion.len, None)?;
    }
    Ok(normalized
        .hash_deletions
        .into_iter()
        .map(|deletion| ClientHashDeletion {
            path: deletion.path,
            sha256: deletion.sha256,
            len: deletion.len,
        })
        .collect())
}

pub fn build_pack_plan(
    version_label: &str,
    change_logs: &str,
    apppath: &AppPath,
    config: &Config,
    pending: &PendingChanges,
) -> Result<PackPlan, String> {
    let version_label = normalize_version_label(version_label)?;

    let index_file = IndexFile::load_from_file(&apppath.index_file);
    if index_file.contains(&version_label) {
        return Err(format!("版本号已经存在: {version_label}"));
    }

    // AutoUpdateClient.exe is the administrator-facing source file. Materialize
    // an immutable sibling before diffing so the running client is never
    // overwritten in place.
    let prepared_updater = prepare_updater_self_update(&apppath.workspace_dir)?;

    let mut history = HistoryFile::new_dir("workspace_root", Weak::new());
    for (_index, meta) in index_file.read_all_metas(&apppath.public_dir) {
        history.replay_operations(&meta);
    }

    let disk_file = DiskFile::new(apppath.workspace_dir.clone(), Weak::new());
    let diff = Diff::diff(&disk_file, &history, Some(&config.core.exclude_rules));
    let mut changes = diff.to_file_changes().into_iter().collect::<Vec<_>>();
    changes.retain(|change| !change_touches_path(change, UPDATER_SOURCE_PATH));
    let mut explicit_paths = HashSet::new();
    let mut pending_deletions = HashSet::new();
    let mut hash_deletions = Vec::new();
    let mut pending_hash_deletions = HashSet::new();

    for deletion in &pending.forced_deletions {
        let path = deletion.path.as_str();
        if changes.iter().any(|change| match change {
            FileChange::MoveFile { from, to } => from == path || to == path,
            _ => false,
        }) {
            return Err(format!(
                "显式删除路径与文件移动冲突，请先处理工作区中的移动: {path}"
            ));
        }

        // 显式删除是目标状态覆盖：即使工作区仍有同名文件，也不能重新发给客户端。
        changes.retain(|change| !change_writes_path(change, path));

        let history_entry = history.find(path);
        if history_entry.as_ref().is_some_and(|entry| entry.is_dir()) {
            return Err(format!("显式文件删除不能用于目录: {path}"));
        }

        let should_emit = deletion.pending || history_entry.is_some();
        if should_emit
            && !changes
                .iter()
                .any(|change| matches!(change, FileChange::DeleteFile { path: old } if old == path))
        {
            changes.push(FileChange::DeleteFile {
                path: path.to_owned(),
            });
        }

        if should_emit {
            explicit_paths.insert(path.to_owned());
        }
        if deletion.pending {
            pending_deletions.insert(path.to_owned());
        }
    }

    for deletion in pending.hash_deletions.iter().filter(|entry| entry.pending) {
        if apppath.workspace_dir.join(&deletion.path).exists() {
            return Err(format!(
                "客户端哈希删除目标仍存在于当前工作区，拒绝打包: {}",
                deletion.path
            ));
        }
        if history.find(&deletion.path).is_some() {
            return Err(format!(
                "客户端哈希删除目标已属于更新历史，应使用普通删除: {}",
                deletion.path
            ));
        }
        hash_deletions.push(ClientHashDeletion {
            path: deletion.path.clone(),
            sha256: deletion.sha256.clone(),
            len: deletion.len,
        });
        pending_hash_deletions.insert(deletion.path.clone());
    }

    changes.sort_by_key(change_sort_key);
    changes.dedup_by(|left, right| canonical_change(left) == canonical_change(right));

    let required_change_ids = changes
        .iter()
        .filter(|change| {
            change_path(change).is_some_and(|path| prepared_updater.required_paths.contains(path))
        })
        .map(change_id)
        .collect::<HashSet<_>>();

    let mut previews = changes
        .iter()
        .map(|change| {
            let existed_before = match change {
                FileChange::UpdateFile { path, .. } => history.find(path).is_some(),
                _ => false,
            };
            preview_change(change, &explicit_paths, existed_before)
        })
        .collect::<Vec<_>>();
    previews.extend(hash_deletions.iter().map(preview_hash_deletion));
    let fingerprint = fingerprint(&version_label, change_logs, &changes, &hash_deletions);
    let counts = count_changes(&previews);

    Ok(PackPlan {
        preview: PackPreview {
            confirmation_required: true,
            fingerprint,
            changes: previews,
            counts,
        },
        changes,
        pending_deletions,
        hash_deletions,
        pending_hash_deletions,
        required_change_ids,
    })
}

pub fn select_pack_changes(
    plan: PackPlan,
    excluded_ids: &[String],
) -> Result<PackSelection, String> {
    let available = plan
        .preview
        .changes
        .iter()
        .map(|change| change.id.as_str())
        .collect::<HashSet<_>>();
    if let Some(unknown) = excluded_ids
        .iter()
        .find(|change_id| !available.contains(change_id.as_str()))
    {
        return Err(format!("待排除的变更不存在或预览已过期: {unknown}"));
    }

    if excluded_ids
        .iter()
        .any(|change_id| plan.required_change_ids.contains(change_id))
    {
        return Err("更新器自更新文件与启动清单必须一起发布，不能单独排除".to_owned());
    }

    let excluded = excluded_ids.iter().collect::<HashSet<_>>();
    let mut emitted_pending_deletions = HashSet::new();
    let mut emitted_pending_hash_deletions = HashSet::new();
    let selected = plan
        .changes
        .into_iter()
        .filter(|change| {
            let id = change_id(change);
            let included = !excluded.contains(&id);
            if included {
                if let FileChange::DeleteFile { path } = change {
                    if plan.pending_deletions.contains(path) {
                        emitted_pending_deletions.insert(path.clone());
                    }
                }
            }
            included
        })
        .collect::<Vec<_>>();

    let selected_hash_deletions = plan
        .hash_deletions
        .into_iter()
        .filter(|deletion| {
            let id = hash_deletion_id(deletion);
            let included = !excluded.contains(&id);
            if included && plan.pending_hash_deletions.contains(&deletion.path) {
                emitted_pending_hash_deletions.insert(deletion.path.clone());
            }
            included
        })
        .collect::<Vec<_>>();

    if selected.is_empty() && selected_hash_deletions.is_empty() {
        return Err("没有选中任何可打包变更".to_owned());
    }

    Ok(PackSelection {
        changes: selected,
        hash_deletions: selected_hash_deletions,
        emitted_pending_deletions,
        emitted_pending_hash_deletions,
    })
}

pub fn task_pack(
    version_label: String,
    change_logs: String,
    one_shot_hash_deletions: Vec<ClientHashDeletion>,
    apppath: &AppPath,
    config: &Config,
    console: &Console,
) -> u8 {
    let pending = match PendingChanges::load(&apppath.pending_changes_file) {
        Ok(state) => state,
        Err(error) => {
            console.log_error(error);
            return 1;
        }
    };
    let mut planning_pending = pending.clone();
    for deletion in one_shot_hash_deletions {
        if planning_pending
            .forced_deletions
            .iter()
            .any(|entry| entry.path.eq_ignore_ascii_case(&deletion.path))
            || planning_pending
                .hash_deletions
                .iter()
                .any(|entry| entry.path.eq_ignore_ascii_case(&deletion.path))
        {
            console.log_error(format!(
                "一次性哈希删除与长期待处理规则冲突: {}",
                deletion.path
            ));
            return 1;
        }
        if let Err(error) =
            planning_pending.add_hash_deletion(&deletion.path, &deletion.sha256, deletion.len, None)
        {
            console.log_error(error);
            return 1;
        }
    }
    let plan = match build_pack_plan(
        &version_label,
        &change_logs,
        apppath,
        config,
        &planning_pending,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            console.log_error(error);
            return 1;
        }
    };
    let selection = match select_pack_changes(plan, &[]) {
        Ok(selection) => selection,
        Err(error) => {
            console.log_error(error);
            return 1;
        }
    };
    let emitted_persistent_hash_deletions = selection
        .emitted_pending_hash_deletions
        .iter()
        .filter(|path| {
            pending
                .hash_deletions
                .iter()
                .any(|entry| entry.path.eq_ignore_ascii_case(path))
        })
        .cloned()
        .collect::<HashSet<_>>();
    let code = task_pack_selected(
        version_label,
        change_logs,
        selection.changes,
        selection.hash_deletions,
        apppath,
        config,
        console,
    );
    if code == 0
        && (!selection.emitted_pending_deletions.is_empty()
            || !emitted_persistent_hash_deletions.is_empty())
    {
        let mut pending = pending;
        pending.mark_emitted(&selection.emitted_pending_deletions);
        let staged_files = pending.mark_hash_emitted(&emitted_persistent_hash_deletions);
        if let Err(error) = pending.save(&apppath.pending_changes_file) {
            console.log_warning(format!("更新包已生成，但删除规则状态保存失败: {error}"));
        } else {
            cleanup_hash_delete_staging(apppath, staged_files, console);
        }
    }
    code
}

pub fn cleanup_hash_delete_staging(
    apppath: &AppPath,
    staged_files: Vec<String>,
    console: &Console,
) {
    let staging_dir = apppath.working_dir.join(".mcpatch-hash-delete-staging");
    for staged_file in staged_files {
        let path = staging_dir.join(staged_file);
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                console.log_warning(format!(
                    "清理已打包的哈希删除暂存文件失败({path:?}): {error}"
                ));
            }
        }
    }
}

pub fn task_pack_selected(
    version_label: String,
    change_logs: String,
    changes: Vec<FileChange>,
    client_hash_deletions: Vec<ClientHashDeletion>,
    apppath: &AppPath,
    config: &Config,
    console: &Console,
) -> u8 {
    let version_label = match normalize_version_label(&version_label) {
        Ok(label) => label,
        Err(error) => {
            console.log_error(error);
            return 1;
        }
    };
    let change_logs = if change_logs.is_empty() {
        std::fs::read_to_string(apppath.working_dir.join("logs.txt"))
            .unwrap_or_else(|_| "没有更新记录".to_owned())
    } else {
        change_logs
    };
    let mut index_file = IndexFile::load_from_file(&apppath.index_file);

    if index_file.contains(&version_label) {
        console.log_error(format!("版本号已经存在: {version_label}"));
        return 1;
    }
    if changes.is_empty() && client_hash_deletions.is_empty() {
        console.log_error("目前没有任何已选择的文件修改");
        return 1;
    }

    // 在创建 tar 之前验证所有输入，避免失败时留下半成品。
    for change in &changes {
        if let FileChange::UpdateFile {
            path, hash, len, ..
        } = change
        {
            let disk_path = apppath.workspace_dir.join(path);
            let mut open = match std::fs::File::open(&disk_path) {
                Ok(file) => file,
                Err(error) => {
                    console.log_error(format!("读取待打包文件失败({disk_path:?}): {error}"));
                    return 1;
                }
            };
            let actual_len = open.metadata().map(|meta| meta.len()).unwrap_or(u64::MAX);
            let actual_hash = calculate_hash(&mut open);
            if actual_len != *len || actual_hash != *hash {
                console.log_error(format!("预览后文件发生变化，请重新确认: {path}"));
                return 1;
            }
        }
    }

    let mut changes = changes.into_iter().collect::<LinkedList<_>>();
    match attach_modrinth_sources(&mut changes, &apppath.workspace_dir, &config.modrinth) {
        Ok(count) if count > 0 => {
            console.log_info(format!("Modrinth 外部下载源已匹配 {count} 个 mod"))
        }
        Ok(_) => (),
        Err(error) => console.log_warning(format!(
            "Modrinth 外部下载源未启用: {error}；将尝试 CurseForge 或 mcpatch"
        )),
    }
    match attach_external_sources(&mut changes, &apppath.workspace_dir, &config.curseforge) {
        Ok(count) if count > 0 => {
            console.log_info(format!("CurseForge 外部下载源已匹配 {count} 个 mod"))
        }
        Ok(_) => (),
        Err(error) => console.log_warning(format!(
            "CurseForge 外部下载源未启用: {error}；其余 mod 将只使用 mcpatch"
        )),
    }

    std::fs::create_dir_all(&apppath.public_dir).unwrap();
    let version_filename = format!("{version_label}.tar");
    let version_file = apppath.public_dir.join(&version_filename);
    let mut writer = TarWriter::new(&version_file);
    let update_count = changes
        .iter()
        .filter(|change| matches!(change, FileChange::UpdateFile { .. }))
        .count();
    let mut counter = 1;

    for change in &changes {
        if let FileChange::UpdateFile { path, len, .. } = change {
            console.log_debug(format!("打包({counter}/{update_count}) {path}"));
            counter += 1;
            let open = std::fs::File::open(apppath.workspace_dir.join(path)).unwrap();
            writer.add_file(open, *len, path, &version_label);
        }
    }

    console.log_debug("写入元数据");
    let meta = VersionMeta::new(
        version_label.clone(),
        change_logs,
        changes,
        client_hash_deletions,
    );
    let meta_info = writer.finish(VersionMetaGroup::with_one(meta));
    index_file.add(VersionIndex {
        label: version_label.clone(),
        filename: version_filename,
        offset: meta_info.offset,
        len: meta_info.length,
        hash: "no hash".to_owned(),
    });

    console.log_debug("正在测试");
    let mut tester = ArchiveTester::new();
    for (index, meta) in index_file.read_all_metas(&apppath.public_dir) {
        tester.feed_version(apppath.public_dir.join(&index.filename), &meta);
    }
    if let Err(error) = tester.finish(|entry| {
        console.log_debug(format!(
            "{}/{} 正在测试 {} 的 {} ({}+{})",
            entry.index, entry.total, entry.label, entry.path, entry.offset, entry.len
        ))
    }) {
        console.log_error(format!("更新包测试失败: {error:?}"));
        let _ = std::fs::remove_file(version_file);
        return 1;
    }

    index_file.save(&apppath.index_file);
    console.log_info("测试通过，打包完成！");
    0
}
fn change_writes_path(change: &FileChange, expected: &str) -> bool {
    match change {
        FileChange::CreateFolder { path } | FileChange::UpdateFile { path, .. } => path == expected,
        FileChange::MoveFile { to, .. } => to == expected,
        _ => false,
    }
}

fn change_path(change: &FileChange) -> Option<&str> {
    match change {
        FileChange::CreateFolder { path }
        | FileChange::UpdateFile { path, .. }
        | FileChange::DeleteFolder { path }
        | FileChange::DeleteFile { path } => Some(path),
        FileChange::MoveFile { .. } => None,
    }
}

fn change_touches_path(change: &FileChange, expected: &str) -> bool {
    match change {
        FileChange::CreateFolder { path }
        | FileChange::UpdateFile { path, .. }
        | FileChange::DeleteFolder { path }
        | FileChange::DeleteFile { path } => path.eq_ignore_ascii_case(expected),
        FileChange::MoveFile { from, to } => {
            from.eq_ignore_ascii_case(expected) || to.eq_ignore_ascii_case(expected)
        }
    }
}

fn change_sort_key(change: &FileChange) -> (bool, String) {
    (
        change_writes_path(change, UPDATER_STARTLIST_PATH),
        canonical_change(change),
    )
}

fn prepare_updater_self_update(workspace_dir: &Path) -> Result<PreparedUpdaterSelfUpdate, String> {
    let source = workspace_dir.join(UPDATER_SOURCE_PATH);
    if !source.exists() {
        return Ok(PreparedUpdaterSelfUpdate {
            required_paths: HashSet::new(),
        });
    }

    let source_metadata = std::fs::symlink_metadata(&source)
        .map_err(|error| format!("读取更新器源文件失败({source:?}): {error}"))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
        return Err(format!(
            "更新器源必须是普通文件，不能是符号链接或目录: {source:?}"
        ));
    }

    let mut source_file = std::fs::File::open(&source)
        .map_err(|error| format!("打开更新器源文件失败({source:?}): {error}"))?;
    let source_sha256 = calculate_sha256(&mut source_file);
    let versioned_name = format!("{UPDATER_VERSION_PREFIX}{}.exe", &source_sha256[..16]);
    let autoupdate_dir = source
        .parent()
        .ok_or_else(|| "更新器源文件缺少父目录".to_owned())?;
    let versioned = autoupdate_dir.join(&versioned_name);
    materialize_versioned_updater(&source, &versioned, &source_sha256)?;

    let startlist = workspace_dir.join(UPDATER_STARTLIST_PATH);
    let mut entries = vec![versioned_name.clone()];
    if startlist.exists() {
        let metadata = std::fs::symlink_metadata(&startlist)
            .map_err(|error| format!("读取更新器启动清单失败({startlist:?}): {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("更新器启动清单必须是普通文件: {startlist:?}"));
        }
        let previous = std::fs::read_to_string(&startlist)
            .map_err(|error| format!("读取更新器启动清单失败({startlist:?}): {error}"))?;
        for entry in previous.lines().map(str::trim) {
            if is_safe_updater_entry(entry)
                && !entries
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(entry))
            {
                entries.push(entry.to_owned());
            }
        }
    }
    if !entries
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(UPDATER_BASE_NAME))
    {
        entries.push(UPDATER_BASE_NAME.to_owned());
    }
    let content = format!("{}\n", entries.join("\n"));
    write_if_changed_atomically(&startlist, content.as_bytes())?;

    Ok(PreparedUpdaterSelfUpdate {
        required_paths: [
            format!(".minecraft/autoupdate/{versioned_name}"),
            UPDATER_STARTLIST_PATH.to_owned(),
        ]
        .into_iter()
        .collect(),
    })
}

fn materialize_versioned_updater(
    source: &Path,
    target: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    if target.exists() {
        let metadata = std::fs::symlink_metadata(target)
            .map_err(|error| format!("读取版本化更新器失败({target:?}): {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("版本化更新器目标必须是普通文件: {target:?}"));
        }
        let mut existing = std::fs::File::open(target)
            .map_err(|error| format!("打开版本化更新器失败({target:?}): {error}"))?;
        if calculate_sha256(&mut existing) == expected_sha256 {
            return Ok(());
        }
        return Err(format!(
            "版本化更新器名称发生哈希冲突，拒绝覆盖: {target:?}"
        ));
    }

    let temporary = temporary_sibling(target);
    if temporary.exists() {
        std::fs::remove_file(&temporary)
            .map_err(|error| format!("清理更新器临时文件失败({temporary:?}): {error}"))?;
    }
    let copy_result = (|| {
        std::fs::copy(source, &temporary)
            .map_err(|error| format!("生成版本化更新器失败({temporary:?}): {error}"))?;
        let mut copied = std::fs::File::open(&temporary)
            .map_err(|error| format!("校验版本化更新器失败({temporary:?}): {error}"))?;
        if calculate_sha256(&mut copied) != expected_sha256 {
            return Err("版本化更新器复制后 SHA-256 不一致".to_owned());
        }
        std::fs::rename(&temporary, target)
            .map_err(|error| format!("提交版本化更新器失败({target:?}): {error}"))?;
        Ok(())
    })();
    if copy_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    copy_result
}

fn write_if_changed_atomically(path: &Path, content: &[u8]) -> Result<(), String> {
    if std::fs::read(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    let temporary = temporary_sibling(path);
    let backup = backup_sibling(path);
    if temporary.exists() {
        std::fs::remove_file(&temporary)
            .map_err(|error| format!("清理启动清单临时文件失败({temporary:?}): {error}"))?;
    }
    if backup.exists() {
        std::fs::remove_file(&backup)
            .map_err(|error| format!("清理启动清单旧备份失败({backup:?}): {error}"))?;
    }
    let write_result = (|| {
        let mut output = std::fs::File::create(&temporary)
            .map_err(|error| format!("创建启动清单临时文件失败({temporary:?}): {error}"))?;
        output
            .write_all(content)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("写入启动清单临时文件失败({temporary:?}): {error}"))?;
        drop(output);
        if path.exists() {
            std::fs::rename(path, &backup)
                .map_err(|error| format!("备份旧启动清单失败({path:?}): {error}"))?;
        }
        if let Err(error) = std::fs::rename(&temporary, path) {
            if backup.exists() {
                let _ = std::fs::rename(&backup, path);
            }
            return Err(format!("提交启动清单失败({path:?}): {error}"));
        }
        if backup.exists() {
            std::fs::remove_file(&backup)
                .map_err(|error| format!("清理启动清单备份失败({backup:?}): {error}"))?;
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

fn backup_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mcupdate");
    path.with_file_name(format!(".{name}.{}.bak", std::process::id()))
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mcupdate");
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

fn is_safe_updater_entry(entry: &str) -> bool {
    if entry.eq_ignore_ascii_case(UPDATER_BASE_NAME) {
        return true;
    }
    let Some(version) = entry.strip_prefix(UPDATER_VERSION_PREFIX) else {
        return false;
    };
    version.strip_suffix(".exe").is_some_and(|hash| {
        (7..=64).contains(&hash.len()) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn normalize_version_label(version_label: &str) -> Result<String, String> {
    let version_label = version_label.trim();
    if version_label.is_empty() {
        return Err("版本号不能为空".to_owned());
    }
    if version_label.chars().any(char::is_whitespace) {
        return Err("版本号不能包含内部空白字符".to_owned());
    }
    Ok(version_label.to_owned())
}

fn preview_change(
    change: &FileChange,
    explicit_paths: &HashSet<String>,
    existed_before: bool,
) -> PackChangePreview {
    let (operation, path, from, to, hash, len) = match change {
        FileChange::CreateFolder { path } => (
            "create-directory",
            Some(path.clone()),
            None,
            None,
            None,
            None,
        ),
        FileChange::UpdateFile {
            path, hash, len, ..
        } => (
            if existed_before {
                "update-file"
            } else {
                "add-file"
            },
            Some(path.clone()),
            None,
            None,
            Some(hash.clone()),
            Some(*len),
        ),
        FileChange::DeleteFolder { path } => (
            "delete-directory",
            Some(path.clone()),
            None,
            None,
            None,
            None,
        ),
        FileChange::DeleteFile { path } => {
            ("delete-file", Some(path.clone()), None, None, None, None)
        }
        FileChange::MoveFile { from, to } => (
            "move-file",
            None,
            Some(from.clone()),
            Some(to.clone()),
            None,
            None,
        ),
    };
    let explicit = path
        .as_ref()
        .is_some_and(|path| explicit_paths.contains(path));

    PackChangePreview {
        id: change_id(change),
        operation: operation.to_owned(),
        path,
        from,
        to,
        hash,
        len,
        explicit,
    }
}

fn preview_hash_deletion(deletion: &ClientHashDeletion) -> PackChangePreview {
    PackChangePreview {
        id: hash_deletion_id(deletion),
        operation: "delete-file-by-hash".to_owned(),
        path: Some(deletion.path.clone()),
        from: None,
        to: None,
        hash: Some(deletion.sha256.clone()),
        len: Some(deletion.len),
        explicit: true,
    }
}

fn count_changes(changes: &[PackChangePreview]) -> PackChangeCounts {
    let mut counts = PackChangeCounts::default();
    for change in changes {
        match change.operation.as_str() {
            "create-directory" => counts.create_directory += 1,
            "add-file" => counts.add_file += 1,
            "update-file" => counts.update_file += 1,
            "move-file" => counts.move_file += 1,
            "delete-file" => counts.delete_file += 1,
            "delete-directory" => counts.delete_directory += 1,
            "delete-file-by-hash" => counts.delete_by_hash += 1,
            _ => {}
        }
    }
    counts
}

fn fingerprint(
    version_label: &str,
    change_logs: &str,
    changes: &[FileChange],
    hash_deletions: &[ClientHashDeletion],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(version_label.as_bytes());
    hasher.update([0]);
    hasher.update(change_logs.as_bytes());
    for change in changes {
        hasher.update([0]);
        hasher.update(canonical_change(change).as_bytes());
    }
    for deletion in hash_deletions {
        hasher.update([0]);
        hasher.update(canonical_hash_deletion(deletion).as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn change_id(change: &FileChange) -> String {
    let digest = Sha256::digest(canonical_change(change).as_bytes());
    format!("{:x}", digest)
}

fn canonical_change(change: &FileChange) -> String {
    match change {
        // Keep replay order compatible with the original diff engine: remove a
        // conflicting target before creating, moving, or writing its replacement.
        FileChange::DeleteFile { path } => format!("1|delete-file|{path}"),
        FileChange::CreateFolder { path } => format!("2|create-directory|{path}"),
        FileChange::MoveFile { from, to } => format!("3|move-file|{from}|{to}"),
        FileChange::UpdateFile {
            path, hash, len, ..
        } => format!("4|update-file|{path}|{hash}|{len}"),
        FileChange::DeleteFolder { path } => format!("5|delete-directory|{path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_pack_plan, change_id, change_sort_key, change_touches_path, count_changes,
        fingerprint, load_one_shot_hash_deletions, normalize_version_label,
        prepare_updater_self_update, preview_change, select_pack_changes, task_pack_selected,
        PackChangeCounts, PackPlan, PackPreview, UPDATER_SOURCE_PATH, UPDATER_STARTLIST_PATH,
    };
    use crate::app_path::AppPath;
    use crate::config::Config;
    use crate::core::data::index_file::IndexFile;
    use crate::core::data::pending_changes::PendingChanges;
    use crate::core::data::version_meta::{ClientHashDeletion, FileChange};
    use crate::web::log::Console;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mcupdate-{name}-{}-{nonce}", std::process::id()))
    }

    fn update_file(path: &str, hash: &str) -> FileChange {
        FileChange::UpdateFile {
            path: path.to_owned(),
            hash: hash.to_owned(),
            len: 1,
            modified: UNIX_EPOCH,
            offset: 0,
            external_source: None,
        }
    }

    #[test]
    fn change_ids_are_stable_and_operation_specific() {
        let delete = FileChange::DeleteFile {
            path: ".minecraft/mods/old.jar".to_owned(),
        };
        let create = FileChange::CreateFolder {
            path: ".minecraft/mods/old.jar".to_owned(),
        };
        assert_eq!(change_id(&delete), change_id(&delete));
        assert_ne!(change_id(&delete), change_id(&create));
    }

    #[test]
    fn fingerprint_covers_release_text_and_changes() {
        let changes = vec![FileChange::DeleteFile {
            path: ".minecraft/mods/old.jar".to_owned(),
        }];
        assert_ne!(
            fingerprint("v1", "first", &changes, &[]),
            fingerprint("v1", "second", &changes, &[])
        );
    }

    #[test]
    fn fingerprint_covers_hash_deletion_metadata() {
        let first = ClientHashDeletion {
            path: ".minecraft/mods/old.jar".to_owned(),
            sha256: "a".repeat(64),
            len: 42,
        };
        let mut second = first.clone();
        second.sha256 = "b".repeat(64);
        assert_ne!(
            fingerprint("v1", "same", &[], &[first]),
            fingerprint("v1", "same", &[], &[second])
        );
    }

    #[test]
    fn preview_distinguishes_added_and_replaced_files() {
        let change = FileChange::UpdateFile {
            path: ".minecraft/mods/example.jar".to_owned(),
            hash: "abc123".to_owned(),
            len: 42,
            modified: SystemTime::UNIX_EPOCH,
            offset: 0,
            external_source: None,
        };
        let explicit_paths = HashSet::new();
        let added = preview_change(&change, &explicit_paths, false);
        let replaced = preview_change(&change, &explicit_paths, true);

        assert_eq!(added.operation, "add-file");
        assert_eq!(replaced.operation, "update-file");

        let counts = count_changes(&[added, replaced]);
        assert_eq!(counts.add_file, 1);
        assert_eq!(counts.update_file, 1);
    }

    #[test]
    fn version_labels_are_trimmed_but_reject_internal_whitespace() {
        assert_eq!(normalize_version_label(" v7.7.448 ").unwrap(), "v7.7.448");
        assert!(normalize_version_label("v7.7. 448").is_err());
        assert!(normalize_version_label("  ").is_err());
    }

    #[test]
    fn one_shot_hash_deletion_manifest_is_normalized_and_validated() {
        let path = std::env::temp_dir().join(format!(
            "mcupdate-one-shot-hash-delete-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                r#"{{"schema":1,"deletions":[{{"path":".minecraft\\mods\\old.jar","sha256":"{}","len":42}}]}}"#,
                "A".repeat(64)
            ),
        )
        .unwrap();

        let deletions = load_one_shot_hash_deletions(&path).unwrap();
        assert_eq!(deletions.len(), 1);
        assert_eq!(deletions[0].path, ".minecraft/mods/old.jar");
        assert_eq!(deletions[0].sha256, "a".repeat(64));
        assert_eq!(deletions[0].len, 42);

        std::fs::write(
            &path,
            format!(
                r#"{{"schema":1,"deletions":[{{"path":"../old.jar","sha256":"{}","len":42}}]}}"#,
                "a".repeat(64)
            ),
        )
        .unwrap();
        assert!(load_one_shot_hash_deletions(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn updater_source_materializes_versioned_binary_and_rollback_list() {
        let workspace = temp_workspace("updater-self-update");
        let autoupdate = workspace.join(".minecraft/autoupdate");
        std::fs::create_dir_all(&autoupdate).unwrap();
        let source = autoupdate.join("AutoUpdateClient.exe");
        let startlist = autoupdate.join("startlist.txt");
        std::fs::write(&source, b"first updater").unwrap();
        std::fs::write(
            &startlist,
            "AutoUpdateClient-aaaaaaaaaaaa.exe\nunsafe.exe\nAutoUpdateClient.exe\n",
        )
        .unwrap();

        let first = prepare_updater_self_update(&workspace).unwrap();
        let first_versioned = first
            .required_paths
            .iter()
            .find(|path| path.ends_with(".exe"))
            .unwrap()
            .clone();
        assert_eq!(
            std::fs::read(workspace.join(&first_versioned)).unwrap(),
            b"first updater"
        );
        let first_list = std::fs::read_to_string(&startlist).unwrap();
        assert!(first_list.starts_with(
            first_versioned
                .strip_prefix(".minecraft/autoupdate/")
                .unwrap()
        ));
        assert!(first_list.contains("AutoUpdateClient-aaaaaaaaaaaa.exe"));
        assert!(!first_list.contains("unsafe.exe"));
        assert!(first_list.ends_with("AutoUpdateClient.exe\n"));

        std::fs::write(&source, b"second updater").unwrap();
        let second = prepare_updater_self_update(&workspace).unwrap();
        let second_versioned = second
            .required_paths
            .iter()
            .find(|path| path.ends_with(".exe"))
            .unwrap();
        assert_ne!(second_versioned, &first_versioned);
        let second_list = std::fs::read_to_string(&startlist).unwrap();
        let entries = second_list.lines().collect::<Vec<_>>();
        assert_eq!(
            entries[0],
            second_versioned
                .strip_prefix(".minecraft/autoupdate/")
                .unwrap()
        );
        assert_eq!(
            entries[1],
            first_versioned
                .strip_prefix(".minecraft/autoupdate/")
                .unwrap()
        );
        assert_eq!(entries.last(), Some(&"AutoUpdateClient.exe"));

        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn updater_self_update_changes_cannot_be_partially_excluded() {
        let binary = update_file(
            ".minecraft/autoupdate/AutoUpdateClient-aaaaaaaaaaaaaaaa.exe",
            "binary",
        );
        let startlist = update_file(UPDATER_STARTLIST_PATH, "list");
        let binary_id = change_id(&binary);
        let previews = vec![
            preview_change(&binary, &HashSet::new(), false),
            preview_change(&startlist, &HashSet::new(), false),
        ];
        let plan = PackPlan {
            preview: PackPreview {
                confirmation_required: true,
                fingerprint: "fingerprint".to_owned(),
                changes: previews,
                counts: PackChangeCounts::default(),
            },
            changes: vec![binary, startlist],
            pending_deletions: HashSet::new(),
            hash_deletions: Vec::new(),
            pending_hash_deletions: HashSet::new(),
            required_change_ids: [binary_id.clone()].into_iter().collect(),
        };

        let error = select_pack_changes(plan, &[binary_id]).err().unwrap();
        assert!(error.contains("必须一起发布"));
    }

    #[test]
    fn updater_startlist_is_applied_after_other_file_updates() {
        let mut changes = vec![
            update_file(UPDATER_STARTLIST_PATH, "list"),
            update_file(
                ".minecraft/autoupdate/AutoUpdateClient-aaaaaaaaaaaaaaaa.exe",
                "binary",
            ),
            update_file(".minecraft/mods/example.jar", "mod"),
        ];
        changes.sort_by_key(change_sort_key);
        assert!(matches!(
            changes.last(),
            Some(FileChange::UpdateFile { path, .. }) if path == UPDATER_STARTLIST_PATH
        ));
    }

    #[test]
    fn pack_plan_publishes_versioned_updater_without_overwriting_fixed_source() {
        let working_dir = temp_workspace("updater-pack-plan");
        let workspace_dir = working_dir.join("workspace");
        let public_dir = working_dir.join("public");
        let autoupdate = workspace_dir.join(".minecraft/autoupdate");
        std::fs::create_dir_all(&autoupdate).unwrap();
        std::fs::create_dir_all(&public_dir).unwrap();
        std::fs::write(autoupdate.join("AutoUpdateClient.exe"), b"updater").unwrap();
        let apppath = AppPath {
            working_dir: working_dir.clone(),
            workspace_dir,
            public_dir: public_dir.clone(),
            web_dir: working_dir.join("webpage"),
            index_file: public_dir.join("index.json"),
            ui_profile_file: public_dir.join("ui-profile.json"),
            pending_changes_file: working_dir.join("pending-changes.json"),
            config_file: working_dir.join("config.toml"),
            auth_file: working_dir.join("user.toml"),
        };

        let config = Config::default();
        let plan = build_pack_plan(
            "v1",
            "self update",
            &apppath,
            &config,
            &PendingChanges::default(),
        )
        .unwrap();
        assert!(!plan
            .changes
            .iter()
            .any(|change| change_touches_path(change, UPDATER_SOURCE_PATH)));
        let update_paths = plan
            .changes
            .iter()
            .filter_map(|change| match change {
                FileChange::UpdateFile { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(update_paths.iter().any(|path| {
            path.starts_with(".minecraft/autoupdate/AutoUpdateClient-") && path.ends_with(".exe")
        }));
        assert_eq!(update_paths.last(), Some(&UPDATER_STARTLIST_PATH));
        assert_eq!(plan.required_change_ids.len(), 2);

        let selection = select_pack_changes(plan, &[]).unwrap();
        assert_eq!(
            task_pack_selected(
                "v1".to_owned(),
                "self update".to_owned(),
                selection.changes,
                selection.hash_deletions,
                &apppath,
                &config,
                &Console::new_cli(),
            ),
            0
        );
        let index = IndexFile::load_from_file(&apppath.index_file);
        let metas = index.read_all_metas(&apppath.public_dir);
        assert_eq!(metas.len(), 1);
        let packaged_paths = metas[0]
            .1
            .changes
            .iter()
            .filter_map(|change| match change {
                FileChange::UpdateFile { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!packaged_paths.contains(&UPDATER_SOURCE_PATH));
        assert_eq!(packaged_paths.last(), Some(&UPDATER_STARTLIST_PATH));

        std::fs::remove_dir_all(working_dir).unwrap();
    }
}

fn hash_deletion_id(deletion: &ClientHashDeletion) -> String {
    let digest = Sha256::digest(canonical_hash_deletion(deletion).as_bytes());
    format!("{:x}", digest)
}

fn canonical_hash_deletion(deletion: &ClientHashDeletion) -> String {
    format!(
        "6|delete-file-by-hash|{}|{}|{}",
        deletion.path, deletion.sha256, deletion.len
    )
}
