//! 快速捕获窗口：合并展示外部捕获（浏览器扩展 / NMH）待确认队列，逐条或批量
//! 确认/忽略；不弹全量新建下载表单，只在用户点「更多选项…」时转交。
//!
//! `file_name` 编辑状态（`Entity<InputState>`）需要 `Window` 才能创建/写值，而
//! [`crate::session::SessionConsumer`] 的 `replace_snapshot`/`apply_event` 没有
//! `Window` 参数：本视图把快照/事件折叠成纯数据字段（`captures`），实际的
//! `Entity<InputState>` 行在 `render()`（持有 `Window`）里按 `transaction_id`
//! 增量对账，保留用户已编辑但仍在队列中的文件名。

use std::rc::Rc;
use std::sync::Arc;

/// 「更多选项…」入口：把预填上下文交给新建下载窗口。
pub type MoreOptionsOpener = Rc<dyn Fn(NewDownloadContext, &mut Window, &mut App)>;

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, CaptureOverridesDto, CaptureResolveParams, DaemonEvent,
    MAIN_QUEUE_ID, PendingCaptureDto, ServiceEvent,
};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    Anchor, App, AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, actions, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, IconName, Sizable as _, Size, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    scroll::ScrollableElement as _,
    tooltip::Tooltip,
    v_flex,
};

use crate::{
    components::dir_picker,
    controller::{DownloadsCommand, DownloadsPort},
    model::format_bytes,
    pages::new_download::{NewDownloadContext, NewDownloadQueue},
};

actions!(quick_capture, [IgnoreAllCaptures]);

/// 视图内私有键上下文；`escape` = 全部忽略，仅在窗口内某控件获得焦点时生效
/// （弹窗以 `focus: false` 创建，不主动抢焦点）。
const KEY_CONTEXT: &str = "QuickCapture";

/// 在 app 侧装配一次；由 `crate::windows::quick_capture::install` 调用。
pub fn bind_quick_capture_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        IgnoreAllCaptures,
        Some(KEY_CONTEXT),
    )]);
}

/// 窗口清空后由消费方（app）关闭窗口。
pub enum QuickCaptureEvent {
    Empty,
    /// 可见行数变化，宿主据此调整窗口高度（[`QuickCaptureView::preferred_height_for`]）。
    RowsChanged(usize),
}

struct CaptureRow {
    dto: PendingCaptureDto,
    file_name: Entity<InputState>,
    size_label: SharedString,
}

pub struct QuickCaptureView {
    strings: QuickCaptureStrings,
    port: Arc<dyn DownloadsPort>,
    captures: Vec<PendingCaptureDto>,
    rows: Vec<CaptureRow>,
    save_dir: Entity<InputState>,
    save_dir_initialized: bool,
    pending_default_save_dir: Option<String>,
    /// 上次通知宿主的行数。
    last_rows: Option<usize>,
    queue_id: String,
    queues: Vec<NewDownloadQueue>,
    more_options: MoreOptionsOpener,
    picking: bool,
}

impl EventEmitter<QuickCaptureEvent> for QuickCaptureView {}

