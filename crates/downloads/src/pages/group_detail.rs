//! 任务组详情：独立窗口，概览 + 成员列表（复用 [`DownloadTableDelegate`]）。
//!
//! 弹出窗口没有主窗口 controller 可借，自建一份 [`DownloadsController`] 并经
//! [`crate::session::attach`] 订阅会话即可获得完整任务/队列/组数据。

use std::{rc::Rc, sync::Arc};

use fluxdown_protocol::{AgentSnapshot, ServiceEvent};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, Size, h_flex,
    table::{DataTable, TableState},
    v_flex,
};

use crate::{
    components::task_table::{DownloadTableDelegate, TableFilter},
    controller::{DownloadsController, DownloadsPort, GroupSummary},
    strings::DownloadStrings,
};

const MEMBER_ROW_HEIGHT: f32 = 32.;

pub struct GroupDetailView {
    group_id: String,
    controller: DownloadsController,
    translator: Entity<Translator>,
    strings: DownloadStrings,
    table_state: Entity<TableState<DownloadTableDelegate>>,
    last_error: Option<SharedString>,
}

impl GroupDetailView {
    pub fn new(
        translator: Entity<Translator>,
        group_id: String,
        port: Arc<dyn DownloadsPort>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let strings = DownloadStrings::from_translator(translator.read(cx));
        let controller = DownloadsController::new(port);
        let store = Rc::clone(controller.store());
        let filter_group_id = group_id.clone();
        let table_state = cx.new(|cx| {
            let mut delegate = DownloadTableDelegate::new(strings.clone(), store);
            delegate.set_filter(TableFilter::Group(filter_group_id));
            TableState::new(delegate, window, cx)
                .row_selectable(false)
                .col_selectable(false)
        });

        cx.observe(&translator, |this, translator, cx| {
            this.strings = DownloadStrings::from_translator(translator.read(cx));
            this.table_state.update(cx, |table, cx| {
                table.delegate_mut().set_strings(this.strings.clone());
                table.delegate_mut().refresh_view();
                table.refresh(cx);
            });
            cx.notify();
        })
        .detach();

        Self {
            group_id,
            controller,
            translator,
            strings,
            table_state,
            last_error: None,
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        self.controller.replace_snapshot(snapshot);
        self.last_error = None;
        self.refresh_table(cx);
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        let changed = self.controller.apply_event(event);
        if changed {
            self.refresh_table(cx);
        } else {
            cx.notify();
        }
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.controller.mark_stale();
        self.last_error = Some(self.strings.disconnected.clone());
        cx.notify();
    }

    fn refresh_table(&mut self, cx: &mut Context<Self>) {
        let queues: Vec<(String, String)> = self
            .controller
            .queues()
            .iter()
            .map(|queue| (queue.queue_id.clone(), queue.name.clone()))
            .collect();
        self.table_state.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.set_queue_names(queues);
            if delegate.refresh_view() {
                table.refresh(cx);
            }
        });
        cx.notify();
    }

    fn summary(&mut self) -> GroupSummary {
        self.controller
            .group_summaries()
            .iter()
            .find(|summary| summary.id == self.group_id)
            .cloned()
            .unwrap_or_default()
    }

    fn t(&self, cx: &App, key: &str) -> SharedString {
        SharedString::from(self.translator.read(cx).text(key).to_owned())
    }

    fn render_overview(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let summary = self.summary();
        let status = if summary.total == 0 {
            self.t(cx, "groupDetailNoMembers")
        } else {
            SharedString::from(format!(
                "{}/{} · {:.0}%",
                summary.completed,
                summary.total,
                summary.progress * 100.0
            ))
        };
        let row = |label: SharedString, value: SharedString| {
            h_flex()
                .justify_between()
                .items_start()
                .gap(tokens.spacing.md)
                .py(tokens.spacing.xxs)
                .child(
                    div()
                        .flex_none()
                        .text_size(tokens.typography.xs.size)
                        .text_color(tokens.colors.muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_size(tokens.typography.xs.size)
                        .text_right()
                        .child(value),
                )
        };
        v_flex()
            .gap(tokens.spacing.xs)
            .p(tokens.spacing.md)
            .border_b_1()
            .border_color(tokens.colors.border)
            .child(
                div()
                    .text_sm()
                    .font_weight(tokens.typography.sm.weight)
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(if summary.name.is_empty() {
                        self.group_id.clone()
                    } else {
                        summary.name.clone()
                    })),
            )
            .child(row(self.t(cx, "groupDetailSubtitle"), status))
            .child(row(
                self.t(cx, "groupDetailSource"),
                SharedString::from(summary.origin_url.clone()),
            ))
            .child(row(
                self.t(cx, "groupDetailSaveDir"),
                SharedString::from(summary.save_dir.clone()),
            ))
    }

    fn render_members(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let table_state = self.table_state.clone();
        div()
            .id("group-detail-members-table")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .left_0()
                    .child(
                        DataTable::new(&table_state)
                            .with_size(Size::Size(px(MEMBER_ROW_HEIGHT)))
                            .stripe(false)
                            .bordered(false)
                            .scrollbar_visible(true, true),
                    ),
            )
            .into_any_element()
    }
}

impl gpui::Render for GroupDetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .size_full()
            .min_h_0()
            .bg(tokens.colors.surface)
            .child(self.render_overview(cx))
            .when_some(self.last_error.clone(), |this, error| {
                this.child(
                    div()
                        .px(tokens.spacing.md)
                        .py(tokens.spacing.xs)
                        .text_size(tokens.typography.xs.size)
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                div()
                    .px(tokens.spacing.md)
                    .py(tokens.spacing.sm)
                    .text_size(tokens.typography.xs.size)
                    .font_weight(tokens.typography.sm.weight)
                    .child(self.t(cx, "groupDetailMembersTab")),
            )
            .child(self.render_members(cx))
    }
}
