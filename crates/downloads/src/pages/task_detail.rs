//! 任务详情：主窗口停靠面板与独立任务窗口共用同一套渲染逻辑。
//!
//! [`DetailMode::Docked`] 由 [`super::downloads::DownloadView`] 持有，不订阅会话，
//! 由宿主在事件后调用 [`TaskDetailView::sync`] 刷新；[`DetailMode::Window`] 由
//! app 侧独立窗口通过 [`crate::session::attach`]（[`SessionConsumer`] 三个同名
//! `pub fn`）订阅，自带一个私有 [`DownloadsController`] 维护任务列表。

use std::{collections::VecDeque, rc::Rc, sync::Arc, time::Duration, time::Instant};

use chrono::{DateTime, Local};
use fluxdown_protocol::{AgentEvent, AgentSnapshot, DaemonEvent, ServiceEvent, TaskDto};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    chart::AreaChart,
    h_flex,
    input::{InputState, NumberInput},
    switch::Switch,
    v_flex,
};

use crate::{
    controller::{DownloadsCommand, DownloadsController, DownloadsPort, SeedLimits},
    model::{RowKey, TaskProtocol, TaskState, TaskStore, format_bytes},
    pages::downloads::DownloadHostActions,
    strings::DownloadStrings,
};

/// 速度曲线保留的采样点数（每秒一次）。
const SPEED_HISTORY_CAPACITY: usize = 120;
/// 状态变迁日志环形缓冲容量。
const LOG_CAPACITY: usize = 200;
/// 任务窗口 Tab 区折叠状态（设备本地，`sync:false`）。
const TASK_WINDOW_COMPACT_PREF: &str = "desktop.task_window.compact";
/// `SeedLimits` 各字段的「跟随全局」哨兵（与 `native/protocol` 一致）。
const SEED_LIMIT_FOLLOW_GLOBAL: i64 = -2;

/// 面板承载方式：决定是否渲染顶部进度头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailMode {
    /// 主窗口停靠面板：不渲染进度头（列表已可见进度）。
    Docked,
    /// 独立任务窗口：渲染进度头 + 可折叠 Tab 区。
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetailTab {
    General,
    Seeding,
    Log,
    Advanced,
}

