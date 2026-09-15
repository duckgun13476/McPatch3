use std::cell::RefCell;
use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nwd::NwgUi;
use nwg::NativeUi;
use pulldown_cmark::{html, Event as MarkdownEvent, Options, Parser};
use tokio::sync::Mutex;

use crate::ui::MpscReceiver;
use crate::ui::MpscSender;
use crate::ui_profile::UiProfile;

use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use winapi::shared::windef::RECT;
use winapi::um::wingdi::{CreateRoundRectRgn, DeleteObject};
use winapi::um::winuser::{
    BringWindowToTop, GetClientRect, GetWindowLongW, GetWindowRect, IsWindowVisible, PostMessageW,
    RedrawWindow, ReleaseCapture, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes,
    SetWindowLongW, SetWindowPos, SetWindowRgn, GWL_EXSTYLE, HTCAPTION, HWND_NOTOPMOST, HWND_TOP,
    HWND_TOPMOST, LWA_ALPHA, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WM_CLOSE, WM_NCLBUTTONDOWN, WS_EX_LAYERED,
};

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: winapi::shared::windef::HWND,
        attribute: u32,
        value: *const std::ffi::c_void,
        value_size: u32,
    ) -> i32;
}
use wry::{
    dpi::{PhysicalPosition, PhysicalSize},
    Rect, WebView, WebViewBuilder,
};

