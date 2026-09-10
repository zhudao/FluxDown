//! 快速捕获窗口：外部捕获（浏览器扩展 / NMH）待确认队列的单例弹窗；置顶、无任务栏、
//! 不抢焦点，多条捕获合并展示。

use std::rc::Rc;
use std::sync::Arc;

use fluxdown_protocol::{AgentEvent, ServiceEvent};
use fluxdown_ui_downloads::{QuickCaptureEvent, QuickCaptureView, bind_quick_capture_keys};
use gpui::{
    App, AppContext as _, Entity, Styled as _, WindowDecorations, WindowKind, WindowOptions, point,
    px,
};
use gpui_component::Root;

use crate::{
    app::Desktop,
    downloads_port::AgentDownloadsPort,
    session::{AgentSession, SessionSignal, agent_body, attach},
    windows::{WindowKey, WindowRegistry, bottom_right_bounds},
};

/// 订阅会话：捕获队列非空 → 打开/聚焦弹窗；清空 → 关闭；启动时回放快照。
pub fn install(cx: &mut App) {
    bind_quick_capture_keys(cx);
    let session = Desktop::global(cx).session.clone();
    if let Some(count) = pending_capture_count(&session, cx) {
        open(cx, count);
    }
    cx.subscribe(&session, |_, signal, cx| match signal {
        SessionSignal::Snapshot(snapshot) => {
            if let Some(body) = agent_body(snapshot) {
                if body.pending_captures.is_empty() {
                    WindowRegistry::close(cx, &WindowKey::QuickCapture);
                } else {
                    open(cx, body.pending_captures.len());
                }
            }
        }
        SessionSignal::Event(frame) => {
            if let ServiceEvent::Agent(AgentEvent::PendingCapturesChanged(captures)) = &frame.event
            {
                if captures.is_empty() {
                    WindowRegistry::close(cx, &WindowKey::QuickCapture);
                } else {
                    open(cx, captures.len());
                }
            }
        }
        SessionSignal::Stale | SessionSignal::Fatal(_) => {}
    })
    .detach();
}

fn pending_capture_count(session: &Entity<AgentSession>, cx: &App) -> Option<usize> {
    session
        .read(cx)
        .agent_snapshot()
        .map(|body| body.pending_captures.len())
        .filter(|count| *count > 0)
}

fn window_size(rows: usize) -> gpui::Size<gpui::Pixels> {
    gpui::size(
        px(fluxdown_ui_downloads::QUICK_CAPTURE_WINDOW_WIDTH),
        px(QuickCaptureView::preferred_height_for(rows)),
    )
}

/// 行数变化 → 按新高度重开窗口（gpui 只有 `resize` 没有移动窗口的 API，原地 resize
/// 会以左上角为锚向下长出屏幕；重开可保持右下角贴边）。
fn apply_rows(rows: usize, cx: &mut App) {
    let Some(handle) = WindowRegistry::handle(cx, &WindowKey::QuickCapture) else {
        return;
    };
    let wanted = QuickCaptureView::preferred_height_for(rows);
    let current = handle
        .update(cx, |_, window, _| f32::from(window.viewport_size().height))
        .unwrap_or(wanted);
    if (current - wanted).abs() < 0.5 {
        return;
    }
    open_window(cx, rows, true);
}

/// 已开则跳过；否则贴屏幕右下角开一个不抢焦点的置顶弹窗，高度按行数自适应。
fn open(cx: &mut App, rows: usize) {
    if WindowRegistry::is_open(cx, &WindowKey::QuickCapture) {
        return;
    }
    open_window(cx, rows, false);
}

fn open_window(cx: &mut App, rows: usize, replace: bool) {
    let desktop = Desktop::global(cx);
    let translator = desktop.translator.clone();
    let client = desktop.client.clone();
    let session = desktop.session.clone();

    let mut options = WindowOptions {
        titlebar: None,
        window_decorations: Some(WindowDecorations::Client),
        focus: false,
        is_resizable: false,
        is_minimizable: false,
        kind: WindowKind::PopUp,
        window_background: gpui::WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    options.window_bounds = Some(gpui::WindowBounds::Windowed(bottom_right_bounds(
        window_size(rows),
        point(px(16.), px(48.)),
        cx,
    )));

    let build = move |window: &mut gpui::Window, cx: &mut App| {
        let port = Arc::new(AgentDownloadsPort::new(client.clone()));
        let more_options: fluxdown_ui_downloads::MoreOptionsOpener =
            Rc::new(|context, _window, cx| {
                crate::windows::new_download::open(cx, context);
            });
        let content =
            cx.new(|cx| QuickCaptureView::new(translator.clone(), port, more_options, window, cx));
        attach(&session, &content, cx);
        cx.subscribe(&content, |_, event, cx| match event {
            QuickCaptureEvent::Empty => WindowRegistry::close(cx, &WindowKey::QuickCapture),
            QuickCaptureEvent::RowsChanged(rows) => apply_rows(*rows, cx),
        })
        .detach();
        // 窗口背景透明只为圆角；Root 不铺底色，卡片自己画背景。
        cx.new(|cx| Root::new(content, window, cx).bg(gpui::transparent_black()))
    };
    if replace {
        WindowRegistry::reopen(cx, WindowKey::QuickCapture, options, build);
    } else {
        WindowRegistry::open_or_focus(cx, WindowKey::QuickCapture, options, build);
    }
}
