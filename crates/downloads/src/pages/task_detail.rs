//! 任务详情：主窗口停靠面板与独立任务窗口共用同一套渲染逻辑。
//!
//! [`DetailMode::Docked`] 由 [`super::downloads::DownloadView`] 持有，不订阅会话，
//! 由宿主在事件后调用 [`TaskDetailView::sync`] 刷新；[`DetailMode::Window`] 由
//! app 侧独立窗口通过 [`crate::session::attach`]（[`SessionConsumer`] 三个同名
//! `pub fn`）订阅，自带一个私有 [`DownloadsController`] 维护任务列表。
#[path = "task_detail_activity.rs"]
mod activity;

use std::{collections::VecDeque, rc::Rc, sync::Arc, time::Duration, time::Instant};

use chrono::{Local, TimeZone as _};
use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, ServiceEvent, TaskDto, TaskRuntimeDto,
};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    chart::AreaChart,
    h_flex,
    input::{InputState, NumberInput},
    v_flex,
};

use activity::ActivityFeed;

use crate::{
    components::segment_progress::render_segment_progress,
    controller::{
        DownloadsCommand, DownloadsController, DownloadsPort, DownloadsResult, SeedLimits,
    },
    model::{DownloadTaskView, RowKey, TaskProtocol, TaskState, TaskStore, format_bytes},
    pages::downloads::DownloadHostActions,
    strings::DownloadStrings,
};

/// 速度曲线保留的采样点数（每秒一次）。
const SPEED_HISTORY_CAPACITY: usize = 120;
/// `SeedLimits` 各字段的「跟随全局」哨兵（与 `native/protocol` 一致）。
const SEED_LIMIT_FOLLOW_GLOBAL: i64 = -2;