const UPDATE_PAGE: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<style>
  :root { color-scheme: light; font-family: "Microsoft YaHei UI", "Segoe UI", sans-serif; --accent: #147d67; --accent-hover: #106b59; --accent-soft: #dff2eb; --background: #f4f7f6; --surface: #ffffff; --log-background: #f7faf9; --text: #16332d; --muted: #648078; --border: #e2ebe8; --launch-label-offset-x: 2px; }
  * { box-sizing: border-box; }
  html, body { width: 100%; height: 100%; overflow: hidden; border-radius: 12px; }
  html { background: transparent; }
  body { margin: 0; color: var(--text); background: transparent; }
  #windowSurface { position: relative; width: 100%; height: 100%; overflow: hidden; border: 1px solid var(--border); border-radius: 12px; background: var(--background); }
  #windowSurface.entering { transform-origin: center; animation: updater-enter 315ms linear both; }
  #windowSurface.exiting { pointer-events: none; transform-origin: center; animation: updater-exit 225ms cubic-bezier(.64, 0, .78, 0) both; }
  @keyframes updater-enter {
    from { opacity: 0; transform: scale(.4) rotate(30deg); }
    to { opacity: 1; transform: scale(1) rotate(0deg); }
  }
  @keyframes updater-exit {
    from { opacity: 1; transform: scale(1); }
    to { opacity: 0; transform: scale(.4); }
  }
  .window-bar { position: absolute; inset: 1px 1px auto 1px; z-index: 20; height: 42px; display: flex; align-items: stretch; }
  .drag-region { flex: 1; min-width: 0; cursor: default; user-select: none; }
  .window-close { width: 48px; height: 42px; display: grid; place-items: center; padding: 0; border: 0; background: transparent; color: var(--muted); cursor: pointer; }
  .window-close svg { width: 17px; height: 17px; display: block; }
  .window-close:hover { background: #d83b3b; color: #fff; }
  .window-close:focus-visible { outline: 2px solid var(--accent); outline-offset: -3px; }
  .shell { height: 100vh; padding: 48px 18px 18px; display: flex; flex-direction: column; gap: 14px; }
  .header { display: flex; justify-content: space-between; align-items: center; }
  .identity { display: flex; align-items: center; gap: 14px; }
  .mark { width: 46px; height: 46px; border-radius: 8px; background: var(--accent); color: white; display: grid; place-items: center; font-weight: 800; font-size: 15px; letter-spacing: 1px; }
  h1 { margin: 0; font-size: 23px; font-weight: 700; letter-spacing: 0; }
  .subtitle { margin-top: 4px; color: var(--muted); font-size: 13px; }
  .badge { padding: 7px 11px; border-radius: 999px; background: var(--accent-soft); color: var(--accent); font-size: 12px; font-weight: 700; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 20px; box-shadow: 0 8px 26px rgba(22, 73, 61, .07); }
  .eyebrow { color: var(--muted); font-size: 12px; font-weight: 700; letter-spacing: 0; }
  .status-row { margin-top: 7px; display: flex; gap: 16px; align-items: baseline; justify-content: space-between; }
  #status { font-size: 21px; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  #percent { color: var(--accent); font-size: 22px; font-weight: 800; min-width: 58px; text-align: right; }
  #detail { min-height: 20px; margin-top: 8px; color: var(--muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .track { height: 12px; margin-top: 18px; overflow: hidden; border-radius: 99px; background: var(--border); }
  #bar { width: 0%; height: 100%; border-radius: inherit; background: var(--accent); transition: width .25s ease; }
  #changelogCard { display: none; min-height: 0; flex: 1; flex-direction: column; overflow: hidden; }
  .changelog-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 24px; min-width: 0; }
  #changelogTitle { flex: 0 0 auto; margin: 0; font-size: 20px; }
  #changelogSummary { min-width: 0; color: var(--muted); font-size: 13px; text-align: right; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  #changelogContent { flex: 1; min-height: 0; margin: 12px 0 0; overflow-x: hidden; overflow-y: auto; padding: 15px 18px; border: 1px solid var(--border); border-radius: 6px; background: var(--log-background); color: var(--text); font: 14px/1.75 "Microsoft YaHei UI", "Segoe UI", sans-serif; overflow-wrap: anywhere; }
  #changelogContent > :first-child { margin-top: 0; }
  #changelogContent > :last-child { margin-bottom: 0; }
  #changelogContent h1, #changelogContent h2, #changelogContent h3 { margin: 1.05em 0 .45em; color: var(--text); line-height: 1.35; }
  #changelogContent h1 { font-size: 20px; }
  #changelogContent h2 { padding-bottom: 6px; border-bottom: 1px solid var(--border); font-size: 18px; }
  #changelogContent h3 { font-size: 16px; }
  #changelogContent p { margin: .55em 0; }
  #changelogContent ul, #changelogContent ol { margin: .55em 0; padding-left: 1.7em; }
  #changelogContent li { margin: .2em 0; }
  #changelogContent blockquote { margin: .75em 0; padding: 1px 12px; border-left: 3px solid var(--accent); color: var(--muted); background: var(--accent-soft); }
  #changelogContent code { padding: 2px 5px; border-radius: 4px; background: var(--accent-soft); color: var(--accent); font: 13px/1.55 Consolas, monospace; }
  #changelogContent pre { overflow-x: auto; margin: .75em 0; padding: 12px; border-radius: 6px; background: var(--text); color: var(--surface); white-space: pre; }
  #changelogContent pre code { padding: 0; background: transparent; color: inherit; }
  #changelogContent a { color: var(--accent); text-decoration-thickness: 1px; text-underline-offset: 2px; }
  #changelogContent hr { height: 1px; margin: 1em 0; border: 0; background: var(--border); }
  #changelogContent table { width: 100%; margin: .75em 0; border-collapse: collapse; }
  #changelogContent th, #changelogContent td { padding: 7px 9px; border: 1px solid var(--border); text-align: left; }
  #changelogContent th { background: var(--accent-soft); color: var(--text); }
  #changelogContent input[type="checkbox"] { accent-color: var(--accent); }
  #dialogCard { display: none; min-height: 0; flex: 1; flex-direction: column; overflow: hidden; }
  .dialog-heading { display: flex; align-items: center; gap: 12px; }
  .dialog-symbol { width: 38px; height: 38px; flex: 0 0 38px; display: grid; place-items: center; border-radius: 50%; background: #fff0d9; color: #a85d00; font-size: 24px; font-weight: 800; }
  #dialogTitle { margin: 0; font-size: 22px; line-height: 1.35; }
  #dialogContent { flex: 1; min-height: 0; margin: 18px 0 0; overflow-x: hidden; overflow-y: auto; padding: 18px 20px; border: 1px solid var(--border); border-radius: 6px; background: var(--log-background); color: var(--text); font: 14px/1.75 "Microsoft YaHei UI", "Segoe UI", sans-serif; white-space: pre-wrap; overflow-wrap: anywhere; }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 12px; margin-top: 16px; }
  .dialog-action { min-width: 176px; height: 48px; padding: 0 22px; border: 1px solid var(--border); border-radius: 24px; background: var(--surface); color: var(--text); font: 700 14px "Microsoft YaHei UI", "Segoe UI", sans-serif; cursor: pointer; }
  .dialog-action.primary { border-color: var(--accent); background: var(--accent); color: #fff; }
  .dialog-action:hover { border-color: var(--accent); }
  .dialog-action.primary:hover { background: var(--accent-hover); }
  .dialog-action:focus-visible { outline: 3px solid var(--accent-soft); outline-offset: 2px; }
  .foot { margin-top: auto; color: var(--muted); font-size: 12px; text-align: center; }
  .foot:not(.complete) { display: grid; grid-template-columns: minmax(0, 1fr) minmax(280px, 38.2%); gap: 16px; align-items: center; }
  .foot:not(.complete) #footer { text-align: left; }
  .foot.complete { display: grid; grid-template-columns: minmax(0, 3fr) minmax(190px, 1fr); gap: 14px; align-items: center; }
  .foot.complete #footer { display: none; }
  #footerActions { display: flex; min-width: 0; align-items: center; justify-content: flex-end; gap: 10px; }
  .foot.complete #footerActions { display: flex; justify-content: flex-end; }
  .traffic-stat { width: clamp(220px, 38.2%, 330px); min-width: 0; display: grid; grid-template-columns: 22px minmax(0, 1fr); align-items: center; gap: 10px; padding: 3px 16px; border-left: 2px solid var(--accent-soft); text-align: left; }
  .traffic-symbol { width: 22px; height: 22px; display: grid; place-items: center; color: var(--accent); font-size: 20px; font-weight: 800; line-height: 1; }
  .traffic-copy { min-width: 0; display: flex; align-items: baseline; gap: 10px; }
  .traffic-label { flex: 0 0 auto; color: var(--muted); font-size: 12px; font-weight: 700; }
  #trafficValue { min-width: 0; color: var(--text); font-size: 14px; font-weight: 700; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  #completeButton { display: none; width: 100%; height: 52px; padding: 0 9px; grid-template-columns: 34px minmax(0, 1fr) 34px; align-items: center; gap: 8px; border: 0; border-radius: 26px; background: var(--accent); color: #fff; font: 700 15px "Microsoft YaHei UI", "Segoe UI", sans-serif; cursor: pointer; }
  .foot.complete #completeButton { display: grid; }
  #completeButton::after { content: ""; width: 34px; height: 34px; }
  .complete-icon { width: 34px; height: 34px; display: grid; place-items: center; border-radius: 50%; background: var(--surface); color: var(--accent); }
  .complete-icon svg { width: 19px; height: 19px; display: block; overflow: visible; }
  .complete-label { min-width: 0; text-align: center; transform: translateX(var(--launch-label-offset-x)); }
  #completeButton:hover { background: var(--accent-hover); }
  #completeButton:focus-visible { outline: 3px solid var(--accent-soft); outline-offset: 2px; }
</style>
</head>
<body>
<div id="windowSurface">
  <div class="window-bar" aria-label="窗口控制">
    <div class="drag-region" id="dragRegion" title="拖动窗口"></div>
    <button class="window-close" id="windowClose" type="button" title="关闭" aria-label="关闭"><svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" d="M6 6l12 12M18 6L6 18"/></svg></button>
  </div>
  <main class="shell">
    <header class="header">
      <div class="identity">
        <div class="mark" id="mark">UP</div>
        <div><h1 id="headline">自动更新器</h1><div class="subtitle" id="subtitle">安全检查并应用客户端更新</div></div>
      </div>
      <div class="badge" id="stage">正在准备</div>
    </header>
    <section class="card" id="progressCard">
      <div class="eyebrow">更新状态</div>
      <div class="status-row"><div id="status">正在检查更新</div><div id="percent">0%</div></div>
      <div id="detail">正在连接更新服务</div>
      <div class="track"><div id="bar"></div></div>
    </section>
    <section class="card" id="changelogCard">
      <div class="changelog-heading"><h2 id="changelogTitle">更新完成</h2><div id="changelogSummary"></div></div>
      <div id="changelogContent"></div>
    </section>
    <section class="card" id="dialogCard" role="alertdialog" aria-modal="true" aria-labelledby="dialogTitle">
      <div class="dialog-heading"><div class="dialog-symbol" aria-hidden="true">!</div><h2 id="dialogTitle">更新未完成</h2></div>
      <div id="dialogContent"></div>
      <div class="dialog-actions"><button class="dialog-action" id="dialogSecondary" type="button">取消</button><button class="dialog-action primary" id="dialogPrimary" type="button">关闭更新器</button></div>
    </section>
    <footer class="foot" id="footerBar">
      <div id="footer">请保持此窗口开启，完成后将自动启动客户端。</div>
      <div id="footerActions" aria-label="更新信息"><div class="traffic-stat"><span class="traffic-symbol" aria-hidden="true">↓</span><div class="traffic-copy"><div class="traffic-label">本次下载</div><div id="trafficValue">无需下载</div></div></div></div>
      <button id="completeButton" type="button"><span class="complete-icon" aria-hidden="true"><svg viewBox="0 0 34 34" focusable="false"><path fill="currentColor" d="M12.1 8.3C10.9 7.6 9.8 8.4 9.9 9.8C10.2 14.8 10.2 19.2 9.9 24.2C9.8 25.6 10.9 26.4 12.1 25.7C16.7 23.1 20.8 20.7 24.5 18.5C25.7 17.8 25.7 16.2 24.5 15.5C20.8 13.3 16.7 10.9 12.1 8.3Z"/></svg></span><span class="complete-label">启动</span></button>
    </footer>
  </main>
</div>
<script>
  window.updateUi = ({ stage, status, detail, progress, traffic }) => {
    document.getElementById('stage').textContent = stage;
    document.getElementById('status').textContent = status;
    document.getElementById('detail').textContent = detail;
    const percent = Math.max(0, Math.min(100, Math.floor(progress / 10)));
    document.getElementById('percent').textContent = percent + '%';
    document.getElementById('bar').style.width = percent + '%';
    document.getElementById('trafficValue').textContent = traffic;
  };
  window.updateProfile = ({ headline, subtitle, footer, iconDataUrl, launchLabelOffsetX, theme }) => {
    document.getElementById('headline').textContent = headline;
    document.getElementById('subtitle').textContent = subtitle;
    document.getElementById('footer').textContent = footer;
    const launchOffset = Math.max(-24, Math.min(24, Number(launchLabelOffsetX) || 0));
    document.documentElement.style.setProperty('--launch-label-offset-x', launchOffset + 'px');
    const mark = document.getElementById('mark');
    if (iconDataUrl) {
      const image = document.createElement('img');
      image.alt = 'UP'; image.src = iconDataUrl;
      image.style.cssText = 'width:100%;height:100%;object-fit:cover;border-radius:inherit;display:block';
      mark.replaceChildren(image);
    } else {
      mark.textContent = 'UP';
    }
    const themeVariables = {
      accent: '--accent', accentHover: '--accent-hover', accentSoft: '--accent-soft',
      background: '--background', surface: '--surface', logBackground: '--log-background',
      text: '--text', muted: '--muted', border: '--border'
    };
    Object.entries(theme || {}).forEach(([key, value]) => {
      if (themeVariables[key]) document.documentElement.style.setProperty(themeVariables[key], value);
    });
  };
  window.showChangelog = ({ title, summary, html }) => {
    document.getElementById('progressCard').style.display = 'none';
    document.getElementById('changelogCard').style.display = 'flex';
    document.getElementById('stage').textContent = '更新完成';
    document.getElementById('changelogTitle').textContent = title;
    document.getElementById('changelogSummary').textContent = summary;
    document.getElementById('changelogContent').innerHTML = html;
    document.getElementById('footerBar').classList.add('complete');
  };
  window.showDialog = ({ title, content, retryable }) => {
    document.getElementById('progressCard').style.display = 'none';
    document.getElementById('changelogCard').style.display = 'none';
    document.getElementById('dialogCard').style.display = 'flex';
    document.getElementById('footerBar').style.display = 'none';
    document.getElementById('stage').textContent = '需要处理';
    document.getElementById('dialogTitle').textContent = title;
    document.getElementById('dialogContent').textContent = content;
    const primary = document.getElementById('dialogPrimary');
    const secondary = document.getElementById('dialogSecondary');
    primary.textContent = retryable ? '重试' : '关闭更新器';
    secondary.textContent = '关闭更新器';
    secondary.style.display = retryable ? 'block' : 'none';
    primary.focus();
  };
  window.hideDialog = () => {
    document.getElementById('dialogCard').style.display = 'none';
    document.getElementById('progressCard').style.display = 'block';
    document.getElementById('footerBar').style.display = '';
    document.getElementById('stage').textContent = '正在重试';
  };
  window.reportVisibleFrame = () => requestAnimationFrame(() => requestAnimationFrame(() => window.ipc.postMessage('visible')));
  window.playEnterAnimation = () => {
    const surface = document.getElementById('windowSurface');
    surface.classList.remove('entering');
    void surface.offsetWidth;
    surface.classList.add('entering');
  };
  let closeStarted = false;
  window.requestAnimatedClose = message => {
    if (closeStarted) return;
    closeStarted = true;
    const surface = document.getElementById('windowSurface');
    surface.classList.remove('entering');
    surface.classList.add('exiting');
    window.ipc.postMessage('begin-close');
    setTimeout(() => window.ipc.postMessage(message), 225);
  };
  document.getElementById('dragRegion').addEventListener('pointerdown', event => {
    if (event.button === 0) window.ipc.postMessage('drag');
  });
  document.getElementById('windowClose').addEventListener('click', () => window.requestAnimatedClose('close'));
  document.getElementById('completeButton').addEventListener('click', () => window.requestAnimatedClose('close'));
  document.getElementById('dialogPrimary').addEventListener('click', () => {
    if (document.getElementById('dialogPrimary').textContent === '重试') {
      window.ipc.postMessage('dialog:yes');
    } else {
      window.requestAnimatedClose('dialog:yes');
    }
  });
  document.getElementById('dialogSecondary').addEventListener('click', () => window.requestAnimatedClose('dialog:no'));
  window.ipc.postMessage('ready');
</script>
</body></html>"#;

struct NativeWindowHandle(isize);

impl HasWindowHandle for NativeWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let hwnd = NonZeroIsize::new(self.0).expect("NWG window handle must not be null");
        let handle = Win32WindowHandle::new(hwnd);
        unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::Win32(handle))) }
    }
}