/// 面板对外事件：任务被删除 / 快照中不再存在时发出，宿主据此关闭承载窗口。
pub enum TaskDetailEvent {
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
struct DetailLogLine {
    at: SharedString,
    text: SharedString,
}

/// 置顶开关回调：`(task_id, next_pinned, window, cx)`，由独立任务窗口注入，
/// 内部以 `WindowKind::Floating` 重建窗口。停靠面板不注入（`None`）。
pub type PinToggle = Rc<dyn Fn(String, bool, &mut Window, &mut App)>;

pub struct TaskDetailView {
    task_id: String,
    mode: DetailMode,
    /// 仅 `DetailMode::Window` 持有：独立窗口自带的会话状态。
    controller: Option<DownloadsController>,
    store: Rc<TaskStore>,
    port: Arc<dyn DownloadsPort>,
    host: DownloadHostActions,
    translator: Entity<Translator>,
    strings: DownloadStrings,
    tab: DetailTab,
    /// 完整 DTO（校验和 / 代理 / 做种详情等 `DownloadTaskView` 未覆盖的字段）。
    /// 停靠模式由宿主经 [`Self::sync`] 提供；窗口模式来自私有 controller。
    dto: Option<TaskDto>,
    queue_names: Vec<(String, String)>,
    speed_history: VecDeque<(Instant, u64)>,
    log: VecDeque<DetailLogLine>,
    last_state: Option<TaskState>,
    /// 任务窗口 Tab 区是否折叠（停靠模式恒不生效）。
    compact: bool,
    pinned: bool,
    pin_toggle: Option<PinToggle>,
    closed: bool,
    last_error: Option<SharedString>,
    seed_ratio: Entity<InputState>,
    seed_post_ratio: Entity<InputState>,
    seed_time_limit: Entity<InputState>,
    seed_inactive_limit: Entity<InputState>,
    seed_upload_limit: Entity<InputState>,
}

impl EventEmitter<TaskDetailEvent> for TaskDetailView {}

impl TaskDetailView {
    /// 独立任务窗口构造（`DetailMode::Window`）：自建私有 `DownloadsController`，
    /// 不需要外部 store（`TaskStore` 是 `pub(crate)`，不能出现在跨 crate 公开签名里）。
    #[allow(
        clippy::too_many_arguments,
        reason = "assembles every session/host input the windowed detail view needs"
    )]
    pub fn new(
        translator: Entity<Translator>,
        task_id: String,
        port: Arc<dyn DownloadsPort>,
        host: DownloadHostActions,
        pinned: bool,
        pin_toggle: Option<PinToggle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_internal(
            translator,
            task_id,
            None,
            port,
            host,
            DetailMode::Window,
            pinned,
            pin_toggle,
            window,
            cx,
        )
    }

    /// 停靠面板构造（`DetailMode::Docked`）：借用宿主 `DownloadView` 已有的
    /// `Rc<TaskStore>`；仅同 crate 可调用。
    #[allow(
        clippy::too_many_arguments,
        reason = "assembles every session/host input the docked detail view needs"
    )]
    pub(crate) fn new_docked(
        translator: Entity<Translator>,
        task_id: String,
        store: Rc<TaskStore>,
        port: Arc<dyn DownloadsPort>,
        host: DownloadHostActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_internal(
            translator,
            task_id,
            Some(store),
            port,
            host,
            DetailMode::Docked,
            false,
            None,
            window,
            cx,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "assembles every session/host input the detail view needs across docked and windowed modes"
    )]
    fn new_internal(
        translator: Entity<Translator>,
        task_id: String,
        store: Option<Rc<TaskStore>>,
        port: Arc<dyn DownloadsPort>,
        host: DownloadHostActions,
        mode: DetailMode,
        pinned: bool,
        pin_toggle: Option<PinToggle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let strings = DownloadStrings::from_translator(translator.read(cx));
        let controller =
            matches!(mode, DetailMode::Window).then(|| DownloadsController::new(Arc::clone(&port)));
        let store = match &controller {
            Some(controller) => Rc::clone(controller.store()),
            None => store.unwrap_or_default(),
        };
        let seed_ratio = Self::seed_input(window, cx);
        let seed_post_ratio = Self::seed_input(window, cx);
        let seed_time_limit = Self::seed_input(window, cx);
        let seed_inactive_limit = Self::seed_input(window, cx);
        let seed_upload_limit = Self::seed_input(window, cx);

        cx.observe(&translator, |this, translator, cx| {
            this.strings = DownloadStrings::from_translator(translator.read(cx));
            cx.notify();
        })
        .detach();

        let this = Self {
            task_id,
            mode,
            controller,
            store,
            port,
            host,
            translator,
            strings,
            tab: DetailTab::General,
            dto: None,
            queue_names: Vec::new(),
            speed_history: VecDeque::new(),
            log: VecDeque::new(),
            last_state: None,
            compact: true,
            pinned,
            pin_toggle,
            closed: false,
            last_error: None,
            seed_ratio,
            seed_post_ratio,
            seed_time_limit,
            seed_inactive_limit,
            seed_upload_limit,
        };
        Self::spawn_speed_ticker(cx);
        this
    }

    fn seed_input(window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        cx.new(|cx| {
            InputState::new(window, cx)
                .validate(|text, _| text.trim().is_empty() || text.trim().parse::<i64>().is_ok())
        })
    }

    fn spawn_speed_ticker(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.sample_speed(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// 切换承载的任务（停靠面板双击另一行时调用）；随后宿主应立即调用 [`Self::sync`]。
    pub fn set_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        if self.task_id == task_id {
            return;
        }
        self.task_id = task_id;
        self.tab = DetailTab::General;
        self.dto = None;
        self.speed_history.clear();
        self.log.clear();
        self.last_state = None;
        self.closed = false;
        cx.notify();
    }

    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// 当前任务文件名（用于独立窗口标题）；未解析或任务不存在时为 `None`。
    #[must_use]
    pub fn file_name(&self) -> Option<String> {
        self.store
            .get(&RowKey::Local(self.task_id.clone()))
            .map(|row| row.name.clone())
            .filter(|name| !name.is_empty())
    }

    /// 停靠面板：宿主在事件后传入最新 DTO（校验和 / 代理等字段来源）刷新视图。
    pub fn sync(&mut self, dto: Option<TaskDto>, cx: &mut Context<Self>) {
        self.dto = dto;
        self.refresh_from_store(cx);
    }

    /// 停靠面板：宿主同步队列名（`DownloadView::sync_delegate_context` 一并调用）。
    pub fn set_queue_names(&mut self, queues: Vec<(String, String)>, cx: &mut Context<Self>) {
        if self.queue_names != queues {
            self.queue_names = queues;
            cx.notify();
        }
    }

    fn current_dto(&self) -> Option<&TaskDto> {
        match &self.controller {
            Some(controller) => controller.task_dto(&self.task_id),
            None => self.dto.as_ref(),
        }
    }

    fn queue_label(&self, queue_id: &str) -> Option<String> {
        if queue_id.is_empty() {
            return None;
        }
        match &self.controller {
            Some(controller) => controller.queue_name(queue_id).map(str::to_owned),
            None => self
                .queue_names
                .iter()
                .find(|(id, _)| id == queue_id)
                .map(|(_, name)| name.clone()),
        }
    }

    fn is_bt(&self) -> bool {
        self.store
            .get(&RowKey::Local(self.task_id.clone()))
            .is_some_and(|row| row.protocol == TaskProtocol::Bt)
    }

    fn t(&self, cx: &App, key: &str) -> SharedString {
        SharedString::from(self.translator.read(cx).text(key).to_owned())
    }

    fn refresh_from_store(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            self.close(cx);
            return;
        };
        let state = row.state;
        drop(row);
        if let Some(previous) = self.last_state
            && previous != state
        {
            let line = transition_log_line(&self.strings, previous, state, Local::now());
            push_bounded(&mut self.log, line, LOG_CAPACITY);
        }
        self.last_state = Some(state);
        cx.notify();
    }

    fn sample_speed(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return;
        };
        let speed = row.speed_bytes_per_second.unwrap_or(0);
        drop(row);
        push_bounded(
            &mut self.speed_history,
            (Instant::now(), speed),
            SPEED_HISTORY_CAPACITY,
        );
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if !self.closed {
            self.closed = true;
            cx.emit(TaskDetailEvent::Closed);
        }
    }

    // ---- SessionConsumer (独立任务窗口经 `attach` 驱动；停靠面板不调用) ----

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        if let Some(controller) = &mut self.controller {
            controller.replace_snapshot(snapshot);
            self.dto = controller.task_dto(&self.task_id).cloned();
            self.compact = controller.preference_bool(TASK_WINDOW_COMPACT_PREF, true);
        }
        self.refresh_from_store(cx);
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        if let ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::TaskDeleted { task_id })) = event
            && *task_id == self.task_id
        {
            self.close(cx);
            return;
        }
        let Some(controller) = &mut self.controller else {
            cx.notify();
            return;
        };
        let changed = controller.apply_event(event);
        if changed {
            self.dto = controller.task_dto(&self.task_id).cloned();
            self.refresh_from_store(cx);
        } else {
            cx.notify();
        }
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        if let Some(controller) = &mut self.controller {
            controller.mark_stale();
        }
        self.last_error = Some(self.strings.disconnected.clone());
        cx.notify();
    }

    // ---- actions ----

    fn select_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
    }

    fn run_command(&mut self, command: DownloadsCommand, cx: &mut Context<Self>) {
        let future = self.port.execute(command);
        cx.spawn(async move |this, cx| {
            let failed = future.await.is_err();
            let _ = this.update(cx, |this, cx| {
                if failed {
                    this.last_error = Some(this.strings.action_failed.clone());
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        self.run_command(
            DownloadsCommand::RevealTask {
                task_id: self.task_id.clone(),
            },
            cx,
        );
    }

    fn open_file(&mut self, cx: &mut Context<Self>) {
        self.run_command(
            DownloadsCommand::OpenTask {
                task_id: self.task_id.clone(),
            },
            cx,
        );
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return;
        };
        let active = matches!(row.state, TaskState::Downloading | TaskState::Pending);
        drop(row);
        let command = if active {
            DownloadsCommand::Pause {
                task_id: self.task_id.clone(),
            }
        } else {
            DownloadsCommand::Resume {
                task_id: self.task_id.clone(),
            }
        };
        self.run_command(command, cx);
    }

    fn copy_link(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return;
        };
        let url = row.share_url().to_owned();
        drop(row);
        cx.write_to_clipboard(ClipboardItem::new_string(url));
    }

    fn save_seed_limits(&mut self, cx: &mut Context<Self>) {
        let limits = SeedLimits {
            ratio_limit_milli: parse_seed_limit(&self.seed_ratio.read(cx).value()),
            post_ratio_limit_milli: parse_seed_limit(&self.seed_post_ratio.read(cx).value()),
            seed_time_limit_minutes: parse_seed_limit(&self.seed_time_limit.read(cx).value()),
            inactive_time_limit_minutes: parse_seed_limit(
                &self.seed_inactive_limit.read(cx).value(),
            ),
            upload_limit_bps: self
                .seed_upload_limit
                .read(cx)
                .value()
                .trim()
                .parse::<i64>()
                .map(|kbps| kbps.saturating_mul(1024))
                .unwrap_or(0),
        };
        self.run_command(
            DownloadsCommand::SetSeedLimits {
                task_id: self.task_id.clone(),
                limits,
            },
            cx,
        );
    }

    fn toggle_compact(&mut self, cx: &mut Context<Self>) {
        self.compact = !self.compact;
        let future = self.port.execute(DownloadsCommand::SetLocalPreference {
            key: TASK_WINDOW_COMPACT_PREF,
            value: serde_json::Value::Bool(self.compact),
        });
        cx.background_spawn(async move {
            let _ = future.await;
        })
        .detach();
        cx.notify();
    }

    fn toggle_pinned(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(toggle) = self.pin_toggle.clone() else {
            return;
        };
        let next = !self.pinned;
        toggle(self.task_id.clone(), next, window, cx);
    }

    fn open_group(&mut self, group_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opener) = self.host.open_group_window.clone() {
            opener(group_id, window, cx);
        }
    }

    // ---- rendering ----

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let is_bt = self.is_bt();
        let candidates: [(DetailTab, &'static str); 4] = [
            (DetailTab::General, "detailTabGeneral"),
            (DetailTab::Seeding, "tabSeeding"),
            (DetailTab::Log, "detailTabLog"),
            (DetailTab::Advanced, "detailTabAdvanced"),
        ];
        h_flex()
            .gap(tokens.spacing.xs)
            .px(tokens.spacing.md)
            .py(tokens.spacing.sm)
            .border_b_1()
            .border_color(tokens.colors.border)
            .children(
                candidates
                    .into_iter()
                    .enumerate()
                    .filter(|(_, (tab, _))| *tab != DetailTab::Seeding || is_bt)
                    .map(|(index, (tab, key))| {
                        let active = self.tab == tab;
                        let label = self.t(cx, key);
                        Button::new(("detail-tab", index))
                            .ghost()
                            .compact()
                            .small()
                            .when(active, gpui_component::button::ButtonVariants::primary)
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| this.select_tab(tab, cx)))
                    }),
            )
    }

    fn render_progress_head(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return div().into_any_element();
        };
        let name = SharedString::from(row.name.clone());
        let progress_label = SharedString::from(row.progress_label.clone());
        let progress = row.progress;
        let speed = row
            .speed_bytes_per_second
            .filter(|speed| *speed > 0)
            .map(|speed| SharedString::from(format!("{}/s", format_bytes(speed))))
            .unwrap_or_else(|| SharedString::from("—"));
        let eta = row
            .eta_seconds
            .map(|seconds| self.strings.format_eta(seconds))
            .unwrap_or_else(|| SharedString::from("—"));
        let size_label = SharedString::from(format!(
            "{} / {}",
            format_bytes(row.downloaded_bytes),
            row.size
        ));
        let active = matches!(row.state, TaskState::Downloading | TaskState::Pending);
        let completed = row.state == TaskState::Completed;
        let status_color = match row.state {
            TaskState::Completed => cx.theme().success,
            TaskState::Failed => cx.theme().danger,
            TaskState::Paused => cx.theme().warning,
            TaskState::Downloading | TaskState::Pending => tokens.colors.primary,
        };
        drop(row);

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
                    .child(name),
            )
            .child(
                div()
                    .relative()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .overflow_hidden()
                    .bg(tokens.colors.muted)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .w(relative(progress))
                            .bg(status_color),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .gap(tokens.spacing.sm)
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(div().child(progress_label))
                    .child(div().flex_1().text_right().child(size_label))
                    .child(div().child(speed))
                    .child(div().child(eta)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .pt(tokens.spacing.xs)
                    .child(
                        Button::new("detail-toggle-pause")
                            .small()
                            .ghost()
                            .icon(if active {
                                IconName::Pause
                            } else {
                                IconName::Play
                            })
                            .tooltip(if active {
                                self.strings.pause.clone()
                            } else {
                                self.strings.resume.clone()
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                    )
                    .when(completed, |this| {
                        this.child(
                            Button::new("detail-open-file")
                                .small()
                                .ghost()
                                .icon(IconName::File)
                                .tooltip(self.strings.open_file.clone())
                                .on_click(cx.listener(|this, _, _, cx| this.open_file(cx))),
                        )
                    })
                    .child(
                        Button::new("detail-open-folder")
                            .small()
                            .ghost()
                            .icon(IconName::FolderOpen)
                            .tooltip(self.strings.open_folder.clone())
                            .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
                    )
                    .when(
                        cfg!(target_os = "macos") && self.pin_toggle.is_some(),
                        |this| {
                            this.child(
                                Switch::new("detail-pin")
                                    .checked(self.pinned)
                                    .label(self.t(cx, "taskWindowPinOnTop"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_pinned(window, cx)
                                    })),
                            )
                        },
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("detail-toggle-compact")
                            .small()
                            .ghost()
                            .icon(if self.compact {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronUp
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_compact(cx))),
                    ),
            )
            .into_any_element()
    }

    fn info_row(
        tokens: &fluxdown_ui_theme::SemanticThemeTokens,
        label: SharedString,
        value: SharedString,
    ) -> impl IntoElement {
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
    }

    fn render_general(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = active_theme(cx).tokens().clone();
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return div().into_any_element();
        };
        let dto = self.current_dto();
        let not_set = self.t(cx, "detailNotSet");
        let follow_global = self.t(cx, "detailFollowGlobal");
        let checksum = dto
            .map(|dto| dto.checksum.clone())
            .filter(|value| !value.is_empty())
            .map(SharedString::from)
            .unwrap_or_else(|| not_set.clone());
        let proxy = dto
            .map(|dto| dto.proxy_url.clone())
            .filter(|value| !value.is_empty())
            .map(SharedString::from)
            .unwrap_or_else(|| follow_global.clone());
        let ignore_tls = dto.is_some_and(|dto| dto.ignore_tls_errors);
        let queue_label = self
            .queue_label(&row.queue_id)
            .map(SharedString::from)
            .unwrap_or_else(|| SharedString::from(row.queue_id.clone()));
        let group_id = row.group_id.clone();
        let boosted = row.boosted;

        let mut list = v_flex()
            .gap(tokens.spacing.xs)
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(tokens.typography.sm.weight)
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(row.name.clone())),
                    )
                    .child(
                        div()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(format!("{} · {}", row.protocol.label(), row.source_site())),
                    )
                    .when(boosted, |this| {
                        this.child(
                            div()
                                .text_size(tokens.typography.xs.size)
                                .text_color(cx.theme().warning)
                                .child(self.t(cx, "detailBoostActive")),
                        )
                    }),
            )
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoStatus"),
                self.strings.state_label(row.state),
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoSize"),
                SharedString::from(row.size.clone()),
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoDownloaded"),
                SharedString::from(format_bytes(row.downloaded_bytes)),
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoSpeed"),
                row.speed_bytes_per_second
                    .filter(|speed| *speed > 0)
                    .map(|speed| SharedString::from(format!("{}/s", format_bytes(speed))))
                    .unwrap_or_else(|| SharedString::from("—")),
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoRemaining"),
                row.eta_seconds
                    .map(|seconds| self.strings.format_eta(seconds))
                    .unwrap_or_else(|| SharedString::from("—")),
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoStartedAt"),
                self.strings.format_created(row.created_at_secs),
            ))
            .when(row.completed_at_secs > 0, |this| {
                this.child(Self::info_row(
                    &tokens,
                    self.t(cx, "infoCompletedAt"),
                    self.strings.format_created(row.completed_at_secs),
                ))
            })
            .child(
                h_flex()
                    .id("detail-reveal-row")
                    .justify_between()
                    .gap(tokens.spacing.md)
                    .py(tokens.spacing.xxs)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.reveal(cx)))
                    .child(
                        div()
                            .flex_none()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(self.t(cx, "infoPath")),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(tokens.typography.xs.size)
                            .text_right()
                            .text_color(tokens.colors.primary)
                            .truncate()
                            .child(SharedString::from(row.save_dir.clone())),
                    ),
            )
            .child(Self::info_row(
                &tokens,
                self.t(cx, "taskQueueLabel"),
                queue_label,
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "taskChecksum"),
                checksum,
            ))
            .child(Self::info_row(&tokens, self.t(cx, "taskProxy"), proxy));

        if ignore_tls {
            list = list.child(Self::info_row(
                &tokens,
                self.t(cx, "taskIgnoreTlsErrors"),
                SharedString::from("✓"),
            ));
        }
        if !row.error_message.is_empty() {
            list = list.child(Self::info_row(
                &tokens,
                self.t(cx, "infoError"),
                SharedString::from(row.error_message.clone()),
            ));
        }
        drop(row);
        if !group_id.is_empty() {
            list = list.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap(tokens.spacing.md)
                    .py(tokens.spacing.xxs)
                    .child(
                        div()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(self.t(cx, "groupMemberOfLabel")),
                    )
                    .child(
                        Button::new("detail-open-group")
                            .ghost()
                            .compact()
                            .small()
                            .h(CONTROL_HEIGHT)
                            .label(self.strings.open_group_in_window.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_group(group_id.clone(), window, cx);
                            })),
                    ),
            );
        }
        list.child(
            h_flex().gap(tokens.spacing.sm).pt(tokens.spacing.sm).child(
                Button::new("detail-copy-link")
                    .ghost()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .icon(IconName::Copy)
                    .label(self.strings.copy_url.clone())
                    .on_click(cx.listener(|this, _, _, cx| this.copy_link(cx))),
            ),
        )
        .into_any_element()
    }

    fn render_seeding(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = active_theme(cx).tokens().clone();
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return div().into_any_element();
        };
        let uploaded_bytes = row.uploaded_bytes.max(0) as u64;
        let size_bytes = row.size_bytes;
        let seeding_status = row.seeding_status;
        drop(row);
        let seeding_time_secs = self
            .current_dto()
            .map(|dto| dto.seeding_time_secs)
            .unwrap_or(0);
        let status_label = self.t(cx, seeding_status_key(seeding_status));
        let ratio = if size_bytes > 0 {
            format!("{:.2}", uploaded_bytes as f64 / size_bytes as f64)
        } else {
            "—".to_owned()
        };
        let duration = format_duration(self.translator.read(cx), seeding_time_secs);
        let chart_points: Vec<(usize, f64)> = self
            .speed_history
            .iter()
            .enumerate()
            .map(|(index, (_, speed))| (index, *speed as f64))
            .collect();
        let has_chart = chart_points.len() > 1;

        v_flex()
            .gap(tokens.spacing.xs)
            .child(Self::info_row(
                &tokens,
                self.t(cx, "seedingStatus"),
                status_label,
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "uploadedTotal"),
                SharedString::from(format_bytes(uploaded_bytes)),
            ))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "seedRatio"),
                SharedString::from(ratio),
            ))
            .child(Self::info_row(&tokens, self.t(cx, "seedTime"), duration))
            .when(has_chart, |this| {
                this.child(
                    div().h(px(64.)).w_full().pt(tokens.spacing.sm).child(
                        AreaChart::new(chart_points)
                            .x(|point: &(usize, f64)| SharedString::from(point.0.to_string()))
                            .y(|point: &(usize, f64)| point.1)
                            .stroke(tokens.colors.primary)
                            .linear()
                            .x_axis(false)
                            .grid(false),
                    ),
                )
            })
            .child(
                div()
                    .h(px(1.))
                    .my(tokens.spacing.sm)
                    .bg(tokens.colors.border),
            )
            .child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .font_weight(tokens.typography.sm.weight)
                    .child(self.t(cx, "btSeedLimitsTitle")),
            )
            .child(self.render_seed_field(cx, "btSeedRatioLimit", self.seed_ratio.clone()))
            .child(self.render_seed_field(cx, "btSeedPostRatioLimit", self.seed_post_ratio.clone()))
            .child(self.render_seed_field(cx, "btSeedTimeLimit", self.seed_time_limit.clone()))
            .child(self.render_seed_field(
                cx,
                "btSeedInactiveTimeLimit",
                self.seed_inactive_limit.clone(),
            ))
            .child(self.render_seed_field(cx, "btSeedUploadLimit", self.seed_upload_limit.clone()))
            .child(
                Button::new("detail-save-seed-limits")
                    .primary()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .label(self.strings.confirm.clone())
                    .on_click(cx.listener(|this, _, _, cx| this.save_seed_limits(cx))),
            )
            .into_any_element()
    }

    fn render_seed_field(
        &self,
        cx: &mut Context<Self>,
        label_key: &str,
        state: Entity<InputState>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .justify_between()
            .items_center()
            .gap(tokens.spacing.md)
            .py(tokens.spacing.xxs)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t(cx, label_key)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(120.))
                    .child(NumberInput::new(&state).with_size(Size::Medium)),
            )
    }

    fn render_log(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = active_theme(cx).tokens().clone();
        let hint = self.t(cx, "detailLogHint");
        if self.log.is_empty() {
            return v_flex()
                .gap(tokens.spacing.xs)
                .child(
                    div()
                        .text_size(tokens.typography.xs.size)
                        .text_color(tokens.colors.muted_foreground)
                        .child(hint),
                )
                .child(
                    div()
                        .text_size(tokens.typography.xs.size)
                        .text_color(tokens.colors.muted_foreground)
                        .child(self.t(cx, "detailLogEmpty")),
                )
                .into_any_element();
        }
        v_flex()
            .gap(px(2.))
            .child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .pb(tokens.spacing.xs)
                    .child(hint),
            )
            .children(self.log.iter().rev().map(|line| {
                h_flex()
                    .gap(tokens.spacing.sm)
                    .child(
                        div()
                            .flex_none()
                            .w(px(64.))
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(line.at.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(tokens.typography.xs.size)
                            .child(line.text.clone()),
                    )
            }))
            .into_any_element()
    }

    fn render_advanced(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = active_theme(cx).tokens().clone();
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return div().into_any_element();
        };
        let dto = self.current_dto();
        let referrer = if row.referrer.is_empty() {
            self.t(cx, "detailNotSet")
        } else {
            SharedString::from(row.referrer.clone())
        };
        let proxy = dto
            .map(|dto| dto.proxy_url.clone())
            .filter(|value| !value.is_empty())
            .map(SharedString::from)
            .unwrap_or_else(|| self.t(cx, "detailFollowGlobal"));
        let ignore_tls = dto.is_some_and(|dto| dto.ignore_tls_errors);
        drop(row);
        v_flex()
            .gap(tokens.spacing.xs)
            .child(Self::info_row(
                &tokens,
                self.t(cx, "infoSourcePage"),
                referrer,
            ))
            .child(Self::info_row(&tokens, self.t(cx, "taskProxy"), proxy))
            .child(Self::info_row(
                &tokens,
                self.t(cx, "taskIgnoreTlsErrors"),
                SharedString::from(if ignore_tls { "✓" } else { "—" }),
            ))
            .into_any_element()
    }
}

