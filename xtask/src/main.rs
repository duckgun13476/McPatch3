use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

type ProcessResult = Result<(), Box<dyn std::error::Error>>;

fn main() -> ProcessResult {
    dist_binary("client", "c", None)
}

fn dist_binary(crate_name: &str, production_name: &str, features: Option<String>) -> ProcessResult {
    std::env::set_var("RUST_BACKTRACE", "1");

    let ref_name = github_ref_name();
    let dist_dir = project_root().join("target/dist");
    let target = TargetInfo::get(crate_name, production_name, &ref_name, &dist_dir);

    // build artifacts
    let cargo = std::env::var("CARGO").unwrap();

    let mut cmd = Command::new(cargo);

    cmd.current_dir(project_root());

    let mut args = Vec::<String>::new();

    args.push("build".to_owned());
    args.push("--release".to_owned());
    args.push("--package".to_owned());
    args.push(crate_name.to_owned());
    args.push("--target".to_owned());
    args.push(target.rustc_target.to_owned());

    if let Some(features) = features {
        args.push("--features".to_owned());
        args.push(features.to_owned());
    }

    cmd.args(args);

    let status = cmd.status()?;

    if !status.success() {
        Err("cargo build failed")?;
    }

    // pick up artifacts
    drop(std::fs::remove_dir_all(&dist_dir));
    std::fs::create_dir_all(&dist_dir).unwrap();

    // executable
    std::fs::copy(
        &target.artifact_path,
        dist_dir.join(&target.artifact_path_versioned),
    )
    .unwrap();

    if target.rustc_target.contains("-windows-") {
        write_self_update_bundle(&target.artifact_path, &dist_dir, &ref_name)?;
    }

    // symbol
    if let Some(symbols) = target.symbols_path {
        std::fs::copy(
            &symbols,
            dist_dir.join(&target.symbols_path_versioned.unwrap()),
        )
        .unwrap();
    }

    Ok(())
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

fn github_ref_name() -> String {
    if let Ok(value) = std::env::var("MCUPDATE_VERSION") {
        return normalize_version_label(&value);
    }

    if let Ok(value) = std::env::var("GITHUB_REF_NAME") {
        return normalize_version_label(&value);
    }

    Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(project_root())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| normalize_version_label(value.trim()))
        .filter(|value| !value.is_empty())
        .unwrap_or("development".to_owned())
}

fn normalize_version_label(value: &str) -> String {
    value
        .trim()
        .strip_prefix('v')
        .unwrap_or(value.trim())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn write_self_update_bundle(artifact: &Path, dist_dir: &Path, version: &str) -> ProcessResult {
    let executable_name = format!("AutoUpdateClient-{version}.exe");
    std::fs::copy(artifact, dist_dir.join(&executable_name))?;

    let mut names = Vec::new();
    let mut seen = HashSet::new();
    push_safe_executable(&mut names, &mut seen, &executable_name);

    if let Ok(previous_list) = std::env::var("MCUPDATE_PREVIOUS_STARTLIST") {
        for line in std::fs::read_to_string(previous_list)?.lines() {
            push_safe_executable(&mut names, &mut seen, line.trim());
        }
    }

    push_safe_executable(&mut names, &mut seen, "AutoUpdateClient.exe");
    std::fs::write(
        dist_dir.join("startlist.txt"),
        format!("{}\n", names.join("\n")),
    )?;

    if let Ok(loader_jar) = std::env::var("MCUPDATE_LOADER_JAR") {
        let loader_jar = PathBuf::from(loader_jar);
        if !loader_jar.is_file() {
            Err(format!(
                "MCUPDATE_LOADER_JAR is not a file: {}",
                loader_jar.display()
            ))?;
        }
        std::fs::copy(loader_jar, dist_dir.join("Loader.jar"))?;
    }
    Ok(())
}

fn push_safe_executable(names: &mut Vec<String>, seen: &mut HashSet<String>, name: &str) {
    let path = Path::new(name);
    let safe = !name.is_empty()
        && path.components().count() == 1
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));

    if safe && seen.insert(name.to_ascii_lowercase()) {
        names.push(name.to_owned());
    }
}

struct TargetInfo {
    rustc_target: String,

    artifact_path: PathBuf,
    symbols_path: Option<PathBuf>,

    artifact_path_versioned: PathBuf,
    symbols_path_versioned: Option<PathBuf>,
}

impl TargetInfo {
    fn get(crate_name: &str, production_name: &str, version_label: &str, dist_dir: &Path) -> Self {
        let rustc_target = match std::env::var("MP_RUSTC_TARGET") {
            Ok(t) => t,
            Err(_) => {
                if cfg!(target_os = "linux") {
                    "x86_64-unknown-linux-gnu".to_owned()
                } else if cfg!(target_os = "windows") {
                    "x86_64-pc-windows-msvc".to_owned()
                } else if cfg!(target_os = "macos") {
                    "x86_64-apple-darwin".to_owned()
                } else {
                    panic!("Unsupported OS, maybe try setting MP_RUSTC_TARGET")
                }
            }
        };
        let cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    project_root().join(path)
                }
            })
            .unwrap_or_else(|| project_root().join("target"));
        let profile_path = cargo_target_dir.join(&rustc_target).join("release");
        let is_windows = rustc_target.contains("-windows-");

        let (exe_suffix, symbols_suffix) = match is_windows {
            true => (".exe", Some(".pdb")),
            false => ("", None),
        };

        let symbols_name = symbols_suffix.map(|e| format!("{}{e}", crate_name.replace("-", "_")));
        let symbols_name_versioned =
            symbols_suffix.map(|e| format!("{production_name}-{version_label}-{rustc_target}{e}"));

        let artifact_name = format!("{crate_name}{exe_suffix}");
        let artifact_name_versioned =
            format!("{production_name}-{version_label}-{rustc_target}{exe_suffix}");

        let artifact_path = profile_path.join(&artifact_name);
        let symbols_path = symbols_name.as_ref().map(|e| profile_path.join(e));

        let artifact_path_versioned = dist_dir.join(&artifact_name_versioned);
        let symbols_path_versioned = symbols_name_versioned.as_ref().map(|e| dist_dir.join(e));

        Self {
            rustc_target,
            artifact_path,
            symbols_path,
            artifact_path_versioned,
            symbols_path_versioned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_labels_are_safe_for_windows_file_names() {
        assert_eq!(normalize_version_label("v0.0.5"), "0.0.5");
        assert_eq!(
            normalize_version_label("release/test build"),
            "release-test-build"
        );
    }

    #[test]
    fn startlist_entries_are_local_executables_and_case_insensitive_unique() {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        for name in [
            "AutoUpdateClient-new.exe",
            "autoupdateclient-NEW.EXE",
            "../outside.exe",
            "nested/client.exe",
            "notes.txt",
            "AutoUpdateClient.exe",
        ] {
            push_safe_executable(&mut names, &mut seen, name);
        }

        assert_eq!(
            names,
            vec!["AutoUpdateClient-new.exe", "AutoUpdateClient.exe"]
        );
    }
}
