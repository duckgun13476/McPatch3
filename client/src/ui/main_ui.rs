use std::cell::RefCell;
use std::sync::Arc;

use nwd::NwgUi;
use nwg::NativeUi;
use tokio::sync::Mutex;

use crate::ui::MpscReceiver;
use crate::ui::MpscSender;

/// 对话框的内容
pub struct DialogContent {
    /// 标题
    pub title: String,

    /// 内容
    pub content: String,

    /// 是否显示Yes+No双按钮，还是仅显示Yes按钮
    pub yesno: bool,
}

/// UI的交互命令
enum Command {
    /// 关闭UI
    Exit,

    /// 设置窗口可见性
    SetVisible(bool),

    /// 设置窗口标题
    SetTitle(String),

    /// 设置窗口里的主文字
    SetLabel(String),

    /// 设置窗口里的副文字
    SetLabelSecondary(String),

    /// 更新进度条
    SetProgress(u32),

    /// 弹出一个模态对话框
    PupopDialog(DialogContent),
}

/// 应用程序的主窗口，负责大部分信息反馈和交互
#[derive(NwgUi)]
pub struct MainWindow {
    #[nwg_control(size: (560, 220), title: "松饼小镇更新器", flags: "WINDOW", center: true, topmost: false)]
    #[nwg_events(OnWindowClose: [MainWindow::close])]
    window: nwg::Window,

    #[nwg_resource(family: "Microsoft YaHei UI", size: 18, weight: 700)]
    brand_font: nwg::Font,

    #[nwg_resource(family: "Microsoft YaHei UI", size: 11, weight: 500)]
    body_font: nwg::Font,

    #[nwg_resource(family: "Microsoft YaHei UI", size: 10, weight: 400)]
    hint_font: nwg::Font,

    #[nwg_control(position: (22, 18), size: (516, 30), text: "齿轮の松饼小镇", font: Some(&data.brand_font),
        flags: "VISIBLE|ELIPSIS", h_align: HTextAlign::Left)]
    brand: nwg::Label,

    #[nwg_control(position: (22, 48), size: (516, 22), text: "正在安全检查客户端更新", font: Some(&data.body_font),
        flags: "VISIBLE|ELIPSIS", h_align: HTextAlign::Left)]
    phase: nwg::Label,

    #[nwg_control(position: (22, 82), size: (430, 28), text: "准备更新", font: Some(&data.body_font),
        flags: "VISIBLE|ELIPSIS", h_align: HTextAlign::Left)]
    label: nwg::Label,

    #[nwg_control(position: (452, 82), size: (86, 28), text: "0%", font: Some(&data.body_font),
        flags: "VISIBLE", h_align: HTextAlign::Right)]
    progress_text: nwg::Label,

    #[nwg_control(position: (22, 112), size: (516, 25), text: "正在准备下载文件", font: Some(&data.hint_font),
        flags: "VISIBLE|ELIPSIS", h_align: HTextAlign::Left)]
    label_secondary: nwg::Label,

    #[nwg_control(position: (22, 148), size: (516, 20), range: 0..1000)]
    progress: nwg::ProgressBar,

    #[nwg_control(position: (22, 178), size: (516, 20), text: "更新期间请保持此窗口开启。完成后将自动启动客户端。", font: Some(&data.hint_font),
        flags: "VISIBLE|ELIPSIS", h_align: HTextAlign::Left)]
    hint: nwg::Label,

    #[nwg_control]
    #[nwg_events(OnNotice: [MainWindow::on_noticed])]
    notice: nwg::Notice,

    commands: RefCell<MpscReceiver<Command>>,
    dialog_result: MpscSender<bool>,
}

impl MainWindow {
    pub fn new() -> (MainUiCommand, main_window_ui::MainWindowUi) {
        let (dialog_result, receiver) = tokio::sync::mpsc::channel(1000);
        let (sender, commands) = tokio::sync::mpsc::channel(1000);
        
        let data = Self {
            window: Default::default(),
            brand_font: Default::default(),
            body_font: Default::default(),
            hint_font: Default::default(),
            brand: Default::default(),
            phase: Default::default(),
            label: Default::default(),
            progress_text: Default::default(),
            label_secondary: Default::default(),
            progress: Default::default(),
            hint: Default::default(),
            notice: Default::default(),
            commands: RefCell::new(commands),
            dialog_result,
        };

        let ui = Self::build_ui(data).unwrap();

        let cmd = MainUiCommand { 
            inner: Arc::new(Mutex::new(MainUiCommandInner {
                sender, 
                receiver, 
                notice_sender: ui.notice.sender(),
            }))
        };

        (cmd, ui)
    }

