//! 服务器地址分组（仅调试构建）：FluxCloud 地址输入框 + 恢复默认按钮，
//! 与 Flutter `_ServerAddressCard` 对齐；失焦/回车提交，成功与否以通知提示。

use fluxdown_protocol::CloudEndpointDto;
use fluxdown_ui_components::{ButtonVariant, button, card};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, SemanticThemeTokens};
use gpui::{ClickEvent, Context, Entity, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{
    Sizable as _, Size, StyledExt as _, h_flex,
    input::{Input, InputState},
};

use crate::t;
use crate::view::AccountView;

pub(crate) fn render(
    translator: &Translator,
    tokens: &SemanticThemeTokens,
    endpoint: &CloudEndpointDto,
    input: &Entity<InputState>,
    disabled: bool,
    cx: &mut Context<AccountView>,
) -> impl IntoElement {
    let heading = t(translator, "accountServerAddress");
    let desc = t(translator, "accountServerAddressDesc");
    let reset_label = t(translator, "accountServerAddressReset");
    let is_custom = endpoint.base_url != endpoint.default_base_url;

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
            card(cx).w_full().child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .px(tokens.spacing.md)
                    .py(tokens.spacing.sm)
                    .child(
                        Input::new(input)
                            .with_size(Size::Medium)
                            .flex_1()
                            .min_w_0()
                            .disabled(disabled),
                    )
                    .child(
                        button(
                            "account-server-address-reset",
                            reset_label,
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .h(CONTROL_HEIGHT)
                        .disabled(disabled || !is_custom)
                        .on_click(cx.listener(
                            |view, _: &ClickEvent, window, cx| {
                                view.set_endpoint(String::new(), window, cx);
                            },
                        )),
                    ),
            ),
        )
}
