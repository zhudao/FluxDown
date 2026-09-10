//! 主窗口：shell + 下载页 + RSS 页；关闭策略与边界持久化。

use std::{rc::Rc, sync::Arc};

use fluxdown_ui_downloads::{DOWNLOAD_ICON_PATH, DownloadHostActions, DownloadView};
use fluxdown_ui_i18n::keys;
use fluxdown_ui_rss::RssView;
use fluxdown_ui_shell::{RouteId, ShellAction, ShellRoute, ShellView, main_window_options};
use gpui::{App, AppContext as _, Window, WindowHandle, px, size};
use gpui_component::{Icon, IconName, Root, WindowExt as _};

use crate::{
    app::Desktop,
    capability_ports::AgentRssPort,
    downloads_port::AgentDownloadsPort,
    session::attach,
    windows::{WindowKey, WindowRegistry},
};

const MAIN_WINDOW_SIZE: gpui::Size<gpui::Pixels> = size(px(1120.), px(760.));

/// 打开或聚焦主窗口。返回新建窗口句柄（已开时 `None`）。
pub fn open(cx: &mut App) -> Option<WindowHandle<Root>> {
    let desktop = Desktop::global(cx);
    let translator = desktop.translator.clone();
    let session = desktop.session.clone();
    let client = desktop.client.clone();
    let settings_store = desktop.settings_store.clone();
    let menu_bar = desktop.menu_bar.clone();
    let stored = Desktop::pref(cx, "desktop.window.main");
    let mut options = main_window_options();
    options.window_bounds = Some(WindowRegistry::restore_bounds(
        &WindowKey::Main,
        stored.as_ref(),
        MAIN_WINDOW_SIZE,
        cx,
    ));

    WindowRegistry::open_or_focus(cx, WindowKey::Main, options, move |window, cx| {
        let downloads_port = Arc::new(AgentDownloadsPort::new(client.clone()));
        let downloads =
            cx.new(|cx| DownloadView::new(translator.clone(), downloads_port, window, cx));
        let rss_port = Arc::new(AgentRssPort::new(client.clone()));
        let rss = cx.new(|cx| RssView::new(translator.clone(), rss_port, window, cx));

        let routes = vec![
            ShellRoute::new(
                RouteId::new("downloads"),
                "activity-downloads",
                "activity-downloads-tooltip",
                keys::MOBILE_NAV_DOWNLOADS,
                Icon::empty().path(DOWNLOAD_ICON_PATH),
                downloads.clone().into(),
            ),
            ShellRoute::new(
                RouteId::new("rss"),
                "activity-rss",
                "activity-rss-tooltip",
                "rssAddSource",
                Icon::new(IconName::Globe),
                rss.clone().into(),
            ),
        ];
        let actions = vec![ShellAction::new(
            "activity-settings",
            "activity-settings-tooltip",
            keys::SETTINGS,
            Icon::new(IconName::Settings),
            move |_, cx| crate::windows::settings::open(cx),
        )];
        let shell =
            cx.new(|cx| ShellView::new(translator.clone(), routes, actions, Some(menu_bar), cx));

        let shell_for_nav = shell.downgrade();
        let settings_for_categories = settings_store.clone();
        let translator_for_categories = translator.clone();
        let shutdown_status = crate::power::status(cx);
        let shutdown_port = crate::power::shutdown_port(cx);
        downloads.update(cx, |downloads, _| {
            downloads.set_host_actions(DownloadHostActions {
                open_new_download: Some(Rc::new(|context, _, cx| {
                    crate::windows::new_download::open(cx, context);
                })),
                open_task_window: Some(Rc::new(|task_id, _, cx| {
                    crate::windows::task_detail::open(cx, task_id);
                })),
                open_group_window: Some(Rc::new(|group_id, _, cx| {
                    crate::windows::group_detail::open(cx, group_id);
                })),
                open_queue_manager: Some(Rc::new(|_, cx| {
                    crate::windows::queue_manager::open(cx);
                })),
                navigate_rss: Some(Rc::new(move |_, cx| {
                    let _ = shell_for_nav.update(cx, |shell, cx| {
                        shell.navigate(RouteId::new("rss"), cx);
                    });
                })),
                open_category_editor: Some(Rc::new(move |id, window, cx| {
                    let translator = translator_for_categories.read(cx).clone();
                    fluxdown_ui_settings::open_category_editor(
                        settings_for_categories.clone(),
                        &translator,
                        id,
                        window,
                        cx,
                    );
                })),
                shutdown_status,
                shutdown: shutdown_port,
            });
        });

        attach(&session, &downloads, cx);
        attach(&session, &rss, cx);
        {
            let desktop = Desktop::global_mut(cx);
            desktop.main_downloads = Some(downloads.downgrade());
            desktop.main_shell = Some(shell.downgrade());
        }

        let root = cx.new(|cx| Root::new(shell, window, cx));
        WindowRegistry::persist_bounds(&WindowKey::Main, client, &root, window, cx);
        install_close_policy(window, cx);
        root
    })
}

/// 关闭策略：托盘驻留 → 直接关；有活跃任务 → 提示后关；否则直接关。
fn install_close_policy(window: &mut Window, cx: &mut App) {
    window.on_window_should_close(cx, |window, cx| {
        let close_to_tray = Desktop::pref_bool(cx, "close_to_tray", true);
        if close_to_tray && WindowRegistry::is_resident(cx) {
            return true;
        }
        if Desktop::active_task_count(cx) == 0 || WindowRegistry::open_count(cx) > 1 {
            return true;
        }
        let translator = Desktop::global(cx).translator.read(cx).clone();
        let title = translator.text("closeWithActiveTasksTitle").to_owned();
        let hint = translator.text("closeWithActiveTasksHint").to_owned();
        let ok = translator.text("menuQuit").to_owned();
        let cancel = translator.text("cancel").to_owned();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .description(hint.clone())
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .ok_text(ok.clone())
                        .cancel_text(cancel.clone()),
                )
                .on_ok(|_, window, _| {
                    window.remove_window();
                    true
                })
        });
        false
    });
}
