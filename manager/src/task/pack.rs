use std::collections::{HashSet, LinkedList};
use std::rc::Weak;

use serde::Serialize;
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
}

pub struct PackSelection {
    pub changes: Vec<FileChange>,
    pub hash_deletions: Vec<ClientHashDeletion>,
    pub emitted_pending_deletions: HashSet<String>,
    pub emitted_pending_hash_deletions: HashSet<String>,
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

    let mut history = HistoryFile::new_dir("workspace_root", Weak::new());
    for (_index, meta) in index_file.read_all_metas(&apppath.public_dir) {
        history.replay_operations(&meta);
    }

    let disk_file = DiskFile::new(apppath.workspace_dir.clone(), Weak::new());
    let diff = Diff::diff(&disk_file, &history, Some(&config.core.exclude_rules));
    let mut changes = diff.to_file_changes().into_iter().collect::<Vec<_>>();
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
        if workspace_contains_sha256(
            &apppath.workspace_dir.join(&deletion.search_root),
            deletion.len,
            &deletion.sha256,
        )? {
            return Err(format!(
                "客户端哈希删除目标仍存在于当前工作区，拒绝打包: {} ({})",
                deletion.name_hint, deletion.sha256
            ));
        }
        hash_deletions.push(ClientHashDeletion {
            sha256: deletion.sha256.clone(),
            len: deletion.len,
            name_hint: deletion.name_hint.clone(),
            search_root: deletion.search_root.clone(),
        });
        pending_hash_deletions.insert(deletion.sha256.clone());
    }

    changes.sort_by_key(canonical_change);
    changes.dedup_by(|left, right| canonical_change(left) == canonical_change(right));

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
            if included && plan.pending_hash_deletions.contains(&deletion.sha256) {
                emitted_pending_hash_deletions.insert(deletion.sha256.clone());
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
    let plan = match build_pack_plan(&version_label, &change_logs, apppath, config, &pending) {
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
            || !selection.emitted_pending_hash_deletions.is_empty())
    {
        let mut pending = pending;
        pending.mark_emitted(&selection.emitted_pending_deletions);
        pending.mark_hash_emitted(&selection.emitted_pending_hash_deletions);
        if let Err(error) = pending.save(&apppath.pending_changes_file) {
            console.log_warning(format!("更新包已生成，但删除规则状态保存失败: {error}"));
        }
    }
    code
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
        path: Some(deletion.name_hint.clone()),
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
    use super::{change_id, count_changes, fingerprint, normalize_version_label, preview_change};
    use crate::core::data::version_meta::{ClientHashDeletion, FileChange};
    use std::collections::HashSet;
    use std::time::SystemTime;

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
            sha256: "a".repeat(64),
            len: 42,
            name_hint: "old.jar".to_owned(),
            search_root: ".minecraft/mods".to_owned(),
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
}

fn hash_deletion_id(deletion: &ClientHashDeletion) -> String {
    let digest = Sha256::digest(canonical_hash_deletion(deletion).as_bytes());
    format!("{:x}", digest)
}

fn canonical_hash_deletion(deletion: &ClientHashDeletion) -> String {
    format!(
        "6|delete-file-by-hash|{}|{}|{}|{}",
        deletion.search_root, deletion.sha256, deletion.len, deletion.name_hint
    )
}

fn workspace_contains_sha256(
    root: &std::path::Path,
    len: u64,
    expected: &str,
) -> Result<bool, String> {
    if !root.exists() {
        return Ok(false);
    }
    let entries = std::fs::read_dir(root)
        .map_err(|error| format!("读取工作区模组目录失败({root:?}): {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取工作区模组条目失败: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取工作区模组类型失败({:?}): {error}", entry.path()))?;
        if !file_type.is_file() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取工作区模组信息失败({:?}): {error}", entry.path()))?;
        if metadata.len() != len {
            continue;
        }
        let mut file = std::fs::File::open(entry.path())
            .map_err(|error| format!("读取工作区模组失败({:?}): {error}", entry.path()))?;
        if calculate_sha256(&mut file) == expected {
            return Ok(true);
        }
    }
    Ok(false)
}
