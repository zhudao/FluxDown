//! 应用菜单（macOS 原生 / Windows·Linux 标题栏菜单栏）、全局键位与应用级动作处理。

use fluxdown_ui_downloads::actions as dl;
use fluxdown_ui_i18n::Translator;
use gpui::{App, Entity, KeyBinding, Menu, MenuItem, SharedString};
use gpui_component::{GlobalState, WindowExt as _, menu::AppMenuBar};

use crate::{
    actions::{
        About, CheckUpdate, OpenLogsFolder, OpenSettings, OpenWebsite, Quit, ShowMainWindow,
    },
    app::Desktop,
    windows::{WindowKey, WindowRegistry},
};

const WEBSITE_URL: &str = "https://fluxdown.zerx.dev";

fn primary() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

/// 注册全局键位。字母键只在下载页上下文生效（页面内再按聚焦输入框二次拦截）。
pub fn bind_keys(cx: &mut App) {
    let p = primary();
    let dl_ctx = Some(dl::KEY_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new(&format!("{p}-n"), dl::NewDownload, None),
        KeyBinding::new(&format!("{p}-,"), OpenSettings, None),
        KeyBinding::new(&format!("{p}-f"), dl::FocusSearch, dl_ctx),
        KeyBinding::new(&format!("{p}-a"), dl::SelectAllTasks, dl_ctx),
        KeyBinding::new("delete", dl::DeleteSelected, dl_ctx),
        KeyBinding::new("backspace", dl::DeleteSelected, dl_ctx),
        KeyBinding::new(&format!("{p}-backspace"), dl::DeleteSelected, dl_ctx),
        KeyBinding::new("shift-delete", dl::DeleteSelectedWithFiles, dl_ctx),
        KeyBinding::new("space", dl::TogglePauseSelected, dl_ctx),
        KeyBinding::new("shift-d", dl::CycleDensity, dl_ctx),
        KeyBinding::new("g", dl::CycleGroupBy, dl_ctx),
        KeyBinding::new("s", dl::CycleSort, dl_ctx),
        KeyBinding::new(&format!("{p}-i"), dl::ToggleDetailPanel, dl_ctx),
        KeyBinding::new("escape", dl::ClearSelection, dl_ctx),
    ];
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-q", Quit, None));
    }
    cx.bind_keys(bindings);
}

fn t(translator: &Translator, key: &str) -> SharedString {
    SharedString::from(translator.text(key).to_owned())
}

