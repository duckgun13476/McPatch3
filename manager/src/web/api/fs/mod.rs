pub mod disk_info;
pub mod list;
pub mod upload;
pub mod download;
pub mod make_directory;
pub mod delete;
pub mod sign_file;
pub mod extract_file;
pub mod r#move;

use std::path::{Component, Path, PathBuf};

use crate::app_path::AppPath;

/// Resolve a browser-supplied path beneath workspace, including existing
/// symlink ancestors. Creation targets may not exist yet, so the nearest
/// existing ancestor is used for the containment check.
pub fn workspace_path(
    apppath: &AppPath,
    requested: &str,
    allow_empty: bool,
) -> Result<PathBuf, String> {
    resolve_workspace_path(&apppath.workspace_dir, requested, allow_empty)
}

fn resolve_workspace_path(
    workspace_dir: &Path,
    requested: &str,
    allow_empty: bool,
) -> Result<PathBuf, String> {
    let normalized = requested.replace('\\', "/");
    if normalized.is_empty() {
        if allow_empty {
            return workspace_dir.canonicalize().map_err(|err| err.to_string());
        }
        return Err("path must not be empty".to_owned());
    }

    let bytes = normalized.as_bytes();
    let has_windows_drive_prefix = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':';
    if normalized.starts_with('/') || has_windows_drive_prefix {
        return Err("path must stay inside workspace".to_owned());
    }

    let relative = Path::new(&normalized);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("path must stay inside workspace".to_owned());
    }

    let workspace = workspace_dir
        .canonicalize()
        .map_err(|err| format!("unable to resolve workspace: {err}"))?;
    let target = workspace.join(relative);
    let mut ancestor = target.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "path must stay inside workspace".to_owned())?;
    }

    let resolved_ancestor = ancestor
        .canonicalize()
        .map_err(|err| format!("unable to resolve path: {err}"))?;
    if !resolved_ancestor.starts_with(&workspace) {
        return Err("path escapes workspace through a symbolic link".to_owned());
    }

    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_only_paths_beneath_workspace() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("mcpatch-workspace-path-{unique}"));
        fs::create_dir_all(workspace.join("mods")).unwrap();

        assert_eq!(
            resolve_workspace_path(&workspace, "mods/example.jar", false).unwrap(),
            workspace.canonicalize().unwrap().join("mods/example.jar")
        );
        assert_eq!(
            resolve_workspace_path(&workspace, "", true).unwrap(),
            workspace.canonicalize().unwrap()
        );

        for unsafe_path in [
            "../config.toml",
            "mods/../config.toml",
            "/etc/passwd",
            "C:\\Windows\\win.ini",
            "C:/Windows/win.ini",
            "C:relative.txt",
            "./mods",
        ] {
            assert!(resolve_workspace_path(&workspace, unsafe_path, false).is_err());
        }

        fs::remove_dir_all(workspace).unwrap();
    }
}
