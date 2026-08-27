use std::collections::BTreeMap;
use std::sync::Arc;

use fluxdown_ui_components::sidebar_navigation_button;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, AnyView, AppContext as _, Context, Div, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Icon, IconName, h_flex,
    input::{InputEvent, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::controller::{SettingsController, SettingsPort};
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

#[derive(Default)]
pub struct SettingsContentSlots {
    pub account: Option<AnyView>,
    pub extensions: Option<AnyView>,
}

/// 设置能力的顶层页面。
pub struct SettingsView {
    pub(crate) controller: SettingsController,
    pub(crate) translator: Entity<Translator>,
    pub(crate) strings: SettingsStrings,
    selected_category: SettingsCategory,
    slots: SettingsContentSlots,
    pub(crate) inputs: BTreeMap<&'static str, Entity<InputState>>,
    pub(crate) last_error: Option<SharedString>,
}

impl SettingsView {
    /// 创建设置页面，并订阅共享翻译状态。
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn SettingsPort>,
        slots: SettingsContentSlots,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let strings = SettingsStrings::from_translator(translator.read(cx));
        cx.observe(&translator, |this, translator, cx| {
            this.strings = SettingsStrings::from_translator(translator.read(cx));
            cx.notify();
        })
        .detach();
        let mut inputs = BTreeMap::new();
        for spec in fluxdown_protocol::SYNC_SETTING_SPECS {
            if fluxdown_protocol::setting_value_kind(spec.key)
                == fluxdown_protocol::SettingValueKind::Boolean
            {
                continue;
            }
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(spec.key));
            let key = spec.key;
            cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.commit_input(key, input, window, cx);
                    }
                },
            )
            .detach();
            inputs.insert(spec.key, input);
        }
        for key in crate::sections::catalog::PROXY_KEYS {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(*key));
            let key = *key;
            cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.commit_input(key, input, window, cx);
                    }
                },
            )
            .detach();
            inputs.insert(key, input);
        }
        Self {
            controller: SettingsController::new(port),
            translator,
            strings,
            slots,
            inputs,
            last_error: None,
            selected_category: SettingsCategory::General,
        }
    }

    pub fn replace_snapshot(
        &mut self,
        snapshot: &fluxdown_protocol::AgentSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.controller.replace_snapshot(snapshot);
        self.last_error = None;
        cx.notify();
    }

    pub fn replace_snapshot_in_window(
        &mut self,
        snapshot: &fluxdown_protocol::AgentSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_error = None;
        self.controller.replace_snapshot(snapshot);
        for (key, input) in &self.inputs {
            let value = self
                .controller
                .value(key)
                .map(|value| match value {
                    serde_json::Value::String(value) => value,
                    value => value.to_string(),
                })
                .or_else(|| self.controller.daemon_raw(key).map(str::to_owned))
                .unwrap_or_default();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        cx.notify();
    }

    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent, cx: &mut Context<Self>) {
        self.controller.apply_event(event);
        cx.notify();
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.controller.mark_stale();
        self.last_error = Some(SharedString::from(
            self.translator
                .read(cx)
                .text("localServiceDisconnected")
                .to_owned(),
        ));
        cx.notify();
    }

    fn render_search_field(&self, cx: &mut Context<Self>) -> Div {
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
            .child(self.render_search_field(cx))
            .child(
                v_flex().gap(tokens.spacing.xxs).children(
                    SettingsCategory::ALL
                        .into_iter()
                        .map(|category| self.render_sidebar_item(category, cx)),
                ),
            )
    }

    fn render_category_summary(&self, cx: &mut Context<Self>) -> Div {
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

    fn render_selected_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.selected_category {
            SettingsCategory::General => self.render_general_content(cx).into_any_element(),
            SettingsCategory::Appearance => v_flex()
                .w_full()
                .gap(active_theme(cx).tokens().spacing.lg)
                .child(
                    h_flex()
                        .items_stretch()
                        .gap(active_theme(cx).tokens().spacing.lg)
                        .child(self.render_theme_card(cx))
                        .child(self.render_language_card(cx)),
                )
                .child(self.render_catalog_content(&["appearance."], cx))
                .into_any_element(),
            SettingsCategory::Download => self
                .render_catalog_content(&["download."], cx)
                .into_any_element(),
            SettingsCategory::BitTorrent => {
                self.render_catalog_content(&["bt."], cx).into_any_element()
            }
            SettingsCategory::Ed2k => self
                .render_catalog_content(&["ed2k."], cx)
                .into_any_element(),
            SettingsCategory::Notify => self
                .render_catalog_content(
                    &[
                        "download.notify_on_complete",
                        "download.silent_download",
                        "download.keep_awake",
                    ],
                    cx,
                )
                .into_any_element(),
            SettingsCategory::Proxy => self.render_proxy_content(cx).into_any_element(),
            SettingsCategory::ApiService => self.render_api_content(cx).into_any_element(),
            SettingsCategory::Doctor => self.render_doctor_content(cx).into_any_element(),
            SettingsCategory::About => self.render_about_content(cx).into_any_element(),
            SettingsCategory::Account => self.slots.account.clone().map_or_else(
                || self.render_category_summary(cx).into_any_element(),
                |view| view.into_any_element(),
            ),
            SettingsCategory::Extensions => self.slots.extensions.clone().map_or_else(
                || self.render_category_summary(cx).into_any_element(),
                |view| view.into_any_element(),
            ),
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
                    .when_some(self.last_error.clone(), |this, error| {
                        this.child(
                            div()
                                .w_full()
                                .px(tokens.spacing.xl)
                                .py(tokens.spacing.sm)
                                .text_size(tokens.typography.sm.size)
                                .text_color(tokens.colors.destructive)
                                .child(error),
                        )
                    })
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
