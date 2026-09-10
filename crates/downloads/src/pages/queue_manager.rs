//! 队列管理窗口：左列队列列表，右侧表单编辑（新建 / 更新 / 定时 / 启停 / 删除）。

use std::sync::Arc;

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, LATER_QUEUE_ID, MAIN_QUEUE_ID, QueueDto, ServiceEvent,
};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, Size, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputState},
    switch::Switch,
    v_flex,
};

use crate::controller::{DownloadsCommand, DownloadsPort, QueueFields};

/// 星期位掩码单日切换：`bit_index` 0 = 周一 … 6 = 周日。
fn toggle_day_bit(days: i32, bit: i32, checked: bool) -> i32 {
    if checked { days | bit } else { days & !bit }
}

/// 一份正在编辑的队列表单；`queue_id = None` 表示「新建」。
struct QueueForm {
    queue_id: Option<String>,
    is_running: bool,
    name: Entity<InputState>,
    max_concurrent: Entity<InputState>,
    speed_limit: Entity<InputState>,
    upload_limit: Entity<InputState>,
    save_dir: Entity<InputState>,
    segments: Entity<InputState>,
    user_agent: Entity<InputState>,
    schedule_enabled: bool,
    schedule_start: Entity<InputState>,
    schedule_stop: Entity<InputState>,
    /// bit0 = 周一 … bit6 = 周日。
    schedule_days: i32,
    picking_dir: bool,
}

impl QueueForm {
    fn blank(window: &mut Window, cx: &mut Context<QueueManagerView>) -> Self {
        Self {
            queue_id: None,
            is_running: true,
            name: cx.new(|cx| InputState::new(window, cx)),
            max_concurrent: cx.new(|cx| InputState::new(window, cx).default_value("0")),
            speed_limit: cx.new(|cx| InputState::new(window, cx).default_value("0")),
            upload_limit: cx.new(|cx| InputState::new(window, cx).default_value("0")),
            save_dir: cx.new(|cx| InputState::new(window, cx)),
            segments: cx.new(|cx| InputState::new(window, cx).default_value("0")),
            user_agent: cx.new(|cx| InputState::new(window, cx)),
            schedule_enabled: false,
            schedule_start: cx.new(|cx| InputState::new(window, cx)),
            schedule_stop: cx.new(|cx| InputState::new(window, cx)),
            schedule_days: 127,
            picking_dir: false,
        }
    }

    fn for_queue(
        queue: &QueueDto,
        window: &mut Window,
        cx: &mut Context<QueueManagerView>,
    ) -> Self {
        Self {
            queue_id: Some(queue.queue_id.clone()),
            is_running: queue.is_running,
            name: cx.new(|cx| InputState::new(window, cx).default_value(queue.name.clone())),
            max_concurrent: cx.new(|cx| {
                InputState::new(window, cx).default_value(queue.max_concurrent.to_string())
            }),
            speed_limit: cx.new(|cx| {
                InputState::new(window, cx).default_value(queue.speed_limit_kbps.to_string())
            }),
            upload_limit: cx.new(|cx| {
                InputState::new(window, cx).default_value(queue.upload_limit_kbps.to_string())
            }),
            save_dir: cx.new(|cx| {
                InputState::new(window, cx).default_value(queue.default_save_dir.clone())
            }),
            segments: cx.new(|cx| {
                InputState::new(window, cx).default_value(queue.default_segments.to_string())
            }),
            user_agent: cx.new(|cx| {
                InputState::new(window, cx).default_value(queue.default_user_agent.clone())
            }),
            schedule_enabled: queue.schedule_enabled,
            schedule_start: cx
                .new(|cx| InputState::new(window, cx).default_value(queue.schedule_start.clone())),
            schedule_stop: cx
                .new(|cx| InputState::new(window, cx).default_value(queue.schedule_stop.clone())),
            schedule_days: queue.schedule_days,
            picking_dir: false,
        }
    }

    fn is_builtin_main(&self) -> bool {
        self.queue_id.as_deref() == Some(MAIN_QUEUE_ID)
    }
}