impl QuickCaptureView {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn DownloadsPort>,
        more_options: MoreOptionsOpener,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let strings = QuickCaptureStrings::from_translator(translator.read(cx));
        cx.observe(&translator, |this, translator, cx| {
            this.strings = QuickCaptureStrings::from_translator(translator.read(cx));
            cx.notify();
        })
        .detach();
        let save_dir =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.save_dir.clone()));
        Self {
            strings,
            port,
            captures: Vec::new(),
            rows: Vec::new(),
            save_dir,
            save_dir_initialized: false,
            pending_default_save_dir: None,
            last_rows: None,
            queue_id: MAIN_QUEUE_ID.to_owned(),
            queues: Vec::new(),
            more_options,
            picking: false,
        }
    }

    /// `SessionConsumer::replace_snapshot`：折叠成纯数据，不接触 `Entity<InputState>`。
    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        self.queues = snapshot
            .daemon
            .queues
            .iter()
            .map(|queue| NewDownloadQueue {
                id: queue.queue_id.clone(),
                name: queue.name.clone(),
            })
            .collect();
        if !self.save_dir_initialized {
            let configured = snapshot
                .daemon
                .config
                .values
                .get("default_save_dir")
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            self.pending_default_save_dir =
                Some(configured.unwrap_or_else(|| snapshot.daemon.runtime_stats.save_dir.clone()));
        }
        self.set_captures(snapshot.pending_captures.clone(), cx);
    }

    /// `SessionConsumer::apply_event`。
    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        match event {
            ServiceEvent::Agent(AgentEvent::PendingCapturesChanged(captures)) => {
                self.set_captures(captures.clone(), cx);
            }
            ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::QueuesChanged(queues)))
            | ServiceEvent::Daemon(DaemonEvent::QueuesChanged(queues)) => {
                self.queues = queues
                    .iter()
                    .map(|queue| NewDownloadQueue {
                        id: queue.queue_id.clone(),
                        name: queue.name.clone(),
                    })
                    .collect();
                cx.notify();
            }
            _ => {}
        }
    }

    /// `SessionConsumer::mark_stale`：断线时保留当前列表，仅刷新只读态。
    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// 只在「非空 → 空」时发 `Empty`：attach 的首帧可能来自连接时的旧快照（尚无捕获），
    /// 随后 `system.snapshot` 重对齐才带来真实列表，不能因旧快照为空就关窗。
    fn set_captures(&mut self, captures: Vec<PendingCaptureDto>, cx: &mut Context<Self>) {
        let was_empty = self.captures.is_empty();
        self.captures = captures;
        if self.captures.is_empty() && !was_empty {
            cx.emit(QuickCaptureEvent::Empty);
        }
        cx.notify();
    }

    /// 按 `transaction_id` 对账 `rows`：保留仍在队列中的行（含用户已编辑的文件名），
    /// 为新出现的捕获创建 `InputState`，丢弃已消费的行。只能在 `render` 内调用
    /// （需要 `Window` 创建/初始化输入框）。
    fn sync_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut next = Vec::with_capacity(self.captures.len());
        for dto in &self.captures {
            let existing = self
                .rows
                .iter()
                .position(|row| row.dto.transaction_id == dto.transaction_id);
            let row = if let Some(index) = existing {
                let mut row = self.rows.remove(index);
                row.dto = dto.clone();
                row.size_label = size_label(&self.strings, dto.file_size);
                row
            } else {
                let initial = if dto.file_name.is_empty() {
                    file_name_from_url(&dto.url)
                } else {
                    dto.file_name.clone()
                };
                let file_name = cx.new(|cx| InputState::new(window, cx).default_value(initial));
                CaptureRow {
                    dto: dto.clone(),
                    file_name,
                    size_label: size_label(&self.strings, dto.file_size),
                }
            };
            next.push(row);
        }
        self.rows = next;
        if !self.save_dir_initialized
            && let Some(default) = self.pending_default_save_dir.take()
        {
            self.save_dir_initialized = true;
            self.save_dir
                .update(cx, |input, cx| input.set_value(default, window, cx));
        }
    }

    fn resolve(
        &mut self,
        transaction_id: String,
        accepted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let overrides = accepted.then(|| {
            let file_name = self
                .rows
                .iter()
                .find(|row| row.dto.transaction_id == transaction_id)
                .map(|row| row.file_name.read(cx).value().trim().to_owned())
                .unwrap_or_default();
            CaptureOverridesDto {
                save_dir: self.save_dir.read(cx).value().trim().to_owned(),
                file_name,
                queue_id: self.queue_id.clone(),
                segments: 0,
            }
        });
        let future = self
            .port
            .execute(DownloadsCommand::CaptureResolve(CaptureResolveParams {
                transaction_id,
                accepted,
                overrides,
            }));
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if result.is_err() {
                    window.push_notification(
                        Notification::error(this.strings.action_failed.clone()),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    fn resolve_all(&mut self, accepted: bool, window: &mut Window, cx: &mut Context<Self>) {
        for transaction_id in self
            .captures
            .iter()
            .map(|dto| dto.transaction_id.clone())
            .collect::<Vec<_>>()
        {
            self.resolve(transaction_id, accepted, window, cx);
        }
    }

    fn ignore_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.resolve_all(false, window, cx);
    }

    fn open_more_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(first) = self.captures.first().cloned() else {
            return;
        };
        let context = NewDownloadContext {
            save_dir: self.save_dir.read(cx).value().trim().to_owned(),
            queue_id: self.queue_id.clone(),
            segments: 0,
            queues: self.queues.clone(),
            manual_proxy_url: String::new(),
            initial_urls: vec![first.url.clone()],
            initial_file_name: first.file_name.clone(),
        };
        self.resolve(first.transaction_id.clone(), false, window, cx);
        (self.more_options)(context, window, cx);
    }

    fn pick_save_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        cx.notify();
        let prompt = self.strings.save_dir.clone();
        dir_picker::pick_directory(window, cx, prompt, |picked, this, window, cx| {
            this.picking = false;
            if let Some(path) = picked {
                this.save_dir
                    .update(cx, |input, cx| input.set_value(path, window, cx));
            }
            cx.notify();
        });
    }

    fn queue_label(&self) -> SharedString {
        self.queues
            .iter()
            .find(|queue| queue.id == self.queue_id)
            .map_or_else(
                || SharedString::from(self.queue_id.clone()),
                |queue| SharedString::from(queue.name.clone()),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .gap(tokens.spacing.sm)
            .px(tokens.spacing.md)
            .pt(tokens.spacing.md)
            .pb(tokens.spacing.sm)
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .child(
                        div()
                            .flex_none()
                            .size(px(28.))
                            .rounded(tokens.radius.md)
                            .bg(tokens.colors.primary.opacity(0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::ArrowDown)
                                    .size(px(15.))
                                    .text_color(tokens.colors.primary),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(tokens.typography.md.size)
                            .font_weight(tokens.typography.md.weight)
                            .font_semibold()
                            .child(self.strings.title.clone()),
                    )
                    .child(self.render_queue_dropdown(cx)),
            )
            .child(
                h_flex()
                    .gap(tokens.spacing.xs)
                    .items_center()
                    .child(
                        Icon::new(IconName::Folder)
                            .size(px(13.))
                            .text_color(tokens.colors.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.save_dir).with_size(Size::Medium)),
                    )
                    .child(
                        Button::new("quick-capture-browse")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .tooltip(self.strings.browse.clone())
                            .disabled(self.picking)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.pick_save_dir(window, cx);
                            })),
                    ),
            )
    }

    fn render_queue_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current_label = self.queue_label();
        let queues = self.queues.clone();
        let this = cx.weak_entity();
        Button::new("quick-capture-queue")
            .ghost()
            .small()
            .h(CONTROL_HEIGHT)
            .icon(IconName::LayoutDashboard)
            .label(current_label)
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                queues.iter().fold(menu, |menu, queue| {
                    let this = this.clone();
                    let queue_id = queue.id.clone();
                    menu.item(
                        PopupMenuItem::new(SharedString::from(queue.name.clone())).on_click(
                            move |_, _, cx| {
                                let queue_id = queue_id.clone();
                                let _ = this.update(cx, |this, cx| {
                                    this.queue_id = queue_id;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                })
            })
    }

    fn render_row(&self, row: &CaptureRow, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let transaction_id = row.dto.transaction_id.clone();
        let transaction_id_for_ignore = transaction_id.clone();
        let url = SharedString::from(row.dto.url.clone());
        let url_for_tooltip = url.clone();
        v_flex()
            .gap(tokens.spacing.xs)
            .mx(tokens.spacing.md)
            .my(tokens.spacing.xs)
            .p(tokens.spacing.sm)
            .rounded(tokens.radius.md)
            .border_1()
            .border_color(tokens.colors.border)
            .bg(tokens.colors.surface)
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .child(
                        Icon::new(IconName::File)
                            .size(px(14.))
                            .text_color(tokens.colors.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&row.file_name).with_size(Size::Medium)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px(tokens.spacing.xs)
                            .py(px(2.))
                            .rounded(tokens.radius.sm)
                            .bg(tokens.colors.muted)
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(row.size_label.clone()),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .child(
                        div()
                            .id(format!("quick-capture-url-{}", row.dto.transaction_id))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(url)
                            .tooltip(move |window, cx| {
                                Tooltip::new(url_for_tooltip.clone()).build(window, cx)
                            }),
                    )
                    .child(
                        Button::new(format!("quick-capture-ignore-{}", row.dto.transaction_id))
                            .ghost()
                            .small()
                            .h(CONTROL_HEIGHT)
                            .label(self.strings.ignore.clone())
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.resolve(transaction_id_for_ignore.clone(), false, window, cx);
                            })),
                    )
                    .child(
                        Button::new(format!("quick-capture-download-{}", row.dto.transaction_id))
                            .primary()
                            .small()
                            .h(CONTROL_HEIGHT)
                            .label(self.strings.download.clone())
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.resolve(transaction_id.clone(), true, window, cx);
                            })),
                    ),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .items_center()
            .justify_between()
            .px(tokens.spacing.md)
            .py(tokens.spacing.sm)
            .child(
                Button::new("quick-capture-more")
                    .link()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .label(self.strings.more_options.clone())
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_more_options(window, cx);
                    })),
            )
            .when(self.rows.len() > 1, |footer| {
                footer.child(
                    h_flex()
                        .gap(tokens.spacing.xs)
                        .child(
                            Button::new("quick-capture-ignore-all")
                                .outline()
                                .small()
                                .h(CONTROL_HEIGHT)
                                .label(self.strings.ignore_all.clone())
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.resolve_all(false, window, cx);
                                })),
                        )
                        .child(
                            Button::new("quick-capture-download-all")
                                .primary()
                                .small()
                                .h(CONTROL_HEIGHT)
                                .label(self.strings.download_all.clone())
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.resolve_all(true, window, cx);
                                })),
                        ),
                )
            })
    }

    /// 超过此行数后列表区固定高度并滚动。
    const MAX_VISIBLE_ROWS: usize = 4;
    const ROW_HEIGHT: f32 = 76.;
    const HEADER_HEIGHT: f32 = 92.;
    const FOOTER_HEIGHT: f32 = 44.;
    const MAX_LIST_HEIGHT: f32 = Self::ROW_HEIGHT * 4.;

    /// 给定行数的窗口高度：头部 + 卡片行（最多 4 行）+ 底栏。
    #[must_use]
    pub fn preferred_height_for(rows: usize) -> f32 {
        Self::HEADER_HEIGHT
            + Self::ROW_HEIGHT * rows.clamp(1, Self::MAX_VISIBLE_ROWS) as f32
            + Self::FOOTER_HEIGHT
    }

    fn render_rows(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = active_theme(cx).tokens().clone();
        let list = v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .py(tokens.spacing.xs)
            .children(self.rows.iter().map(|row| self.render_row(row, cx)));
        if self.rows.len() > Self::MAX_VISIBLE_ROWS {
            list.h(px(Self::MAX_LIST_HEIGHT))
                .overflow_y_scrollbar()
                .into_any_element()
        } else {
            list.into_any_element()
        }
    }
}

