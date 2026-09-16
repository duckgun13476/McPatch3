use std::collections::HashMap;
use std::collections::LinkedList;
use std::rc::Weak;

use crate::app_path::AppPath;
use crate::config::Config;
use crate::core::archive_tester::ArchiveTester;
use crate::core::data::index_file::IndexFile;
use crate::core::data::index_file::VersionIndex;
use crate::core::data::version_meta::FileChange;
use crate::core::data::version_meta_group::VersionMetaGroup;
use crate::core::file_hash::calculate_sha256;
use crate::core::tar_reader::TarReader;
use crate::core::tar_writer::TarWriter;
use crate::diff::history_file::HistoryFile;
use crate::web::log::Console;

pub const COMBINED_FILENAME: &str = "combined.tar";

/// 代表新的合并包中的某个文件数据要从哪个旧包中复制过来
struct Location {
    /// 所在的版本
    pub label: String,

    /// 所在的tar包的文件名
    pub filename: String,

    /// 最原始的文件路径（不受后续移动操作的影响）
    pub path: String,

    /// tar包中的文件偏移
    pub offset: u64,

    /// 数据的长度
    pub len: u64,
}

pub fn task_combine(apppath: &AppPath, _config: &Config, console: &Console) -> u8 {
    let index_file = IndexFile::load_from_file(&apppath.index_file);

    // Capture each source archive before combined.tar is replaced and the
    // standalone archives are removed. Existing recorded facts take priority.
    let mut archive_facts = HashMap::<String, (String, u64)>::new();
    for index in &index_file {
        let facts = archive_facts.entry(index.filename.clone()).or_insert_with(|| {
            let path = apppath.public_dir.join(&index.filename);
            let size = std::fs::metadata(&path).unwrap().len();
            let hash = calculate_sha256(&mut std::fs::File::open(path).unwrap());
            (hash, size)
        });
        if index.hash != "no hash" {
            facts.0 = index.hash.clone();
        }
        if let Some(size) = index.archive_size {
            facts.1 = size;
        }
    }

    // 执行合并前需要先测试一遍
    console.log_debug("正在执行合并前的解压测试");
    let mut tester = ArchiveTester::new();
    for (index, meta) in index_file.read_all_metas(&apppath.public_dir) {
        tester.feed_version(apppath.public_dir.join(&index.filename), &meta);
    }
    tester.finish(|e| console.log_debug(format!("{}/{} 正在测试 {} 的 {} ({}+{})", e.index, e.total, e.label, e.path, e.offset, e.len))).unwrap();
    console.log_debug("测试通过，开始更新包合并流程");

    // 开始合并流程
    let versions_to_be_combined = (&index_file).into_iter()
        .filter(|e| e.filename != COMBINED_FILENAME)
        .collect::<LinkedList<_>>();

    if versions_to_be_combined.is_empty() {
        console.log_info("没有更新包可以合并");
        return 1;
    }

    console.log_debug("正在读取数据");
    
    let mut history = HistoryFile::new_dir("workspace_root", Weak::new());
    let mut data_locations = HashMap::<String, Location>::new();

    // 保留所有元数据，最后会合并写入tar包里
    let mut meta_group = VersionMetaGroup::new();

    // 读取现有更新包，并复现在history上
    for (index, meta) in index_file.read_all_metas(&apppath.public_dir) {
        if meta_group.contains_meta(&meta.label) {
            continue;
        }
        
        history.replay_operations(&meta);
        
        // 记录所有文件的数据和来源
        for change in &meta.changes {
            match change {
                FileChange::UpdateFile { path, offset, len, .. } => {
                    data_locations.insert(path.to_owned(), Location {
                        label: meta.label.clone(),
                        filename: index.filename.to_owned(),
                        path: path.to_owned(),
                        offset: *offset,
                        len: *len,
                    });
                },
                FileChange::DeleteFile { path } => {
                    data_locations.remove(path);
                },
                FileChange::MoveFile { from, to } => {
                    let hold = data_locations.remove(from).unwrap();
                    data_locations.insert(to.to_owned(), hold);
                }
                _ => (),
            }
        }

        meta_group.add_meta(meta);
    }

    console.log_debug("正在合并数据");

    // 生成新的合并包
    let temp_public = apppath.public_dir.join(".temp");
    
    if !std::fs::exists(&temp_public).unwrap() {
        std::fs::create_dir(&temp_public).unwrap();
    }
    
    let new_tar_file = temp_public.join("combined.tar");
    let mut writer = TarWriter::new(&new_tar_file);

    // 写入每个版本里的所有文件数据
    for (_, loc) in &data_locations {
        // 读取原tar包中的文件，然后复制到合并包中
        let mut reader = TarReader::new(apppath.public_dir.join(&loc.filename));
        let read = reader.open_file(loc.offset, loc.len);
        writer.add_file(read, loc.len, &loc.path, &loc.label);
    }

    console.log_debug("正在更新元数据");

    // 写入元数据
    let version_count = meta_group.0.len();
    let meta_loc = writer.finish(meta_group);

    // 更新索引文件
    let new_index_filepath = temp_public.join("index.json");
    let mut new_index = IndexFile::new();
    for (index, _meta) in index_file.read_all_metas(&apppath.public_dir) {
        let (archive_hash, archive_size) = archive_facts.get(&index.filename).unwrap();
        new_index.add(VersionIndex {
            label: index.label.to_owned(),
            filename: COMBINED_FILENAME.to_owned(),
            offset: meta_loc.offset,
            len: meta_loc.length,
            hash: if index.hash == "no hash" { archive_hash.clone() } else { index.hash },
            archive_size: Some(index.archive_size.unwrap_or(*archive_size)),
        })
    }
    new_index.save(&new_index_filepath);

    // 测试合并包
    let mut tester = ArchiveTester::new();
    for (_index, meta) in new_index.read_all_metas(&temp_public) {
        tester.feed_version(&new_tar_file, &meta);
    }
    tester.finish(|e| console.log_debug(format!("{}/{} 正在测试 {} 的 {} ({}+{})", e.index, e.total, e.label, e.path, e.offset, e.len))).unwrap();
    
    // 合并回原包
    // 1.移动索引文件
    std::fs::remove_file(&apppath.index_file).unwrap();
    std::fs::rename(&new_index_filepath, &apppath.index_file).unwrap();
    
    // 2.移动更新包文件
    let combine_file = apppath.public_dir.join(COMBINED_FILENAME);
    
    let _ = std::fs::remove_file(&combine_file);
    std::fs::rename(&new_tar_file, &combine_file).unwrap();
    
    // 3.清理多余更新包
    for v in &versions_to_be_combined {
        std::fs::remove_file(apppath.public_dir.join(&v.filename)).unwrap();
    }

    // 4.清理临时目录
    let _ = std::fs::remove_dir(temp_public);
    
    console.log_info(format!("合并完成！一共合并了 {} 个版本", version_count));

    // // 生成上传脚本
    // let context = TemplateContext {
    //     upload_files: vec![combine_file.strip_prefix(&ctx.working_dir).unwrap().to_str().unwrap().to_owned()],
    //     delete_files: versions_to_be_combined.iter().map(|e| {
    //         ctx.public_dir.join(&e.filename)
    //             .strip_prefix(&ctx.working_dir).unwrap()
    //             .to_str().unwrap()
    //             .to_owned()
    //     }).collect(),
    // };

    // generate_upload_script(context, ctx, "combined");

    0
}

