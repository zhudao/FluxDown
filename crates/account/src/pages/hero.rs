//! 未登录 hero 卡片：accent 圆形图标 + 标题 + 说明 + 登录/注册按钮。

use std::sync::Arc;

use fluxdown_ui_components::{ButtonVariant, button, card};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, SemanticThemeTokens};
use gpui::{
    Context, Entity, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName, StyledExt as _, h_flex};

use crate::view::AccountView;
use crate::{AccountPort, dialogs, t};

pub(crate) fn render(
    translator: &Translator,
    translator_entity: &Entity<Translator>,
    tokens: &SemanticThemeTokens,
    port: Arc<dyn AccountPort>,
    disabled: bool,
    last_error: Option<SharedString>,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let title = t(translator, "accountLoginDialogTitle");
    let subtitle = t(translator, "accountHeroSubtitle");
    let login_label = t(translator, "accountLogin");
    let register_label = t(translator, "accountRegister");
    let login_translator = translator_entity.clone();
    let login_port = port.clone();
    let register_translator = translator_entity.clone();
    let register_port = port;

    card(cx)
        .w_full()
        .p(tokens.spacing.xl)
        .flex()
        .flex_col()
        .items_center()
        .child(
            div()
                .size(px(64.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens.radius.full)
                .bg(tokens.colors.accent)
                .child(
                    Icon::new(IconName::CircleUser)
                        .size(px(30.))
                        .text_color(tokens.colors.accent_foreground),
                ),
        )
        .child(
            div()
                .mt(tokens.spacing.lg)
                .text_size(px(16.))
                .font_semibold()
                .text_color(tokens.colors.foreground)
                .child(title),
        )
        .child(
            div()
                .mt(tokens.spacing.xs)
                .text_size(px(12.))
                .text_color(tokens.colors.muted_foreground)
                .text_center()
                .child(subtitle),
        )
        .child(
            h_flex()
                .mt(tokens.spacing.lg)
                .gap(tokens.spacing.sm)
                .child(
                    button(
                        "account-hero-login",
                        login_label,
                        ButtonVariant::Primary,
                        cx,
                    )
                    .h(CONTROL_HEIGHT)
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        dialogs::login::open(
                            login_translator.clone(),
                            login_port.clone(),
                            window,
                            cx,
                        );
                    }),
                )
                .child(
                    button(
                        "account-hero-register",
                        register_label,
                        ButtonVariant::Secondary,
                        cx,
                    )
                    .h(CONTROL_HEIGHT)
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        dialogs::register::open(
                            register_translator.clone(),
                            register_port.clone(),
                            window,
                            cx,
                        );
                    }),
                ),
        )
        .when_some(last_error, |this, error| {
            this.child(
                div()
                    .mt(tokens.spacing.md)
                    .text_size(px(11.5))
                    .text_color(tokens.colors.destructive)
                    .child(error),
            )
        })
}
