//! 队列管理窗口：左列队列列表，右侧表单编辑（新建 / 更新 / 定时 / 启停 / 删除）。

use std::sync::Arc;

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, LATER_QUEUE_ID, MAIN_QUEUE_ID, QueueDto, ServiceEvent,
};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, Size, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement as _,
    switch::Switch,
    v_flex,
};

use crate::controller::{DownloadsCommand, DownloadsPort, QueueFields};

/// 左列队列列表宽度。
const LIST_WIDTH: gpui::Pixels = px(184.);
/// 定时时间输入宽度（`HH:MM`）。
const TIME_INPUT_WIDTH: gpui::Pixels = px(112.);

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

    /// 内置队列（主队列 / 稍后下载）：引擎侧拒绝改名与删除。
    fn is_builtin(&self) -> bool {
        matches!(
            self.queue_id.as_deref(),
            Some(MAIN_QUEUE_ID) | Some(LATER_QUEUE_ID)
        )
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
            ServiceEvent::Agent(AgentEvent::DaemonSnapshotReplaced(snapshot))
            | ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::SnapshotReplaced(snapshot))) => {
                self.absorb_queues(snapshot.queues.clone());
                self.stale = false;
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
        // 运行态来自 daemon，不是本地草稿：切换后以事件为准，确保按钮可以反向操作。
        // 已选队列被远端删除则清除表单，避免向失效 id 保存。
        if let Some(form) = &mut self.form
            && let Some(id) = &form.queue_id
        {
            if let Some(queue) = queues.iter().find(|queue| &queue.queue_id == id) {
                form.is_running = queue.is_running;
            } else {
                self.form = None;
            }
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

    fn label(&self, key: &str, cx: &App) -> gpui::Div {
        let tokens = active_theme(cx).tokens();
        div()
            .text_xs()
            .font_weight(tokens.typography.sm.weight)
            .text_color(tokens.colors.muted_foreground)
            .child(self.t(key, cx))
    }

    fn hint(&self, key: &str, cx: &App) -> gpui::Div {
        let tokens = active_theme(cx).tokens();
        div()
            .text_xs()
            .text_color(tokens.colors.muted_foreground)
            .child(self.t(key, cx))
    }

    fn render_list_row(
        &self,
        id: SharedString,
        selected: bool,
        leading: impl IntoElement,
        label: SharedString,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let colors = active_theme(cx).tokens().colors;
        div()
            .id(id)
            .w_full()
            .mb(px(2.))
            .px(px(10.))
            .py(px(7.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .rounded(px(6.))
            .when(selected, |this| this.bg(colors.accent))
            .when(!selected, |this| {
                this.hover(move |style| style.bg(colors.muted.opacity(0.7)))
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                on_click(this, window, cx);
            }))
            .child(leading)
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
                    .text_color(if selected {
                        colors.accent_foreground
                    } else {
                        colors.foreground
                    })
                    .child(label),
            )
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let colors = active_theme(cx).tokens().colors;
        let creating = self
            .form
            .as_ref()
            .is_some_and(|form| form.queue_id.is_none());
        let selected_id = self.form.as_ref().and_then(|form| form.queue_id.clone());

        let mut rows = v_flex().flex_1().min_h_0().w_full();
        for queue in &self.queues {
            let id = queue.queue_id.clone();
            let selected = selected_id.as_deref() == Some(id.as_str());
            let dot = div()
                .flex_none()
                .size(px(7.))
                .rounded_full()
                .bg(if queue.is_running {
                    cx.theme().success
                } else {
                    colors.muted_foreground.opacity(0.6)
                });
            rows = rows.child(self.render_list_row(
                SharedString::from(format!("queue-manager-row-{id}")),
                selected,
                dot,
                self.queue_label(queue, cx),
                move |this, window, cx| this.select_queue(id.clone(), window, cx),
                cx,
            ));
        }
        if creating {
            rows = rows.child(
                self.render_list_row(
                    SharedString::from("queue-manager-row-draft"),
                    true,
                    Icon::new(IconName::Plus)
                        .size(px(12.))
                        .text_color(colors.accent_foreground),
                    self.t("createQueueAction", cx),
                    |_, _, _| {},
                    cx,
                ),
            );
        }

        v_flex()
            .flex_none()
            .w(LIST_WIDTH)
            .h_full()
            .min_h_0()
            .px(px(8.))
            .py(px(12.))
            .bg(colors.background)
            .border_r_1()
            .border_color(colors.border.opacity(0.8))
            .child(
                h_flex()
                    .w_full()
                    .pl(px(10.))
                    .pr(px(4.))
                    .pb(px(8.))
                    .justify_between()
                    .items_center()
                    .child(self.label("sidebarQueues", cx))
                    .child(
                        Button::new("queue-manager-new")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip(self.t("createQueueAction", cx))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.new_queue(window, cx);
                            })),
                    ),
            )
            .child(rows.overflow_y_scrollbar())
    }

    /// 标题行：队列显示名 + 运行状态徽标（新建时只有标题）。
    fn render_header(&self, form: &QueueForm, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let title = match &form.queue_id {
            None => self.t("createQueueAction", cx),
            Some(id) => self
                .queues
                .iter()
                .find(|queue| &queue.queue_id == id)
                .map(|queue| self.queue_label(queue, cx))
                .unwrap_or_default(),
        };
        let badge = form.queue_id.as_ref().map(|_| {
            let (text, color) = if form.is_running {
                (self.t("queueRunningBadge", cx), cx.theme().success)
            } else {
                (self.t("queueStoppedBadge", cx), colors.muted_foreground)
            };
            div()
                .flex_none()
                .px(tokens.spacing.sm)
                .py(px(2.))
                .rounded(tokens.radius.full)
                .bg(color.opacity(0.12))
                .text_xs()
                .text_color(color)
                .child(text)
        });
        h_flex()
            .w_full()
            .flex_none()
            .px(tokens.spacing.lg)
            .pt(px(16.))
            .pb(px(12.))
            .gap(tokens.spacing.sm)
            .items_center()
            .border_b_1()
            .border_color(colors.border.opacity(0.5))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(title),
            )
            .children(badge)
    }

    /// 等宽栅格单元：标签 + 输入 + 可选说明；并排时 `items_start` 保证输入框对齐。
    fn render_field(
        &self,
        label_key: &str,
        input: &Entity<InputState>,
        hint_key: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(tokens.spacing.xs)
            .child(self.label(label_key, cx))
            .child(Input::new(input).with_size(Size::Medium).w_full())
            .when_some(hint_key, |this, key| this.child(self.hint(key, cx)))
    }

    fn render_name_field(
        &self,
        form: &QueueForm,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if form.is_builtin() {
            // 内置队列名称固定：标题已显示本地化名，这里只留说明。
            return self.hint("builtinQueueRenameHint", cx).into_any_element();
        }
        self.render_field("queueNameLabel", &form.name, None, cx)
            .into_any_element()
    }

    fn render_save_dir_field(
        &self,
        form: &QueueForm,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let picking = form.picking_dir;
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("queueSaveDir", cx))
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
                            .outline()
                            .small()
                            .h(CONTROL_HEIGHT)
                            .label(self.t("browse", cx))
                            .disabled(picking)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.pick_save_dir(window, cx);
                            })),
                    ),
            )
            .child(self.hint("queueDirInheritHint", cx))
    }

    fn render_time_field(
        &self,
        label_key: &str,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label(label_key, cx))
            .child(
                Input::new(input)
                    .with_size(Size::Medium)
                    .w(TIME_INPUT_WIDTH),
            )
    }

    fn render_schedule(
        &self,
        form: &QueueForm,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let enabled = form.schedule_enabled;
        let days = form.schedule_days;
        let column = v_flex().gap(tokens.spacing.md).child(
            h_flex()
                .gap(tokens.spacing.lg)
                .items_start()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(tokens.spacing.xxs)
                        .child(div().text_sm().child(self.t("queueScheduleEnable", cx)))
                        .child(self.hint("queueScheduleDesc", cx)),
                )
                .child(
                    Switch::new("queue-schedule-enabled")
                        .checked(enabled)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            if let Some(form) = &mut this.form {
                                form.schedule_enabled = *checked;
                            }
                            cx.notify();
                        })),
                ),
        );
        if !enabled {
            return column;
        }

        let mut days_row = h_flex().flex_wrap().gap(tokens.spacing.md);
        for (index, label) in self.t("weekdaysShort", cx).split(',').enumerate() {
            let bit = 1 << index;
            days_row = days_row.child(
                Checkbox::new(("queue-schedule-day", index))
                    .label(label.to_owned())
                    .checked(days & bit != 0)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        if let Some(form) = &mut this.form {
                            form.schedule_days = toggle_day_bit(form.schedule_days, bit, *checked);
                        }
                        cx.notify();
                    })),
            );
        }
        column
            .child(
                v_flex()
                    .gap(tokens.spacing.xs)
                    .child(
                        h_flex()
                            .gap(tokens.spacing.md)
                            .items_start()
                            .child(self.render_time_field(
                                "queueScheduleStartLabel",
                                &form.schedule_start,
                                cx,
                            ))
                            .child(self.render_time_field(
                                "queueScheduleStopLabel",
                                &form.schedule_stop,
                                cx,
                            )),
                    )
                    .child(self.hint("queueScheduleTimeHint", cx)),
            )
            .child(
                v_flex()
                    .gap(tokens.spacing.xs)
                    .child(self.label("queueScheduleDays", cx))
                    .child(days_row),
            )
    }

    fn render_body(&self, form: &QueueForm, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .id("queue-manager-body")
            .flex_1()
            .min_h_0()
            .w_full()
            .px(tokens.spacing.lg)
            .pt(px(16.))
            .pb(px(20.))
            .gap(tokens.spacing.md)
            .overflow_y_scrollbar()
            .child(self.render_name_field(form, cx))
            .child(
                h_flex()
                    .gap(tokens.spacing.md)
                    .items_start()
                    .child(self.render_field(
                        "queueSpeedLimit",
                        &form.speed_limit,
                        Some("queueSpeedLimitHint"),
                        cx,
                    ))
                    .child(self.render_field(
                        "queueUploadLimit",
                        &form.upload_limit,
                        Some("queueUploadLimitDesc"),
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap(tokens.spacing.md)
                    .items_start()
                    .child(self.render_field(
                        "queueMaxConcurrent",
                        &form.max_concurrent,
                        Some("queueMaxConcurrentHint"),
                        cx,
                    ))
                    .child(self.render_field(
                        "queueDefaultSegments",
                        &form.segments,
                        Some("queueDefaultSegmentsHint"),
                        cx,
                    )),
            )
            .child(self.render_save_dir_field(form, cx))
            .child(self.render_field(
                "queueDefaultUserAgent",
                &form.user_agent,
                Some("queueUaHint"),
                cx,
            ))
            .child(
                div()
                    .h(px(1.))
                    .w_full()
                    .bg(tokens.colors.border.opacity(0.5)),
            )
            .child(self.render_schedule(form, cx))
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(tokens.colors.destructive)
                        .child(error),
                )
            })
    }

    fn render_footer(&self, form: &QueueForm, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let is_creating = form.queue_id.is_none();
        let mut footer = h_flex()
            .w_full()
            .flex_none()
            .p(tokens.spacing.md)
            .items_center()
            .gap(tokens.spacing.xs)
            .border_t_1()
            .border_color(tokens.colors.border);
        if !is_creating && !form.is_builtin() {
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
                    .outline()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .label(if form.is_running {
                        self.t("stopQueueAction", cx)
                    } else {
                        self.t("startQueueAction", cx)
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_running(cx);
                    })),
            );
        }
        footer.child(div().flex_1()).child(
            Button::new("queue-manager-save")
                .primary()
                .small()
                .h(CONTROL_HEIGHT)
                .label(self.t("queueSaveAction", cx))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.save(window, cx);
                })),
        )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = active_theme(cx).tokens().clone();
        let column = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.colors.surface);
        let Some(form) = &self.form else {
            // 只有断连且快照尚未到达时才会没有表单：给出只读提示而不是空白面板。
            return column
                .items_center()
                .justify_center()
                .p(tokens.spacing.lg)
                .when(self.stale, |this| {
                    this.child(self.hint("localServiceDisconnected", cx))
                })
                .into_any_element();
        };
        column
            .when(self.stale, |this| {
                this.child(
                    div()
                        .w_full()
                        .px(tokens.spacing.lg)
                        .py(tokens.spacing.sm)
                        .text_xs()
                        .text_color(tokens.colors.muted_foreground)
                        .child(self.t("localServiceDisconnected", cx)),
                )
            })
            .child(self.render_header(form, cx))
            .child(self.render_body(form, cx))
            .child(self.render_footer(form, cx))
            .into_any_element()
    }
}

impl Render for QueueManagerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_form(window, cx);
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .size_full()
            .min_h_0()
            // `h_flex` 默认交叉轴居中；两列都要撑满高度。
            .items_stretch()
            .bg(tokens.colors.surface)
            .child(self.render_list(cx))
            .child(self.render_content(cx))
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