/// 菜单树（macOS 原生菜单与标题栏菜单栏共用一份）。
pub fn build_menus(translator: &Translator) -> Vec<Menu> {
    let mut menus = Vec::with_capacity(6);
    if cfg!(target_os = "macos") {
        menus.push(Menu::new("FluxDown").items([
            MenuItem::action(t(translator, "menuAbout"), About),
            MenuItem::separator(),
            MenuItem::action(t(translator, "menuSettings"), OpenSettings),
            MenuItem::separator(),
            MenuItem::action(t(translator, "menuQuit"), Quit),
        ]));
    }
    let mut file = vec![
        MenuItem::action(t(translator, "menuNewDownload"), dl::NewDownload),
        MenuItem::action(t(translator, "openTorrentFile"), dl::OpenTorrentFile),
    ];
    if !cfg!(target_os = "macos") {
        file.push(MenuItem::separator());
        file.push(MenuItem::action(t(translator, "menuQuit"), Quit));
    }
    menus.push(Menu::new(t(translator, "menuFile")).items(file));
    menus.push(Menu::new(t(translator, "menuTasks")).items([
        MenuItem::action(t(translator, "pause"), dl::PauseSelected),
        MenuItem::action(t(translator, "resume"), dl::ResumeSelected),
        MenuItem::action(t(translator, "deleteTask"), dl::DeleteSelected),
        MenuItem::action(
            t(translator, "deleteTaskAndFile"),
            dl::DeleteSelectedWithFiles,
        ),
        MenuItem::separator(),
        MenuItem::action(t(translator, "selectAll"), dl::SelectAllTasks),
        MenuItem::action(t(translator, "pauseAll"), dl::PauseAll),
        MenuItem::action(t(translator, "resumeAll"), dl::ResumeAll),
        MenuItem::action(t(translator, "menuClearFinished"), dl::ClearFinished),
        MenuItem::separator(),
        MenuItem::action(t(translator, "openFile"), dl::OpenSelected),
        MenuItem::action(t(translator, "openFolder"), dl::RevealSelected),
        MenuItem::action(t(translator, "copyUrl"), dl::CopySelectedUrl),
        MenuItem::action(t(translator, "renameTask"), dl::RenameSelected),
        MenuItem::action(t(translator, "redownloadTask"), dl::RedownloadSelected),
        MenuItem::action(t(translator, "boostDownload"), dl::ToggleBoostSelected),
        MenuItem::action(t(translator, "menuOpenInWindow"), dl::OpenSelectedInWindow),
    ]));
    menus.push(Menu::new(t(translator, "menuView")).items([
        MenuItem::action(t(translator, "menuCycleDensity"), dl::CycleDensity),
        MenuItem::action(t(translator, "menuCycleGroupBy"), dl::CycleGroupBy),
        MenuItem::action(t(translator, "menuCycleSort"), dl::CycleSort),
        MenuItem::separator(),
        MenuItem::action(t(translator, "menuDetailPanel"), dl::ToggleDetailPanel),
    ]));
    let mut tools = Vec::with_capacity(4);
    if !cfg!(target_os = "macos") {
        tools.push(MenuItem::action(
            t(translator, "menuSettings"),
            OpenSettings,
        ));
    }
    tools.extend([
        MenuItem::action(t(translator, "manageQueueAction"), dl::OpenQueueManager),
        MenuItem::action(t(translator, "menuCheckForUpdates"), CheckUpdate),
        MenuItem::action(t(translator, "menuOpenLogsFolder"), OpenLogsFolder),
    ]);
    menus.push(Menu::new(t(translator, "menuTools")).items(tools));
    let mut help = vec![MenuItem::action(t(translator, "menuWebsite"), OpenWebsite)];
    if !cfg!(target_os = "macos") {
        help.push(MenuItem::action(t(translator, "menuAbout"), About));
    }
    menus.push(Menu::new(t(translator, "menuHelp")).items(help));
    menus
}

/// 装配菜单：原生菜单 + 标题栏菜单栏；翻译变化时重建。
pub fn install(cx: &mut App, translator: &Entity<Translator>) -> Entity<AppMenuBar> {
    apply_menus(translator, cx);
    let menu_bar = AppMenuBar::new(cx);
    let observed_bar = menu_bar.clone();
    cx.observe(translator, move |translator, cx| {
        apply_menus(&translator, cx);
        observed_bar.update(cx, |bar, cx| bar.reload(cx));
    })
    .detach();
    menu_bar
}

fn apply_menus(translator: &Entity<Translator>, cx: &mut App) {
    let translator = translator.read(cx).clone();
    cx.set_menus(build_menus(&translator));
    GlobalState::global_mut(cx).set_app_menus(
        build_menus(&translator)
            .into_iter()
            .map(Menu::owned)
            .collect(),
    );
}

/// 应用级动作（不依赖具体窗口）。
pub fn install_global_actions(cx: &mut App) {
    cx.on_action(|_: &OpenSettings, cx| crate::windows::settings::open(cx));
    cx.on_action(|_: &ShowMainWindow, cx| {
        crate::windows::main::open(cx);
        cx.activate(true);
    });
    cx.on_action(|_: &Quit, cx| request_quit(cx));
    cx.on_action(|_: &OpenWebsite, cx| cx.open_url(WEBSITE_URL));
    cx.on_action(|_: &dl::NewDownload, cx| crate::windows::new_download::open_default(cx));
    cx.on_action(|_: &dl::OpenQueueManager, cx| crate::windows::queue_manager::open(cx));
    cx.on_action(|_: &CheckUpdate, cx| check_update(cx));
    cx.on_action(|_: &OpenLogsFolder, cx| open_logs_folder(cx));
    cx.on_action(|_: &About, cx| show_about(cx));
}

