use fluxdown_ui_components::{ButtonVariant, button, card};
use fluxdown_ui_theme::{active_theme, toggle_theme};
use gpui::{Context, Div, ParentElement, Styled, div};
use gpui_component::{h_flex, v_flex};

use crate::{component_locale, view::SettingsView};

impl SettingsView {
    pub(crate) fn render_theme_card(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        let next_mode = if active_theme(cx).mode().is_dark() {
            self.strings.theme_mode_light.clone()
        } else {
            self.strings.theme_mode_dark.clone()
        };

        card(cx).p(tokens.spacing.lg).flex_1().child(
            h_flex()
                .items_center()
                .justify_between()
                .gap(tokens.spacing.lg)
                .child(
                    v_flex()
                        .gap(tokens.spacing.xxs)
                        .child(
                            div()
                                .text_size(tokens.typography.md.size)
                                .font_weight(tokens.typography.md.weight)
                                .child(self.strings.theme_mode.clone()),
                        )
                        .child(
                            div()
                                .text_size(tokens.typography.sm.size)
                                .text_color(tokens.colors.muted_foreground)
                                .child(self.strings.theme_mode_desc.clone()),
                        ),
                )
                .child(
                    button("toggle-theme", next_mode, ButtonVariant::Primary, cx).on_click(
                        cx.listener(|_, _, window, cx| {
                            toggle_theme(window, cx);
                            cx.notify();
                        }),
                    ),
                ),
        )
    }

    pub(crate) fn render_language_card(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        let next_locale = if self.translator.read(cx).locale() == "zh" {
            self.strings.language_english.clone()
        } else {
            self.strings.language_chinese.clone()
        };
        let translator = self.translator.clone();

        card(cx).p(tokens.spacing.lg).flex_1().child(
            h_flex()
                .items_center()
                .justify_between()
                .gap(tokens.spacing.lg)
                .child(
                    v_flex()
                        .gap(tokens.spacing.xxs)
                        .child(
                            div()
                                .text_size(tokens.typography.md.size)
                                .font_weight(tokens.typography.md.weight)
                                .child(self.strings.language.clone()),
                        )
                        .child(
                            div()
                                .text_size(tokens.typography.sm.size)
                                .text_color(tokens.colors.muted_foreground)
                                .child(self.strings.language_desc.clone()),
                        ),
                )
                .child(
                    button("toggle-language", next_locale, ButtonVariant::Secondary, cx).on_click(
                        move |_, _, cx| {
                            translator.update(cx, |translator, cx| {
                                let next = if translator.locale() == "zh" {
                                    "en"
                                } else {
                                    "zh"
                                };
                                if translator.set_locale(next) {
                                    gpui_component::set_locale(component_locale(next));
                                    cx.notify();
                                }
                            });
                        },
                    ),
                ),
        )
    }
}
