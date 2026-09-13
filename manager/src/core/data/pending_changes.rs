use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PendingChanges {
    pub schema: u8,
    pub forced_deletions: Vec<ForcedDeletion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ForcedDeletion {
    pub path: String,
    pub pending: bool,
}

impl Default for PendingChanges {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            forced_deletions: Vec::new(),
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
        let mut state: Self = serde_json::from_str(&content)
            .map_err(|error| format!("解析待处理变更失败: {error}"))?;

        if state.schema != SCHEMA_VERSION {
            return Err(format!("不支持的待处理变更版本: {}", state.schema));
        }

        let mut seen = HashSet::new();
        for deletion in &mut state.forced_deletions {
            deletion.path = normalize_client_path(&deletion.path)?;
            if !seen.insert(deletion.path.clone()) {
                return Err(format!("重复的客户端删除路径: {}", deletion.path));
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
        if let Some(existing) = self
            .forced_deletions
            .iter_mut()
            .find(|entry| entry.path == path)
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
        self.forced_deletions.retain(|entry| entry.path != path);
        Ok(before != self.forced_deletions.len())
    }

    pub fn mark_emitted<'a>(&mut self, paths: impl IntoIterator<Item = &'a String>) {
        let emitted = paths.into_iter().collect::<HashSet<_>>();
        for deletion in &mut self.forced_deletions {
            if emitted.contains(&deletion.path) {
                deletion.pending = false;
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{normalize_client_path, PendingChanges};

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
}