#[cfg(test)]
mod tests {
    use std::collections::LinkedList;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{task_combine, COMBINED_FILENAME};
    use crate::app_path::AppPath;
    use crate::config::Config;
    use crate::core::data::index_file::{IndexFile, VersionIndex};
    use crate::core::data::version_meta::VersionMeta;
    use crate::core::data::version_meta_group::VersionMetaGroup;
    use crate::core::file_hash::calculate_sha256;
    use crate::core::tar_writer::TarWriter;
    use crate::web::log::Console;

    #[test]
    fn combine_preserves_each_original_archive_hash_and_size() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mcupdate-combine-history-{}-{nonce}",
            std::process::id()
        ));
        let public_dir = root.join("public");
        std::fs::create_dir_all(&public_dir).unwrap();
        let apppath = AppPath {
            working_dir: root.clone(),
            workspace_dir: root.join("workspace"),
            public_dir: public_dir.clone(),
            web_dir: root.join("webpage"),
            index_file: public_dir.join("index.json"),
            ui_profile_file: public_dir.join("ui-profile.json"),
            pending_changes_file: root.join("pending-changes.json"),
            config_file: root.join("config.toml"),
            auth_file: root.join("user.toml"),
        };

        let mut index = IndexFile::new();
        let mut expected = Vec::new();
        for (label, logs) in [("v1", "first"), ("v2", "second version with more text")] {
            let filename = format!("{label}.tar");
            let path = public_dir.join(&filename);
            let writer = TarWriter::new(&path);
            let meta = VersionMeta::new(
                label.to_owned(),
                logs.to_owned(),
                LinkedList::new(),
                Vec::new(),
            );
            let location = writer.finish(VersionMetaGroup::with_one(meta));
            let size = std::fs::metadata(&path).unwrap().len();
            let hash = calculate_sha256(&mut std::fs::File::open(&path).unwrap());
            expected.push((label.to_owned(), hash, size));
            index.add(VersionIndex {
                label: label.to_owned(),
                filename,
                offset: location.offset,
                len: location.length,
                hash: "no hash".to_owned(),
                archive_size: None,
            });
        }
        index.save(&apppath.index_file);

        assert_eq!(task_combine(&apppath, &Config::default(), &Console::new_cli()), 0);

        let combined = IndexFile::load_from_file(&apppath.index_file);
        for (label, hash, size) in expected {
            let version = combined.find(&label).unwrap();
            assert_eq!(version.filename, COMBINED_FILENAME);
            assert_eq!(version.hash, hash);
            assert_eq!(version.archive_size, Some(size));
        }
        assert!(public_dir.join(COMBINED_FILENAME).is_file());
        assert!(!public_dir.join("v1.tar").exists());
        assert!(!public_dir.join("v2.tar").exists());

        std::fs::remove_dir_all(root).unwrap();
    }
}