/// 对话框的内容
pub struct DialogContent {
    /// 标题
    pub title: String,

    /// 内容
    pub content: String,

    /// 网络异常时显示“重试”和“关闭更新器”，其他错误仅允许关闭。
    pub retryable: bool,
}

struct ChangelogView {
    title: String,
    summary: String,
    markdown: String,
    html: String,
}

fn render_markdown(markdown: &str) -> String {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(markdown, options).map(|event| match event {
        MarkdownEvent::SoftBreak => MarkdownEvent::HardBreak,
        other => other,
    });
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    ammonia::clean(&rendered)
}

/// UI的交互命令
enum Command {
    /// 关闭UI
    Exit,

    /// 设置窗口可见性
    SetVisible(bool),

    /// 在屏幕外显示并预热 WebView，避免用户看到空白宿主窗口
    PrepareVisible,

    /// 将已完成首帧合成的窗口移回原位置
    RevealVisible,

    /// 设置窗口标题
    SetTitle(String),

    /// 设置服务端缓存的界面资料
    SetProfile(UiProfile),

    /// 设置窗口里的主文字
    SetLabel(String),

    /// 设置窗口里的副文字
    SetLabelSecondary(String),

    /// 更新进度条
    SetProgress(u32),

    /// 更新本次实际下载量与计划下载总量
    SetTransfer { downloaded: u64, total: u64 },

