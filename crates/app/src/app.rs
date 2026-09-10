//! composition root：一个 agent 会话、一个窗口注册表、全局菜单与动作；各窗口按需装配。

use std::{borrow::Cow, collections::BTreeMap, env, sync::Arc};

use fluxdown_protocol::{AgentEvent, DaemonEvent, DaemonRuntimeStatsDto, ServiceEvent};
use fluxdown_ui_downloads::DownloadView;
use fluxdown_ui_i18n::{I18nCatalog, I18nError, Translator};
use fluxdown_ui_settings::{SettingsStore, component_locale};
use fluxdown_ui_shell::ShellView;
use gpui::{App, AppContext as _, Entity, Global, WeakEntity};
use gpui_component::menu::AppMenuBar;
use tokio::sync::mpsc;

use crate::agent_client::{AgentClient, AgentClientConfig};
use crate::assets::DesktopAssets;
use crate::instance_ipc::{self, ActivateMessage, Endpoint};
use crate::launch::{self, LaunchOptions};
use crate::service_bootstrap::ServiceBootstrap;
use crate::session::{AgentSession, SessionSignal, attach};
use crate::settings_port::AgentSettingsPort;
use crate::windows::{WindowKey, WindowRegistry};

const MI_SANS_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Regular.ttf");
const MI_SANS_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Medium.ttf");
const MI_SANS_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Semibold.ttf");

/// 事件泵单次批量上限。
const EVENT_BATCH: usize = 256;

/// 跨窗口共享的应用状态（composition root 独有）。
pub(crate) struct Desktop {
    pub translator: Entity<Translator>,
    pub session: Entity<AgentSession>,
    pub client: Arc<AgentClient>,
    pub settings_store: Entity<SettingsStore>,
    pub menu_bar: Entity<AppMenuBar>,
    /// 主窗口内的下载页（主窗口关闭后失效）。
    pub main_downloads: Option<WeakEntity<DownloadView>>,
    pub main_shell: Option<WeakEntity<ShellView>>,
    /// 最新偏好（快照 + `PreferencesChanged` 折叠）。
    pub preferences: BTreeMap<String, serde_json::Value>,
    /// 最新运行时统计（关窗 / 退出提示与托盘 tooltip 用）。
    pub runtime_stats: DaemonRuntimeStatsDto,
}

impl Global for Desktop {}

impl Desktop {
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    pub fn active_task_count(cx: &App) -> u32 {
        Self::global(cx).runtime_stats.active_tasks
    }

    pub fn pref_bool(cx: &App, key: &str, default: bool) -> bool {
        Self::global(cx)
            .preferences
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    }

    pub fn pref(cx: &App, key: &str) -> Option<serde_json::Value> {
        Self::global(cx).preferences.get(key).cloned()
    }
}

