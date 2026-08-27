//! 插件、市场、受管组件与 Webhook capability。

use std::{future::Future, pin::Pin, sync::Arc};

use fluxdown_ui_i18n::Translator;
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _, button::Button, h_flex, switch::Switch, v_flex,
};

pub type PortFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, fluxdown_protocol::RpcErrorData>> + Send + 'static>>;

pub trait ExtensionsPort: Send + Sync {
    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> PortFuture<serde_json::Value>;
}

pub struct ExtensionsController {
    port: Arc<dyn ExtensionsPort>,
    plugins: Vec<fluxdown_protocol::PluginDto>,
    components: Vec<fluxdown_protocol::ComponentStatusDto>,
    stale: bool,
}

impl ExtensionsController {
    pub fn new(port: Arc<dyn ExtensionsPort>) -> Self {
        Self {
            port,
            plugins: Vec::new(),
            components: Vec::new(),
            stale: true,
        }
    }
    pub fn replace_snapshot(&mut self, snapshot: &fluxdown_protocol::AgentSnapshot) {
        self.plugins.clone_from(&snapshot.daemon.plugins);
        self.components.clone_from(&snapshot.daemon.components);
        self.stale = false;
    }
    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent) {
        let fluxdown_protocol::ServiceEvent::Agent(event) = event else {
            return;
        };
        match event {
            fluxdown_protocol::AgentEvent::DaemonSnapshotReplaced(snapshot) => {
                self.plugins.clone_from(&snapshot.plugins);
                self.components.clone_from(&snapshot.components);
                self.stale = false;
            }
            fluxdown_protocol::AgentEvent::DaemonConnectionChanged(connected) => {
                self.stale = !connected;
            }
            fluxdown_protocol::AgentEvent::Daemon(
                fluxdown_protocol::DaemonEvent::PluginsChanged(plugins),
            ) => self.plugins.clone_from(plugins),
            fluxdown_protocol::AgentEvent::Daemon(
                fluxdown_protocol::DaemonEvent::ComponentsChanged(components),
            ) => self.components.clone_from(components),
            _ => {}
        }
    }
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }
    pub fn plugins(&self) -> &[fluxdown_protocol::PluginDto] {
        &self.plugins
    }
    pub fn components(&self) -> &[fluxdown_protocol::ComponentStatusDto] {
        &self.components
    }
    pub fn is_stale(&self) -> bool {
        self.stale
    }
    pub fn install_component(
        &self,
        component: fluxdown_protocol::ComponentKind,
    ) -> PortFuture<serde_json::Value> {
        if self.stale {
            return unavailable();
        }
        self.port.call(
            fluxdown_protocol::method::DAEMON_COMPONENT_INSTALL,
            serde_json::json!({"component": component, "version": null}),
        )
    }
    pub fn uninstall_component(
        &self,
        component: fluxdown_protocol::ComponentKind,
    ) -> PortFuture<serde_json::Value> {
        if self.stale {
            return unavailable();
        }
        self.port.call(
            fluxdown_protocol::method::DAEMON_COMPONENT_UNINSTALL,
            serde_json::json!({"component": component}),
        )
    }
    pub fn set_plugin_enabled(
        &self,
        identity: String,
        enabled: bool,
    ) -> PortFuture<serde_json::Value> {
        if self.stale {
            return Box::pin(async {
                Err(fluxdown_protocol::RpcErrorData::new(
                    fluxdown_protocol::ApplicationErrorCode::Unavailable,
                    true,
                ))
            });
        }
        self.port.call(
            fluxdown_protocol::method::DAEMON_PLUGIN_SET_ENABLED,
            serde_json::json!({"identity": identity, "enabled": enabled}),
        )
    }
}

fn unavailable() -> PortFuture<serde_json::Value> {
    Box::pin(async {
        Err(fluxdown_protocol::RpcErrorData::new(
            fluxdown_protocol::ApplicationErrorCode::Unavailable,
            true,
        ))
    })
}

pub struct ExtensionsView {
    translator: Entity<Translator>,
    controller: ExtensionsController,
    last_error: Option<String>,
}

