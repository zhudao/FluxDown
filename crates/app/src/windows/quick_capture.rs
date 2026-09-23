//! 快速捕获窗口：外部捕获（浏览器扩展 / NMH）待确认队列的单例弹窗；置顶、无任务栏、
//! 不抢焦点，多条捕获合并展示。

use std::rc::Rc;
use std::sync::Arc;

use fluxdown_protocol::{AgentEvent, ServiceEvent};
use fluxdown_ui_downloads::{QuickCaptureEvent, QuickCaptureView, bind_quick_capture_keys};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, WindowBounds, WindowDecorations,
    WindowKind, WindowOptions, px,
};
use gpui_component::Root;

use crate::{
    app::Desktop,
    downloads_port::AgentDownloadsPort,
    session::{AgentSession, SessionSignal, agent_body, attach},
    windows::{WindowKey, WindowRegistry},
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

// 首帧尺寸可能被平台调整；只能原地 resize，不能从 RowsChanged 回调重建视图。
// 重建会重置 last_rows，并在新窗口首次绘制时再次触发同一回调。
fn apply_rows(handle: AnyWindowHandle, rows: usize, cx: &mut App) {
    let wanted = window_size(rows);
    let _ = handle.update(cx, |_, window, _| {
        if window.viewport_size() != wanted {
            window.resize(wanted);
        }
    });
}

/// 已开则跳过；否则在主窗口所在显示器居中开一个不抢焦点的置顶弹窗，高度按行数自适应。
fn open(cx: &mut App, rows: usize) {
    if WindowRegistry::is_open(cx, &WindowKey::QuickCapture) {
        return;
    }
    open_window(cx, rows);
}

fn open_window(cx: &mut App, rows: usize) {
    let desktop = Desktop::global(cx);
    let translator = desktop.translator.clone();
    let client = desktop.client.clone();
    let session = desktop.session.clone();

    let display_id = WindowRegistry::main_display_id(cx);
    let options = WindowOptions {
        display_id,
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            display_id,
            window_size(rows),
            cx,
        ))),
        titlebar: None,
        window_decorations: Some(WindowDecorations::Client),
        focus: false,
        is_resizable: false,
        is_minimizable: false,
        kind: WindowKind::PopUp,
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };

    let build = move |window: &mut gpui::Window, cx: &mut App| {
        let port = Arc::new(AgentDownloadsPort::new(client.clone()));
        let more_options: fluxdown_ui_downloads::MoreOptionsOpener =
            Rc::new(|context, _window, cx| {
                crate::windows::new_download::open(cx, context);
            });
        let content =
            cx.new(|cx| QuickCaptureView::new(translator.clone(), port, more_options, window, cx));
        attach(&session, &content, cx);
        let handle = window.window_handle();
        cx.subscribe(&content, move |_, event, cx| match event {
            QuickCaptureEvent::Empty => {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            QuickCaptureEvent::RowsChanged(rows) => apply_rows(handle, *rows, cx),
        })
        .detach();
        // 原生窗口负责外轮廓与阴影；完整铺底，避免透明内容参与动态阴影轮廓。
        cx.new(|cx| Root::new(content, window, cx))
    };
    WindowRegistry::open_or_focus(cx, WindowKey::QuickCapture, options, build);
}
