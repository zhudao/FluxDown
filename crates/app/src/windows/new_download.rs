//! 「新建下载」窗口：提交经主窗口下载页建任务；成功后关窗并在主窗口 toast。

use std::rc::Rc;

use fluxdown_ui_downloads::{NewDownloadContext, NewDownloadView};
use fluxdown_ui_i18n::keys;
use fluxdown_ui_shell::{AuxiliaryWindowView, auxiliary_window_options};
use gpui::{App, AppContext as _, Bounds, WindowBounds, px, size};
use gpui_component::{Root, WindowExt as _};

use crate::{
    app::Desktop,
    windows::{WindowKey, WindowRegistry},
};

const NEW_DOWNLOAD_WINDOW_SIZE: gpui::Size<gpui::Pixels> = size(px(640.), px(530.));
const NEW_DOWNLOAD_WINDOW_MIN_SIZE: gpui::Size<gpui::Pixels> = size(px(560.), px(440.));

/// 菜单 / 快捷键入口：上下文取自主窗口下载页；主窗口不存在时先重建主窗口。
pub fn open_default(cx: &mut App) {
    let downloads = main_downloads(cx);
    let Some(downloads) = downloads else {
        return;
    };
    let context = downloads.read(cx).new_download_context();
    open(cx, context);
}

fn main_downloads(cx: &mut App) -> Option<gpui::Entity<fluxdown_ui_downloads::DownloadView>> {
    if let Some(downloads) = Desktop::global(cx)
        .main_downloads
        .as_ref()
        .and_then(|weak| weak.upgrade())
    {
        return Some(downloads);
    }
    crate::windows::main::open(cx);
    Desktop::global(cx)
        .main_downloads
        .as_ref()
        .and_then(|weak| weak.upgrade())
}

pub fn open(cx: &mut App, context: NewDownloadContext) {
    let desktop = Desktop::global(cx);
    let translator = desktop.translator.clone();
    let title = translator.read(cx).text(keys::NEW_DOWNLOAD).to_owned();
    let display_id = WindowRegistry::handle(cx, &WindowKey::Main)
        .and_then(|handle| {
            handle
                .update(cx, |_, window, cx| window.display(cx))
                .ok()
                .flatten()
        })
        .map(|display| display.id());
    let bounds = Bounds::centered(display_id, NEW_DOWNLOAD_WINDOW_SIZE, cx);
    let mut options = auxiliary_window_options(title);
    options.display_id = display_id;
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    options.window_min_size = Some(NEW_DOWNLOAD_WINDOW_MIN_SIZE);
    options.is_resizable = true;

    WindowRegistry::open_or_focus(cx, WindowKey::NewDownload, options, move |window, cx| {
        let on_submit = Rc::new(move |submission, window: &mut gpui::Window, cx: &mut App| {
            let Some(downloads) = Desktop::global(cx)
                .main_downloads
                .as_ref()
                .and_then(|weak| weak.upgrade())
            else {
                return;
            };
            let task = downloads.update(cx, |downloads, cx| {
                downloads.create_download(submission, cx)
            });
            let translator = Desktop::global(cx).translator.clone();
            window
                .spawn(cx, async move |cx| {
                    let ok = task.await;
                    let _ = cx.update(|window, cx| {
                        let translator = translator.read(cx);
                        if ok {
                            let message = translator.text("taskCreatedToast").to_owned();
                            if let Some(main) = WindowRegistry::handle(cx, &WindowKey::Main) {
                                let _ = main.update(cx, |_, window, cx| {
                                    window.push_notification(message, cx);
                                });
                            }
                            window.remove_window();
                        } else {
                            let message = translator.text("localServiceActionFailed").to_owned();
                            window.push_notification(
                                gpui_component::notification::Notification::error(message),
                                cx,
                            );
                        }
                    });
                })
                .detach();
        });
        let form = cx.new(|cx| {
            NewDownloadView::new(translator.clone(), context.clone(), on_submit, window, cx)
        });
        let window_view =
            cx.new(|cx| AuxiliaryWindowView::new(translator, keys::NEW_DOWNLOAD, form.into(), cx));
        cx.new(|cx| Root::new(window_view, window, cx))
    });
}
