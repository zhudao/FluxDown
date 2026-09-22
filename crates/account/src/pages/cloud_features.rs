//! 云功能分组：配置同步开关（真实读写 `agent.sync.*`）+ 多设备协同状态行
//! （无独立开关，登录后自动可用，展示当前在线设备数）。

use fluxdown_protocol::{CloudDevice, SyncStatusDto};
use fluxdown_ui_components::card;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::SemanticThemeTokens;
use gpui::{
    Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, IconName, StyledExt as _, h_flex, switch::Switch, tooltip::Tooltip,
    v_flex,
};

use crate::view::AccountView;
use crate::{AccountCommand, t, t_with};

pub(crate) fn render(
    translator: &Translator,
    tokens: &SemanticThemeTokens,
    logged_in: bool,
    sync: &SyncStatusDto,
    devices: &[CloudDevice],
    disabled: bool,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let heading = t(translator, "accountGroupCloudFeatures");
    let desc = t(translator, "accountCloudFeaturesDesc");

    div()
        .flex()
        .flex_col()
        .gap(tokens.spacing.sm)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(tokens.spacing.xxs)
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_semibold()
                        .text_color(tokens.colors.foreground)
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(tokens.colors.muted_foreground)
                        .child(desc),
                ),
        )
        .child(
            card(cx)
                .w_full()
                .flex()
                .flex_col()
                .child(config_sync_row(
                    translator, tokens, logged_in, sync, disabled, cx,
                ))
                .child(
                    div()
                        .w_full()
                        .border_t_1()
                        .border_color(tokens.colors.border.opacity(0.5)),
                )
                .child(multi_device_row(translator, tokens, logged_in, devices)),
        )
}

fn row_icon(tokens: &SemanticThemeTokens, icon: IconName) -> impl IntoElement {
    div()
        .size(px(30.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens.radius.md)
        .bg(tokens.colors.muted)
        .child(
            Icon::new(icon)
                .size(px(15.))
                .text_color(tokens.colors.muted_foreground),
        )
}

fn config_sync_row(
    translator: &Translator,
    tokens: &SemanticThemeTokens,
    logged_in: bool,
    sync: &SyncStatusDto,
    disabled: bool,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let title = t(translator, "cloudSyncTitle");
    let active = logged_in && sync.enabled;
    let subtitle: SharedString = if !active {
        t(translator, "cloudSyncDesc")
    } else if let Some(error) = sync.last_error.as_deref().filter(|error| !error.is_empty()) {
        t_with(translator, "cloudSyncStatusError", &[("reason", error)])
    } else if !sync.dirty_keys.is_empty() {
        t(translator, "cloudSyncStatusSyncing")
    } else {
        t(translator, "cloudSyncStatusSynced")
    };
    let login_required_label = t(translator, "cloudSyncLoginRequired");
    let checked = sync.enabled;
    let switch_disabled = disabled || !logged_in;

    let switch = Switch::new("account-sync-enable")
        .checked(checked)
        .disabled(switch_disabled)
        .on_click(cx.listener(move |view, checked: &bool, _, cx| {
            let method = if *checked {
                fluxdown_protocol::method::AGENT_SYNC_ENABLE
            } else {
                fluxdown_protocol::method::AGENT_SYNC_DISABLE
            };
            let future = view.controller.port().execute(AccountCommand::Sync {
                method,
                params: serde_json::json!({}),
            });
            view.spawn_action(future, cx);
        }));

    h_flex()
        .w_full()
        .items_center()
        .gap(tokens.spacing.md)
        .px(tokens.spacing.md)
        .py(tokens.spacing.sm)
        .child(row_icon(tokens, IconName::Redo2))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(tokens.spacing.xxs)
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tokens.colors.foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(tokens.colors.muted_foreground)
                        .truncate()
                        .child(subtitle),
                ),
        )
        .child(if logged_in {
            switch.into_any_element()
        } else {
            div()
                .id("account-sync-login-required")
                .tooltip(move |window, cx| {
                    Tooltip::new(login_required_label.clone()).build(window, cx)
                })
                .child(switch)
                .into_any_element()
        })
}

fn multi_device_row(
    translator: &Translator,
    tokens: &SemanticThemeTokens,
    logged_in: bool,
    devices: &[CloudDevice],
) -> impl IntoElement {
    let title = t(translator, "multiDeviceTitle");
    let desc = t(translator, "multiDeviceDesc");
    let online_count = devices.iter().filter(|device| device.is_online).count();

    h_flex()
        .w_full()
        .items_center()
        .gap(tokens.spacing.md)
        .px(tokens.spacing.md)
        .py(tokens.spacing.sm)
        .child(row_icon(tokens, IconName::Network))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(tokens.spacing.xxs)
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tokens.colors.foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(tokens.colors.muted_foreground)
                        .child(desc),
                ),
        )
        .when(logged_in, |this| {
            this.child(
                div()
                    .px(tokens.spacing.sm)
                    .py(tokens.spacing.xxs)
                    .rounded(tokens.radius.full)
                    .bg(tokens.colors.muted)
                    .text_size(px(10.5))
                    .text_color(tokens.colors.muted_foreground)
                    .child(t_with(
                        translator,
                        "devicesOnlineCount",
                        &[("count", &online_count.to_string())],
                    )),
            )
        })
}
