pub mod bootstrap;
pub mod changelog_history;
pub mod error;
pub mod global_config;
pub mod log;
pub mod network;
pub mod speed_sampler;
pub mod ui_profile;
pub mod user_preferences;
pub mod work;

pub mod common;
pub mod data;
#[cfg(target_os = "windows")]
pub mod ui;
pub mod utility;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use crate::global_config::GlobalConfig;
use crate::log::log_error;
use crate::utility::is_running_under_cargo;
use crate::work::run;

pub struct AppContext {
    pub working_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub public_dir: PathBuf,
    pub index_file: PathBuf,
    pub config: GlobalConfig,
}

pub struct StartupParameter {
    pub graphic_mode: bool,
    pub standalone_progress: bool,
    pub disable_log_file: bool,
    pub manual_history: bool,
    // pub external_config_file: String,
}

pub struct McpatchExitCode(pub i8);

pub fn program() -> McpatchExitCode {
    std::env::set_var("RUST_BACKTRACE", "1");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();

    #[cfg(target_os = "windows")]
    {
        nwg::init().expect("Failed to init Native Windows GUI");
        nwg::Font::set_global_family("Segoe UI").expect("Failed to set default font");
    }

    #[cfg(target_os = "windows")]
    let window_close_signal = tokio::sync::oneshot::channel::<()>();

    #[cfg(target_os = "windows")]
    let bootstrap_pending = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .is_some_and(|path| crate::bootstrap::initialization_requested(&path));
    let (ui_cmd, _ui) = crate::ui::main_ui::MainWindow::new(bootstrap_pending);
    let panic_info_captured = Arc::new(Mutex::new(Option::<String>::None));

    // 捕获异常
    let panic_info_captured2 = panic_info_captured.clone();
    let old_handler = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |_info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let text = format!(
            "program paniked!!!\n{:#?}\nBacktrace: \n{}",
            _info, backtrace
        );

        log_error(format!("-----------\n{}-----------", text));
        *panic_info_captured2.lock().unwrap() = Some(text);

        #[cfg(target_os = "windows")]
        popup_error_dialog(_info, backtrace);

        if !is_running_under_cargo() {
            old_handler(_info);
        }
    }));

    let params = StartupParameter {
        graphic_mode: true,
        standalone_progress: true,
        disable_log_file: false,
        manual_history: launched_manually(),
    };

    // 带ui的逻辑
    #[cfg(target_os = "windows")]
    {
        // 开始执行更新逻辑
        let mut ui_cmd2 = ui_cmd.clone();
        let work = runtime.spawn(async move {
            tokio::select! {
                _ = window_close_signal.1 => McpatchExitCode(0),
                code = run(params, &mut ui_cmd2) => code
            }
        });

        // 守护逻辑，用于关闭ui
        let guard = runtime.spawn(async move {
            let result = work.await;

            // work结束运行后，无论是正常结束，还是panic导致的结束，都要关闭ui
            ui_cmd.exit().await;

            match result {
                Ok(code) => code,
                Err(_) => McpatchExitCode(1),
            }
        });

        // 开始ui事件循环
        #[cfg(target_os = "windows")]
        nwg::dispatch_thread_events();

        // 发送成功代表用户手动关闭了窗口
        if let Ok(_) = window_close_signal.0.send(()) {
            println!("interupted by user");
        }

        // guard不允许出现panic
        return runtime.block_on(guard).unwrap();
    }

    // 不带ui的逻辑
    #[cfg(not(target_os = "windows"))]
    {
        // 开始执行更新逻辑
        return runtime.block_on(run(params, ()));
    }
}

fn launched_manually() -> bool {
    if std::env::args().any(|arg| arg == "--show-history") {
        return true;
    }
    if std::env::args().any(|arg| arg == "--automatic") {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        return parent_process_name().is_some_and(|name| name.eq_ignore_ascii_case("explorer.exe"));
    }

    #[cfg(not(target_os = "windows"))]
    false
}

#[cfg(target_os = "windows")]
fn parent_process_name() -> Option<String> {
    use std::mem::size_of;

    use winapi::shared::minwindef::FALSE;
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::GetCurrentProcessId;
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let current_pid = GetCurrentProcessId();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut has_entry = Process32FirstW(snapshot, &mut entry) != FALSE;
        while has_entry {
            if entry.th32ProcessID == current_pid {
                let parent_pid = entry.th32ParentProcessID;
                let mut parent: PROCESSENTRY32W = std::mem::zeroed();
                parent.dwSize = size_of::<PROCESSENTRY32W>() as u32;
                let mut has_parent = Process32FirstW(snapshot, &mut parent) != FALSE;
                while has_parent {
                    if parent.th32ProcessID == parent_pid {
                        let parent_end = parent
                            .szExeFile
                            .iter()
                            .position(|character| *character == 0)
                            .unwrap_or(parent.szExeFile.len());
                        found = Some(String::from_utf16_lossy(&parent.szExeFile[..parent_end]));
                        break;
                    }
                    has_parent = Process32NextW(snapshot, &mut parent) != FALSE;
                }
                break;
            }
            has_entry = Process32NextW(snapshot, &mut entry) != FALSE;
        }
        CloseHandle(snapshot);
        found
    }
}

/// 根据配置显示或隐藏控制台窗口
#[cfg(target_os = "windows")]
pub fn apply_console_visibility(show: bool) {
    use winapi::um::consoleapi::AllocConsole;
    use winapi::um::wincon::GetConsoleWindow;
    use winapi::um::winuser::{ShowWindow, SW_HIDE, SW_SHOW};

    unsafe {
        let mut hwnd = GetConsoleWindow();
        if show && hwnd.is_null() && AllocConsole() != 0 {
            hwnd = GetConsoleWindow();
        }
        if !hwnd.is_null() {
            ShowWindow(hwnd, if show { SW_SHOW } else { SW_HIDE });
        }
    }
}

/// 报错弹框
#[cfg(target_os = "windows")]
fn popup_error_dialog(info: &std::panic::PanicHookInfo, backtrace: std::backtrace::Backtrace) {
    let mp = nwg::MessageParams {
        title: "Fatal error occurred",
        content: "程序出现错误，即将结束运行。点击确定直接退出，点击取消打印错误信息",
        buttons: nwg::MessageButtons::OkCancel,
        icons: nwg::MessageIcons::Error,
    };

    match nwg::message(&mp) {
        nwg::MessageChoice::Ok => {}
        nwg::MessageChoice::Cancel => {
            nwg::error_message("Error detail", &format!("{:?}\n{}", info, backtrace));
        }
        _ => (),
    }
}