/// 队列管理页面；实现 [`crate::session`]（app 侧）期望的三方法供 `attach` 驱动。
pub struct QueueManagerView {
    translator: Entity<Translator>,
    port: Arc<dyn DownloadsPort>,
    queues: Vec<QueueDto>,
    form: Option<QueueForm>,
    error: Option<SharedString>,
    stale: bool,
}

impl QueueManagerView {
    #[must_use]
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn DownloadsPort>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        Self {
            translator,
            port,
            queues: Vec::new(),
            form: None,
            error: None,
            stale: true,
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        self.absorb_queues(snapshot.daemon.queues.clone());
        self.stale = false;
        cx.notify();
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        match event {
            ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::QueuesChanged(queues))) => {
                self.absorb_queues(queues.clone());
                cx.notify();
            }
            ServiceEvent::Agent(AgentEvent::DaemonConnectionChanged(connected)) => {
                self.stale = !connected;
                cx.notify();
            }
            _ => {}
        }
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        cx.notify();
    }

    fn absorb_queues(&mut self, mut queues: Vec<QueueDto>) {
        queues.sort_by_key(|queue| queue.position);
        // 已选队列被远端删除：回退到「新建」表单，避免对着已消失的 id 保存。
        if let Some(form) = &self.form
            && let Some(id) = &form.queue_id
            && !queues.iter().any(|queue| &queue.queue_id == id)
        {
            self.form = None;
        }
        self.queues = queues;
    }

    fn ensure_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.is_some() || self.queues.is_empty() {
            return;
        }
        let queue = self
            .queues
            .iter()
            .find(|queue| queue.queue_id == MAIN_QUEUE_ID)
            .or_else(|| self.queues.first())
            .cloned();
        if let Some(queue) = queue {
            self.form = Some(QueueForm::for_queue(&queue, window, cx));
        }
    }

    fn t(&self, key: &str, cx: &App) -> SharedString {
        SharedString::from(self.translator.read(cx).text(key).to_owned())
    }

    fn queue_label(&self, queue: &QueueDto, cx: &App) -> SharedString {
        match queue.queue_id.as_str() {
            MAIN_QUEUE_ID => self.t("mainQueue", cx),
            LATER_QUEUE_ID => self.t("laterQueue", cx),
            _ => SharedString::from(queue.name.clone()),
        }
    }

    fn fail(&mut self, key: &str, cx: &mut Context<Self>) {
        self.error = Some(self.t(key, cx));
        cx.notify();
    }

    fn run_command(&mut self, command: DownloadsCommand, cx: &mut Context<Self>) {
        let future = self.port.execute(command);
        cx.spawn(async move |this, cx| {
            if future.await.is_err() {
                let _ = this.update(cx, |this, cx| this.fail("localServiceActionFailed", cx));
            }
        })
        .detach();
    }

    fn select_queue(&mut self, queue_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(queue) = self
            .queues
            .iter()
            .find(|queue| queue.queue_id == queue_id)
            .cloned()
        {
            self.form = Some(QueueForm::for_queue(&queue, window, cx));
            self.error = None;
            cx.notify();
        }
    }

    fn new_queue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = Some(QueueForm::blank(window, cx));
        self.error = None;
        cx.notify();
    }

    fn parse_int(input: &Entity<InputState>, cx: &App) -> i64 {
        input.read(cx).value().trim().parse().unwrap_or(0)
    }

    /// `HH:MM`；空串合法（表示不定时）。
    fn valid_time(text: &str) -> bool {
        if text.is_empty() {
            return true;
        }
        let Some((hours, minutes)) = text.split_once(':') else {
            return false;
        };
        let Ok(hours) = hours.parse::<u32>() else {
            return false;
        };
        let Ok(minutes) = minutes.parse::<u32>() else {
            return false;
        };
        hours < 24 && minutes < 60
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let name = form.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.fail("queueNameRequired", cx);
            return;
        }
        let start = form.schedule_start.read(cx).value().trim().to_owned();
        let stop = form.schedule_stop.read(cx).value().trim().to_owned();
        if form.schedule_enabled && (!Self::valid_time(&start) || !Self::valid_time(&stop)) {
            self.fail("queueScheduleTimeInvalid", cx);
            return;
        }
        let fields = QueueFields {
            name,
            speed_limit_kbps: Self::parse_int(&form.speed_limit, cx),
            upload_limit_kbps: Self::parse_int(&form.upload_limit, cx),
            max_concurrent: Self::parse_int(&form.max_concurrent, cx) as i32,
            default_save_dir: form.save_dir.read(cx).value().trim().to_owned(),
            default_segments: Self::parse_int(&form.segments, cx) as i32,
            default_user_agent: form.user_agent.read(cx).value().trim().to_owned(),
        };
        let schedule_enabled = form.schedule_enabled;
        let schedule_days = form.schedule_days;
        match form.queue_id.clone() {
            Some(queue_id) => {
                self.run_command(
                    DownloadsCommand::QueueUpdate {
                        queue_id: queue_id.clone(),
                        fields,
                    },
                    cx,
                );
                self.run_command(
                    DownloadsCommand::QueueSchedule {
                        queue_id,
                        enabled: schedule_enabled,
                        start_time: start,
                        stop_time: stop,
                        days: schedule_days,
                    },
                    cx,
                );
            }
            None => {
                self.run_command(DownloadsCommand::QueueCreate(fields), cx);
                self.form = None;
            }
        }
        let _ = window;
        cx.notify();
    }

    fn toggle_running(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let Some(queue_id) = form.queue_id.clone() else {
            return;
        };
        let command = if form.is_running {
            DownloadsCommand::QueueStop { queue_id }
        } else {
            DownloadsCommand::QueueStart { queue_id }
        };
        self.run_command(command, cx);
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let Some(queue_id) = form.queue_id.clone() else {
            return;
        };
        let name = form.name.read(cx).value().to_string();
        let view = cx.weak_entity();
        let title = self.t("deleteQueueAction", cx);
        let description = SharedString::from(
            self.translator
                .read(cx)
                .text_with("queueDeleteConfirmDesc", &[("name", &name)]),
        );
        let cancel = self.t("cancel", cx);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let queue_id = queue_id.clone();
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
                        this.run_command(
                            DownloadsCommand::QueueDelete {
                                queue_id: queue_id.clone(),
                            },
                            cx,
                        );
                        this.form = None;
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn pick_save_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.as_ref().is_some_and(|form| form.picking_dir) {
            return;
        }
        if let Some(form) = &mut self.form {
            form.picking_dir = true;
        }
        cx.notify();
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths.first().map(|path| path.display().to_string()),
                _ => None,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(form) = &mut this.form {
                    form.picking_dir = false;
                    if let Some(path) = picked {
                        form.save_dir
                            .update(cx, |input, cx| input.set_value(path, window, cx));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_list(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let selected_id = self.form.as_ref().and_then(|form| form.queue_id.clone());
        let mut list = v_flex()
            .w(px(200.))
            .flex_none()
            .h_full()
            .min_h_0()
            .border_r_1()
            .border_color(tokens.colors.border)
            .child(
                h_flex()
                    .p(tokens.spacing.sm)
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(tokens.typography.sm.weight)
                            .text_color(tokens.colors.muted_foreground)
                            .child(self.t("sidebarQueues", cx)),
                    )
                    .child(
                        Button::new("queue-manager-new")
                            .ghost()
                            .small()
                            .h(CONTROL_HEIGHT)
                            .label(self.t("createQueueAction", cx))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.new_queue(window, cx);
                            })),
                    ),
            );
        for queue in self.queues.clone() {
            let id = queue.queue_id.clone();
            let selected = selected_id.as_deref() == Some(id.as_str());
            let label = self.queue_label(&queue, cx);
            let running = queue.is_running;
            list = list.child(
                div()
                    .id(SharedString::from(format!("queue-manager-row-{id}")))
                    .px(tokens.spacing.sm)
                    .py(tokens.spacing.xs)
                    .mx(tokens.spacing.xs)
                    .rounded(tokens.radius.md)
                    .cursor_pointer()
                    .when(selected, |this| this.bg(tokens.colors.accent))
                    .hover(move |style| style.bg(tokens.colors.muted))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.select_queue(id.clone(), window, cx);
                    }))
                    .child(
                        h_flex()
                            .gap(tokens.spacing.xs)
                            .items_center()
                            .child(div().size(px(6.)).rounded_full().bg(if running {
                                cx.theme().success
                            } else {
                                tokens.colors.muted_foreground
                            }))
                            .child(div().text_sm().truncate().child(label)),
                    ),
            );
        }
        let _ = window;
        list
    }

    fn render_field(
        &self,
        label_key: &str,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .gap(tokens.spacing.xs)
            .child(
                div()
                    .text_xs()
                    .font_weight(tokens.typography.sm.weight)
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t(label_key, cx)),
            )
            .child(Input::new(input).with_size(Size::Medium).w_full())
    }

    fn render_save_dir_field(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let Some(form) = &self.form else {
            return div();
        };
        let picking = form.picking_dir;
        div().child(
            v_flex()
                .gap(tokens.spacing.xs)
                .child(
                    div()
                        .text_xs()
                        .font_weight(tokens.typography.sm.weight)
                        .text_color(tokens.colors.muted_foreground)
                        .child(self.t("queueSaveDir", cx)),
                )
                .child(
                    h_flex()
                        .gap(tokens.spacing.sm)
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&form.save_dir).with_size(Size::Medium).w_full()),
                        )
                        .child(
                            Button::new("queue-manager-browse-dir")
                                .secondary()
                                .small()
                                .h(CONTROL_HEIGHT)
                                .label(self.t("browse", cx))
                                .disabled(picking)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.pick_save_dir(window, cx);
                                })),
                        ),
                ),
        )
    }

    fn render_schedule(
        &self,
        form_enabled: bool,
        days: i32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let weekday_labels: Vec<String> = self
            .t("weekdaysShort", cx)
            .split(',')
            .map(str::to_owned)
            .collect();
        let mut days_row = h_flex().gap(tokens.spacing.xs);
        for (index, label) in weekday_labels.into_iter().enumerate() {
            let bit = 1 << index;
            let checked = days & bit != 0;
            days_row = days_row.child(
                Checkbox::new(("queue-schedule-day", index))
                    .label(label)
                    .checked(checked)
                    .disabled(!form_enabled)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        if let Some(form) = &mut this.form {
                            form.schedule_days = toggle_day_bit(form.schedule_days, bit, *checked);
                        }
                        cx.notify();
                    })),
            );
        }
        v_flex()
            .gap(tokens.spacing.sm)
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .child(
                        Switch::new("queue-schedule-enabled")
                            .checked(form_enabled)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(form) = &mut this.form {
                                    form.schedule_enabled = *checked;
                                }
                                cx.notify();
                            })),
                    )
                    .child(div().text_sm().child(self.t("queueScheduleEnable", cx))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t("queueScheduleDesc", cx)),
            )
            .when(form_enabled, |this| {
                let this = this.child(
                    h_flex()
                        .gap(tokens.spacing.md)
                        .child(self.render_start_stop_field("queueScheduleStartLabel", true, cx))
                        .child(self.render_start_stop_field("queueScheduleStopLabel", false, cx)),
                );
                this.child(
                    v_flex()
                        .gap(tokens.spacing.xs)
                        .child(
                            div()
                                .text_xs()
                                .text_color(tokens.colors.muted_foreground)
                                .child(self.t("queueScheduleDays", cx)),
                        )
                        .child(days_row),
                )
            })
    }

    fn render_start_stop_field(
        &self,
        label_key: &str,
        is_start: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let Some(form) = &self.form else {
            return div();
        };
        let input = if is_start {
            &form.schedule_start
        } else {
            &form.schedule_stop
        };
        div().child(
            v_flex()
                .gap(tokens.spacing.xs)
                .child(
                    div()
                        .text_xs()
                        .text_color(tokens.colors.muted_foreground)
                        .child(self.t(label_key, cx)),
                )
                .child(Input::new(input).with_size(Size::Medium).w(px(120.))),
        )
    }

    fn render_form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let Some(form) = &self.form else {
            return v_flex()
                .flex_1()
                .p(tokens.spacing.lg)
                .child(div().text_sm().child(self.t("queueNoPendingTasks", cx)))
                .into_any_element();
        };
        let is_creating = form.queue_id.is_none();
        let is_main = form.is_builtin_main();
        let is_running = form.is_running;
        let schedule_enabled = form.schedule_enabled;
        let schedule_days = form.schedule_days;
        let name_field = self.render_field("queueNameLabel", &form.name, cx);
        let max_concurrent_field = {
            let field = self.render_field("queueMaxConcurrent", &form.max_concurrent, cx);
            v_flex().gap(tokens.spacing.xs).child(field).child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t("queueMaxConcurrentHint", cx)),
            )
        };
        let speed_field = self.render_field("queueSpeedLimit", &form.speed_limit, cx);
        let upload_field = {
            let field = self.render_field("queueUploadLimit", &form.upload_limit, cx);
            v_flex().gap(tokens.spacing.xs).child(field).child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t("queueUploadLimitDesc", cx)),
            )
        };
        let segments_field = self.render_field("queueDefaultSegments", &form.segments, cx);
        let save_dir_field = self.render_save_dir_field(cx);
        let user_agent_field = self.render_field("queueDefaultUserAgent", &form.user_agent, cx);
        let schedule_section = self.render_schedule(schedule_enabled, schedule_days, cx);

        let mut footer = h_flex().w_full().items_center().gap(tokens.spacing.sm);
        if !is_creating && !is_main {
            footer = footer.child(
                Button::new("queue-manager-delete")
                    .danger()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .label(self.t("deleteQueueAction", cx))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.confirm_delete(window, cx);
                    })),
            );
        }
        if !is_creating {
            footer = footer.child(
                Button::new("queue-manager-toggle-run")
                    .secondary()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .label(if is_running {
                        self.t("stopQueueAction", cx)
                    } else {
                        self.t("startQueueAction", cx)
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_running(cx);
                    })),
            );
        }
        footer = footer.child(div().flex_1()).child(
            Button::new("queue-manager-save")
                .primary()
                .small()
                .h(CONTROL_HEIGHT)
                .label(self.t("queueSaveAction", cx))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.save(window, cx);
                })),
        );

        let mut column = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .p(tokens.spacing.lg)
            .gap(tokens.spacing.md)
            .child(name_field)
            .child(
                h_flex()
                    .gap(tokens.spacing.md)
                    .child(speed_field)
                    .child(upload_field),
            )
            .child(
                h_flex()
                    .gap(tokens.spacing.md)
                    .child(max_concurrent_field)
                    .child(segments_field),
            )
            .child(save_dir_field)
            .child(user_agent_field)
            .child(schedule_section);
        if let Some(error) = self.error.clone() {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.destructive)
                    .child(error),
            );
        }
        let _ = window;
        column.child(footer).into_any_element()
    }
}