impl gpui::Render for TaskDetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let has_row = self
            .store
            .get(&RowKey::Local(self.task_id.clone()))
            .is_some();
        if !has_row {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_size(tokens.typography.sm.size)
                .text_color(tokens.colors.muted_foreground)
                .child(self.t(cx, "selectTaskHint"));
        }
        let show_tabs = self.mode != DetailMode::Window || !self.compact;
        v_flex()
            .size_full()
            .min_h_0()
            .bg(tokens.colors.surface)
            .when(self.mode == DetailMode::Window, |this| {
                this.child(self.render_progress_head(cx))
            })
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
            .when(show_tabs, |this| {
                this.child(self.render_tab_bar(cx)).child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .p(tokens.spacing.md)
                        .child(match self.tab {
                            DetailTab::General => self.render_general(cx),
                            DetailTab::Seeding => self.render_seeding(cx),
                            DetailTab::Log => self.render_log(cx),
                            DetailTab::Advanced => self.render_advanced(cx),
                        }),
                )
            })
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T, capacity: usize) {
    queue.push_back(item);
    while queue.len() > capacity {
        queue.pop_front();
    }
}

fn transition_log_line(
    strings: &DownloadStrings,
    from: TaskState,
    to: TaskState,
    at: DateTime<Local>,
) -> DetailLogLine {
    DetailLogLine {
        at: SharedString::from(at.format("%H:%M:%S").to_string()),
        text: SharedString::from(format!(
            "{} → {}",
            strings.state_label(from),
            strings.state_label(to)
        )),
    }
}

