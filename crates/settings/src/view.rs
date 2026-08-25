use fluxdown_ui_components::sidebar_navigation_button;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, Context, Div, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
};
use gpui_component::{Icon, IconName, h_flex, scroll::ScrollableElement as _, v_flex};

use crate::strings::SettingsStrings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    General,
    Account,
    Appearance,
    Download,
    BitTorrent,
    Ed2k,
    Proxy,
    ApiService,
    Notify,
    Extensions,
    Doctor,
    About,
}

impl SettingsCategory {
    const ALL: [Self; 12] = [
        Self::General,
        Self::Account,
        Self::Appearance,
        Self::Download,
        Self::BitTorrent,
        Self::Ed2k,
        Self::Proxy,
        Self::ApiService,
        Self::Notify,
        Self::Extensions,
        Self::Doctor,
        Self::About,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Account => "settings-account",
            Self::Appearance => "settings-appearance",
            Self::Download => "settings-download",
            Self::BitTorrent => "settings-bt",
            Self::Ed2k => "settings-ed2k",
            Self::Proxy => "settings-proxy",
            Self::ApiService => "settings-api",
            Self::Notify => "settings-notify",
            Self::Extensions => "settings-extensions",
            Self::Doctor => "settings-doctor",
            Self::About => "settings-about",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::General | Self::ApiService | Self::Extensions => IconName::Settings2,
            Self::Account => IconName::User,
            Self::Appearance => IconName::Palette,
            Self::Download | Self::BitTorrent | Self::Ed2k => IconName::HardDrive,
            Self::Proxy => IconName::Globe,
            Self::Notify => IconName::Bell,
            Self::Doctor | Self::About => IconName::Info,
        }
    }
}

/// 设置能力的顶层页面。
pub struct SettingsView {
    pub(crate) translator: Entity<Translator>,
    pub(crate) strings: SettingsStrings,
    selected_category: SettingsCategory,
}

impl SettingsView {
    /// 创建设置页面，并订阅共享翻译状态。
    pub fn new(translator: Entity<Translator>, cx: &mut Context<Self>) -> Self {
        let strings = SettingsStrings::from_translator(translator.read(cx));
        cx.observe(&translator, |this, translator, cx| {
            this.strings = SettingsStrings::from_translator(translator.read(cx));
            cx.notify();
        })
        .detach();
        Self {
            translator,
            strings,
            selected_category: SettingsCategory::General,
        }
    }

    fn render_search_placeholder(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        h_flex()
            .h(px(32.))
            .w_full()
            .px(tokens.spacing.sm)
            .gap(tokens.spacing.sm)
            .items_center()
            .rounded(tokens.radius.md)
            .border_1()
            .border_color(tokens.colors.input)
            .bg(tokens.colors.background)
            .text_color(tokens.colors.muted_foreground)
            .text_size(tokens.typography.xs.size)
            .child(Icon::new(IconName::Search).size(px(13.)))
            .child(self.strings.search_hint.clone())
    }

    fn render_sidebar_item(
        &self,
        category: SettingsCategory,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_category == category;
        let tokens = active_theme(cx).tokens();
        let indicator = div().w(px(2.)).h(px(16.)).rounded_full().bg(tokens
            .colors
            .accent_foreground
            .opacity(if selected { 1. } else { 0. }));

        sidebar_navigation_button(
            category.id(),
            self.strings.category_label(category),
            Icon::new(category.icon()).size(px(14.)),
            indicator,
            selected,
            cx,
        )
        .h(px(36.))
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.selected_category != category {
                this.selected_category = category;
                cx.notify();
            }
        }))
        .into_any_element()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .size_full()
            .min_w_0()
            .p(tokens.spacing.sm)
            .gap(tokens.spacing.xs)
            .overflow_y_scrollbar()
            .bg(tokens.colors.surface)
            .child(self.render_search_placeholder(cx))
            .child(
                v_flex().gap(tokens.spacing.xxs).children(
                    SettingsCategory::ALL
                        .into_iter()
                        .map(|category| self.render_sidebar_item(category, cx)),
                ),
            )
    }

    fn render_placeholder_content(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        v_flex().w_full().max_w(px(560.)).child(
            fluxdown_ui_components::card(cx).p(tokens.spacing.xl).child(
                div()
                    .text_size(tokens.typography.sm.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.strings.category_description(self.selected_category)),
            ),
        )
    }

    fn render_selected_content(&self, cx: &mut Context<Self>) -> Div {
        match self.selected_category {
            SettingsCategory::General => self.render_general_content(cx),
            SettingsCategory::Appearance => h_flex()
                .items_stretch()
                .gap(active_theme(cx).tokens().spacing.lg)
                .child(self.render_theme_card(cx))
                .child(self.render_language_card(cx)),
            _ => self.render_placeholder_content(cx),
        }
    }

    fn page_title(&self) -> SharedString {
        self.strings.category_label(self.selected_category)
    }

    fn page_description(&self) -> SharedString {
        self.strings.category_description(self.selected_category)
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.colors.background)
            .child(
                div()
                    .w(px(184.))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(tokens.colors.border)
                    .child(self.render_sidebar(cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        h_flex()
                            .h(px(56.))
                            .flex_none()
                            .items_center()
                            .gap(tokens.spacing.md)
                            .px(tokens.spacing.xl)
                            .border_b_1()
                            .border_color(tokens.colors.border)
                            .bg(tokens.colors.surface)
                            .child(
                                div()
                                    .text_size(tokens.typography.lg.size)
                                    .font_weight(tokens.typography.lg.weight)
                                    .child(self.page_title()),
                            )
                            .child(
                                div()
                                    .text_size(tokens.typography.xs.size)
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(self.page_description()),
                            ),
                    )
                    .child(
                        div().flex_1().min_h_0().overflow_y_scrollbar().child(
                            div()
                                .w_full()
                                .p(tokens.spacing.xl)
                                .child(self.render_selected_content(cx)),
                        ),
                    ),
            )
    }
}
