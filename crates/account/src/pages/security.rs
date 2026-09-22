//! 账号与安全分组：分组标题 + 邮箱只读行。
//!
//! Flutter 侧可点邮箱打开修改邮箱对话框（`/me/email` 多步验证）；本能力尚未
//! 暴露对应端口方法，故此处保持只读展示，见交付报告的协议缺口清单。

use fluxdown_protocol::AgentSessionDto;
use fluxdown_ui_components::card;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::SemanticThemeTokens;
use gpui::{Context, FontWeight, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{StyledExt as _, h_flex};

use crate::t;
use crate::view::AccountView;

pub(crate) fn render(
    translator: &Translator,
    tokens: &SemanticThemeTokens,
    session: &AgentSessionDto,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let heading = t(translator, "accountSecurityGroup");
    let desc = t(translator, "accountSecurityGroupDesc");
    let email_label = t(translator, "accountEmailPlaceholder");

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
            card(cx).w_full().p(tokens.spacing.md).child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tokens.colors.foreground)
                            .child(email_label),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(tokens.colors.muted_foreground)
                            .child(session.user.email.clone()),
                    ),
            ),
        )
}
