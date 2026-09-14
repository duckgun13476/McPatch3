//! 版本元数据
//!
//! 版本元数据是对一个版本更新的描述信息。包括版本号，更新记录，文件变更列表等
//!
//! 版本元数据在存储时会序列化成一个Json对象，格式如下：
//!
//! ```json
//! {
//!     "label": "1.0", // 版本号
//!     "logs": "这是这个版本的更新记录文字示例", // 这个版本的更新日志
//!     "changes": [ // 记录所有文件修改操作
//!         {
//!             "operation": "create-directory", // 创建一个目录
//!             "path": ".minecraft/mods"        // 要创建目录的路径
//!         },
//!         {
//!             "operation": "update-file",      // 新增或者更新现有文件
//!             "path": "游玩指南.txt",           // 要写入的文件路径
//!             "hash": "82e09fc553b335ab_1306", // 文件校验值
//!             "length": 13761,                 // 文件长度
//!             "modified": 1705651134,          // 文件的修改时间
//!             "offset": 98724                  // 二进制数据在更新包中的偏移值
//!         },
//!         {
//!             "operation": "delete-directory", // 删除一个目录
//!             "path": ".minecraft/logs"        // 要删除的目录的路径
//!         },
//!         {
//!             "operation": "delete-file",      // 删除一个文件
//!             "path": ".minecraft/logs/1.log"  // 要删除的文件的路径
//!         },
//!         {
//!             "operation": "move-file",        // 移动一个文件
//!             "from": ".minecraft/mods/a.jar", // 从哪里来
//!             "to": ".minecraft/mods/b.txt"    // 到哪里去
//!         }
//!     ]
//! }
//! ```
//! 所有这些文件修改操作会被记录下来，并发送到客户端，客户端收到后，会复现这些操作，这样就完成了文件同步
//!
//! 在复现这些文件修改时需要讲究严格顺序：删除旧文件 -> 覆盖文件 -> 移动文件 -> 更新文件 -> 删除目录
//!
//! 所有“覆盖的文件”除了有路径和哈希以外，打包时还得额外带上这个文件本身的二进制数据，这样客户端才可以进行解压覆盖。而其它文件操作则只需要有路径就够了，没有必要带着完整的文件数据

use std::collections::LinkedList;
use std::ops::Add;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use json::JsonValue;

#[derive(Clone)]
pub struct ExternalSource {
    pub provider: String,
    pub url: String,
    pub fallback_to_mcpatch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientHashDeletion {
    pub sha256: String,
    pub len: u64,
    pub name_hint: String,
    pub search_root: String,
}

/// 代表单个文件操作
#[derive(Clone)]
pub enum FileChange {
    /// 创建一个目录
    CreateFolder {
        /// 要创建目录的路径
        path: String,
    },

    /// 新增新的文件或者更新现有文件
    UpdateFile {
        /// 要更新的文件路径
        path: String,

        /// 文件校验值
        hash: String,

        /// 文件长度
        len: u64,

        /// 文件的修改时间
        modified: SystemTime,

        /// 文件二进制数据在更新包中的偏移值
        offset: u64,

        /// Optional public CDN source supplied by the update manager.
        external_source: Option<ExternalSource>,
    },

    /// 删除一个目录
    DeleteFolder {
        /// 要删除的目录的路径
        path: String,
    },

    /// 删除一个文件
    DeleteFile {
        /// 要删除的文件的路径
        path: String,
    },

    /// 移动一个文件
    MoveFile {
        /// 文件从哪里来
        from: String,

        /// 文件到哪里去
        to: String,
    },
}

/// 代表一个版本的元数据
pub struct VersionMeta {
    /// 版本号（也叫标签）
    pub label: String,

    /// 这个版本的更新日志
    pub logs: String,

    /// 文件变动列表
    pub changes: LinkedList<FileChange>,

    pub client_hash_deletions: Vec<ClientHashDeletion>,
}

impl VersionMeta {
    /// 创建一个新的版本元数据
    pub fn new(label: String, logs: String, changes: LinkedList<FileChange>) -> Self {
        Self {
            label,
            logs,
            changes,
            client_hash_deletions: Vec::new(),
        }
    }

    /// 加载一个现有的版本元数据
    pub fn load(obj: &JsonValue) -> Self {
        Self {
            label: obj["label"].as_str().unwrap().to_owned(),
            logs: obj["logs"].as_str().unwrap().to_owned(),
            changes: obj["changes"]
                .members()
                .map(|e| Self::parse_change(e))
                .collect::<LinkedList<_>>(),
            client_hash_deletions: obj["delete-by-hash"]
                .members()
                .filter_map(Self::parse_hash_deletion)
                .collect(),
        }
    }

    /// 将版本元数据序列化成JsonObject
    pub fn serialize(&self) -> JsonValue {
        let mut obj = JsonValue::new_object();
        let mut changes = JsonValue::new_array();

        for change in &self.changes {
            changes.push(Self::serialize_change(change)).unwrap();
        }

        obj.insert("label", self.label.clone()).unwrap();
        obj.insert("logs", self.logs.clone()).unwrap();
        obj.insert("changes", changes).unwrap();
        obj
    }