    /// 弹出一个模态对话框
    PupopDialog(DialogContent),

    /// 收起可恢复错误页并重新显示下载进度。
    HideDialog,

    /// 在主窗口内展示更新日志
    ShowChangelog {
        title: String,
        summary: String,
        content: String,
    },
}

/// 应用程序的主窗口，负责大部分信息反馈和交互
#[derive(NwgUi)]
pub struct MainWindow {
    #[nwg_resource(source_bin: Some(include_bytes!("../../app.ico")))]
    app_icon: nwg::Icon,

    #[nwg_control(size: (1235, 720), title: "自动更新器", flags: "POPUP", center: true, topmost: false, icon: Some(&data.app_icon))]
    #[nwg_events(OnInit: [MainWindow::init_webview], OnWindowClose: [MainWindow::close])]
    window: nwg::Window,

    #[nwg_control(position: (0, 0), size: (1235, 720), text: "", background_color: Some([244, 247, 246]), flags: "VISIBLE")]
    placeholder_background: nwg::Label,

    #[nwg_resource(family: "Microsoft YaHei UI", size: 28, weight: 700)]
    placeholder_title_font: nwg::Font,

    #[nwg_resource(family: "Microsoft YaHei UI", size: 20, weight: 700)]
    placeholder_status_font: nwg::Font,

    #[nwg_resource(family: "Microsoft YaHei UI", size: 14)]
    placeholder_body_font: nwg::Font,

    #[nwg_control(position: (38, 30), size: (900, 48), text: "自动更新器", font: Some(&data.placeholder_title_font), background_color: Some([244, 247, 246]), flags: "VISIBLE|ELIPSIS")]
    placeholder_title: nwg::Label,

    #[nwg_control(position: (40, 80), size: (900, 28), text: "安全检查并应用客户端更新", font: Some(&data.placeholder_body_font), background_color: Some([244, 247, 246]), flags: "VISIBLE|ELIPSIS")]
    placeholder_subtitle: nwg::Label,

    #[nwg_control(position: (40, 176), size: (1120, 32), text: "更新状态", font: Some(&data.placeholder_body_font), background_color: Some([244, 247, 246]), flags: "VISIBLE|ELIPSIS")]
    placeholder_phase: nwg::Label,

    #[nwg_control(position: (40, 218), size: (1030, 42), text: "正在准备更新界面", font: Some(&data.placeholder_status_font), background_color: Some([244, 247, 246]), flags: "VISIBLE|ELIPSIS")]
    placeholder_status: nwg::Label,

    #[nwg_control(position: (1070, 218), size: (90, 42), text: "0%", font: Some(&data.placeholder_status_font), background_color: Some([244, 247, 246]), flags: "VISIBLE", h_align: HTextAlign::Right)]
    placeholder_percent: nwg::Label,

    #[nwg_control(position: (40, 272), size: (1120, 30), text: "下载尚未开始", font: Some(&data.placeholder_body_font), background_color: Some([244, 247, 246]), flags: "VISIBLE|ELIPSIS")]
    placeholder_detail: nwg::Label,

    #[nwg_control(position: (40, 330), size: (1120, 18), range: 0..1000)]
    placeholder_progress: nwg::ProgressBar,

    #[nwg_control(position: (40, 652), size: (1120, 28), text: "正在准备更新页面，请稍候", font: Some(&data.placeholder_body_font), background_color: Some([244, 247, 246]), flags: "VISIBLE|ELIPSIS")]
    placeholder_hint: nwg::Label,

    #[nwg_control(position: (1180, 8), size: (42, 34), text: "×", font: Some(&data.placeholder_status_font), flags: "VISIBLE")]
    #[nwg_events(OnButtonClick: [MainWindow::close])]
    placeholder_close: nwg::Button,

    #[nwg_control]
    #[nwg_events(OnNotice: [MainWindow::on_noticed])]
    notice: nwg::Notice,

    commands: RefCell<MpscReceiver<Command>>,
    dialog_result: MpscSender<bool>,
    webview: RefCell<Option<WebView>>,
    webview_ready: Arc<AtomicBool>,
    visible_frame_generation: Arc<AtomicUsize>,
    profile: RefCell<UiProfile>,
    webview_error: Arc<std::sync::Mutex<Option<String>>>,
    status: RefCell<String>,
    detail: RefCell<String>,
    progress_value: RefCell<u32>,
    transfer: RefCell<(u64, u64)>,
    transfer_speed: RefCell<u64>,
    transfer_sample: RefCell<(u64, Instant)>,
    pending_changelog: RefCell<Option<ChangelogView>>,
    entry_animation_played: RefCell<bool>,
    close_animation_started: Arc<AtomicBool>,
}

