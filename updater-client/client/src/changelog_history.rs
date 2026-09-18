use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::data::index_file::IndexFile;
use crate::data::version_meta::VersionMeta;
use crate::error::{BusinessResult, ResultToBusinessError};
use crate::network::Network;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangelogHistoryCache {
    latest_label: String,
    entries: Vec<ChangelogEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangelogEntry {
    label: String,
    logs: String,
}

pub struct ChangelogHistory {
    pub markdown: String,
    pub entry_count: usize,
}

pub async fn load_complete_history(
    network: &mut Network<'_>,
    versions: &IndexFile,
    cache_file: &Path,
) -> BusinessResult<ChangelogHistory> {
    let latest_label = versions[versions.len() - 1].label.as_str();
    if let Some(cache) = load_cache(cache_file).await {
        if cache.latest_label == latest_label {
            return Ok(history_from_entries(&cache.entries));
        }
    }

    let mut fetched_ranges = HashSet::<(String, u64, u32)>::new();
    let mut logs_by_label = HashMap::<String, String>::new();

    for version in versions {
        let key = (version.filename.clone(), version.offset, version.len);
        if !fetched_ranges.insert(key) {
            continue;
        }

        let range = version.offset..(version.offset + version.len as u64);
        let text = network
            .request_text(
                &version.filename,
                range,
                format!("changelog metadata of {}", version.label),
            )
            .await?;
        let root = json::parse(&text).be(|error| {
            format!(
                "更新日志元数据解析失败（{}），原因：{error:?}",
                version.label
            )
        })?;
        for value in root.members() {
            let meta = VersionMeta::load(value);
            logs_by_label.entry(meta.label).or_insert(meta.logs);
        }
    }

    let entries = versions
        .into_iter()
        .filter_map(|version| {
            logs_by_label
                .remove(&version.label)
                .map(|logs| ChangelogEntry {
                    label: version.label.clone(),
                    logs,
                })
        })
        .collect::<Vec<_>>();
    let cache = ChangelogHistoryCache {
        latest_label: latest_label.to_owned(),
        entries,
    };
    save_cache(cache_file, &cache).await;
    Ok(history_from_entries(&cache.entries))
}

pub fn markdown_from_current(entries: impl IntoIterator<Item = (String, String)>) -> String {
    let entries = entries
        .into_iter()
        .map(|(label, logs)| ChangelogEntry { label, logs })
        .collect::<Vec<_>>();
    history_from_entries(&entries).markdown
}

fn history_from_entries(entries: &[ChangelogEntry]) -> ChangelogHistory {
    let mut markdown = String::new();
    for entry in entries.iter().rev() {
        markdown.push_str("## ");
        markdown.push_str(&entry.label);
        markdown.push_str("\n\n");
        if entry.logs.trim().is_empty() {
            markdown.push_str("_本版本未提供更新说明_");
        } else {
            markdown.push_str(entry.logs.trim());
        }
        markdown.push_str("\n\n");
    }
    ChangelogHistory {
        markdown: markdown.trim().to_owned(),
        entry_count: entries.len(),
    }
}

async fn load_cache(path: &Path) -> Option<ChangelogHistoryCache> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&content).ok()
}

async fn save_cache(path: &Path, cache: &ChangelogHistoryCache) {
    let Ok(content) = serde_json::to_vec(cache) else {
        return;
    };
    let _ = tokio::fs::write(path, content).await;
}

#[cfg(test)]
mod tests {
    use super::{history_from_entries, ChangelogEntry};

    #[test]
    fn complete_history_is_newest_first_without_a_count_limit() {
        let entries = (0..1_200)
            .map(|index| ChangelogEntry {
                label: format!("v{index}"),
                logs: format!("change {index}"),
            })
            .collect::<Vec<_>>();

        let history = history_from_entries(&entries);
        assert_eq!(history.entry_count, 1_200);
        assert!(history.markdown.starts_with("## v1199\n\nchange 1199"));
        assert!(history.markdown.ends_with("## v0\n\nchange 0"));
    }

    #[test]
    fn empty_log_is_still_visible_in_history() {
        let history = history_from_entries(&[ChangelogEntry {
            label: "v1".to_owned(),
            logs: String::new(),
        }]);
        assert!(history.markdown.contains("本版本未提供更新说明"));
    }
}
