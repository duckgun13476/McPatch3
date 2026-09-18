use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferences {
    #[serde(default)]
    pub auto_launch_after_update: bool,
}

impl UserPreferences {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let content = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::UserPreferences;

    #[test]
    fn missing_or_invalid_preferences_fall_back_to_disabled() {
        let root = std::env::temp_dir().join(format!(
            "mcupdate-preferences-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("user-preferences.json");

        assert!(!UserPreferences::load(&path).auto_launch_after_update);
        std::fs::write(&path, b"not-json").unwrap();
        assert!(!UserPreferences::load(&path).auto_launch_after_update);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn auto_launch_preference_round_trips() {
        let root = std::env::temp_dir().join(format!(
            "mcupdate-preferences-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("user-preferences.json");

        UserPreferences {
            auto_launch_after_update: true,
        }
        .save(&path)
        .unwrap();

        assert!(UserPreferences::load(&path).auto_launch_after_update);
        let _ = std::fs::remove_dir_all(&root);
    }
}