impl Render for QueueManagerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_form(window, cx);
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .size_full()
            .min_h_0()
            .bg(tokens.colors.surface)
            .child(self.render_list(window, cx))
            .child(self.render_form(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{QueueManagerView, toggle_day_bit};

    #[test]
    fn valid_time_accepts_empty_and_well_formed_hhmm() {
        assert!(QueueManagerView::valid_time(""));
        assert!(QueueManagerView::valid_time("9:30"));
        assert!(QueueManagerView::valid_time("00:00"));
        assert!(QueueManagerView::valid_time("23:59"));
    }

    #[test]
    fn valid_time_rejects_out_of_range_or_malformed() {
        assert!(!QueueManagerView::valid_time("24:00"));
        assert!(!QueueManagerView::valid_time("12:60"));
        assert!(!QueueManagerView::valid_time("12"));
        assert!(!QueueManagerView::valid_time("ab:cd"));
        assert!(!QueueManagerView::valid_time("-1:00"));
    }

    #[test]
    fn toggle_day_bit_sets_and_clears_only_targeted_bit() {
        // bit0 = 周一；从全选（127 = 0b1111111）取消周一，只清掉 bit0。
        assert_eq!(toggle_day_bit(0b111_1111, 0b1, false), 0b111_1110);
        // 从空掩码勾选周日（bit6），只置位 bit6，不影响其余位。
        assert_eq!(toggle_day_bit(0, 0b100_0000, true), 0b100_0000);
        // 对已置位的位再次勾选（checked=true）是幂等的。
        assert_eq!(toggle_day_bit(0b1, 0b1, true), 0b1);
    }
}