pub(crate) fn run() -> Result<(), I18nError> {
    let launch = LaunchOptions::from_args(env::args().skip(1));
    let token_path = agent_token_path();
    let instance_dir = launch::instance_dir(&token_path);
    let endpoint = Endpoint::for_instance_dir(&instance_dir);
    let instance_lock = match launch::InstanceLock::try_acquire(&instance_dir) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => None,
        Err(error) => {
            eprintln!("FluxDown desktop instance lock unavailable: {error:#}");
            None
        }
    };
    let agent_config = AgentClientConfig {
        rpc_url: env::var("FLUXDOWN_AGENT_URL")
            .unwrap_or_else(|_| "ws://127.0.0.1:17800/rpc".to_owned()),
        bearer_path: token_path,
    };
    if instance_lock.is_none() {
        // 已有实例：先经激活通道交给主实例（可激活已有窗口），不可达再直接交给 agent。
        let message = ActivateMessage {
            urls: launch.urls.clone(),
            files: launch.torrent_files.clone(),
            activate: !launch.capture_only,
        };
        if !instance_ipc::try_send_to_primary(&endpoint, &message) {
            forward_urls_and_exit(&agent_config, &launch.urls, &launch.torrent_files);
        }
        return Ok(());
    }
    let _instance_lock = instance_lock;

    let catalog = Arc::new(I18nCatalog::load_embedded()?);
    let translator = catalog.translator(&system_locale());
    let locale = component_locale(translator.locale()).to_owned();

    let bootstrap = Arc::new(ServiceBootstrap::new());
    let (agent_client, mut agent_events) = match AgentClient::start(agent_config, bootstrap) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("failed to start FluxDown agent client: {error:#}");
            return Ok(());
        }
    };
    submit_captures_detached(
        &agent_client,
        launch.urls.clone(),
        launch.torrent_files.clone(),
    );
    let (activate_tx, mut activate_rx) = mpsc::channel::<ActivateMessage>(16);
    agent_client.spawn_background(instance_ipc::listen(endpoint, activate_tx));
    let open_urls_client = agent_client.clone();

    let application = gpui_platform::application().with_assets(DesktopAssets);
    application.on_open_urls(move |urls| {
        let files = urls
            .iter()
            .filter_map(|url| launch::torrent_path(url))
            .collect();
        let urls = urls
            .into_iter()
            .filter(|url| launch::is_capture_url(url))
            .collect();
        submit_captures_detached(&open_urls_client, urls, files);
    });
    application.on_reopen(|cx| {
        if cx.has_global::<Desktop>() && !WindowRegistry::is_open(cx, &WindowKey::Main) {
            crate::windows::main::open(cx);
        }
    });
    application.run(move |cx| {
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
        let session = cx.new(|_| AgentSession::new(agent_client.clone()));
        WindowRegistry::init(cx, agent_client.clone());

        // 设置存储跨窗口存活：窗口关闭后防抖中的写回仍完成，快照/事件持续进入。
        let settings_store =
            cx.new(|_| SettingsStore::new(Arc::new(AgentSettingsPort::new(agent_client.clone()))));
        attach(&session, &settings_store, cx);
        let quit_store = settings_store.clone();
        cx.on_app_quit(move |cx| {
            let calls = quit_store.update(cx, |store, _| store.drain_pending_calls());
            async move {
                for call in calls {
                    let _ = call.await;
                }
            }
        })
        .detach();

        crate::menus::bind_keys(cx);
        let menu_bar = crate::menus::install(cx, &translator);
        crate::menus::install_global_actions(cx);
        cx.set_global(Desktop {
            translator: translator.clone(),
            session: session.clone(),
            client: agent_client.clone(),
            settings_store,
            menu_bar,
            main_downloads: None,
            main_shell: None,
            preferences: BTreeMap::new(),
            runtime_stats: DaemonRuntimeStatsDto::default(),
        });

        // 会话 → 偏好 / 运行时统计折叠进 Desktop；外观与语言随偏好变化。
        cx.subscribe(&session, |_, signal, cx| match signal {
            SessionSignal::Snapshot(snapshot) => {
                if let Some(body) = crate::session::agent_body(snapshot) {
                    let values = body.preferences.values.clone();
                    let stats = body.daemon.runtime_stats.clone();
                    let desktop = Desktop::global_mut(cx);
                    desktop.preferences = values;
                    desktop.runtime_stats = stats;
                    apply_preferences(cx);
                }
            }
            SessionSignal::Event(frame) => match &frame.event {
                ServiceEvent::Agent(AgentEvent::PreferencesChanged(prefs)) => {
                    Desktop::global_mut(cx).preferences = prefs.values.clone();
                    apply_preferences(cx);
                }
                ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::RuntimeStatsChanged(
                    stats,
                )))
                | ServiceEvent::Daemon(DaemonEvent::RuntimeStatsChanged(stats)) => {
                    Desktop::global_mut(cx).runtime_stats = stats.clone();
                }
                ServiceEvent::Agent(AgentEvent::DaemonSnapshotReplaced(snapshot))
                | ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::SnapshotReplaced(
                    snapshot,
                ))) => {
                    Desktop::global_mut(cx).runtime_stats = snapshot.runtime_stats.clone();
                }
                _ => {}
            },
            SessionSignal::Fatal(error) => {
                eprintln!("fatal FluxDown agent error: {:?}", error.code);
            }
            SessionSignal::Stale => {}
        })
        .detach();

        // 事件泵：按帧批处理，一次 `update` 广播一批。
        let pump_session = session.clone();
        cx.spawn(async move |cx| {
            loop {
                let Some(first) = agent_events.recv().await else {
                    break;
                };
                let mut batch = vec![first];
                while batch.len() < EVENT_BATCH {
                    match agent_events.try_recv() {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                cx.update(|cx| {
                    pump_session.update(cx, |session, cx| session.ingest(batch, cx));
                });
            }
        })
        .detach();

        // 单实例激活通道：链接交给 agent，`activate` 重建 / 聚焦主窗口。
        let activate_client = agent_client.clone();
        cx.spawn(async move |cx| {
            while let Some(message) = activate_rx.recv().await {
                submit_captures_detached(&activate_client, message.urls, message.files);
                if message.activate {
                    cx.update(|cx| {
                        crate::windows::main::open(cx);
                        cx.activate(true);
                    });
                }
            }
        })
        .detach();

        // 常驻能力：托盘 / 剪贴板监听 / 完成后关机（须在下方启动分支判断 resident 前完成安装）。
        crate::tray::install(cx);
        crate::clipboard_watch::install(cx);
        crate::power::install(cx);
        // 引擎选择请求 / 外部捕获确认窗口：跟随会话事件独立开关，不依赖主窗口存在。
        crate::windows::selection::install(cx);
        crate::windows::quick_capture::install(cx);

        if launch.capture_only {
            // 由 agent 为捕获拉起：不开主窗口；捕获窗口随 `PendingCapturesChanged` 打开，
            // 清空后由退出判定收尾。
            return;
        }
        if launch.minimized {
            // 自启动：等首个快照决定是「托盘驻留」还是「最小化主窗口」。
            open_main_after_first_snapshot(cx);
            return;
        }
        crate::windows::main::open(cx);
        cx.activate(true);
    });

    Ok(())
}

