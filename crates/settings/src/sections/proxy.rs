//! 代理：模式、手动服务器、连通性测试、站点凭据。

use fluxdown_protocol::method;
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{App, ParentElement, SharedString, Styled, div};
use gpui_component::{IconName, h_flex};
use serde_json::json;

use super::{SectionContext, site_auth};
use crate::ui::{Control, SettingsPage, SettingsSection};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingsPage {
    let mode = ctx.store.read(cx).daemon_str("proxy_mode");
    let mut sections = vec![mode_section(ctx)];
    if matches!(mode.as_str(), "manual" | "auto") {
        sections.push(manual_section(ctx));
    }
    sections.push(site_auth::group(ctx, cx));
    SettingsPage::new(
        "proxy",
        ctx.t("settingsCatProxy"),
        ctx.t("settingsCatProxyDesc"),
        IconName::Globe,
    )
    .sections(sections)
}

fn mode_section(ctx: &SectionContext) -> SettingsSection {
    let options = vec![
        (SharedString::from("none"), ctx.t("proxyModeNone")),
        (SharedString::from("system"), ctx.t("proxyModeSystem")),
        (SharedString::from("manual"), ctx.t("proxyModeManual")),
        (SharedString::from("auto"), ctx.t("proxyModeAuto")),
    ];
    SettingsSection::new()
        .title(ctx.t("proxySettings"))
        .subtitle(ctx.t("proxyBtNote"))
        .row(ctx.item(
            "proxySettings",
            Some("proxySettingsDesc"),
            ctx.daemon_dropdown("proxy_mode", options),
        ))
}

fn manual_section(ctx: &SectionContext) -> SettingsSection {
    let types = vec![
        (SharedString::from("http"), SharedString::from("HTTP")),
        (SharedString::from("https"), SharedString::from("HTTPS")),
        (SharedString::from("socks4"), SharedString::from("SOCKS4")),
        (SharedString::from("socks5"), SharedString::from("SOCKS5")),
    ];
    SettingsSection::new()
        .title(ctx.t("proxyModeManual"))
        .subtitle(ctx.t("proxyModeManualDesc"))
        .row(ctx.item("proxyType", None, ctx.daemon_dropdown("proxy_type", types)))
        .row(ctx.item(
            "proxyHost",
            Some("proxyHostPlaceholder"),
            ctx.daemon_input("proxy_host"),
        ))
        .row(ctx.item(
            "proxyPort",
            Some("proxyPortPlaceholder"),
            ctx.daemon_input("proxy_port"),
        ))
        .row(ctx.item(
            "proxyUsername",
            Some("proxyUsernamePlaceholder"),
            ctx.daemon_input("proxy_username"),
        ))
        .row(ctx.item(
            "proxyPassword",
            Some("proxyPasswordPlaceholder"),
            ctx.daemon_input("proxy_password"),
        ))
        .row(ctx.item(
            "proxyNoList",
            Some("proxyNoListDesc"),
            ctx.daemon_input("proxy_no_list"),
        ))
        .row(ctx.item("proxyTestConnection", None, test_control(ctx)))
}

fn test_control(ctx: &SectionContext) -> Control {
    let store = ctx.store();
    let label = ctx.t("proxyTestConnection");
    let testing = ctx.t("proxyTesting");
    let translator = ctx.translator.clone();
    Control::custom(move |_disabled, _key, _window, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let busy = store.read(cx).is_busy("proxyTest");
        let result = store
            .read(cx)
            .transient("proxy_test_result")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let click_store = store.clone();
        h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(SharedString::from(result.unwrap_or_default())),
            )
            .child(
                button(
                    "proxy-test",
                    if busy { testing.clone() } else { label.clone() },
                    ButtonVariant::Secondary,
                    cx,
                )
                .h(CONTROL_HEIGHT)
                .disabled(busy)
                .on_click({
                    let translator = translator.clone();
                    move |_, _, cx| {
                        let translator = translator.clone();
                        click_store.update(cx, |store, cx| {
                            let params = json!({
                                "proxyType": store.daemon_str("proxy_type"),
                                "host": store.daemon_str("proxy_host"),
                                "port": store.daemon_str("proxy_port"),
                                "username": store.daemon_str("proxy_username"),
                                "password": store.daemon_str("proxy_password"),
                            });
                            store.call_with(
                                "proxyTest",
                                method::DAEMON_CONFIG_PROXY_TEST,
                                params,
                                cx,
                                move |store, result, cx| {
                                    let text = match result {
                                        Ok(value) => {
                                            let ms = value
                                                .get("latencyMs")
                                                .and_then(serde_json::Value::as_i64)
                                                .unwrap_or(0)
                                                .to_string();
                                            translator.text_with("proxyTestSuccess", &[("ms", &ms)])
                                        }
                                        Err(error) => translator.text_with(
                                            "proxyTestFailed",
                                            &[("error", &format!("{:?}", error.code))],
                                        ),
                                    };
                                    store.set_transient("proxy_test_result", json!(text), cx);
                                },
                            );
                        });
                    }
                }),
            )
    })
}
