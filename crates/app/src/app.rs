use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    env,
    rc::Rc,
    sync::Arc,
};

use fluxdown_ui_account::AccountView;
use fluxdown_ui_downloads::{DOWNLOAD_ICON_PATH, DownloadView};
use fluxdown_ui_extensions::ExtensionsView;
use fluxdown_ui_i18n::{I18nCatalog, I18nError, Translator, keys};
use fluxdown_ui_rss::RssView;
use fluxdown_ui_settings::{SettingsContentSlots, SettingsView, component_locale};
use fluxdown_ui_shell::{
    AuxiliaryWindowView, RouteId, ShellAction, ShellRoute, ShellView, auxiliary_window_options,
    main_window_options,
};
use gpui::{
    App, AppContext as _, Bounds, Entity, WeakEntity, Window, WindowBounds, WindowHandle, px, size,
};
use gpui_component::{Icon, IconName, Root};

use crate::account_port::AgentAccountPort;
use crate::agent_client::{AgentClient, AgentClientConfig, AgentClientEvent};
use crate::assets::DesktopAssets;
use crate::capability_ports::{AgentExtensionsPort, AgentRssPort};
use crate::downloads_port::AgentDownloadsPort;
use crate::service_bootstrap::ServiceBootstrap;
use crate::settings_port::AgentSettingsPort;

const MI_SANS_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Regular.ttf");
const MI_SANS_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Medium.ttf");
const MI_SANS_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Semibold.ttf");