    /// 解析单个文件变动操作
    fn parse_change(v: &JsonValue) -> FileChange {
        match v["operation"].as_str().unwrap() {
            "create-directory" => FileChange::CreateFolder {
                path: v["path"].as_str().unwrap().to_owned(),
            },
            "update-file" => {
                let external = &v["external-source"];
                let external_source =
                    match (external["provider"].as_str(), external["url"].as_str()) {
                        (Some(provider), Some(url)) if !provider.is_empty() && !url.is_empty() => {
                            Some(ExternalSource {
                                provider: provider.to_owned(),
                                url: url.to_owned(),
                                fallback_to_mcpatch: external["fallback-to-mcpatch"]
                                    .as_bool()
                                    .unwrap_or(true),
                            })
                        }
                        _ => None,
                    };

                FileChange::UpdateFile {
                    path: v["path"].as_str().unwrap().to_owned(),
                    hash: v["hash"].as_str().unwrap().to_owned(),
                    len: v["len"].as_u64().unwrap(),
                    modified: UNIX_EPOCH.add(Duration::from_secs(v["modified"].as_u64().unwrap())),
                    offset: v["offset"].as_u64().unwrap(),
                    external_source,
                }
            }
            "delete-directory" => FileChange::DeleteFolder {
                path: v["path"].as_str().unwrap().to_owned(),
            },
            "delete-file" => FileChange::DeleteFile {
                path: v["path"].as_str().unwrap().to_owned(),
            },
            "move-file" => FileChange::MoveFile {
                from: v["from"].as_str().unwrap().to_owned(),
                to: v["to"].as_str().unwrap().to_owned(),
            },
            _ => panic!("unknown operation: {}", v["operation"].as_str().unwrap()),
        }
    }

    fn parse_hash_deletion(v: &JsonValue) -> Option<ClientHashDeletion> {
        let sha256 = v["sha256"].as_str()?.to_ascii_lowercase();
        let len = v["len"].as_u64()?;
        let name_hint = v["name-hint"].as_str()?.to_owned();
        let search_root = v["search-root"].as_str()?.to_owned();
        if sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || search_root != ".minecraft/mods"
        {
            return None;
        }
        Some(ClientHashDeletion {
            sha256,
            len,
            name_hint,
            search_root,
        })
    }

    /// 序列化一个文件变动操作
    fn serialize_change(change: &FileChange) -> JsonValue {
        let mut obj = JsonValue::new_object();

        match change {
            FileChange::CreateFolder { path } => {
                obj.insert("operation", "create-directory").unwrap();
                obj.insert("path", path.to_owned()).unwrap();
            }
            FileChange::UpdateFile {
                path,
                hash,
                len,
                modified,
                offset,
                external_source,
            } => {
                obj.insert("operation", "update-file").unwrap();
                obj.insert("path", path.to_owned()).unwrap();
                obj.insert("hash", hash.to_owned()).unwrap();
                obj.insert("len", len.to_owned()).unwrap();
                obj.insert(
                    "modified",
                    modified.duration_since(UNIX_EPOCH).unwrap().as_secs(),
                )
                .unwrap();
                obj.insert("offset", offset.to_owned()).unwrap();
                if let Some(source) = external_source {
                    let mut source_obj = JsonValue::new_object();
                    source_obj
                        .insert("provider", source.provider.to_owned())
                        .unwrap();
                    source_obj.insert("url", source.url.to_owned()).unwrap();
                    source_obj
                        .insert("fallback-to-mcpatch", source.fallback_to_mcpatch)
                        .unwrap();
                    obj.insert("external-source", source_obj).unwrap();
                }
            }
            FileChange::DeleteFolder { path } => {
                obj.insert("operation", "delete-directory").unwrap();
                obj.insert("path", path.to_owned()).unwrap();
            }
            FileChange::DeleteFile { path } => {
                obj.insert("operation", "delete-file").unwrap();
                obj.insert("path", path.to_owned()).unwrap();
            }
            FileChange::MoveFile { from, to } => {
                obj.insert("operation", "move-file").unwrap();
                obj.insert("from", from.to_owned()).unwrap();
                obj.insert("to", to.to_owned()).unwrap();
            }
        }

        obj
    }
}

#[cfg(test)]
mod tests {
    use super::{FileChange, VersionMeta};

    #[test]
    fn parses_external_download_source() {
        let value = json::parse(
            r#"{
                "operation":"update-file",
                "path":".minecraft/mods/example.jar",
                "hash":"abc_1234",
                "len":42,
                "modified":1,
                "offset":512,
                "external-source":{
                    "provider":"modrinth",
                    "url":"https://cdn.modrinth.com/example.jar",
                    "fallback-to-mcpatch":false
                }
            }"#,
        )
        .unwrap();

        match VersionMeta::parse_change(&value) {
            FileChange::UpdateFile {
                external_source: Some(source),
                ..
            } => {
                assert_eq!(source.provider, "modrinth");
                assert_eq!(source.url, "https://cdn.modrinth.com/example.jar");
                assert!(!source.fallback_to_mcpatch);
            }
            _ => panic!("external source was not parsed"),
        }
    }

    #[test]
    fn parses_backward_compatible_hash_deletion_metadata() {
        let value = json::parse(
            r#"{
                "label":"v1",
                "logs":"",
                "changes":[],
                "delete-by-hash":[{
                    "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "len":42,
                    "name-hint":"old.jar",
                    "search-root":".minecraft/mods"
                }]
            }"#,
        )
        .unwrap();
        let meta = VersionMeta::load(&value);
        assert_eq!(meta.client_hash_deletions.len(), 1);
        assert_eq!(meta.client_hash_deletions[0].name_hint, "old.jar");
    }
}