/// 退出：有活跃任务时先在当前窗口提示「下载将继续由后台服务执行」。
pub fn request_quit(cx: &mut App) {
    let active = Desktop::active_task_count(cx);
    let Some(window) = cx
        .active_window()
        .or_else(|| WindowRegistry::handle(cx, &WindowKey::Main))
        .filter(|_| active > 0)
    else {
        cx.quit();
        return;
    };
    let translator = Desktop::global(cx).translator.read(cx).clone();
    let title = t(&translator, "closeWithActiveTasksTitle");
    let hint = t(&translator, "closeWithActiveTasksHint");
    let ok = t(&translator, "menuQuit");
    let cancel = t(&translator, "cancel");
    let _ = window.update(cx, |_, window, cx| {
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .description(hint.clone())
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .ok_text(ok.clone())
                        .cancel_text(cancel.clone()),
                )
                .on_ok(|_, _, cx| {
                    cx.quit();
                    true
                })
        });
    });
}

fn check_update(cx: &mut App) {
    let desktop = Desktop::global(cx);
    let client = desktop.client.clone();
    let translator = desktop.translator.clone();
    let future = client.call::<serde_json::Value, fluxdown_protocol::UpdateCheckResultDto>(
        fluxdown_protocol::method::AGENT_UPDATE_CHECK,
        Some(serde_json::json!({})),
    );
    cx.spawn(async move |cx| {
        let result = future.await;
        cx.update(|cx| {
            let translator = translator.read(cx).clone();
            let Some(window) = cx
                .active_window()
                .or_else(|| WindowRegistry::handle(cx, &WindowKey::Main))
            else {
                return;
            };
            let _ = window.update(cx, |_, window, cx| match result {
                Ok(result) if result.has_update => {
                    let url = if result.release_page_url.is_empty() {
                        result.download_url.clone()
                    } else {
                        result.release_page_url.clone()
                    };
                    let message = translator
                        .text("updateAvailableToast")
                        .replace("{v}", &result.latest_version);
                    let action_label = t(&translator, "goToDownload");
                    let note = gpui_component::notification::Notification::info(message).action(
                        move |_, _, _| {
                            let url = url.clone();
                            gpui_component::button::Button::new("update-go-download")
                                .label(action_label.clone())
                                .on_click(move |_, _, cx| cx.open_url(&url))
                        },
                    );
                    window.push_notification(note, cx);
                }
                Ok(_) => window.push_notification(t(&translator, "upToDate"), cx),
                Err(_) => window.push_notification(
                    gpui_component::notification::Notification::error(t(
                        &translator,
                        "localServiceActionFailed",
                    )),
                    cx,
                ),
            });
        });
    })
    .detach();
}

fn open_logs_folder(cx: &mut App) {
    let client = Desktop::global(cx).client.clone();
    let future = client.call::<(), fluxdown_protocol::LogPathsDto>(
        fluxdown_protocol::method::AGENT_DIAGNOSTICS_LOG_PATHS,
        None,
    );
    cx.spawn(async move |cx| {
        if let Ok(paths) = future.await {
            let dir = if paths.agent_log_dir.is_empty() {
                paths.daemon_log_dir
            } else {
                paths.agent_log_dir
            };
            cx.update(|cx| cx.reveal_path(std::path::Path::new(&dir)));
        }
    })
    .detach();
}

fn show_about(cx: &mut App) {
    let Some(window) = cx
        .active_window()
        .or_else(|| WindowRegistry::handle(cx, &WindowKey::Main))
    else {
        return;
    };
    let translator = Desktop::global(cx).translator.read(cx).clone();
    let title = t(&translator, "menuAbout");
    let version_label = t(&translator, "currentVersion");
    let protocol_label = t(&translator, "protocolVersionLabel");
    let website_label = t(&translator, "menuWebsite");
    let _ = window.update(cx, |_, window, cx| {
        window.open_dialog(cx, move |dialog, _, _| {
            use gpui::{ParentElement as _, Styled as _};
            let version_label = version_label.clone();
            let protocol_label = protocol_label.clone();
            let website_label = website_label.clone();
            dialog
                .title(title.clone())
                .w(gpui::px(420.))
                .content(move |content, _, _| {
                    content.child(
                        gpui_component::v_flex()
                            .gap_2()
                            .child(format!("{version_label}: v{}", env!("CARGO_PKG_VERSION")))
                            .child(format!(
                                "{protocol_label}: {}",
                                fluxdown_protocol::PROTOCOL_VERSION
                            ))
                            .child(
                                gpui_component::link::Link::new("about-website")
                                    .href(WEBSITE_URL)
                                    .child(website_label.clone()),
                            ),
                    )
                })
        });
    });
}
