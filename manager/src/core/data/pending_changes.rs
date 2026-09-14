use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u8 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PendingChanges {
    pub schema: u8,
    pub forced_deletions: Vec<ForcedDeletion>,
    pub hash_deletions: Vec<PendingHashDeletion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ForcedDeletion {
    pub path: String,
    pub pending: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PendingHashDeletion {
    pub path: String,
    pub sha256: String,
    pub len: u64,
    pub staged_file: Option<String>,
    pub pending: bool,
}

impl Default for PendingChanges {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            forced_deletions: Vec::new(),
            hash_deletions: Vec::new(),
        }
    }
}

impl PendingChanges {
    pub fn load(path: &Path) -> Result<Self, String> {
        let backup = path.with_extension("json.bak");
        let source = if path.exists() {
            path
        } else if backup.exists() {
            backup.as_path()
        } else {
            return Ok(Self::default());
        };

        let content = std::fs::read_to_string(source)
            .map_err(|error| format!("读取待处理变更失败: {error}"))?;
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|error| format!("解析待处理变更失败: {error}"))?;

        if value.get("schema").and_then(|value| value.as_u64()) == Some(1) {
            let has_legacy_hash_deletions = value
                .get("hash-deletions")
                .and_then(|value| value.as_array())
                .is_some_and(|entries| !entries.is_empty());
            if has_legacy_hash_deletions {
                return Err(
                    "检测到旧版无路径哈希删除规则；为避免误删，必须先在旧版管理端撤销这些规则"
                        .to_owned(),
                );
            }
            value["schema"] = serde_json::Value::from(SCHEMA_VERSION);
        }

        let mut state: Self = serde_json::from_value(value)
            .map_err(|error| format!("解析待处理变更失败: {error}"))?;

        if state.schema != SCHEMA_VERSION {
            return Err(format!("不支持的待处理变更版本: {}", state.schema));
        }

        let mut seen = HashSet::new();
        for deletion in &mut state.forced_deletions {
            deletion.path = normalize_client_path(&deletion.path)?;
            if !seen.insert(path_key(&deletion.path)) {
                return Err(format!("重复的客户端删除路径: {}", deletion.path));
            }
        }
        for deletion in &mut state.hash_deletions {
            deletion.path = normalize_client_path(&deletion.path)?;
            deletion.sha256 = normalize_sha256(&deletion.sha256)?;
            if let Some(staged_file) = &deletion.staged_file {
                normalize_staged_file(staged_file)?;
            }
            if !seen.insert(path_key(&deletion.path)) {
                return Err(format!(
                    "客户端路径存在重复或冲突的删除规则: {}",
                    deletion.path
                ));
            }
        }

        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("序列化待处理变更失败: {error}"))?;
        let temporary = path.with_extension("json.tmp");
        let backup = path.with_extension("json.bak");

        let mut output = std::fs::File::create(&temporary)
            .map_err(|error| format!("创建待处理变更临时文件失败: {error}"))?;
        output
            .write_all(&content)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("写入待处理变更临时文件失败: {error}"))?;
        drop(output);

        if path.exists() {
            if backup.exists() {
                std::fs::remove_file(&backup)
                    .map_err(|error| format!("清理待处理变更旧备份失败: {error}"))?;
            }
            std::fs::rename(path, &backup)
                .map_err(|error| format!("备份待处理变更文件失败: {error}"))?;
        }

        if let Err(error) = std::fs::rename(&temporary, path) {
            if backup.exists() && !path.exists() {
                let _ = std::fs::rename(&backup, path);
            }
            return Err(format!("提交待处理变更文件失败: {error}"));
        }
        if backup.exists() {
            std::fs::remove_file(&backup)
                .map_err(|error| format!("清理待处理变更备份失败: {error}"))?;
        }
        Ok(())
    }

    pub fn add_forced_deletion(&mut self, path: &str) -> Result<String, String> {
        let path = normalize_client_path(path)?;
        if self
            .hash_deletions
            .iter()
            .any(|entry| paths_equal(&entry.path, &path))
        {
            return Err(format!("该路径已登记为哈希删除: {path}"));
        }
        if let Some(existing) = self
            .forced_deletions
            .iter_mut()
            .find(|entry| paths_equal(&entry.path, &path))
        {
            existing.pending = true;
            return Ok(path);
        }

        self.forced_deletions.push(ForcedDeletion {
            path: path.clone(),
            pending: true,
        });
        self.forced_deletions
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(path)
    }

    pub fn remove_forced_deletion(&mut self, path: &str) -> Result<bool, String> {
        let path = normalize_client_path(path)?;
        let before = self.forced_deletions.len();
        self.forced_deletions
            .retain(|entry| !paths_equal(&entry.path, &path));
        Ok(before != self.forced_deletions.len())
    }

    pub fn add_hash_deletion(
        &mut self,
        path: &str,
        sha256: &str,
        len: u64,
        staged_file: Option<String>,
    ) -> Result<String, String> {
        let path = normalize_client_path(path)?;
        let sha256 = normalize_sha256(sha256)?;
        if let Some(staged_file) = &staged_file {
            normalize_staged_file(staged_file)?;
        }
        if self
            .forced_deletions
            .iter()
            .any(|entry| paths_equal(&entry.path, &path))
        {
            return Err(format!("该路径已登记为强制删除: {path}"));
        }
        if let Some(existing) = self
            .hash_deletions
            .iter_mut()
            .find(|entry| paths_equal(&entry.path, &path))
        {
            existing.sha256 = sha256;
            existing.len = len;
            existing.staged_file = staged_file;
            existing.pending = true;
            return Ok(path);
        }
        self.hash_deletions.push(PendingHashDeletion {
            path: path.clone(),
            sha256,
            len,
            staged_file,
            pending: true,
        });
        self.hash_deletions
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(path)
    }

    pub fn remove_hash_deletion(
        &mut self,
        path: &str,
    ) -> Result<Option<PendingHashDeletion>, String> {
        let path = normalize_client_path(path)?;
        let Some(index) = self
            .hash_deletions
            .iter()
            .position(|entry| paths_equal(&entry.path, &path))
        else {
            return Ok(None);
        };
        Ok(Some(self.hash_deletions.remove(index)))
    }

    pub fn mark_emitted<'a>(&mut self, paths: impl IntoIterator<Item = &'a String>) {
        let emitted = paths.into_iter().collect::<HashSet<_>>();
        for deletion in &mut self.forced_deletions {
            if emitted.contains(&deletion.path) {
                deletion.pending = false;
            }
        }
    }

    pub fn mark_hash_emitted<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a String>,
    ) -> Vec<String> {
        let emitted = paths.into_iter().collect::<HashSet<_>>();
        let mut staged_files = Vec::new();
        for deletion in &mut self.hash_deletions {
            if emitted.contains(&deletion.path) {
                deletion.pending = false;
                if let Some(staged_file) = deletion.staged_file.take() {
                    staged_files.push(staged_file);
                }
            }
        }
        staged_files
    }
}

