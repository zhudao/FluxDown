//! 系统托盘（Windows / macOS）：显示窗口 / 全部暂停 / 全部恢复 / 退出。
//!
//! Linux 不提供托盘（GPUI 端关窗即退出），[`install`] 在该平台是空操作。安装成功后
//! 进程进入「驻留态」（[`WindowRegistry::set_resident`]），关闭全部窗口不再退出进程。

use gpui::App;

/// 装配全局托盘图标；Linux 上什么也不做。
#[cfg(any(windows, target_os = "macos"))]
pub fn install(cx: &mut App) {
    imp::install(cx);
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn install(_cx: &mut App) {}

/// 完成后关机倒计时期间、无主窗口时用于显示剩余时间；`None` 恢复默认提示文案。
/// Linux 上什么也不做。
#[cfg(any(windows, target_os = "macos"))]
pub fn set_tooltip(cx: &App, text: Option<&str>) {
    imp::set_tooltip(cx, text);
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_tooltip(_cx: &App, _text: Option<&str>) {}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use std::cell::Cell;
    use std::time::Duration;

    use fluxdown_protocol::method;
    use gpui::{App, Global, WindowAppearance};
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use crate::app::Desktop;
    use crate::menus;
    use crate::windows::WindowRegistry;

    /// 托盘点击 / 菜单事件轮询间隔（tray-icon 无原生事件循环回调，靠轮询取事件）。
    const POLL_INTERVAL: Duration = Duration::from_millis(120);
    const DEFAULT_TOOLTIP: &str = "FluxDown";

    /// macOS 用「模板图标」：系统按菜单栏当前外观自动重新着色，无需手动切换。
    #[cfg(target_os = "macos")]
    const MACOS_ICON_BYTES: &[u8] = include_bytes!("../../../assets/logo/tray_iconTemplate.png");
    /// Windows 没有模板图标机制，深/浅两套素材随 `window_appearance()` 手动切换。
    #[cfg(windows)]
    const WINDOWS_DARK_ICON_BYTES: &[u8] = include_bytes!("../../../assets/logo/logo_on_dark.png");
    #[cfg(windows)]
    const WINDOWS_LIGHT_ICON_BYTES: &[u8] =
        include_bytes!("../../../assets/logo/fluxdown_logo.png");

    #[derive(Clone)]
    struct TrayIds {
        show: MenuId,
        pause_all: MenuId,
        resume_all: MenuId,
        quit: MenuId,
    }

    struct TrayState {
        icon: TrayIcon,
        ids: TrayIds,
        /// Windows 深/浅色两套图标（(dark, light)）；macOS 恒为 `None`（模板图标自适配）。
        windows_icons: Option<(Icon, Icon)>,
        appearance: Cell<WindowAppearance>,
    }

    impl Global for TrayState {}

    fn decode_icon(bytes: &[u8]) -> Option<Icon> {
        let decoded = image::load_from_memory(bytes).ok()?.into_rgba8();
        let (width, height) = decoded.dimensions();
        Icon::from_rgba(decoded.into_raw(), width, height).ok()
    }

    fn is_dark(appearance: WindowAppearance) -> bool {
        matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        )
    }

    pub(super) fn install(cx: &mut App) {
        if cx.has_global::<TrayState>() {
            return;
        }

        let appearance = cx.window_appearance();
        #[cfg(target_os = "macos")]
        let (icon, windows_icons) = {
            let Some(icon) = decode_icon(MACOS_ICON_BYTES) else {
                return;
            };
            (icon, None)
        };
        #[cfg(windows)]
        let (icon, windows_icons) = {
            let Some(dark) = decode_icon(WINDOWS_DARK_ICON_BYTES) else {
                return;
            };
            let Some(light) = decode_icon(WINDOWS_LIGHT_ICON_BYTES) else {
                return;
            };
            let icon = if is_dark(appearance) {
                dark.clone()
            } else {
                light.clone()
            };
            (icon, Some((dark, light)))
        };

        let translator = Desktop::global(cx).translator.read(cx).clone();
        let show_item = MenuItem::new(translator.text("trayShowWindow"), true, None);
        let pause_item = MenuItem::new(translator.text("pauseAll"), true, None);
        let resume_item = MenuItem::new(translator.text("resumeAll"), true, None);
        let quit_item = MenuItem::new(translator.text("trayExit"), true, None);
        let ids = TrayIds {
            show: show_item.id().clone(),
            pause_all: pause_item.id().clone(),
            resume_all: resume_item.id().clone(),
            quit: quit_item.id().clone(),
        };
        let Ok(menu) = Menu::with_items(&[
            &show_item,
            &PredefinedMenuItem::separator(),
            &pause_item,
            &resume_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ]) else {
            return;
        };

        let Ok(icon) = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(DEFAULT_TOOLTIP)
            .with_menu_on_left_click(false)
            .with_icon(icon)
            .with_icon_as_template(cfg!(target_os = "macos"))
            .build()
        else {
            return;
        };

        cx.set_global(TrayState {
            icon,
            ids,
            windows_icons,
            appearance: Cell::new(appearance),
        });
        WindowRegistry::set_resident(cx, true);
        spawn_event_pump(cx);
    }

    pub(super) fn set_tooltip(cx: &App, text: Option<&str>) {
        let Some(state) = cx.try_global::<TrayState>() else {
            return;
        };
        let _ = state
            .icon
            .set_tooltip(Some(text.unwrap_or(DEFAULT_TOOLTIP)));
    }

    fn spawn_event_pump(cx: &mut App) {
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let mut alive = true;
                cx.update(|cx| {
                    if !cx.has_global::<TrayState>() {
                        alive = false;
                        return;
                    }
                    pump_events(cx);
                });
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn pump_events(cx: &mut App) {
        let Some(ids) = cx.try_global::<TrayState>().map(|state| state.ids.clone()) else {
            return;
        };

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    ..
                } => show_main_window(cx),
                TrayIconEvent::DoubleClick { .. } => show_main_window(cx),
                _ => {}
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == ids.show {
                show_main_window(cx);
            } else if event.id == ids.pause_all {
                fire_and_forget(cx, method::DAEMON_TASK_PAUSE_ALL);
            } else if event.id == ids.resume_all {
                fire_and_forget(cx, method::DAEMON_TASK_RESUME_ALL);
            } else if event.id == ids.quit {
                menus::request_quit(cx);
            }
        }

        sync_appearance(cx);
    }

    fn sync_appearance(cx: &mut App) {
        let appearance = cx.window_appearance();
        let Some(state) = cx.try_global::<TrayState>() else {
            return;
        };
        if state.appearance.get() == appearance {
            return;
        }
        state.appearance.set(appearance);
        let Some((dark, light)) = state.windows_icons.as_ref() else {
            return;
        };
        let icon = if is_dark(appearance) {
            dark.clone()
        } else {
            light.clone()
        };
        let _ = state.icon.set_icon(Some(icon));
    }

    fn show_main_window(cx: &mut App) {
        crate::windows::main::open(cx);
        cx.activate(true);
    }

    fn fire_and_forget(cx: &mut App, method_name: &'static str) {
        let client = Desktop::global(cx).client.clone();
        let future = client.call::<(), serde_json::Value>(method_name, None);
        cx.spawn(async move |_cx| {
            let _ = future.await;
        })
        .detach();
    }
}
