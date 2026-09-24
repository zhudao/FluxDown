//! 活动栏 Webhook 页面；复用共享配置存储，不挂载到设置分类或下载页。

use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    Context, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, px,
};
use gpui_component::{scroll::ScrollableElement as _, v_flex};

use crate::sections::{SectionContext, webhook};
use crate::store::{SettingsErrorKind, SettingsStore};

/// 主窗口中的独立 Webhook 管理页面。
pub struct WebhookView {
    store: Entity<SettingsStore>,
    translator: Entity<Translator>,
}

impl WebhookView {
    /// 复用 app 持有的配置存储；关闭设置窗口不影响事件订阅和待保存的修改。
    pub fn new(
        translator: Entity<Translator>,
        store: Entity<SettingsStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store, translator }
    }

    fn feedback(&self, cx: &Context<Self>) -> Option<SharedString> {
        let store = self.store.read(cx);
        let translator = self.translator.read(cx);
        if !store.daemon_connected() {
            return Some(
                translator
                    .text(SettingsErrorKind::Disconnected.i18n_key())
                    .to_owned()
                    .into(),
            );
        }
        store.last_error().map(|error| {
            let mut text = translator.text(error.kind.i18n_key()).to_owned();
            if error.kind == SettingsErrorKind::InvalidArgument && !error.detail.is_empty() {
                text.push_str(": ");
                text.push_str(&error.detail);
            }
            text.into()
        })
    }
}

impl Render for WebhookView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let translator = self.translator.read(cx).clone();
        let ctx = SectionContext {
            store: &self.store,
            translator: &translator,
            translator_entity: &self.translator,
        };
        let sections = [
            webhook::endpoints_group(&ctx, cx),
            webhook::delivery_log_group(&ctx, cx),
        ];
        let feedback = self.feedback(cx);

        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.colors.surface)
            .text_color(tokens.colors.foreground)
            .child(
                v_flex()
                    .flex_none()
                    .px(px(24.))
                    .py(px(16.))
                    .gap(px(4.))
                    .border_b_1()
                    .border_color(tokens.colors.border)
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(ctx.t("webhookNavTitle")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(tokens.colors.muted_foreground)
                            .child(ctx.t("webhookEmptyDesc")),
                    ),
            )
            .children(feedback.map(|text| {
                div()
                    .px(px(24.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .text_color(tokens.colors.destructive)
                    .child(text)
            }))
            .child(
                v_flex()
                    .id("webhook-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .p(px(24.))
                    .gap(px(20.))
                    .overflow_y_scrollbar()
                    .children(sections.iter().enumerate().map(|(index, section)| {
                        section
                            .render("webhooks", index, window, cx)
                            .into_any_element()
                    })),
            )
    }
}
