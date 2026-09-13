use std::path::Path;

const DEFAULT_UI_PROFILE: &str = r#"{
  "schema": 1,
  "headline": "自动更新器",
  "subtitle": "安全检查并应用客户端更新",
  "footer": "请保持此窗口开启，完成后将自动启动客户端。",
  "icon": "",
  "stages": {
    "prepare": "正在准备",
    "checking": "正在检查",
    "downloading": "正在下载",
    "applying": "正在应用",
    "completed": "更新完成"
  }
}
"#;

/// 初始化一次可编辑的客户端界面资料。存在时绝不覆盖管理员配置。
pub fn ensure_ui_profile(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::write(path, DEFAULT_UI_PROFILE)?;
    }
    Ok(())
}