/// 窗口宽度固定；高度由 [`QuickCaptureView::preferred_height_for`] 决定。
pub const QUICK_CAPTURE_WINDOW_WIDTH: f32 = 460.;

impl Render for QuickCaptureView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_rows(window, cx);
        if self.last_rows != Some(self.rows.len()) {
            self.last_rows = Some(self.rows.len());
            cx.emit(QuickCaptureEvent::RowsChanged(self.rows.len()));
        }
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .key_context(KEY_CONTEXT)
            .size_full()
            .bg(tokens.colors.background)
            .rounded(tokens.radius.lg)
            .border_1()
            .border_color(tokens.colors.border)
            .on_action(cx.listener(|this, _: &IgnoreAllCaptures, window, cx| {
                this.ignore_all(window, cx);
            }))
            .child(self.render_header(cx))
            .child(self.render_rows(cx))
            .child(div().h(px(1.)).w_full().bg(tokens.colors.border))
            .child(self.render_footer(cx))
    }
}

#[derive(Clone)]
struct QuickCaptureStrings {
    title: SharedString,
    download: SharedString,
    ignore: SharedString,
    download_all: SharedString,
    ignore_all: SharedString,
    more_options: SharedString,
    save_dir: SharedString,
    browse: SharedString,
    unknown_size: SharedString,
    action_failed: SharedString,
}

