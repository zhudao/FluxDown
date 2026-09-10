//! 独立任务窗口：AB Download Manager 式逐任务进度窗口。
//!
//! 标题栏用平台原生装饰（`WindowOptions::default()`），标题随任务名解析 /
//! 变化实时刷新（`window.set_window_title`）。置顶开关以 `WindowKind::Floating`
//! 重建窗口（gpui 不支持窗口创建后修改 kind），置顶状态保存在进程内存中
//! （跨重启不持久化，符合「AB Download 式」轻量窗口定位）。

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc, time::Duration};

use fluxdown_ui_downloads::{DownloadHostActions, TaskDetailView};
use gpui::{
    App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowKind, WindowOptions, px,
    size,
};
use gpui_component::Root;

use crate::{
    app::Desktop,
    downloads_port::AgentDownloadsPort,
    session::attach,
    windows::{WindowKey, WindowRegistry},
};

const TASK_WINDOW_SIZE: gpui::Size<gpui::Pixels> = size(px(560.), px(420.));
const TASK_WINDOW_MIN_SIZE: gpui::Size<gpui::Pixels> = size(px(420.), px(220.));

thread_local! {
    /// 当前进程内已置顶的任务窗口（macOS `WindowKind::Floating`）。不持久化。
    static PINNED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

fn is_pinned(task_id: &str) -> bool {
    PINNED.with(|pinned| pinned.borrow().contains(task_id))
}

fn set_pinned(task_id: &str, pinned: bool) {
    PINNED.with(|set| {
        let mut set = set.borrow_mut();
        if pinned {
            set.insert(task_id.to_owned());
        } else {
            set.remove(task_id);
        }
    });
}

/// 打开或聚焦任务窗口，沿用之前记录的置顶状态。
pub fn open(cx: &mut App, task_id: String) {
    let kind = if is_pinned(&task_id) {
        WindowKind::Floating
    } else {
        WindowKind::Normal
    };
    open_window(cx, task_id, kind);
}

/// 置顶开关的核心逻辑：以新 `WindowKind` 重建窗口（gpui 不支持窗口创建后修改
/// kind）。已开的窗口先关闭，等注册表确认清理后再用新 kind 重新打开。
pub fn open_with_kind(cx: &mut App, task_id: String, pinned: bool) {
    let kind = if pinned {
        WindowKind::Floating
    } else {
        WindowKind::Normal
    };
    set_pinned(&task_id, pinned);
    let key = WindowKey::TaskDetail(task_id.clone());
    if !WindowRegistry::is_open(cx, &key) {
        open_window(cx, task_id, kind);
        return;
    }
    WindowRegistry::close(cx, &key);
    let retry_task_id = task_id;
    cx.spawn(async move |cx| {
        for _ in 0..50u32 {
            let still_open = cx.update(|cx| {
                WindowRegistry::is_open(cx, &WindowKey::TaskDetail(retry_task_id.clone()))
            });
            if !still_open {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(20))
                .await;
        }
        cx.update(|cx| open_window(cx, retry_task_id, kind));
    })
    .detach();
}

fn open_window(cx: &mut App, task_id: String, kind: WindowKind) {
    let desktop = Desktop::global(cx);
    let translator = desktop.translator.clone();
    let session = desktop.session.clone();
    let client = desktop.client.clone();
    let placeholder_title = translator.read(cx).text("detail").to_owned();

    let mut options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(placeholder_title.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    options.kind = kind.clone();
    options.window_min_size = Some(TASK_WINDOW_MIN_SIZE);
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(
        None,
        TASK_WINDOW_SIZE,
        cx,
    )));

    let key = WindowKey::TaskDetail(task_id.clone());
    WindowRegistry::open_or_focus(cx, key, options, move |window, cx| {
        let downloads_port = Arc::new(AgentDownloadsPort::new(client.clone()));
        let pinned = kind == WindowKind::Floating;
        let host = DownloadHostActions {
            open_group_window: Some(Rc::new(|group_id, _window, cx| {
                crate::windows::group_detail::open(cx, group_id);
            })),
            ..DownloadHostActions::default()
        };
        let pin_toggle: fluxdown_ui_downloads::PinToggle = Rc::new(
            |task_id, pinned, _window: &mut gpui::Window, cx: &mut App| {
                open_with_kind(cx, task_id, pinned);
            },
        );
        let detail = cx.new(|cx| {
            TaskDetailView::new(
                translator.clone(),
                task_id.clone(),
                downloads_port,
                host,
                pinned,
                Some(pin_toggle),
                window,
                cx,
            )
        });
        attach(&session, &detail, cx);

        cx.new(|cx| {
            let root = Root::new(detail.clone(), window, cx);
            cx.observe_in(&detail, window, |_, detail, window, cx| {
                let title = detail
                    .read(cx)
                    .file_name()
                    .unwrap_or_else(|| detail.read(cx).task_id().to_owned());
                window.set_window_title(&title);
            })
            .detach();
            cx.subscribe_in(&detail, window, |_, _, event, window, _cx| {
                if matches!(event, fluxdown_ui_downloads::TaskDetailEvent::Closed) {
                    window.remove_window();
                }
            })
            .detach();
            root
        })
    });
}