impl ExtensionsView {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn ExtensionsPort>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        Self {
            translator,
            controller: ExtensionsController::new(port),
            last_error: None,
        }
    }
    pub fn replace_snapshot(
        &mut self,
        snapshot: &fluxdown_protocol::AgentSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.last_error = None;
        self.controller.replace_snapshot(snapshot);
        cx.notify();
    }
    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent, cx: &mut Context<Self>) {
        self.controller.apply_event(event);
        cx.notify();
    }
    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.last_error = Some(
            self.translator
                .read(cx)
                .text("localServiceDisconnected")
                .to_owned(),
        );
        self.controller.mark_stale();
        cx.notify();
    }

    fn spawn_action(&mut self, future: PortFuture<serde_json::Value>, cx: &mut Context<Self>) {
        self.last_error = None;
        cx.spawn(async move |this, cx| {
            let failed = future.await.is_err();
            let _ = this.update(cx, |this, cx| {
                this.last_error = failed.then(|| {
                    this.translator
                        .read(cx)
                        .text("localServiceActionFailed")
                        .to_owned()
                });
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ExtensionsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let translator = self.translator.read(cx);
        let title = translator.text("settingsCatExtensions").to_owned();
        let components_title = translator.text("componentsTitle").to_owned();
        let install = translator.text("componentsInstallButton").to_owned();
        let uninstall = translator.text("componentsUninstallButton").to_owned();
        let stale = self.controller.is_stale();
        let plugins = self.controller.plugins().to_vec();
        let components = self
            .controller
            .components()
            .iter()
            .map(|status| match status {
                fluxdown_protocol::ComponentStatusDto::Ffmpeg(status) => (
                    fluxdown_protocol::ComponentKind::Ffmpeg,
                    "FFmpeg",
                    status.version.clone(),
                    !status.managed_version.is_empty(),
                    status.managed_supported,
                ),
                fluxdown_protocol::ComponentStatusDto::Ytdlp(status) => (
                    fluxdown_protocol::ComponentKind::Ytdlp,
                    "yt-dlp",
                    status.version.clone(),
                    !status.managed_version.is_empty(),
                    status.managed_supported,
                ),
            })
            .collect::<Vec<_>>();
        v_flex()
            .size_full()
            .gap_3()
            .p_4()
            .child(div().text_lg().font_semibold().child(title))
            .when_some(self.last_error.clone(), |this, error| {
                this.child(div().text_sm().child(error))
            })
            .children(plugins.into_iter().enumerate().map(|(index, plugin)| {
                let identity = plugin.identity.clone();
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .py_2()
                    .border_b_1()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(div().text_sm().font_semibold().child(plugin.name))
                            .child(
                                div()
                                    .text_xs()
                                    .child(format!("{} · {}", plugin.version, plugin.description)),
                            ),
                    )
                    .child(
                        Switch::new(("plugin-enabled", index))
                            .checked(plugin.enabled)
                            .disabled(stale)
                            .on_click(cx.listener(move |this, checked, _, cx| {
                                let future = this
                                    .controller
                                    .set_plugin_enabled(identity.clone(), *checked);
                                this.spawn_action(future, cx);
                            })),
                    )
            }))
            .child(div().text_sm().font_semibold().child(components_title))
            .children(components.into_iter().enumerate().map(
                |(index, (kind, name, version, installed, supported))| {
                    let action_kind = kind;
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .py_2()
                        .child(
                            v_flex()
                                .child(div().text_sm().font_semibold().child(name))
                                .child(div().text_xs().child(version)),
                        )
                        .child(
                            Button::new(("component-action", index))
                                .small()
                                .outline()
                                .label(if installed {
                                    uninstall.clone()
                                } else {
                                    install.clone()
                                })
                                .disabled(stale || (!installed && !supported))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let future = if installed {
                                        this.controller.uninstall_component(action_kind)
                                    } else {
                                        this.controller.install_component(action_kind)
                                    };
                                    this.spawn_action(future, cx);
                                })),
                        )
                },
            ))
    }
}
