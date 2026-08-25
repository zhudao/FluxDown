use std::{borrow::Cow, cell::Cell, env, rc::Rc, sync::Arc};

use fluxdown_ui_downloads::{DOWNLOAD_ICON_PATH, DownloadView};
use fluxdown_ui_i18n::{I18nCatalog, I18nError, Translator, keys};
use fluxdown_ui_settings::{SettingsView, component_locale};
use fluxdown_ui_shell::{
    AuxiliaryWindowView, RouteId, ShellAction, ShellRoute, ShellView, auxiliary_window_options,
    main_window_options,
};
use gpui::{App, AppContext as _, Bounds, Entity, Window, WindowBounds, WindowHandle, px, size};
use gpui_component::{Icon, IconName, Root};

use crate::assets::DesktopAssets;

const MI_SANS_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Regular.ttf");
const MI_SANS_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Medium.ttf");
const MI_SANS_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Semibold.ttf");

const SETTINGS_WINDOW_SIZE: gpui::Size<gpui::Pixels> = size(px(1240.), px(760.));
pub(crate) fn run() -> Result<(), I18nError> {
    let catalog = Arc::new(I18nCatalog::load_embedded()?);
    let translator = catalog.translator(&system_locale());
    let locale = component_locale(translator.locale()).to_owned();

    gpui_platform::application()
        .with_assets(DesktopAssets)
        .run(move |cx| {
            if let Err(error) = cx.text_system().add_fonts(vec![
                Cow::Borrowed(MI_SANS_REGULAR),
                Cow::Borrowed(MI_SANS_MEDIUM),
                Cow::Borrowed(MI_SANS_SEMIBOLD),
            ]) {
                eprintln!("failed to load FluxDown UI fonts: {error:#}");
                return;
            }

            gpui_component::init(cx);
            fluxdown_ui_theme::init(cx);
            gpui_component::set_locale(&locale);
            let translator = cx.new(|_| translator);
            let bounds = Bounds::centered(None, size(px(1120.), px(760.)), cx);
            let mut options = main_window_options();
            options.window_bounds = Some(WindowBounds::Windowed(bounds));

            let settings_window = Rc::new(Cell::new(None));
            if let Err(error) = cx.open_window(options, move |window, cx| {
                let downloads = cx.new(|cx| DownloadView::new(translator.clone(), window, cx));
                let routes = vec![ShellRoute::new(
                    RouteId::new("downloads"),
                    "activity-downloads",
                    "activity-downloads-tooltip",
                    keys::MOBILE_NAV_DOWNLOADS,
                    Icon::empty().path(DOWNLOAD_ICON_PATH),
                    downloads.into(),
                )];
                let settings_translator = translator.clone();
                let settings_window = Rc::clone(&settings_window);
                let actions = vec![ShellAction::new(
                    "activity-settings",
                    "activity-settings-tooltip",
                    keys::SETTINGS,
                    Icon::new(IconName::Settings),
                    move |window, cx| {
                        show_settings_window(
                            settings_translator.clone(),
                            settings_window.as_ref(),
                            window,
                            cx,
                        );
                    },
                )];
                let shell = cx.new(|cx| ShellView::new(translator, routes, actions, cx));
                cx.new(|cx| Root::new(shell, window, cx))
            }) {
                eprintln!("failed to open FluxDown desktop window: {error:#}");
                return;
            }

            cx.activate(true);
        });

    Ok(())
}

fn show_settings_window(
    translator: Entity<Translator>,
    settings_window: &Cell<Option<WindowHandle<Root>>>,
    parent_window: &mut Window,
    cx: &mut App,
) {
    if let Some(handle) = settings_window.get() {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        settings_window.set(None);
    }

    let display_id = parent_window.display(cx).map(|display| display.id());
    let title = translator.read(cx).text(keys::SETTINGS).to_owned();
    let bounds = Bounds::centered(display_id, SETTINGS_WINDOW_SIZE, cx);
    let mut options = auxiliary_window_options(title.clone());
    options.display_id = display_id;
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    options.window_min_size = Some(size(px(1000.), px(600.)));

    match cx.open_window(options, |window, cx| {
        let window_translator = translator.clone();
        let settings = cx.new(|cx| SettingsView::new(translator, cx));
        let window_view = cx.new(|cx| {
            AuxiliaryWindowView::new(window_translator, keys::SETTINGS, settings.into(), cx)
        });
        cx.new(|cx| Root::new(window_view, window, cx))
    }) {
        Ok(handle) => settings_window.set(Some(handle)),
        Err(error) => eprintln!("failed to open FluxDown settings window: {error:#}"),
    }
}

fn system_locale() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(locale) = env::var(key)
            && !locale.trim().is_empty()
        {
            return locale;
        }
    }
    "en".to_owned()
}
