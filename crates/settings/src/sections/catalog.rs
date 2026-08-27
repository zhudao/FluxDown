use fluxdown_ui_components::{ButtonVariant, button, card};
use fluxdown_ui_theme::active_theme;
use gpui::{Context, Div, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _, h_flex,
    input::{Input, InputState, NumberInput},
    switch::Switch,
    v_flex,
};

use crate::controller::{PortFuture, SettingsCommand, SettingsResult};
use crate::view::SettingsView;

pub(crate) const PROXY_KEYS: &[&str] = &[
    "proxy_mode",
    "proxy_type",
    "proxy_host",
    "proxy_port",
    "proxy_username",
    "proxy_password",
    "proxy_no_list",
];

fn proxy_label(key: &'static str) -> &'static str {
    match key {
        "proxy_type" => "proxyType",
        "proxy_host" => "proxyHost",
        "proxy_port" => "proxyPort",
        "proxy_username" => "proxyUsername",
        "proxy_password" => "proxyPassword",
        _ => key,
    }
}

impl SettingsView {
    pub(crate) fn render_catalog_content(&self, selectors: &[&str], cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        let specs = fluxdown_protocol::SYNC_SETTING_SPECS
            .iter()
            .filter(|spec| {
                selectors.iter().any(|selector| {
                    if selector.ends_with('.') {
                        spec.key.starts_with(selector)
                    } else {
                        spec.key == *selector
                    }
                })
            })
            .filter(|spec| {
                selectors.first() != Some(&"download.")
                    || !matches!(
                        spec.key,
                        "download.notify_on_complete"
                            | "download.silent_download"
                            | "download.keep_awake"
                    )
            });
        v_flex().w_full().gap(tokens.spacing.sm).child(
            card(cx)
                .w_full()
                .overflow_hidden()
                .children(specs.map(|spec| self.render_catalog_row(spec, cx))),
        )
    }

    fn render_catalog_row(
        &self,
        spec: &'static fluxdown_protocol::SettingSpec,
        cx: &mut Context<Self>,
    ) -> Div {
        let tokens = active_theme(cx).tokens();
        let read_only =
            self.controller.is_read_only() && spec.owner == fluxdown_protocol::SettingOwner::Daemon;
        let field = match fluxdown_protocol::setting_value_kind(spec.key) {
            fluxdown_protocol::SettingValueKind::Boolean => {
                let checked = self
                    .controller
                    .value(spec.key)
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                let key = spec.key.to_owned();
                Switch::new(spec.key)
                    .checked(checked)
                    .disabled(read_only)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        let future = this
                            .controller
                            .set_value(key.clone(), serde_json::Value::Bool(*checked));
                        this.spawn_setting_update(future, cx);
                    }))
                    .into_any_element()
            }
            fluxdown_protocol::SettingValueKind::Integer
            | fluxdown_protocol::SettingValueKind::Float => self
                .inputs
                .get(spec.key)
                .map(|input| {
                    NumberInput::new(input)
                        .small()
                        .disabled(read_only)
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element()),
            fluxdown_protocol::SettingValueKind::String => self
                .inputs
                .get(spec.key)
                .map(|input| {
                    Input::new(input)
                        .small()
                        .disabled(read_only)
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element()),
        };
        h_flex()
            .w_full()
            .min_h_12()
            .items_center()
            .justify_between()
            .gap_4()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(tokens.colors.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().child(SharedString::from(spec.key)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(tokens.colors.muted_foreground)
                            .child(SharedString::from(spec.storage_key)),
                    ),
            )
            .child(div().w_64().child(field))
    }

    pub(crate) fn commit_input(
        &mut self,
        key: &'static str,
        input: &Entity<InputState>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = input.read(cx).value().trim().to_owned();
        let value = match fluxdown_protocol::setting_value_kind(key) {
            fluxdown_protocol::SettingValueKind::Integer => text
                .parse::<i64>()
                .map(serde_json::Value::from)
                .map_err(|_| "invalid integer"),
            fluxdown_protocol::SettingValueKind::Float => text
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
                .ok_or("invalid number"),
            fluxdown_protocol::SettingValueKind::String => Ok(serde_json::Value::String(text)),
            fluxdown_protocol::SettingValueKind::Boolean => return,
        };
        match value {
            Ok(serde_json::Value::String(value)) if PROXY_KEYS.contains(&key) => {
                let future = self.controller.set_daemon_raw(key, value);
                self.spawn_setting_update(future, cx);
            }
            Ok(value) => {
                let future = self.controller.set_value(key.to_owned(), value);
                self.spawn_setting_update(future, cx);
            }
            Err(error) => {
                self.last_error = Some(SharedString::from(error));
                cx.notify();
            }
        }
    }

    fn spawn_setting_update(&mut self, future: PortFuture<SettingsResult>, cx: &mut Context<Self>) {
        self.last_error = None;
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let _ = this.update(cx, |this, cx| {
                this.last_error = result.err().map(|error| {
                    let key = if error.code == fluxdown_protocol::ApplicationErrorCode::Conflict {
                        "localServiceConflict"
                    } else if error.code == fluxdown_protocol::ApplicationErrorCode::Unavailable {
                        "localServiceDisconnected"
                    } else {
                        "localServiceActionFailed"
                    };
                    SharedString::from(this.translator.read(cx).text(key).to_owned())
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn render_proxy_content(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        v_flex()
            .w_full()
            .gap_2()
            .child(
                card(cx)
                    .w_full()
                    .overflow_hidden()
                    .children(PROXY_KEYS.iter().map(|key| {
                        let field = self
                            .inputs
                            .get(key)
                            .map(|input| Input::new(input).small().into_any_element())
                            .unwrap_or_else(|| div().into_any_element());
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(tokens.colors.border)
                            .child(
                                div().text_sm().child(
                                    self.translator.read(cx).text(proxy_label(key)).to_owned(),
                                ),
                            )
                            .child(div().w_64().child(field))
                    })),
            )
    }

    pub(crate) fn render_api_content(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        let switches = [
            ("takeoverEnabled", "apiServiceTakeover"),
            ("jsonrpcEnabled", "apiServiceJsonrpc"),
            ("apiEnabled", "apiServiceApi"),
            ("mcpEnabled", "apiServiceMcp"),
            ("corsEnabled", "apiServiceCorsAllowAll"),
        ];
        v_flex()
            .w_full()
            .child(card(cx).w_full().children(switches.map(|(key, label)| {
                let checked = self.controller.gateway_bool(key);
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(tokens.colors.border)
                    .child(
                        div()
                            .text_sm()
                            .child(self.translator.read(cx).text(label).to_owned()),
                    )
                    .child(Switch::new(key).checked(checked).on_click(cx.listener(
                        move |this, value: &bool, _, cx| {
                            let future = this.controller.set_gateway_bool(key, *value);
                            this.spawn_setting_update(future, cx);
                        },
                    )))
            })))
    }

    pub(crate) fn render_doctor_content(&self, cx: &mut Context<Self>) -> Div {
        let description = self.translator.read(cx).text("doctorDesc").to_owned();
        let run_label = self.translator.read(cx).text("doctorRun").to_owned();
        let tokens = active_theme(cx).tokens();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.colors.muted_foreground)
                    .child(description),
            )
            .child(
                button("run-doctor", run_label, ButtonVariant::Primary, cx).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let future = this.controller.execute(SettingsCommand::RunDiagnostics);
                        this.spawn_setting_update(future, cx);
                    },
                )),
            )
    }

    pub(crate) fn render_about_content(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        card(cx).w_full().p_4().child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .child(self.strings.category_about.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(tokens.colors.muted_foreground)
                        .child(self.strings.category_about_desc.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(tokens.colors.muted_foreground)
                        .child(env!("CARGO_PKG_VERSION")),
                ),
        )
    }
}