impl MainWindow {
    pub fn new(initial_keepalive: bool) -> (MainUiCommand, main_window_ui::MainWindowUi) {
        let (dialog_result, receiver) = tokio::sync::mpsc::channel(1000);
        let (sender, commands) = tokio::sync::mpsc::channel(1000);
        let webview_error = Arc::new(std::sync::Mutex::new(None));
        let webview_ready = Arc::new(AtomicBool::new(false));
        let visible_frame_generation = Arc::new(AtomicUsize::new(0));
        let close_animation_started = Arc::new(AtomicBool::new(false));

        let data = Self {
            app_icon: Default::default(),
            window: Default::default(),
            placeholder_background: Default::default(),
            placeholder_title_font: Default::default(),
            placeholder_status_font: Default::default(),
            placeholder_body_font: Default::default(),
            placeholder_title: Default::default(),
            placeholder_subtitle: Default::default(),
            placeholder_phase: Default::default(),
            placeholder_status: Default::default(),
            placeholder_percent: Default::default(),
            placeholder_detail: Default::default(),
            placeholder_progress: Default::default(),
            placeholder_hint: Default::default(),
            placeholder_close: Default::default(),
            notice: Default::default(),
            commands: RefCell::new(commands),
            dialog_result,
            webview: RefCell::new(None),
            webview_ready: webview_ready.clone(),
            visible_frame_generation: visible_frame_generation.clone(),
            profile: RefCell::new(UiProfile::default()),
            webview_error: webview_error.clone(),
            status: RefCell::new("正在检查更新".to_owned()),
            detail: RefCell::new("正在连接更新服务".to_owned()),
            progress_value: RefCell::new(0),
            transfer: RefCell::new((0, 0)),
            transfer_speed: RefCell::new(0),
            transfer_sample: RefCell::new((0, Instant::now())),
            pending_changelog: RefCell::new(None),
            entry_animation_played: RefCell::new(false),
            close_animation_started,
        };

        let ui = Self::build_ui(data).unwrap();
        if initial_keepalive {
            if let Some(hwnd) = ui.window.handle.hwnd() {
                Self::set_window_alpha(hwnd, 0);
            }
        }
        ui.window.set_visible(initial_keepalive);
        let window_handle = ui.window.handle.hwnd().expect("main window must be HWND") as isize;

        let cmd = MainUiCommand {
            inner: Arc::new(Mutex::new(MainUiCommandInner {
                sender,
                receiver,
                notice_sender: ui.notice.sender(),
            })),
            webview_error,
            webview_ready,
            visible_frame_generation,
            window_handle,
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
                }
                Command::SetVisible(visible) => {
                    self.window.set_visible(visible);
                }
                Command::PrepareVisible => {
                    self.resize_webview_to_client();
                    let status = self.status.borrow();
                    let detail = self.detail.borrow();
                    self.update_webview(
                        Self::phase_for_status(&status),
                        &status,
                        &detail,
                        *self.progress_value.borrow(),
                    );
                    drop(detail);
                    drop(status);

                    let first_reveal = !*self.entry_animation_played.borrow();
                    self.set_placeholder_visible(first_reveal);
                    if first_reveal {
                        if let Some(hwnd) = self.window.handle.hwnd() {
                            Self::set_window_alpha(hwnd, 0);
                        }
                    }
                    self.window.set_visible(true);
                    if let Some(hwnd) = self.window.handle.hwnd() {
                        unsafe {
                            RedrawWindow(
                                hwnd,
                                std::ptr::null(),
                                std::ptr::null_mut(),
                                RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
                            );
                        }
                    }
                    if let Some(webview) = self.webview.borrow().as_ref() {
                        let _ = webview.evaluate_script("window.reportVisibleFrame();");
                    } else {
                        self.visible_frame_generation.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Command::RevealVisible => {
                    self.set_placeholder_visible(false);
                    if !*self.entry_animation_played.borrow() {
                        if let Some(webview) = self.webview.borrow().as_ref() {
                            let _ = webview.evaluate_script("window.playEnterAnimation();");
                        }
                        if let Some(hwnd) = self.window.handle.hwnd() {
                            unsafe {
                                SetWindowPos(
                                    hwnd,
                                    HWND_TOPMOST,
                                    0,
                                    0,
                                    0,
                                    0,
                                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                                );
                                SetWindowPos(
                                    hwnd,
                                    HWND_NOTOPMOST,
                                    0,
                                    0,
                                    0,
                                    0,
                                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                                );
                                BringWindowToTop(hwnd);
                                SetForegroundWindow(hwnd);
                            }
                            Self::animate_window_visual_async(hwnd, true, 21, 15);
                        }
                        *self.entry_animation_played.borrow_mut() = true;
                    }
                    if let Some(webview) = self.webview.borrow().as_ref() {
                        let _ = webview.evaluate_script("window.reportVisibleFrame();");
                    } else {
                        self.visible_frame_generation.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Command::SetTitle(title) => {
                    let _ = title;
                    self.window.set_text("自动更新器");
                }
                Command::SetProfile(profile) => {
                    *self.profile.borrow_mut() = profile;
                    self.resize_webview_to_client();
                    let status = self.status.borrow();
                    let detail = self.detail.borrow();
                    self.update_webview(
                        Self::phase_for_status(&status),
                        &status,
                        &detail,
                        *self.progress_value.borrow(),
                    );
                }
                Command::SetLabel(label) => {
                    *self.status.borrow_mut() = label;
                    self.placeholder_status
                        .set_text(self.status.borrow().as_str());
                    let status = self.status.borrow();
                    let detail = self.detail.borrow();
                    self.update_webview(
                        Self::phase_for_status(&status),
                        &status,
                        &detail,
                        *self.progress_value.borrow(),
                    );
                }
                Command::SetProgress(progress) => {
                    *self.progress_value.borrow_mut() = progress;
                    self.placeholder_progress.set_pos(progress);
                    self.placeholder_percent
                        .set_text(&format!("{}%", progress / 10));
                    let status = self.status.borrow();
                    let detail = self.detail.borrow();
                    self.update_webview(
                        Self::phase_for_status(&status),
                        &status,
                        &detail,
                        progress,
                    );
                }
                Command::SetTransfer { downloaded, total } => {
                    self.sample_transfer_speed(downloaded);
                    *self.transfer.borrow_mut() = (downloaded, total);
                    let status = self.status.borrow();
                    let detail = self.detail.borrow();
                    self.update_webview(
                        Self::phase_for_status(&status),
                        &status,
                        &detail,
                        *self.progress_value.borrow(),
                    );
                }
                Command::SetLabelSecondary(label) => {
                    *self.detail.borrow_mut() = label;
                    self.placeholder_detail
                        .set_text(self.detail.borrow().as_str());
                    let status = self.status.borrow();
                    let detail = self.detail.borrow();
                    self.update_webview(
                        Self::phase_for_status(&status),
                        &status,
                        &detail,
                        *self.progress_value.borrow(),
                    );
                }
                Command::PupopDialog(dialog) => {
                    if !self.render_dialog(&dialog) {
                        let params = nwg::MessageParams {
                            title: &dialog.title,
                            content: &dialog.content,
                            buttons: match dialog.retryable {
                                true => nwg::MessageButtons::OkCancel,
                                false => nwg::MessageButtons::Ok,
                            },
                            icons: nwg::MessageIcons::Info,
                        };
                        let choice = nwg::modal_message(&self.window, &params);
                        let accepted =
                            matches!(choice, nwg::MessageChoice::Yes | nwg::MessageChoice::Ok);
                        let _ = self.dialog_result.blocking_send(accepted);
                    }
                }
                Command::HideDialog => {
                    if let Some(webview) = self.webview.borrow().as_ref() {
                        let _ = webview.evaluate_script("window.hideDialog();");
                    }
                }
                Command::ShowChangelog {
                    title,
                    summary,
                    content,
                } => {
                    *self.pending_changelog.borrow_mut() = Some(ChangelogView {
                        title,
                        summary,
                        markdown: content.clone(),
                        html: render_markdown(&content),
                    });
                    self.render_pending_changelog();
                }
            }
        }
    }

    fn close(&self) {
        let _ = self.dialog_result.try_send(false);
        if !self.close_animation_started.swap(true, Ordering::AcqRel) {
            if let Some(hwnd) = self.window.handle.hwnd() {
                Self::animate_window_visual(hwnd, false, 15, 14);
            }
        }
        nwg::stop_thread_dispatch();
    }

    fn set_window_alpha(hwnd: winapi::shared::windef::HWND, alpha: u8) {
        unsafe {
            let extended_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            if extended_style & WS_EX_LAYERED as i32 == 0 {
                SetWindowLongW(hwnd, GWL_EXSTYLE, extended_style | WS_EX_LAYERED as i32);
            }
            SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
        }
    }

    fn animate_window_visual(
        hwnd: winapi::shared::windef::HWND,
        entering: bool,
        frames: u16,
        frame_millis: u64,
    ) {
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return;
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return;
        }
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);

        for frame in 0..=frames {
            let progress = frame as f64 / frames as f64;
            let eased = progress * progress * (3.0 - 2.0 * progress);
            let visual_progress = if entering { eased } else { 1.0 - eased };
            let scale = 0.4 + 0.6 * visual_progress;
            let alpha_progress = if entering {
                1.0 - (1.0 - progress).powi(3)
            } else {
                visual_progress
            };
            let alpha = 255.0 * alpha_progress;
            let region_width = (width as f64 * scale).round().max(1.0) as i32;
            let region_height = (height as f64 * scale).round().max(1.0) as i32;
            let left = (width - region_width) / 2;
            let top = (height - region_height) / 2;
            let region = unsafe {
                CreateRoundRectRgn(
                    left,
                    top,
                    left + region_width + 1,
                    top + region_height + 1,
                    24,
                    24,
                )
            };
            if !region.is_null() && unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
                unsafe {
                    DeleteObject(region as _);
                }
            }
            Self::set_window_alpha(hwnd, alpha.round().clamp(0.0, 255.0) as u8);
            std::thread::sleep(Duration::from_millis(frame_millis));
        }

