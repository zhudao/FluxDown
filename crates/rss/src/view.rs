//! RSS 工作区：订阅列表、条目流和批量操作。

use crate::{RSS_ICON_PATH, RssController, RssPort, editor};
use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, RssItemDto, RssSourceDto, ServiceEvent, WsServerMsg,
    method,
};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div, prelude::FluentBuilder as _, px, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, FocusableExt as _, Icon, IconName, Sizable as _, Size,
    WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct RssView {
    translator: Entity<Translator>,
    controller: RssController,
    search: Entity<InputState>,
    oldest_first: bool,
    last_error: Option<String>,
    feedback: Option<String>,
    load_error: bool,
}

impl RssView {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn RssPort>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(translator.read(cx).text("rssSearchHint"))
        });
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            translator,
            controller: RssController::new(port),
            search,
            oldest_first: false,
            last_error: None,
            feedback: None,
            load_error: false,
        }
    }

    fn t(&self, key: &str, cx: &App) -> SharedString {
        SharedString::from(self.translator.read(cx).text(key).to_owned())
    }
    fn with(&self, key: &str, args: &[(&str, &str)], cx: &App) -> String {
        self.translator.read(cx).text_with(key, args)
    }
    fn fail(&mut self, cx: &mut Context<Self>) {
        self.last_error = Some(self.t("localServiceActionFailed", cx).to_string());
        cx.notify();
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        self.controller.replace_snapshot(snapshot);
        self.last_error = None;
        self.load_error = false;
        self.fetch_items(cx);
        cx.notify();
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        let before = self.controller.selected_source.clone();
        let changed = matches!(
            event,
            ServiceEvent::Agent(
                AgentEvent::DaemonSnapshotReplaced(_)
                    | AgentEvent::Daemon(DaemonEvent::SnapshotReplaced(_))
                    | AgentEvent::DaemonConnectionChanged(true)
                    | AgentEvent::Daemon(DaemonEvent::RssChanged { .. })
            )
        );
        let item_event = match event {
            ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::Engine(
                WsServerMsg::RssItemsChanged {
                    source_id, items, ..
                },
            ))) if before.as_deref() == Some(source_id) => {
                let before_items: HashSet<_> = self
                    .controller
                    .items
                    .iter()
                    .map(|item| item.guid.as_str())
                    .collect();
                Some(
                    items
                        .iter()
                        .filter(|item| !before_items.contains(item.guid.as_str()))
                        .count(),
                )
            }
            _ => None,
        };
        self.controller.apply_event(event);
        if before != self.controller.selected_source || changed {
            self.fetch_items(cx);
        }
        if let Some(count) = item_event.filter(|n| *n > 0) {
            self.feedback = Some(self.with("rssItemsUpdated", &[("n", &count.to_string())], cx));
        }
        cx.notify();
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.controller.mark_stale();
        self.last_error = Some(self.t("localServiceDisconnected", cx).to_string());
        cx.notify();
    }

    fn fetch_items(&mut self, cx: &mut Context<Self>) {
        let Some(load) = self.controller.load_items() else {
            return;
        };
        self.load_error = false;
        cx.spawn(async move |view, cx| {
            let result = load.future.await;
            let _ = view.update(cx, |this, cx| {
                let current = this.controller.selected_source.as_deref() == Some(&load.source_id)
                    && this.controller.revision == load.revision;
                if current {
                    this.last_error = None;
                    if !this
                        .controller
                        .finish_load(&load.source_id, load.revision, result)
                    {
                        this.load_error = true;
                        this.fail(cx);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn open_editor(
        &self,
        source: Option<RssSourceDto>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.controller.stale {
            editor::open_editor(
                self.translator.clone(),
                self.controller.port.clone(),
                source,
                self.controller.queues.clone(),
                window,
                cx,
            );
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.controller.stale || self.controller.refresh_busy {
            return;
        }
        let Some(id) = self.controller.selected_source.clone() else {
            return;
        };
        self.controller.refresh_busy = true;
        let epoch = self.controller.action_epoch;
        let future = self.controller.refresh(id.clone());
        cx.notify();
        cx.spawn(async move |view, cx| {
            let result = future.await;
            let _ = view.update(cx, |this, cx| {
                if this.controller.action_epoch != epoch
                    || this.controller.selected_source.as_deref() != Some(&id)
                {
                    return;
                }
                this.controller.refresh_busy = false;
                if result.is_err() {
                    this.fail(cx);
                } else {
                    this.fetch_items(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn read_all(&mut self, cx: &mut Context<Self>) {
        if self.controller.stale || self.controller.read_busy {
            return;
        }
        let Some(id) = self.controller.selected_source.clone() else {
            return;
        };
        self.controller.read_busy = true;
        let epoch = self.controller.action_epoch;
        let future = self.controller.item_action(&id, "", "readAll");
        cx.notify();
        cx.spawn(async move |view, cx| {
            let result = future.await;
            let _ = view.update(cx, |this, cx| {
                if this.controller.action_epoch != epoch
                    || this.controller.selected_source.as_deref() != Some(&id)
                {
                    return;
                }
                this.controller.read_busy = false;
                if result.is_err() {
                    this.fail(cx);
                } else {
                    this.fetch_items(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn act(&mut self, guids: Vec<String>, action: &'static str, cx: &mut Context<Self>) {
        if self.controller.stale {
            return;
        }
        let Some(source_id) = self.controller.selected_source.clone() else {
            return;
        };
        let guids: Vec<_> = guids
            .into_iter()
            .filter(|guid| self.controller.begin_action(&source_id, guid))
            .collect();
        if guids.is_empty() {
            return;
        }
        let epoch = self.controller.action_epoch;
        let port = self.controller.port.clone();
        self.last_error = None;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let mut done = 0;
            let mut failed = 0;
            for guid in guids {
                let result = port
                    .call(
                        method::DAEMON_RSS_ITEM_ACTION,
                        serde_json::json!({"sourceId": source_id, "guid": guid, "action": action}),
                    )
                    .await;
                if result.is_ok() {
                    done += 1;
                } else {
                    failed += 1;
                }
                let _ = view.update(cx, |this, cx| {
                    this.controller
                        .finish_action(&source_id, &guid, epoch, result.is_ok());
                    cx.notify();
                });
            }
            let _ = view.update(cx, |this, cx| {
                if this.controller.action_epoch == epoch
                    && this.controller.selected_source.as_deref() == Some(&source_id)
                {
                    if failed > 0 {
                        this.fail(cx);
                    }
                    this.feedback = Some(if done == 1 && failed == 0 && action == "download" {
                        this.t("rssTaskCreated", cx).to_string()
                    } else {
                        this.with(
                            "rssBatchResult",
                            &[("done", &done.to_string()), ("failed", &failed.to_string())],
                            cx,
                        )
                    });
                    if done > 0 {
                        this.fetch_items(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn confirm_delete(
        &mut self,
        source: RssSourceDto,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.controller.stale || self.controller.delete_busy {
            return;
        }
        let title = self.t("rssDeleteSource", cx);
        let name = display_name(&source);
        let description =
            SharedString::from(self.with("rssDeleteConfirmDesc", &[("name", &name)], cx));
        let cancel = self.t("cancel", cx);
        let view = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let source_id = source.source_id.clone();
            alert
                .title(title.clone())
                .description(description.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(title.clone())
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(cancel.clone())
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let _ = view.update(cx, |this, cx| {
                        if this.controller.stale || this.controller.delete_busy {
                            return;
                        }
                        this.controller.delete_busy = true;
                        let future = this.controller.delete(source_id.clone());
                        cx.spawn(async move |view, cx| {
                            let result = future.await;
                            let _ = view.update(cx, |this, cx| {
                                this.controller.delete_busy = false;
                                if result.is_err() {
                                    this.fail(cx);
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                        cx.notify();
                    });
                    true
                })
        });
    }
}

fn display_name(source: &RssSourceDto) -> String {
    if source.name.trim().is_empty() {
        source.url.clone()
    } else {
        source.name.clone()
    }
}

fn date_text(timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    // UTC civil date from Unix days, without a dependency solely for list-row formatting.
    let days = timestamp.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    let hours = timestamp.rem_euclid(86_400) / 3_600;
    let minutes = timestamp.rem_euclid(3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02} UTC")
}

fn bytes_text(bytes: i64) -> String {
    if bytes <= 0 {
        return String::new();
    }
    let mut size = bytes as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB"] {
        if size < 1024. {
            break;
        }
        size /= 1024.;
        unit = next;
    }
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {unit}")
    }
}

impl RssView {
    fn status_line(&self, source: &RssSourceDto, cx: &App) -> String {
        let interval = if source.interval_minutes > 0 {
            source.interval_minutes
        } else {
            30
        };
        let mut parts = vec![self.with("rssEveryMinutes", &[("n", &interval.to_string())], cx)];
        if self.controller.refresh_busy
            && self.controller.selected_source.as_deref() == Some(&source.source_id)
        {
            parts.push(self.t("rssRefreshing", cx).to_string());
        } else if !source.last_error.is_empty() {
            parts.push(self.with(
                "rssFailedTimes",
                &[("n", &source.fail_count.to_string())],
                cx,
            ));
            parts.push(source.last_error.clone());
        } else if source.last_success_at > 0 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs() as i64);
            let age = now.saturating_sub(source.last_success_at);
            let when = if age < 60 {
                self.t("rssJustNow", cx).to_string()
            } else if age < 3_600 {
                self.with("rssMinutesAgo", &[("n", &(age / 60).to_string())], cx)
            } else if age < 86_400 {
                self.with("rssHoursAgo", &[("n", &(age / 3_600).to_string())], cx)
            } else {
                self.with("rssDaysAgo", &[("n", &(age / 86_400).to_string())], cx)
            };
            parts.push(self.with("rssLastFetch", &[("when", &when)], cx));
        } else {
            parts.push(self.t("rssNeverFetched", cx).to_string());
        }
        parts.push(
            self.t(
                if source.auto_download {
                    "rssAutoDownloadOn"
                } else {
                    "rssCollectMode"
                },
                cx,
            )
            .to_string(),
        );
        parts.join(" · ")
    }

    fn render_source(
        &self,
        source: RssSourceDto,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = active_theme(cx).tokens().colors;
        let selected = self.controller.selected_source.as_deref() == Some(&source.source_id);
        let unhealthy = !source.last_error.is_empty();
        let name = display_name(&source);
        let status = self.status_line(&source, cx);
        let id = source.source_id.clone();
        let count = source.unread_count;
        let unread = self.with("rssUnreadCount", &[("n", &count.to_string())], cx);
        let badge_id = format!("rss-unread-{id}");
        div()
            .id(SharedString::from(format!("rss-source-{id}")))
            .w_full()
            .px(px(10.))
            .py(px(9.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(selected, |row| row.bg(colors.accent))
            .when(!selected, |row| {
                row.hover(move |style| style.bg(colors.muted.opacity(0.7)))
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if this.controller.select_source(&id) {
                    this.fetch_items(cx);
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::empty().path(RSS_ICON_PATH).size(px(15.)).text_color(
                        if unhealthy {
                            cx.theme().danger
                        } else {
                            colors.muted_foreground
                        },
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(status),
                            ),
                    )
                    .when(count > 0, |row| {
                        row.child(
                            div()
                                .id(badge_id)
                                .flex_none()
                                .text_xs()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(10.))
                                .bg(colors.primary.opacity(0.12))
                                .text_color(colors.primary)
                                .tooltip(move |window, cx| {
                                    gpui_component::tooltip::Tooltip::new(unread.clone())
                                        .build(window, cx)
                                })
                                .child(count.to_string()),
                        )
                    }),
            )
    }

    fn render_item(&self, item: RssItemDto, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let colors = active_theme(cx).tokens().colors;
        let guid = item.guid.clone();
        let source_id = item.source_id.clone();
        let busy = self
            .controller
            .busy
            .contains(&format!("{source_id}\0{guid}"));
        let selected = self.controller.selected.contains(&guid);
        let can_act = !self.controller.stale && !busy;
        let title = item.title.clone();
        let status = self.t(self.controller.status_key(&item), cx);
        let reason_key = match item.reason.as_str() {
            "not_included" => Some("rssReasonNotIncluded"),
            "excluded" => Some("rssReasonExcluded"),
            "too_small" => Some("rssReasonTooSmall"),
            "too_large" => Some("rssReasonTooLarge"),
            "dup_episode" => Some("rssReasonDupEpisode"),
            _ => None,
        };
        let date = date_text(item.pub_date);
        let size = bytes_text(item.enclosure_length);
        let label = self.t(
            if busy {
                "rssActionPreparing"
            } else if item.status == 1 {
                "rssActionRedownload"
            } else {
                "rssActionDownload"
            },
            cx,
        );
        let can_ignore = item.status == 0;
        let task_missing = item.status == 1 && self.controller.task_status(&item).is_none();
        v_flex()
            .w_full()
            .h(px(66.))
            .flex_none()
            .justify_center()
            .px(px(16.))
            .gap_1()
            .border_b_1()
            .border_color(colors.border.opacity(0.7))
            .hover(move |style| style.bg(colors.muted.opacity(0.4)))
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Checkbox::new(SharedString::from(format!("rss-select-{source_id}-{guid}")))
                            .with_size(Size::XSmall)
                            .checked(selected)
                            .focus_ring(false)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if *checked {
                                    this.controller.selected.insert(guid.clone());
                                } else {
                                    this.controller.selected.remove(&guid);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "rss-title-{}-{}",
                                        item.source_id, item.guid
                                    )))
                                    .w_full()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(colors.foreground)
                                    .tooltip(move |window, cx| {
                                        gpui_component::tooltip::Tooltip::new(title.clone())
                                            .build(window, cx)
                                    })
                                    .child(item.title),
                            )
                            .child(
                                h_flex()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .when(!date.is_empty(), |row| {
                                        row.child(format!(
                                            "{} · {date}",
                                            self.t("rssPublishedAt", cx)
                                        ))
                                    })
                                    .when(!size.is_empty(), |row| row.child(size)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(145.))
                            .flex_none()
                            .text_right()
                            .text_xs()
                            .text_color(if task_missing {
                                cx.theme().danger
                            } else {
                                colors.muted_foreground
                            })
                            .child(status),
                    )
                    .child(
                        h_flex()
                            .w(px(177.))
                            .flex_none()
                            .justify_end()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "rss-download-{}-{}",
                                    item.source_id, item.guid
                                )))
                                .ghost()
                                .xsmall()
                                .label(label)
                                .disabled(!can_act)
                                .on_click(cx.listener({
                                    let guid = item.guid.clone();
                                    move |this, _, _, cx| {
                                        this.act(vec![guid.clone()], "download", cx)
                                    }
                                })),
                            )
                            .when(can_ignore, |row| {
                                row.child(
                                    Button::new(SharedString::from(format!(
                                        "rss-ignore-{}-{}",
                                        item.source_id, item.guid
                                    )))
                                    .ghost()
                                    .xsmall()
                                    .label(self.t("rssActionIgnore", cx))
                                    .disabled(!can_act)
                                    .on_click(cx.listener({
                                        let guid = item.guid.clone();
                                        move |this, _, _, cx| {
                                            this.act(vec![guid.clone()], "ignore", cx)
                                        }
                                    })),
                                )
                            }),
                    ),
            )
            .when_some(reason_key, |row, key| {
                row.child(
                    div()
                        .pl(px(27.))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(self.t(key, cx)),
                )
            })
    }

    fn render_empty(
        &self,
        title: &str,
        description: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = active_theme(cx).tokens().colors;
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .px(px(40.))
            .child(
                Icon::empty()
                    .path(RSS_ICON_PATH)
                    .size(px(32.))
                    .text_color(colors.muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.t(title, cx)),
            )
            .child(
                div()
                    .text_xs()
                    .text_center()
                    .text_color(colors.muted_foreground)
                    .child(self.t(description, cx)),
            )
    }
}

impl Render for RssView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = active_theme(cx).tokens().colors;
        let stale = self.controller.stale;
        let source = self
            .controller
            .sources
            .iter()
            .find(|source| {
                Some(source.source_id.as_str()) == self.controller.selected_source.as_deref()
            })
            .cloned();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let visible = self.controller.visible_indices(&query, self.oldest_first);
        let all_visible = !visible.is_empty()
            && visible.iter().all(|&index| {
                self.controller
                    .selected
                    .contains(&self.controller.items[index].guid)
            });
        let selected_count = self.controller.selected.len();
        let selected_guids: Vec<_> = self.controller.selected.iter().cloned().collect();
        let selected_for_ignore: Vec<_> = self
            .controller
            .items
            .iter()
            .filter(|item| item.status == 0 && self.controller.selected.contains(&item.guid))
            .map(|item| item.guid.clone())
            .collect();
        let side = v_flex()
            .w(px(234.))
            .flex_none()
            .h_full()
            .min_h_0()
            .border_r_1()
            .border_color(colors.border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .py(px(12.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.muted_foreground)
                            .child(self.t("rssSubscriptions", cx)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(self.controller.sources.len().to_string()),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(8.))
                    .when(self.controller.sources.is_empty(), |list| {
                        list.child(
                            div()
                                .px(px(10.))
                                .py(px(16.))
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(self.t("rssSidebarEmptyHint", cx)),
                        )
                    })
                    .children(
                        self.controller
                            .sources
                            .clone()
                            .into_iter()
                            .map(|source| self.render_source(source, cx)),
                    )
                    .overflow_y_scrollbar(),
            );
        let mut main = v_flex().flex_1().min_w_0().min_h_0().h_full();
        if let Some(source) = source {
            let status = self.status_line(&source, cx);
            let title = display_name(&source);
            let unhealthy = !source.last_error.is_empty();
            let source_for_manage = source.clone();
            let source_for_delete = source.clone();
            main = main.child(
                v_flex()
                    .flex_none()
                    .px(px(16.))
                    .py(px(11.))
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::empty().path(RSS_ICON_PATH).size(px(17.)).text_color(
                                if unhealthy {
                                    cx.theme().danger
                                } else {
                                    colors.muted_foreground
                                },
                            ))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .id("rss-selected-source-title")
                                            .truncate()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_sm()
                                            .tooltip(move |window, cx| {
                                                gpui_component::tooltip::Tooltip::new(title.clone())
                                                    .build(window, cx)
                                            })
                                            .child(display_name(&source)),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_xs()
                                            .text_color(if unhealthy {
                                                cx.theme().danger
                                            } else {
                                                colors.muted_foreground
                                            })
                                            .child(status),
                                    ),
                            )
                            .child(
                                Button::new("rss-manage")
                                    .ghost()
                                    .small()
                                    .label(self.t("rssManageTitle", cx))
                                    .disabled(stale)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_editor(
                                            Some(source_for_manage.clone()),
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new("rss-delete")
                                    .ghost()
                                    .small()
                                    .label(self.t("rssDeleteSource", cx))
                                    .disabled(stale || self.controller.delete_busy)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.confirm_delete(source_for_delete.clone(), window, cx)
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("rss-sort")
                                    .outline()
                                    .small()
                                    .label(self.t(
                                        if self.oldest_first {
                                            "rssSortOldest"
                                        } else {
                                            "rssSortNewest"
                                        },
                                        cx,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.oldest_first = !this.oldest_first;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("rss-read-all")
                                    .ghost()
                                    .small()
                                    .label(self.t("rssMarkAllRead", cx))
                                    .disabled(stale || self.controller.read_busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.read_all(cx))),
                            )
                            .child(
                                Button::new("rss-refresh")
                                    .ghost()
                                    .small()
                                    .label(self.t(
                                        if self.controller.refresh_busy {
                                            "rssRefreshing"
                                        } else {
                                            "rssRefreshNow"
                                        },
                                        cx,
                                    ))
                                    .disabled(stale || self.controller.refresh_busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                            )
                            .child(div().flex_1().min_w(px(8.)))
                            .child(
                                div().w(px(185.)).child(
                                    Input::new(&self.search).with_size(Size::Small).prefix(
                                        Icon::new(IconName::Search)
                                            .size(px(13.))
                                            .text_color(colors.muted_foreground),
                                    ),
                                ),
                            ),
                    ),
            );
            if !visible.is_empty() || selected_count > 0 {
                main = main.child(
                    h_flex()
                        .flex_wrap()
                        .flex_none()
                        .items_center()
                        .gap_2()
                        .px(px(16.))
                        .py(px(7.))
                        .border_b_1()
                        .border_color(colors.border)
                        .child(
                            Checkbox::new("rss-select-visible")
                                .with_size(Size::XSmall)
                                .checked(all_visible)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    let query = this.search.read(cx).value().to_string();
                                    this.controller.select_visible(
                                        &query,
                                        this.oldest_first,
                                        *checked,
                                    );
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(self.t("rssSelectVisible", cx)),
                        )
                        .when(selected_count > 0, |bar| {
                            bar.child(div().text_xs().text_color(colors.muted_foreground).child(
                                self.with(
                                    "rssSelectedCount",
                                    &[("n", &selected_count.to_string())],
                                    cx,
                                ),
                            ))
                            .child(
                                Button::new("rss-download-selected")
                                    .primary()
                                    .xsmall()
                                    .label(self.t("rssDownloadSelected", cx))
                                    .disabled(
                                        stale
                                            || selected_guids.iter().all(|guid| {
                                                self.controller.busy.contains(&format!(
                                                    "{}\0{guid}",
                                                    source.source_id
                                                ))
                                            }),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.act(selected_guids.clone(), "download", cx)
                                    })),
                            )
                            .child(
                                Button::new("rss-ignore-selected")
                                    .outline()
                                    .xsmall()
                                    .label(self.t("rssIgnoreSelected", cx))
                                    .disabled(
                                        stale
                                            || selected_for_ignore.iter().all(|guid| {
                                                self.controller.busy.contains(&format!(
                                                    "{}\0{guid}",
                                                    source.source_id
                                                ))
                                            }),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.act(selected_for_ignore.clone(), "ignore", cx)
                                    })),
                            )
                            .child(
                                Button::new("rss-clear-selection")
                                    .ghost()
                                    .xsmall()
                                    .label(self.t("rssClearSelection", cx))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.controller.selected.clear();
                                        cx.notify();
                                    })),
                            )
                        }),
                );
            }
            if self.controller.loading && visible.is_empty() {
                main =
                    main.child(self.render_empty("rssEmptyFetching", "rssEmptyFetchingHint", cx));
            } else if visible.is_empty() {
                if !query.is_empty() && !self.controller.items.is_empty() {
                    main = main.child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(self.with("rssNoMatch", &[("query", &query)], cx)),
                    );
                } else if unhealthy || self.load_error {
                    let retry_fetch = self.load_error;
                    let source_for_manage = source.clone();
                    main = main.child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap_3()
                            .px(px(40.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().danger)
                                    .child(self.t("rssEmptyError", cx)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_center()
                                    .text_color(colors.muted_foreground)
                                    .child(self.t("rssEmptyErrorHint", cx)),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("rss-empty-retry")
                                            .outline()
                                            .small()
                                            .label(self.t("rssEmptyRetry", cx))
                                            .disabled(stale)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if retry_fetch {
                                                    this.fetch_items(cx);
                                                    cx.notify();
                                                } else {
                                                    this.refresh(cx);
                                                }
                                            })),
                                    )
                                    .child(
                                        Button::new("rss-empty-manage")
                                            .outline()
                                            .small()
                                            .label(self.t("rssCheckConfig", cx))
                                            .disabled(stale)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.open_editor(
                                                    Some(source_for_manage.clone()),
                                                    window,
                                                    cx,
                                                )
                                            })),
                                    ),
                            ),
                    );
                } else if !source.seeded && source.enabled {
                    main = main.child(self.render_empty(
                        "rssEmptyFetching",
                        "rssEmptyFetchingHint",
                        cx,
                    ));
                } else {
                    main = main.child(self.render_empty("rssEmptyTitle", "rssEmptyDesc", cx));
                }
            } else {
                let count = visible.len();
                main = main.child(
                    uniform_list(
                        "rss-items",
                        count,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|index| {
                                    this.render_item(
                                        this.controller.items[visible[index]].clone(),
                                        cx,
                                    )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .flex_1()
                    .min_h_0()
                    .w_full(),
                );
            }
        } else {
            main = main.child(self.render_empty("rssAddSource", "rssSidebarEmptyHint", cx));
        }
        v_flex()
            .size_full()
            .bg(colors.background)
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px(px(18.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.t("rssSubscriptions", cx)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(self.t("rssPageDescription", cx)),
                            ),
                    )
                    .child(
                        Button::new("rss-add-source")
                            .primary()
                            .small()
                            .h(CONTROL_HEIGHT)
                            .icon(IconName::Plus)
                            .label(self.t("rssAddSource", cx))
                            .disabled(stale)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_editor(None, window, cx)
                            })),
                    ),
            )
            .when_some(self.last_error.clone(), |page, error| {
                page.child(
                    div()
                        .flex_none()
                        .px(px(18.))
                        .py(px(7.))
                        .bg(cx.theme().danger.opacity(0.1))
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .when_some(self.feedback.clone(), |page, feedback| {
                page.child(
                    h_flex()
                        .flex_none()
                        .px(px(18.))
                        .py(px(7.))
                        .gap_3()
                        .bg(colors.primary.opacity(0.08))
                        .child(div().flex_1().text_xs().child(feedback))
                        .child(
                            Button::new("rss-dismiss-feedback")
                                .ghost()
                                .xsmall()
                                .label(self.t("close", cx))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.feedback = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(h_flex().flex_1().min_h_0().child(side).child(main))
    }
}