impl QuickCaptureStrings {
    fn from_translator(translator: &Translator) -> Self {
        Self {
            title: shared(translator.text("quickCaptureTitle")),
            download: shared(translator.text("quickCaptureDownload")),
            ignore: shared(translator.text("quickCaptureIgnore")),
            download_all: shared(translator.text("quickCaptureDownloadAll")),
            ignore_all: shared(translator.text("quickCaptureIgnoreAll")),
            more_options: shared(translator.text("quickCaptureMoreOptions")),
            save_dir: shared(translator.text("saveDir")),
            browse: shared(translator.text("browse")),
            unknown_size: shared(translator.text("unknownSize")),
            action_failed: shared(translator.text("localServiceActionFailed")),
        }
    }
}

/// 捕获方未给文件名时按 URL 路径末段预填（去 query/fragment，`%20` 等简单解码）。
fn file_name_from_url(url: &str) -> String {
    if let Some(query) = url.strip_prefix("magnet:?") {
        return query
            .split('&')
            .find_map(|pair| pair.strip_prefix("dn="))
            .map(percent_decode)
            .unwrap_or_default();
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    // 去掉 scheme + host，只看路径部分。
    let Some((_, rest)) = path.split_once("://") else {
        return String::new();
    };
    let Some((_, path)) = rest.split_once('/') else {
        return String::new();
    };
    let segment = path.rsplit('/').next().unwrap_or("");
    if segment.is_empty() {
        return String::new();
    }
    percent_decode(segment)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod file_name_tests {
    use super::file_name_from_url;

    #[test]
    fn derives_last_path_segment_without_query() {
        assert_eq!(
            file_name_from_url("https://h.example/a/b/100MB.bin?x=1#f"),
            "100MB.bin"
        );
        assert_eq!(file_name_from_url("https://h.example/dir/"), "");
        assert_eq!(file_name_from_url("https://h.example"), "");
        assert_eq!(
            file_name_from_url("https://h.example/my%20file.zip"),
            "my file.zip"
        );
        assert_eq!(
            file_name_from_url("magnet:?xt=urn:btih:abc&dn=ubuntu%2024.iso&tr=x"),
            "ubuntu 24.iso"
        );
        assert_eq!(file_name_from_url("magnet:?xt=urn:btih:abc"), "");
    }
}

fn size_label(strings: &QuickCaptureStrings, file_size: i64) -> SharedString {
    if file_size > 0 {
        SharedString::from(format_bytes(file_size as u64))
    } else {
        strings.unknown_size.clone()
    }
}

fn shared(value: &str) -> SharedString {
    SharedString::from(value.to_owned())
}