        if entering {
            unsafe {
                SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
            }
            Self::set_window_alpha(hwnd, 255);
        }
    }

    fn animate_window_visual_async(
        hwnd: winapi::shared::windef::HWND,
        entering: bool,
        frames: u16,
        frame_millis: u64,
    ) {
        let hwnd = hwnd as isize;
        std::thread::spawn(move || {
            Self::animate_window_visual(hwnd as _, entering, frames, frame_millis);
        });
    }

    fn set_placeholder_visible(&self, visible: bool) {
        self.placeholder_background.set_visible(visible);
        self.placeholder_title.set_visible(visible);
        self.placeholder_subtitle.set_visible(visible);
        self.placeholder_phase.set_visible(visible);
        self.placeholder_status.set_visible(visible);
        self.placeholder_percent.set_visible(visible);
        self.placeholder_detail.set_visible(visible);
        self.placeholder_progress.set_visible(visible);
        self.placeholder_hint.set_visible(visible);
        self.placeholder_close.set_visible(visible);

        if !visible {
            return;
        }

        self.placeholder_status
            .set_text(self.status.borrow().as_str());
        self.placeholder_detail
            .set_text(self.detail.borrow().as_str());
        let progress = *self.progress_value.borrow();
        self.placeholder_progress.set_pos(progress);
        self.placeholder_percent
            .set_text(&format!("{}%", progress / 10));

        for control in [
            &self.placeholder_background.handle,
            &self.placeholder_title.handle,
            &self.placeholder_subtitle.handle,
            &self.placeholder_phase.handle,
            &self.placeholder_status.handle,
            &self.placeholder_percent.handle,
            &self.placeholder_detail.handle,
            &self.placeholder_progress.handle,
            &self.placeholder_hint.handle,
            &self.placeholder_close.handle,
        ] {
            if let Some(hwnd) = control.hwnd() {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
        }
    }

    fn init_webview(&self) {
        self.window.set_text("自动更新器");
        let hwnd = self.window.handle.hwnd().expect("main window must be HWND") as isize;
        Self::apply_rounded_corners(hwnd as _);
        let parent = NativeWindowHandle(hwnd);
        let webview_ready = self.webview_ready.clone();
        let visible_frame_generation = self.visible_frame_generation.clone();
        let dialog_result = self.dialog_result.clone();
        let close_animation_started = self.close_animation_started.clone();
        let webview = WebViewBuilder::new()
            .with_html(UPDATE_PAGE)
            .with_transparent(true)
            .with_ipc_handler(move |request| match request.body().as_str() {
                "ready" => webview_ready.store(true, Ordering::Release),
                "visible" => {
                    visible_frame_generation.fetch_add(1, Ordering::AcqRel);
                }
                "drag" => unsafe {
                    ReleaseCapture();
                    SendMessageW(hwnd as _, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
                },
                "close" => unsafe {
                    PostMessageW(hwnd as _, WM_CLOSE, 0, 0);
                },
                "begin-close" => {
                    if !close_animation_started.swap(true, Ordering::AcqRel) {
                        MainWindow::animate_window_visual_async(hwnd as _, false, 15, 14);
                    }
                }
                "dialog:yes" => {
                    let _ = dialog_result.try_send(true);
                }
                "dialog:no" => {
                    let _ = dialog_result.try_send(false);
                }
                _ => {}
            })
            .build(&parent);

        match webview {
            Ok(webview) => {
                *self.webview.borrow_mut() = Some(webview);
                self.resize_webview_to_client();
                let status = self.status.borrow();
                let detail = self.detail.borrow();
                self.update_webview(
                    Self::phase_for_status(&status),
                    &status,
                    &detail,
                    *self.progress_value.borrow(),
                );
                drop(detail);
                drop(status);
                self.render_pending_changelog();
            }
            Err(error) => {
                // Keep the native layout visible as a compatibility fallback.
                self.webview_ready.store(true, Ordering::Release);
                self.visible_frame_generation.fetch_add(2, Ordering::AcqRel);
                if let Ok(mut slot) = self.webview_error.lock() {
                    *slot = Some(error.to_string());
                }
                nwg::simple_message(
                    "自动更新器",
                    "更新界面组件未加载，更新将继续在兼容模式下执行。",
                );
                if let Some(changelog) = self.pending_changelog.borrow_mut().take() {
                    nwg::simple_message(
                        &changelog.title,
                        &format!("{}\n\n{}", changelog.summary, changelog.markdown),
                    );
                    let _ = self.dialog_result.blocking_send(true);
                }
            }
        }
    }

    fn apply_rounded_corners(hwnd: winapi::shared::windef::HWND) {
        const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
        const DWMWCP_ROUND: u32 = 2;
        let preference = DWMWCP_ROUND;

        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const u32 as *const _,
                std::mem::size_of_val(&preference) as u32,
            );
        }
    }

    fn render_pending_changelog(&self) {
        let webview_slot = self.webview.borrow();
        let Some(webview) = webview_slot.as_ref() else {
            return;
        };
        let pending = self.pending_changelog.borrow();
        let Some(changelog) = pending.as_ref() else {
            return;
        };
        let payload = serde_json::json!({
            "title": &changelog.title,
            "summary": &changelog.summary,
            "html": &changelog.html,
        });
        let _ = webview.evaluate_script(&format!("window.showChangelog({payload});"));
    }

    fn render_dialog(&self, dialog: &DialogContent) -> bool {
        let webview_slot = self.webview.borrow();
        let Some(webview) = webview_slot.as_ref() else {
            return false;
        };
        let payload = serde_json::json!({
            "title": &dialog.title,
            "content": &dialog.content,
            "retryable": dialog.retryable,
        });
        webview
            .evaluate_script(&format!("window.showDialog({payload});"))
            .is_ok()
    }

    fn resize_webview_to_client(&self) {
        let webview_slot = self.webview.borrow();
        let Some(webview) = webview_slot.as_ref() else {
            return;
        };
        let hwnd = self.window.handle.hwnd().expect("main window must be HWND") as _;
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
            return;
        }
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let _ = webview.set_bounds(Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(width, height).into(),
        });
        let _ = webview.set_visible(true);
    }

    fn update_webview(&self, stage: &str, status: &str, detail: &str, progress: u32) {
        let webview_slot = self.webview.borrow();
        let Some(webview) = webview_slot.as_ref() else {
            return;
        };

        let payload = serde_json::json!({
            "stage": self.profile.borrow().stage_label(Self::stage_key(stage)),
            "status": status,
            "detail": detail,
            "progress": progress,
            "traffic": Self::format_transfer(
                *self.transfer.borrow(),
                *self.transfer_speed.borrow(),
            ),
        });
        let _ = webview.evaluate_script(&format!("window.updateUi({payload});"));

        let profile = self.profile.borrow();
        let profile_payload = serde_json::json!({
            "headline": &profile.headline,
            "subtitle": &profile.subtitle,
            "footer": &profile.footer,
            "launchLabelOffsetX": profile.launch_label_offset_x,
            "iconDataUrl": &profile.icon_data_url,
            "theme": &profile.theme,
        });
        let _ = webview.evaluate_script(&format!("window.updateProfile({profile_payload});"));
    }

    fn phase_for_status(status: &str) -> &'static str {
        if status.contains("下载") {
            "正在下载更新"
        } else if status.contains("移动")
            || status.contains("处理")
            || status.contains("清理")
            || status.contains("收尾")
        {
            "正在应用更新"
        } else if status.contains("没有更新") {
            "客户端已是最新版本"
        } else if status.contains("检查") || status.contains("收集") || status.contains("元数据")
        {
            "正在检查更新"
        } else {
            "正在准备更新"
        }
    }

    fn stage_key(stage: &str) -> &'static str {
        if stage.contains("下载") {
            "downloading"
        } else if stage.contains("应用") {
            "applying"
        } else if stage.contains("已是最新") {
            "completed"
        } else if stage.contains("检查") {
            "checking"
        } else {
            "prepare"
        }
    }

    fn sample_transfer_speed(&self, downloaded: u64) {
        let now = Instant::now();
        let mut sample = self.transfer_sample.borrow_mut();
        let elapsed = now.duration_since(sample.1);
        if downloaded < sample.0 || downloaded == 0 {
            *self.transfer_speed.borrow_mut() = 0;
            *sample = (downloaded, now);
        } else if elapsed >= Duration::from_millis(100) {
            let bytes = downloaded - sample.0;
            *self.transfer_speed.borrow_mut() = (bytes as f64 / elapsed.as_secs_f64()) as u64;
            *sample = (downloaded, now);
        }
    }

    fn format_transfer((downloaded, total): (u64, u64), speed: u64) -> String {
        if total == 0 {
            "无需下载".to_owned()
        } else if downloaded >= total {
            format!("{}", crate::utility::convert_bytes(downloaded))
        } else {
            format!(
                "{} / {} · {}/s",
                crate::utility::convert_bytes(downloaded),
                crate::utility::convert_bytes(total),
                crate::utility::convert_bytes(speed),
            )
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
    webview_error: Arc<std::sync::Mutex<Option<String>>>,
    webview_ready: Arc<AtomicBool>,
    visible_frame_generation: Arc<AtomicUsize>,
    window_handle: isize,
}

impl MainUiCommand {
    pub fn webview_error(&self) -> Option<String> {
        self.webview_error.lock().ok()?.clone()
    }

    pub async fn exit(&self) {
        let this = self.inner.lock().await;

        this.sender.send(Command::Exit).await.unwrap();
        this.notice_sender.notice();
        unsafe {
            PostMessageW(self.window_handle as _, WM_CLOSE, 0, 0);
        }
    }

    pub async fn set_visible(&self, visible: bool) {
        if !visible {
            let this = self.inner.lock().await;
            this.sender.send(Command::SetVisible(false)).await.unwrap();
            this.notice_sender.notice();
            return;
        }

        for _ in 0..150 {
            if self.webview_ready.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let prepared_generation = self.visible_frame_generation.load(Ordering::Acquire);
        {
            let this = self.inner.lock().await;
            this.sender.send(Command::PrepareVisible).await.unwrap();
            this.notice_sender.notice();
        }
        for _ in 0..150 {
            if self.visible_frame_generation.load(Ordering::Acquire) > prepared_generation {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let revealed_generation = self.visible_frame_generation.load(Ordering::Acquire);
        {
            let this = self.inner.lock().await;
            this.sender.send(Command::RevealVisible).await.unwrap();
            this.notice_sender.notice();
        }
        for _ in 0..150 {
            if self.visible_frame_generation.load(Ordering::Acquire) > revealed_generation {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    pub async fn set_title(&self, title: String) {
        let this = self.inner.lock().await;

        this.sender.send(Command::SetTitle(title)).await.unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_profile(&self, profile: UiProfile) {
        let this = self.inner.lock().await;

        this.sender
            .send(Command::SetProfile(profile))
            .await
            .unwrap();
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

    pub async fn set_transfer(&self, downloaded: u64, total: u64) {
        let this = self.inner.lock().await;

        this.sender
            .send(Command::SetTransfer { downloaded, total })
            .await
            .unwrap();
        this.notice_sender.notice();
    }

    pub async fn set_label_secondary(&self, text: String) {
        let this = self.inner.lock().await;

        this.sender
            .send(Command::SetLabelSecondary(text))
            .await
            .unwrap();
        this.notice_sender.notice();
    }

    pub async fn popup_dialog(&self, dialog: DialogContent) -> bool {
        let retryable = dialog.retryable;
        {
            let this = self.inner.lock().await;
            this.sender
                .send(Command::PupopDialog(dialog))
                .await
                .unwrap();
            this.notice_sender.notice();
        }
        self.set_visible(true).await;
        let mut this = self.inner.lock().await;
        let accepted = this.receiver.recv().await.unwrap();
        if accepted && retryable {
            this.sender.send(Command::HideDialog).await.unwrap();
            this.notice_sender.notice();
        }
        accepted
    }

    pub async fn show_changelog(&self, title: String, summary: String, content: String) {
        let mut this = self.inner.lock().await;

        this.sender
            .send(Command::ShowChangelog {
                title,
                summary,
                content,
            })
            .await
            .unwrap();
        this.notice_sender.notice();
        let _ = this.receiver.recv().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{render_markdown, MainWindow, UPDATE_PAGE};

    #[test]
    fn renders_common_changelog_markdown() {
        let html = render_markdown("## v1.2.3\n\n- **修复**下载\n- `安全`回退");

        assert!(html.contains("<h2>v1.2.3</h2>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<strong>修复</strong>"));
        assert!(html.contains("<code>安全</code>"));
    }

    #[test]
    fn sanitizes_untrusted_changelog_html_and_links() {
        let html = render_markdown(
            "正常文本<script>alert('x')</script> [危险链接](javascript:alert('x'))",
        );

        assert!(!html.contains("<script"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("alert('x')</script>"));
        assert!(html.contains("正常文本"));
    }

    #[test]
    fn formats_download_traffic_for_footer() {
        assert_eq!(MainWindow::format_transfer((0, 0), 0), "无需下载");
        assert_eq!(
            MainWindow::format_transfer((1024, 2048), 512),
            "1.0 KB / 2.0 KB · 512 B/s"
        );
        assert_eq!(MainWindow::format_transfer((2048, 2048), 512), "2.0 KB");
    }

    #[test]
    fn update_failures_render_inside_the_main_webview() {
        assert!(UPDATE_PAGE.contains("id=\"dialogCard\""));
        assert!(UPDATE_PAGE.contains("window.showDialog"));
        assert!(UPDATE_PAGE.contains("dialog:yes"));
        assert!(UPDATE_PAGE.contains("关闭更新器"));
    }

    #[test]
    fn entry_animation_grows_rotates_and_fades_in() {
        assert!(UPDATE_PAGE.contains("scale(.4) rotate(30deg)"));
        assert!(UPDATE_PAGE.contains("scale(1) rotate(0deg)"));
        assert!(UPDATE_PAGE.contains("window.playEnterAnimation"));
    }
}