    fn on_noticed(&self) {
        // 在本函数里调用nwg::modal_message()会触发on_noticed()的递归，导致运行时借用检查panic
        // 所以吧poll逻辑单独卸载一个闭包里，最小化运行时借用的范围以避免栈溢出的问题
        let poll_command = || -> Option<Command> {
            let mut receiver = self.commands.borrow_mut();

            match receiver.is_empty() {
                true => None,
                false => receiver.blocking_recv(),
            }
        };
        
        while let Some(cmd) = poll_command() {
            match cmd {
                Command::Exit => {
                    self.close();
                },
                Command::SetVisible(visible) => {
                    self.window.set_visible(visible);
                },
                Command::SetTitle(title) => {
                    self.window.set_text(&title);
                },
                Command::SetLabel(label) => {
                    self.phase.set_text(Self::phase_for_status(&label));
                    self.label.set_text(&label);
                },
                Command::SetProgress(progress) => {
                    self.progress.set_pos(progress);
                    self.progress_text.set_text(&format!("{}%", progress / 10));
                },
                Command::SetLabelSecondary(label) => {
                    self.label_secondary.set_text(&label);
                },
                Command::PupopDialog(dialog) => {
                    let prams = nwg::MessageParams {
                        title: &dialog.title,
                        content: &dialog.content,
                        buttons: match dialog.yesno {
                            true => nwg::MessageButtons::OkCancel,
                            false => nwg::MessageButtons::Ok,
                        },
                        icons: nwg::MessageIcons::Info,
                    };
                
                    let choice = nwg::modal_message(&self.window, &prams);

                    let result = match choice {
                        nwg::MessageChoice::No => false,
                        nwg::MessageChoice::Yes => true,
                        nwg::MessageChoice::Ok => true,
                        _ => false,
                    };

                    self.dialog_result.blocking_send(result).unwrap();
                },
            }
        }
    }
    
    fn close(&self) {
        nwg::stop_thread_dispatch();
    }

    fn phase_for_status(status: &str) -> &'static str {
        if status.contains("下载") {
            "正在下载更新"
        } else if status.contains("移动") || status.contains("处理") || status.contains("清理") || status.contains("收尾") {
            "正在应用更新"
        } else if status.contains("没有更新") {
            "客户端已是最新版本"
        } else if status.contains("检查") || status.contains("收集") || status.contains("元数据") {
            "正在检查更新"
        } else {
            "正在准备更新"
        }
    }
}

struct MainUiCommandInner {
    /// 向窗口发送命令的对象
    sender: MpscSender<Command>,

    /// 接收窗口返回的对话框的用户选择，看看用户点击了Yes还是No按钮
    receiver: MpscReceiver<bool>,

    /// 通知窗口有新的命令到达了，需要进行处理
    notice_sender: nwg::NoticeSender,
}

/// 主窗口向外暴露的命令对象，通过channel来和窗口进行交互
#[derive(Clone)]
pub struct MainUiCommand {
    inner: Arc<Mutex<MainUiCommandInner>>,
}

impl MainUiCommand {
    pub async fn exit(&self) {
        let this = self.inner.lock().await;

        this.sender.send(Command::Exit).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_visible(&self, visible: bool) {
        let this = self.inner.lock().await;
        
        this.sender.send(Command::SetVisible(visible)).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_title(&self, title: String) {
        let this = self.inner.lock().await;
        
        this.sender.send(Command::SetTitle(title)).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_label(&self, text: String) {
        let this = self.inner.lock().await;
        
        this.sender.send(Command::SetLabel(text)).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_progress(&self, value: u32) {
        let this = self.inner.lock().await;
        
        this.sender.send(Command::SetProgress(value)).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_label_secondary(&self, text: String) {
        let this = self.inner.lock().await;
        
        this.sender.send(Command::SetLabelSecondary(text)).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn popup_dialog(&self, dialog: DialogContent) -> bool {
        let mut this = self.inner.lock().await;
        
        this.sender.send(Command::PupopDialog(dialog)).await.unwrap();
        this.notice_sender.notice();

        this.receiver.recv().await.unwrap()
    }
}