/// 面板承载方式：决定是否渲染顶部进度头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailMode {
    /// 主窗口停靠面板：不渲染进度头（列表已可见进度）。
    Docked,
    /// 独立任务窗口：渲染进度头和完整详情 Tab 区。
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
    activity: ActivityFeed,
    runtime: Option<TaskRuntimeDto>,
    activity_stale: bool,
    activity_online: bool,
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
    pub fn new(
        translator: Entity<Translator>,
        task_id: String,
        port: Arc<dyn DownloadsPort>,
        host: DownloadHostActions,
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
            window,
            cx,
        )
    }

    /// 停靠面板构造（`DetailMode::Docked`）：借用宿主 `DownloadView` 已有的
    /// `Rc<TaskStore>`；仅同 crate 可调用。
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
            activity: ActivityFeed::new(task_id.clone()),
            runtime: None,
            activity_stale: false,
            activity_online: true,
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
        self.activity.switch_task(self.task_id.clone());
        self.activity_stale = false;
        self.speed_history.clear();
        self.runtime = None;
        self.closed = false;
        self.fetch_activity(cx);
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
        self.fetch_activity(cx);
    }

    /// 停靠面板与独立窗口共用的即时传输状态（不是持久历史）。
    pub fn sync_runtime(&mut self, runtime: Option<TaskRuntimeDto>, cx: &mut Context<Self>) {
        if self.runtime != runtime {
            self.runtime = runtime;
            cx.notify();
        }
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
        if self
            .store
            .get(&RowKey::Local(self.task_id.clone()))
            .is_none()
        {
            self.close(cx);
            return;
        }
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
            self.runtime = controller.task_runtime(&self.task_id).cloned();
        }
        if snapshot.daemon_connected {
            self.activity_online = true;
            self.last_error = None;
            if self.activity.loaded() || self.activity_stale {
                self.activity.reconnect();
            }
            self.activity_stale = false;
        } else {
            self.activity_online = false;
            self.activity_stale = true;
            self.activity.suspend();
            self.runtime = None;
            self.last_error = Some(self.strings.disconnected.clone());
        }
        self.refresh_from_store(cx);
        self.fetch_activity(cx);
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        if let ServiceEvent::Agent(AgentEvent::DaemonConnectionChanged(connected)) = event {
            if *connected {
                self.activity_online = true;
                self.activity.reconnect();
                self.activity_stale = false;
                self.last_error = None;
                self.fetch_activity(cx);
            } else {
                self.mark_stale(cx);
            }
        }
        if let ServiceEvent::Agent(AgentEvent::Daemon(daemon_event)) = event {
            match daemon_event {
                DaemonEvent::TaskDeleted { task_id } if *task_id == self.task_id => {
                    self.close(cx);
                    return;
                }
                DaemonEvent::TaskActivityAdded(entry) => {
                    if self.activity.add(entry.clone()) {
                        cx.notify();
                    }
                }
                DaemonEvent::TaskRuntimeChanged(runtime) if runtime.task_id == self.task_id => {
                    self.sync_runtime(Some(runtime.clone()), cx);
                }
                _ => {}
            }
        }
        if let Some(controller) = &mut self.controller {
            let changed = controller.apply_event(event);
            if changed {
                self.dto = controller.task_dto(&self.task_id).cloned();
                self.refresh_from_store(cx);
            }
        }
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        if let Some(controller) = &mut self.controller {
            controller.mark_stale();
        }
        self.runtime = None;
        self.activity_stale = true;
        self.activity.suspend();
        self.activity_online = false;
        self.last_error = Some(self.strings.disconnected.clone());
        cx.notify();
    }

    fn fetch_activity(&mut self, cx: &mut Context<Self>) {
        if self.closed || !self.activity_online {
            return;
        }
        let Some(ticket) = self.activity.begin() else {
            return;
        };
        let future = self
            .port
            .execute(DownloadsCommand::TaskActivity(ticket.query.clone()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let page = match future.await {
                Ok(DownloadsResult::TaskActivity(page)) => Some(page),
                _ => None,
            };
            let _ = this.update(cx, |this, cx| {
                if this.activity.finish(&ticket, page) {
                    cx.notify();
                    this.fetch_activity(cx);
                }
            });
        })
        .detach();
    }

    fn load_older_activity(&mut self, cx: &mut Context<Self>) {
        self.activity.load_older();
        self.fetch_activity(cx);
    }

    fn retry_activity(&mut self, cx: &mut Context<Self>) {
        self.activity.retry();
        self.fetch_activity(cx);
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

    fn render_completed_head(
        &self,
        row: &DownloadTaskView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let tokens = active_theme(cx).tokens().clone();
        let size = if row.size_bytes > 0 {
            row.size.clone()
        } else {
            format_bytes(row.downloaded_bytes)
        };
        v_flex()
            .gap(tokens.spacing.md)
            .p(tokens.spacing.md)
            .border_b_1()
            .border_color(tokens.colors.border)
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.md)
                    .child(
                        Icon::new(IconName::Check)
                            .size(px(28.))
                            .text_color(cx.theme().success),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(tokens.spacing.xs)
                            .child(
                                div()
                                    .text_size(tokens.typography.sm.size)
                                    .font_weight(tokens.typography.sm.weight)
                                    .text_color(cx.theme().success)
                                    .child(self.strings.state_label(TaskState::Completed)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(tokens.typography.sm.size)
                                    .child(SharedString::from(row.name.clone())),
                            )
                            .child(
                                div()
                                    .text_size(tokens.typography.xs.size)
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(size),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(tokens.spacing.sm)
                    .child(
                        Button::new("detail-open-file")
                            .primary()
                            .small()
                            .icon(IconName::File)
                            .label(self.strings.open_file.clone())
                            .disabled(row.file_missing)
                            .on_click(cx.listener(|this, _, _, cx| this.open_file(cx))),
                    )
                    .child(
                        Button::new("detail-open-folder")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label(self.strings.open_folder.clone())
                            .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_progress_head(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let Some(row) = self.store.get(&RowKey::Local(self.task_id.clone())) else {
            return div().into_any_element();
        };
        if row.state == TaskState::Completed {
            return self.render_completed_head(&row, cx);
        }
        let downloading = row.state == TaskState::Downloading;
        let name = SharedString::from(row.name.clone());
        let progress_label = SharedString::from(row.progress_label.clone());
        let progress = row.progress;
        let speed = row
            .speed_bytes_per_second
            .filter(|_| downloading)
            .map(|speed| SharedString::from(format!("{}/s", format_bytes(speed))));
        let eta = row
            .eta_seconds
            .filter(|_| downloading)
            .map(|seconds| self.strings.format_eta(seconds));
        let size_label = SharedString::from(if row.size_bytes > 0 {
            format!("{} / {}", format_bytes(row.downloaded_bytes), row.size)
        } else {
            format_bytes(row.downloaded_bytes)
        });
        let active = matches!(row.state, TaskState::Downloading | TaskState::Pending);
        let status_color = match row.state {
            TaskState::Completed => cx.theme().success,
            TaskState::Failed => cx.theme().danger,
            TaskState::Paused => cx.theme().warning,
            TaskState::Downloading | TaskState::Pending => tokens.colors.primary,
        };
        let active_transfers = row.active_transfers().filter(|_| downloading).map(|count| {
            self.runtime
                .as_ref()
                .and_then(|runtime| runtime.parallelism_limit)
                .map_or_else(|| count.to_string(), |limit| format!("{count} / {limit}"))
        });
        let connected_peers = self
            .runtime
            .as_ref()
            .filter(|_| downloading && row.runtime_connected && self.is_bt())
            .and_then(|runtime| runtime.connected_peers);
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
            .child(render_segment_progress(
                self.runtime.as_ref(),
                progress,
                (f32::from(window.viewport_size().width) - 2. * f32::from(tokens.spacing.md))
                    .max(0.),
                6.,
                status_color,
                tokens.colors.muted,
            ))
            .child(
                h_flex()
                    .justify_between()
                    .gap(tokens.spacing.sm)
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(div().child(progress_label))
                    .child(div().flex_1().text_right().child(size_label))
                    .when_some(speed, |this, speed| this.child(div().child(speed)))
                    .when_some(eta, |this, eta| this.child(div().child(eta))),
            )
            .when_some(active_transfers, |this, count| {
                this.child(Self::info_row(
                    &tokens,
                    self.t(cx, "detailActiveTransfers"),
                    count.into(),
                ))
            })
            .when_some(connected_peers, |this, count| {
                this.child(Self::info_row(
                    &tokens,
                    self.t(cx, "detailConnectedPeers"),
                    count.to_string().into(),
                ))
            })
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
                    .child(
                        Button::new("detail-open-folder")
                            .small()
                            .ghost()
                            .icon(IconName::FolderOpen)
                            .tooltip(self.strings.open_folder.clone())
                            .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
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
                    .when(self.mode == DetailMode::Docked, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .font_weight(tokens.typography.sm.weight)
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(row.name.clone())),
                        )
                    })
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
            .when(
                self.mode == DetailMode::Docked || row.state != TaskState::Completed,
                |this| {
                    this.child(Self::info_row(
                        &tokens,
                        self.t(cx, "infoStatus"),
                        self.strings.state_label(row.state),
                    ))
                    .when(row.size_bytes > 0, |this| {
                        this.child(Self::info_row(
                            &tokens,
                            self.t(cx, "infoSize"),
                            row.size.clone().into(),
                        ))
                    })
                },
            )
            .when(row.state != TaskState::Completed, |this| {
                this.child(Self::info_row(
                    &tokens,
                    self.t(cx, "infoDownloaded"),
                    format_bytes(row.downloaded_bytes).into(),
                ))
            })
            .when(row.state == TaskState::Downloading, |this| {
                this.when_some(row.speed_bytes_per_second, |this, speed| {
                    this.child(Self::info_row(
                        &tokens,
                        self.t(cx, "infoSpeed"),
                        format!("{}/s", format_bytes(speed)).into(),
                    ))
                })
                .when_some(row.eta_seconds, |this, seconds| {
                    this.child(Self::info_row(
                        &tokens,
                        self.t(cx, "infoRemaining"),
                        self.strings.format_eta(seconds),
                    ))
                })
            })
            .when(row.created_at_secs > 0, |this| {
                this.child(Self::info_row(
                    &tokens,
                    self.t(cx, "infoStartedAt"),
                    DownloadStrings::format_detail_datetime(row.created_at_secs),
                ))
            })
            .when(row.completed_at_secs > 0, |this| {
                this.child(Self::info_row(
                    &tokens,
                    self.t(cx, "infoCompletedAt"),
                    DownloadStrings::format_detail_datetime(row.completed_at_secs),
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
        let (oldest, newest, truncated) = self.activity.retained_range();
        let mut content = v_flex().gap(tokens.spacing.xs).child(
            div()
                .text_size(tokens.typography.xs.size)
                .text_color(tokens.colors.muted_foreground)
                .child(self.t(cx, "detailLogHint")),
        );
        if truncated {
            content = content.child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .text_color(cx.theme().warning)
                    .child(self.t(cx, "detailActivityTruncated")),
            );
        }
        if self.activity.has_journal_gap() {
            content = content.child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .text_color(cx.theme().warning)
                    .child(self.t(cx, "detailActivityJournalGap")),
            );
        }
        if let (Some(oldest), Some(newest)) = (oldest, newest) {
            content = content.child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(format!(
                        "{}: #{oldest}–#{newest}",
                        self.t(cx, "detailActivityRetainedRange")
                    )),
            );
        }
        if self.activity.failed() {
            content = content.child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .items_center()
                    .child(
                        div()
                            .text_size(tokens.typography.xs.size)
                            .text_color(cx.theme().danger)
                            .child(self.t(cx, "detailActivityQueryFailed")),
                    )
                    .child(
                        Button::new("detail-activity-retry")
                            .small()
                            .label(self.t(cx, "detailActivityRetry"))
                            .on_click(cx.listener(|this, _, _, cx| this.retry_activity(cx))),
                    ),
            );
        }
        if self.activity.is_loading() {
            content = content.child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t(cx, "detailActivityLoading")),
            );
        }
        if self.activity.loaded() && self.activity.entries().next().is_none() {
            content = content.child(
                div()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t(cx, "detailLogEmpty")),
            );
        }
        content = content.children(self.activity.entries().rev().map(|entry| {
            let kind_key = activity_kind_key(&entry.kind);
            let label = self.t(cx, kind_key);
            let state = if entry.kind == "status" {
                entry
                    .status
                    .map(|status| self.strings.state_label(activity_state(status)))
            } else {
                None
            };
            let text = if let Some(state) = state {
                format!("{label}: {state}")
            } else if kind_key == "detailActivityKindUnknown" {
                if entry.message.is_empty() {
                    format!("{label} ({})", entry.kind)
                } else {
                    format!("{label} ({}): {}", entry.kind, entry.message)
                }
            } else if entry.message.is_empty() {
                label.to_string()
            } else {
                format!("{label}: {}", entry.message)
            };
            h_flex()
                .gap(tokens.spacing.sm)
                .items_start()
                .child(
                    div()
                        .flex_none()
                        .w(px(180.))
                        .text_size(tokens.typography.xs.size)
                        .text_color(tokens.colors.muted_foreground)
                        .child(format_activity_timestamp(entry.timestamp_ms)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_size(tokens.typography.xs.size)
                        .when(entry.kind == "journal_overflow", |this| {
                            this.text_color(cx.theme().warning)
                        })
                        .child(text),
                )
        }));
        if self.activity.has_older() && !self.activity.is_loading() && !self.activity.failed() {
            content = content.child(
                Button::new("detail-activity-load-older")
                    .small()
                    .ghost()
                    .label(self.t(cx, "detailActivityLoadMore"))
                    .on_click(cx.listener(|this, _, _, cx| this.load_older_activity(cx))),
            );
        }
        content.into_any_element()
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        v_flex()
            .size_full()
            .min_h_0()
            .bg(tokens.colors.surface)
            .when(self.mode == DetailMode::Window, |this| {
                this.child(self.render_progress_head(window, cx))
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
            .child(self.render_tab_bar(cx))
            .child(
                div()
                    .id("task-detail-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(tokens.spacing.md)
                    .child(match self.tab {
                        DetailTab::General => self.render_general(cx),
                        DetailTab::Seeding => self.render_seeding(cx),
                        DetailTab::Log => self.render_log(cx),
                        DetailTab::Advanced => self.render_advanced(cx),
                    }),
            )
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T, capacity: usize) {
    queue.push_back(item);
    while queue.len() > capacity {
        queue.pop_front();
    }
}

fn format_activity_timestamp(timestamp_ms: i64) -> SharedString {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map_or_else(
            || SharedString::from("—"),
            |at| SharedString::from(at.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
        )
}

fn activity_state(status: i32) -> TaskState {
    match status {
        0 | 5 => TaskState::Pending,
        1 => TaskState::Downloading,
        2 => TaskState::Paused,
        3 => TaskState::Completed,
        _ => TaskState::Failed,
    }
}

fn activity_kind_key(kind: &str) -> &'static str {
    match kind {
        "status" => "detailActivityKindStatus",
        "error" => "detailActivityKindError",
        "split" => "detailActivityKindSplit",
        "cdn_pool" => "detailActivityKindCdnPool",
        "cdn_kick" => "detailActivityKindCdnKick",
        "cdn_breaker" => "detailActivityKindCdnBreaker",
        "cdn_fallback" => "detailActivityKindCdnFallback",
        "cdn_summary" => "detailActivityKindCdnSummary",
        "retry" => "detailActivityKindRetry",
        "journal_overflow" => "detailActivityKindJournalOverflow",
        _ => "detailActivityKindUnknown",
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
    use chrono::{TimeZone as _, Utc};

    use super::format_activity_timestamp;

    #[test]
    fn activity_uses_source_milliseconds_with_calendar_date() {
        let timestamp = Utc
            .with_ymd_and_hms(2026, 6, 10, 12, 30, 4)
            .single()
            .unwrap()
            .timestamp_millis()
            + 123;
        let label = format_activity_timestamp(timestamp);
        assert!(label.starts_with("2026-06-"));
        assert!(label.ends_with(".123"));
        assert_eq!(format_activity_timestamp(i64::MAX).as_ref(), "—");
    }
}
