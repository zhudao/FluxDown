//! 设置窗口内容：左侧分类导航 + 搜索，右侧「标题 + 描述 + 子 Tab」头部与白色内容区。
//!
//! 布局与 Flutter 桌面端 `lib/src/pages/settings_page.dart` 对齐：分类同序、
//! 子 Tab 同分区、宽视口双列分组卡片。

use std::collections::HashMap;

use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyView, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    IconName, Sizable as _, Size,
    input::{Input, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::sections::{
    self, SectionContext, about, api, appearance, bt, doctor, download, ed2k, general, notify,
    proxy,
};
use crate::store::{SettingsErrorKind, SettingsStore};
use crate::ui::SettingsPage;

const SIDEBAR_WIDTH: f32 = 190.;
/// 内容区左右留白（Flutter 内容区 40 / 36）。
const CONTENT_PADDING_LEFT: f32 = 28.;
const CONTENT_PADDING_RIGHT: f32 = 24.;

/// app 注入的外部内容槽：账户页与扩展页由对应 capability 提供。
#[derive(Default)]
pub struct SettingsContentSlots {
    pub account: Option<AnyView>,
    pub extensions: Option<AnyView>,
}

/// 设置能力的顶层页面。
pub struct SettingsView {
    store: Entity<SettingsStore>,
    translator: Entity<Translator>,
    slots: SettingsContentSlots,
    search: Entity<InputState>,
    /// 当前选中分类的 key。
    selected: SharedString,
    /// 每个分类会话内记住的子 Tab id。
    tab_by_page: HashMap<SharedString, &'static str>,
}

impl SettingsView {
    /// 创建设置页面，并订阅共享翻译状态与设置存储。
    ///
    /// `store` 由 app 持有并跨窗口复用：设置窗口关闭后防抖中的写回仍会完成，
    /// 快照/事件也持续进入同一存储。
    pub fn new(
        translator: Entity<Translator>,
        store: Entity<SettingsStore>,
        slots: SettingsContentSlots,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let placeholder = translator.read(cx).text("settingsSearchHint").to_owned();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            store,
            translator,
            slots,
            search,
            selected: SharedString::from("general"),
            tab_by_page: HashMap::new(),
        }
    }

    #[must_use]
    pub fn store(&self) -> Entity<SettingsStore> {
        self.store.clone()
    }

    fn feedback(&self, cx: &Context<Self>) -> Option<(SharedString, bool)> {
        let store = self.store.read(cx);
        let translator = self.translator.read(cx);
        if let Some(error) = store.last_error() {
            let mut text = translator.text(error.kind.i18n_key()).to_owned();
            if error.kind == SettingsErrorKind::InvalidArgument && !error.detail.is_empty() {
                text.push_str(": ");
                text.push_str(&error.detail);
            }
            return Some((SharedString::from(text), true));
        }
        store
            .last_notice()
            .map(|key| (SharedString::from(translator.text(key).to_owned()), false))
    }

    fn pages(&self, cx: &mut Context<Self>) -> Vec<SettingsPage> {
        let translator = self.translator.read(cx).clone();
        let ctx = SectionContext {
            store: &self.store,
            translator: &translator,
            translator_entity: &self.translator,
        };
        vec![
            general::page(&ctx, cx),
            sections::slot_page(
                &ctx,
                "account",
                "settingsCatAccount",
                "settingsCatAccountDesc",
                IconName::User,
                self.slots.account.clone(),
            ),
            appearance::page(&ctx, cx),
            download::page(&ctx, cx),
            bt::page(&ctx, cx),
            ed2k::page(&ctx, cx),
            proxy::page(&ctx, cx),
            api::page(&ctx, cx),
            notify::page(&ctx, cx),
            sections::slot_page(
                &ctx,
                "extensions",
                "settingsCatExtensions",
                "settingsCatExtensionsDesc",
                IconName::Settings2,
                self.slots.extensions.clone(),
            ),
            doctor::page(&ctx, cx),
            about::page(&ctx, cx),
        ]
    }

    fn render_nav_item(&self, page: &SettingsPage, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let selected = self.selected == page.title || self.selected == page.key;
        let key = SharedString::from(page.key);
        let foreground = if selected {
            colors.accent_foreground
        } else {
            colors.foreground
        };

        div()
            .id(gpui::ElementId::from(SharedString::from(format!(
                "settings-nav-{}",
                page.key
            ))))
            .w_full()
            .mb(px(2.))
            .px(px(10.))
            .py(px(7.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .rounded(px(6.))
            .when(selected, |this| this.bg(colors.accent))
            .when(!selected, |this| {
                this.hover(|style| style.bg(colors.muted.opacity(0.7)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.selected != key {
                    this.selected = key.clone();
                    cx.notify();
                }
            }))
            .child(page.nav_icon().size(px(15.)).text_color(if selected {
                colors.accent_foreground
            } else {
                colors.muted_foreground
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(foreground)
                    .child(page.title.clone()),
            )
            .when(selected, |this| {
                this.child(
                    div()
                        .flex_none()
                        .w(px(3.))
                        .h(px(14.))
                        .rounded(px(2.))
                        .bg(colors.accent_foreground),
                )
            })
    }

    fn render_sidebar(&self, pages: &[SettingsPage], cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let items: Vec<_> = pages
            .iter()
            .map(|page| self.render_nav_item(page, cx).into_any_element())
            .collect();

        v_flex()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .min_h_0()
            .px(px(8.))
            .py(px(12.))
            .bg(colors.background)
            .border_r_1()
            .border_color(colors.border.opacity(0.8))
            .child(
                div().w_full().pb(px(10.)).child(
                    Input::new(&self.search).with_size(Size::Medium).prefix(
                        gpui_component::Icon::new(IconName::Search)
                            .size(px(13.))
                            .text_color(colors.muted_foreground),
                    ),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .children(items)
                    .overflow_y_scrollbar(),
            )
    }

    fn active_tab_id(&self, page: &SettingsPage) -> &'static str {
        let tabs = page.visible_tabs();
        if tabs.is_empty() {
            return "";
        }
        let saved = self.tab_by_page.get(&SharedString::from(page.key)).copied();
        match saved {
            Some(id) if tabs.iter().any(|tab| tab.id == id) => id,
            _ => tabs.first().map_or("", |tab| tab.id),
        }
    }

    fn render_tab_button(
        &self,
        page_key: &'static str,
        id: &'static str,
        label: SharedString,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let key = SharedString::from(page_key);

        div()
            .id(gpui::ElementId::from(SharedString::from(format!(
                "settings-tab-{page_key}-{id}"
            ))))
            .mr(px(18.))
            .pt(px(4.))
            .pb(px(8.))
            .px(px(2.))
            .cursor_pointer()
            .border_b_2()
            .border_color(if selected {
                colors.accent_foreground
            } else {
                colors.accent_foreground.opacity(0.)
            })
            .text_size(px(13.))
            .font_weight(if selected {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .text_color(if selected {
                colors.foreground
            } else {
                colors.muted_foreground
            })
            .when(!selected, |this| {
                this.hover(|style| style.text_color(colors.foreground))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tab_by_page.insert(key.clone(), id);
                cx.notify();
            }))
            .child(label)
    }

    fn render_content(
        &self,
        page: &SettingsPage,
        content_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let tab_id = self.active_tab_id(page);
        let tabs: Vec<_> = page
            .visible_tabs()
            .iter()
            .map(|tab| {
                self.render_tab_button(page.key, tab.id, tab.label.clone(), tab.id == tab_id, cx)
                    .into_any_element()
            })
            .collect();
        let has_tabs = !tabs.is_empty();

        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(colors.surface)
            .child(
                v_flex()
                    .w_full()
                    .flex_none()
                    .px(px(CONTENT_PADDING_LEFT))
                    .pt(px(16.))
                    .border_b_1()
                    .border_color(colors.border.opacity(0.5))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_baseline()
                            .gap(px(10.))
                            .pb(if has_tabs { px(8.) } else { px(12.) })
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.foreground)
                                    .child(page.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(colors.muted_foreground)
                                    .child(page.description.clone()),
                            ),
                    )
                    .when(has_tabs, |this| {
                        this.child(div().w_full().flex().items_center().children(tabs))
                    }),
            )
            .child(
                v_flex()
                    .id(gpui::ElementId::from(SharedString::from(format!(
                        "settings-body-{}-{tab_id}",
                        page.key
                    ))))
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .pl(px(CONTENT_PADDING_LEFT))
                    .pr(px(CONTENT_PADDING_RIGHT))
                    .pt(px(20.))
                    .pb(px(24.))
                    .overflow_y_scrollbar()
                    .child(page.render_tab(tab_id, content_width, window, cx)),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let pages: Vec<SettingsPage> = self
            .pages(cx)
            .into_iter()
            .filter_map(|page| page.filtered(&query))
            .collect();
        if !pages.iter().any(|page| self.selected == page.key)
            && let Some(first) = pages.first()
        {
            self.selected = SharedString::from(first.key);
        }
        // 内容区可用宽度：窗口宽 - 侧栏 - 左右留白（列数判定用，不参与布局）。
        let content_width = f32::from(window.viewport_size().width)
            - SIDEBAR_WIDTH
            - CONTENT_PADDING_LEFT
            - CONTENT_PADDING_RIGHT;
        let feedback = self.feedback(cx);
        let active = pages
            .iter()
            .find(|page| self.selected == page.key)
            .or_else(|| pages.first());

        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.colors.surface)
            .when_some(feedback, |this, (text, is_error)| {
                this.child(
                    div()
                        .w_full()
                        .px(px(CONTENT_PADDING_LEFT))
                        .py(tokens.spacing.sm)
                        .text_size(px(12.))
                        .text_color(if is_error {
                            tokens.colors.destructive
                        } else {
                            tokens.colors.muted_foreground
                        })
                        .child(text),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_stretch()
                    .child(self.render_sidebar(&pages, cx))
                    .children(
                        active.map(|page| self.render_content(page, content_width, window, cx)),
                    ),
            )
    }
}