/// `--minimized`：首个快照到达后按 `start_minimized_to_tray` 决定是否开主窗口。
fn open_main_after_first_snapshot(cx: &mut App) {
    let session = Desktop::global(cx).session.clone();
    if Desktop::global(cx).session.read(cx).latest().is_some() {
        open_main_minimized(cx);
        return;
    }
    let subscription = std::rc::Rc::new(std::cell::RefCell::new(None));
    let holder = std::rc::Rc::clone(&subscription);
    *subscription.borrow_mut() = Some(cx.subscribe(&session, move |_, signal, cx| {
        if matches!(signal, SessionSignal::Snapshot(_)) {
            open_main_minimized(cx);
            holder.borrow_mut().take();
        }
    }));
}

fn open_main_minimized(cx: &mut App) {
    let to_tray =
        WindowRegistry::is_resident(cx) && Desktop::pref_bool(cx, "start_minimized_to_tray", false);
    if to_tray {
        return;
    }
    if let Some(handle) = crate::windows::main::open(cx) {
        let _ = handle.update(cx, |_, window, _| window.minimize_window());
    }
}

/// 偏好快照 → 全局外观与语言。每次快照/偏好事件都幂等应用。
fn apply_preferences(cx: &mut App) {
    let values = Desktop::global(cx).preferences.clone();
    let translator = Desktop::global(cx).translator.clone();
    fluxdown_ui_theme::apply_appearance_preferences(&values, cx);
    if let Some(locale) = values
        .get("general.locale")
        .and_then(serde_json::Value::as_str)
    {
        let target = if locale == "system" {
            system_locale()
        } else {
            locale.to_owned()
        };
        translator.update(cx, |translator, cx| {
            if translator.set_locale(&target) {
                gpui_component::set_locale(component_locale(translator.locale()));
                cx.notify();
            }
        });
    }
}

/// 外部链接 / `.torrent` 文件 → agent 捕获入口的 RPC 列表。
fn capture_calls(
    client: &Arc<AgentClient>,
    urls: Vec<String>,
    files: Vec<std::path::PathBuf>,
) -> Vec<(String, crate::agent_client::AgentFuture<serde_json::Value>)> {
    let mut calls = Vec::with_capacity(urls.len() + files.len());
    for url in urls {
        let url = launch::normalize_capture_url(&url);
        let future = client.call::<serde_json::Value, serde_json::Value>(
            fluxdown_protocol::method::AGENT_CAPTURE_SUBMIT,
            Some(serde_json::json!({ "request": { "url": url }, "silent": true })),
        );
        calls.push((url, future));
    }
    for file in files {
        let path = file.display().to_string();
        let future = client.call::<serde_json::Value, serde_json::Value>(
            fluxdown_protocol::method::AGENT_CAPTURE_SUBMIT_TORRENT_FILE,
            Some(serde_json::json!({ "path": path, "silent": true })),
        );
        calls.push((path, future));
    }
    calls
}

/// 主实例：不阻塞 UI 线程，在后台把链接交给 agent。
pub(crate) fn submit_captures_detached(
    client: &Arc<AgentClient>,
    urls: Vec<String>,
    files: Vec<std::path::PathBuf>,
) {
    if urls.is_empty() && files.is_empty() {
        return;
    }
    let calls = capture_calls(client, urls, files);
    client.spawn_background(async move {
        for (source, future) in calls {
            if let Err(error) = future.await {
                eprintln!("failed to submit {source}: {:?}", error.code);
            }
        }
    });
}

/// 次实例且主实例激活通道不可达：不开窗口，只把链接交给 agent 后退出。
fn forward_urls_and_exit(
    config: &AgentClientConfig,
    urls: &[String],
    files: &[std::path::PathBuf],
) {
    if urls.is_empty() && files.is_empty() {
        return;
    }
    let bootstrap = Arc::new(ServiceBootstrap::new());
    let Ok((client, _events)) = AgentClient::start(config.clone(), bootstrap) else {
        eprintln!("failed to reach the running FluxDown instance");
        return;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    for (source, future) in capture_calls(&client, urls.to_vec(), files.to_vec()) {
        if let Err(error) = runtime.block_on(future) {
            eprintln!("failed to forward {source}: {:?}", error.code);
        }
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

pub(crate) fn system_locale() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(locale) = env::var(key)
            && !locale.trim().is_empty()
        {
            return locale;
        }
    }
    "en".to_owned()
}