fn normalize_sha256(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("客户端删除哈希必须是 64 位 SHA-256".to_owned());
    }
    Ok(value)
}

fn normalize_staged_file(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("哈希删除暂存文件名无效".to_owned());
    }
    Ok(value.to_owned())
}

pub fn normalize_client_path(path: &str) -> Result<String, String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return Err("客户端路径不能为空".to_owned());
    }
    if path.starts_with('/') || path.contains(':') {
        return Err("客户端路径必须是相对路径".to_owned());
    }

    let mut normalized = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err("客户端路径包含不安全的目录片段".to_owned());
        }
        normalized.push(part);
    }

    Ok(normalized.join("/"))
}

fn path_key(path: &str) -> String {
    path.to_ascii_lowercase()
}

fn paths_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(test)]
mod tests {
    use super::{normalize_client_path, PendingChanges};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state_file(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mcpatch-{name}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    #[test]
    fn normalizes_safe_client_paths() {
        assert_eq!(
            normalize_client_path(r#".minecraft\mods\old.jar"#).unwrap(),
            ".minecraft/mods/old.jar"
        );
    }

    #[test]
    fn rejects_path_escape_and_absolute_paths() {
        assert!(normalize_client_path("../mods/old.jar").is_err());
        assert!(normalize_client_path("C:/mods/old.jar").is_err());
        assert!(normalize_client_path("/mods/old.jar").is_err());
    }

    #[test]
    fn forced_deletions_are_deduplicated_and_rearmed() {
        let mut state = PendingChanges::default();
        let path = state
            .add_forced_deletion(".minecraft/mods/old.jar")
            .unwrap();
        state.mark_emitted([&path]);
        assert!(!state.forced_deletions[0].pending);

        state
            .add_forced_deletion(".minecraft/mods/old.jar")
            .unwrap();
        assert_eq!(state.forced_deletions.len(), 1);
        assert!(state.forced_deletions[0].pending);
    }

    #[test]
    fn hash_deletions_are_exact_deduplicated_and_bounded() {
        let mut state = PendingChanges::default();
        let hash = "a".repeat(64);
        let path = ".minecraft/mods/old.jar";
        state.add_hash_deletion(path, &hash, 42, None).unwrap();
        let owned_path = path.to_owned();
        state.mark_hash_emitted([&owned_path]);
        assert!(!state.hash_deletions[0].pending);
        state.add_hash_deletion(path, &hash, 43, None).unwrap();
        assert_eq!(state.hash_deletions.len(), 1);
        assert_eq!(state.hash_deletions[0].len, 43);
        assert!(state.hash_deletions[0].pending);
        assert!(state
            .add_hash_deletion("../bad.jar", &"b".repeat(64), 1, None)
            .is_err());
        assert!(state.add_hash_deletion(path, "bad", 1, None).is_err());
    }

    #[test]
    fn exact_path_delete_modes_are_mutually_exclusive() {
        let mut state = PendingChanges::default();
        let path = ".minecraft/mods/old.jar";
        state
            .add_hash_deletion(path, &"a".repeat(64), 42, None)
            .unwrap();
        assert!(state
            .add_forced_deletion(".MINECRAFT/MODS/OLD.JAR")
            .is_err());

        state.remove_hash_deletion(path).unwrap();
        state.add_forced_deletion(path).unwrap();
        assert!(state
            .add_hash_deletion(".MINECRAFT/MODS/OLD.JAR", &"b".repeat(64), 42, None)
            .is_err());
    }

    #[test]
    fn migrates_empty_schema_one_but_rejects_unsafe_legacy_hash_rules() {
        let empty = temp_state_file("pending-empty-v1");
        std::fs::write(
            &empty,
            r#"{"schema":1,"forced-deletions":[],"hash-deletions":[]}"#,
        )
        .unwrap();
        assert_eq!(PendingChanges::load(&empty).unwrap().schema, 2);
        std::fs::remove_file(&empty).unwrap();

        let legacy = temp_state_file("pending-hash-v1");
        std::fs::write(
            &legacy,
            format!(
                r#"{{"schema":1,"forced-deletions":[],"hash-deletions":[{{"sha256":"{}","len":1,"name-hint":"old.jar","search-root":".minecraft/mods","pending":true}}]}}"#,
                "a".repeat(64)
            ),
        )
        .unwrap();
        let error = PendingChanges::load(&legacy).unwrap_err();
        assert!(error.contains("旧版无路径哈希删除规则"));
        std::fs::remove_file(&legacy).unwrap();
    }
}