struct ClientProjection {
    last_snapshot: Option<fluxdown_protocol::AgentSnapshot>,
    settings: Option<WeakEntity<SettingsView>>,
    account: Option<WeakEntity<AccountView>>,
    rss: Option<WeakEntity<RssView>>,
    extensions: Option<WeakEntity<ExtensionsView>>,
}
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

            let bootstrap = Arc::new(ServiceBootstrap::new());
            let agent_config = AgentClientConfig {
                rpc_url: env::var("FLUXDOWN_AGENT_URL")
                    .unwrap_or_else(|_| "ws://127.0.0.1:17800/rpc".to_owned()),
                bearer_path: agent_token_path(),
            };
            let (agent_client, mut agent_events) = match AgentClient::start(agent_config, bootstrap)
            {
                Ok(client) => client,
                Err(error) => {
                    eprintln!("failed to start FluxDown agent client: {error:#}");
                    return;
                }
            };

            let client_projection = Rc::new(RefCell::new(ClientProjection {
                last_snapshot: None,
                settings: None,
                account: None,
                rss: None,
                extensions: None,
            }));
            let settings_window = Rc::new(Cell::new(None));
            if let Err(error) = cx.open_window(options, move |window, cx| {
                let downloads_port = Arc::new(AgentDownloadsPort::new(agent_client.clone()));
                let downloads =
                    cx.new(|cx| DownloadView::new(translator.clone(), downloads_port, window, cx));
                let rss_port = Arc::new(AgentRssPort::new(agent_client.clone()));
                let rss = cx.new(|cx| RssView::new(translator.clone(), rss_port, window, cx));
                client_projection.borrow_mut().rss = Some(rss.downgrade());
                let downloads_events = downloads.downgrade();
                let projection_events = Rc::clone(&client_projection);
                cx.spawn(async move |cx| {
                    while let Some(event) = agent_events.recv().await {
                        if let AgentClientEvent::Snapshot(snapshot) = &event {
                            projection_events.borrow_mut().last_snapshot =
                                Some(snapshot.as_ref().clone());
                        }
                        if downloads_events
                            .update(cx, |downloads, cx| match &event {
                                AgentClientEvent::Snapshot(snapshot) => {
                                    downloads.replace_snapshot(snapshot, cx);
                                }
                                AgentClientEvent::Event(frame) => {
                                    downloads.apply_event(&frame.event, cx);
                                }
                                AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                    downloads.mark_stale(cx);
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                        let settings = projection_events.borrow().settings.clone();
                        if let Some(settings) = settings {
                            let _ = settings.update(cx, |settings, cx| match &event {
                                AgentClientEvent::Snapshot(snapshot) => {
                                    settings.replace_snapshot(snapshot, cx);
                                }
                                AgentClientEvent::Event(frame) => {
                                    settings.apply_event(&frame.event, cx);
                                }
                                AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                    settings.mark_stale(cx);
                                }
                            });
                        }
                        let account = projection_events.borrow().account.clone();
                        if let Some(account) = account {
                            let _ = account.update(cx, |account, cx| match &event {
                                AgentClientEvent::Snapshot(snapshot) => {
                                    account.replace_snapshot(snapshot, cx);
                                }
                                AgentClientEvent::Event(frame) => {
                                    account.apply_event(&frame.event, cx);
                                }
                                AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                    account.mark_stale(cx);
                                }
                            });
                        }
                        let rss = projection_events.borrow().rss.clone();
                        if let Some(rss) = rss {
                            let _ = rss.update(cx, |rss, cx| match &event {
                                AgentClientEvent::Snapshot(snapshot) => {
                                    rss.replace_snapshot(snapshot, cx);
                                }
                                AgentClientEvent::Event(frame) => {
                                    rss.apply_event(&frame.event, cx);
                                }
                                AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                    rss.mark_stale(cx);
                                }
                            });
                        }
                        let extensions = projection_events.borrow().extensions.clone();
                        if let Some(extensions) = extensions {
                            let _ = extensions.update(cx, |extensions, cx| match &event {
                                AgentClientEvent::Snapshot(snapshot) => {
                                    extensions.replace_snapshot(snapshot, cx);
                                }
                                AgentClientEvent::Event(frame) => {
                                    extensions.apply_event(&frame.event, cx);
                                }
                                AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                    extensions.mark_stale(cx);
                                }
                            });
                        }
                        if let AgentClientEvent::Fatal(error) = &event {
                            eprintln!("fatal FluxDown agent error: {:?}", error.code);
                        }
                    }
                })
                .detach();
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
                let settings_translator = translator.clone();
                let settings_window = Rc::clone(&settings_window);
                let settings_client = agent_client.clone();
                let settings_projection = Rc::clone(&client_projection);
                let actions = vec![ShellAction::new(
                    "activity-settings",
                    "activity-settings-tooltip",
                    keys::SETTINGS,
                    Icon::new(IconName::Settings),
                    move |window, cx| {
                        show_settings_window(
                            settings_translator.clone(),
                            settings_client.clone(),
                            Rc::clone(&settings_projection),
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
    agent_client: Arc<AgentClient>,
    projection: Rc<RefCell<ClientProjection>>,
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

    match cx.open_window(options, move |window, cx| {
        let window_translator = translator.clone();
        let settings_port = Arc::new(AgentSettingsPort::new(agent_client.clone()));
        let account_port = Arc::new(AgentAccountPort::new(agent_client.clone()));
        let extensions_port = Arc::new(AgentExtensionsPort::new(agent_client));
        let account = cx.new(|cx| AccountView::new(translator.clone(), account_port, window, cx));
        let extensions = cx.new(|cx| ExtensionsView::new(translator.clone(), extensions_port, cx));
        let settings = cx.new(|cx| {
            SettingsView::new(
                translator,
                settings_port,
                SettingsContentSlots {
                    account: Some(account.clone().into()),
                    extensions: Some(extensions.clone().into()),
                },
                window,
                cx,
            )
        });
        if let Some(snapshot) = projection.borrow().last_snapshot.as_ref() {
            account.update(cx, |account, cx| {
                account.replace_snapshot(snapshot, cx);
            });
            extensions.update(cx, |extensions, cx| {
                extensions.replace_snapshot(snapshot, cx);
            });
            settings.update(cx, |settings, cx| {
                settings.replace_snapshot_in_window(snapshot, window, cx);
            });
        }
        let mut projection = projection.borrow_mut();
        projection.account = Some(account.downgrade());
        projection.extensions = Some(extensions.downgrade());
        projection.settings = Some(settings.downgrade());
        drop(projection);
        let window_view = cx.new(|cx| {
            AuxiliaryWindowView::new(
                window_translator,
                keys::SETTINGS,
                settings.clone().into(),
                cx,
            )
        });
        cx.new(|cx| Root::new(window_view, window, cx))
    }) {
        Ok(handle) => settings_window.set(Some(handle)),
        Err(error) => eprintln!("failed to open FluxDown settings window: {error:#}"),
    }
}

fn agent_token_path() -> std::path::PathBuf {
    if let Some(path) = env::var_os("FLUXDOWN_AGENT_TOKEN_FILE") {
        return path.into();
    }
    if let Some(path) = env::var_os("FLUXDOWN_AGENT_DATA_DIR") {
        return std::path::PathBuf::from(path).join("agent.token");
    }
    directories::ProjectDirs::from("dev", "zerx", "FluxDown")
        .map(|project| project.data_dir().join("agent").join("agent.token"))
        .unwrap_or_else(|| std::path::PathBuf::from("agent.token"))
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