fn parse_seed_limit(text: &str) -> i64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        SEED_LIMIT_FOLLOW_GLOBAL
    } else {
        trimmed.parse::<i64>().unwrap_or(SEED_LIMIT_FOLLOW_GLOBAL)
    }
}

fn seeding_status_key(code: i32) -> &'static str {
    match code {
        1 => "seedingStatusSeeding",
        2 => "seedingStatusRatioReached",
        3 => "seedingStatusTimeReached",
        4 => "seedingStatusUserStopped",
        5 => "seedingStatusDeleted",
        6 => "seedingStatusSessionReleased",
        7 => "seedingStatusInactiveReached",
        8 => "seedingStatusQueued",
        _ => "seedingStatusNone",
    }
}

fn format_duration(translator: &Translator, total_seconds: i64) -> SharedString {
    let minutes = total_seconds.max(0) / 60;
    if minutes < 60 {
        SharedString::from(format!("{minutes} {}", translator.text("timeUnitMinutes")))
    } else {
        let hours = minutes / 60;
        let remaining = minutes % 60;
        SharedString::from(format!(
            "{hours} {} {remaining} {}",
            translator.text("timeUnitHours"),
            translator.text("timeUnitMinutes")
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc, time::Instant};

    use chrono::TimeZone;
    use fluxdown_ui_i18n::I18nCatalog;

    use super::{
        DetailLogLine, LOG_CAPACITY, SPEED_HISTORY_CAPACITY, push_bounded, transition_log_line,
    };
    use crate::{model::TaskState, strings::DownloadStrings};

    #[test]
    fn log_ring_buffer_evicts_oldest_beyond_capacity() {
        let mut log: VecDeque<DetailLogLine> = VecDeque::new();
        for index in 0..(LOG_CAPACITY + 5) {
            push_bounded(
                &mut log,
                DetailLogLine {
                    at: format!("{index}").into(),
                    text: format!("line-{index}").into(),
                },
                LOG_CAPACITY,
            );
        }
        assert_eq!(log.len(), LOG_CAPACITY);
        assert_eq!(log.front().expect("front").text.as_ref(), "line-5");
        assert_eq!(
            log.back().expect("back").text.as_ref(),
            format!("line-{}", LOG_CAPACITY + 4)
        );
    }

    #[test]
    fn speed_history_retains_last_120_points() {
        let mut history: VecDeque<(Instant, u64)> = VecDeque::new();
        let base = Instant::now();
        for speed in 0..130u64 {
            push_bounded(&mut history, (base, speed), SPEED_HISTORY_CAPACITY);
        }
        assert_eq!(history.len(), SPEED_HISTORY_CAPACITY);
        assert_eq!(history.front().expect("front").1, 10);
        assert_eq!(history.back().expect("back").1, 129);
    }

    #[test]
    fn transition_records_localized_state_arrow() -> Result<(), fluxdown_ui_i18n::I18nError> {
        let catalog = Arc::new(I18nCatalog::load_embedded()?);
        let strings = DownloadStrings::from_translator(&catalog.translator("en"));
        let at = chrono::Local
            .with_ymd_and_hms(2026, 3, 18, 9, 5, 3)
            .single()
            .expect("valid local time");
        let line = transition_log_line(&strings, TaskState::Downloading, TaskState::Paused, at);
        assert_eq!(line.at.as_ref(), "09:05:03");
        assert!(line.text.contains('→'));
        Ok(())
    }
}
