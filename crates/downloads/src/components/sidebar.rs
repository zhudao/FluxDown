use fluxdown_protocol::{CustomCategoryDto, QueueDto, RssSourceDto};
use fluxdown_ui_components::sidebar_navigation_button;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div, percentage,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, WindowExt as _,
    animation::ease_in_out_cubic,
    h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::{
    controller::DownloadsCommand,
    model::{
        DownloadFilter, DownloadStatusFilter, SidebarSection, SidebarSelection, StatusFolderMotion,
        TaskSource,
    },
    pages::downloads::DownloadView,
};

const NAV_ITEM_HEIGHT: f32 = 32.;

/// 分类 wire 图标名 → 渲染图标（与 `crates/settings/src/sections/category_dialog.rs`
/// 的 `CATEGORY_ICONS` 表保持一致）；未知名字回退通用文件图标。
fn category_icon(name: &str) -> IconName {
    match name {
        "folders" => IconName::Folder,
        "film" => IconName::GalleryVerticalEnd,
        "music" => IconName::Play,
        "fileText" => IconName::File,
        "image" => IconName::GalleryVerticalEnd,
        "archive" => IconName::Inbox,
        "code" => IconName::SquareTerminal,
        "database" | "hardDrive" => IconName::HardDrive,
        "gamepad" => IconName::Bot,
        "globe" => IconName::Globe,
        "bookmark" => IconName::Star,
        "box" | "package2" => IconName::Inbox,
        "cpu" => IconName::Cpu,
        "disc" | "smartphone" => IconName::MemoryStick,
        "font" | "type" => IconName::ALargeSmall,
        "library" => IconName::BookOpen,
        "pen" => IconName::Replace,
        "printer" => IconName::Frame,
        "subtitles" => IconName::CaseSensitive,
        "zap" => IconName::BatteryCharging,
        _ => IconName::File,
    }
}

fn folder_id(status: DownloadStatusFilter) -> &'static str {
    match status {
        DownloadStatusFilter::All => "download-folder-all",
        DownloadStatusFilter::Incomplete => "download-folder-incomplete",
        DownloadStatusFilter::Completed => "download-folder-completed",
        DownloadStatusFilter::Failed => "download-folder-failed",
        DownloadStatusFilter::Paused => "download-folder-paused",
    }
}

impl DownloadView {
    /// 分区标题：点击折叠 / 展开，右键「隐藏此区块」写回 `ui.show_sidebar_*` 偏好。
    fn section_header(
        &self,
        id: &'static str,
        label: SharedString,
        section: SidebarSection,
        open_amount: f32,
        trailing: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let header_color = tokens.colors.muted_foreground.opacity(0.65);
        let chevron_rotation = percentage(open_amount * 0.25);
        let this = cx.weak_entity();
        let hide_label = self.strings.hide_section.clone();

        h_flex()
            .id(id)
            .h(px(28.))
            .px(tokens.spacing.sm)
            .items_center()
            .justify_between()
            .cursor_pointer()
            .rounded(tokens.radius.sm)
            .text_size(px(10.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(header_color)
            .hover(|style| {
                style
                    .bg(tokens.colors.muted)
                    .text_color(tokens.colors.muted_foreground)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_section(section, cx);
                cx.notify();
            }))
            .child(label)
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens.spacing.xs)
                    .children(trailing)
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .size(px(12.))
                            .rotate(chevron_rotation),
                    ),
            )
            .context_menu(move |menu, _window, _cx| {
                let this = this.clone();
                menu.item(
                    PopupMenuItem::new(hide_label.clone()).on_click(move |_, _window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.execute_commands(
                                vec![DownloadsCommand::SetSyncedPreference {
                                    key: section.visibility_pref(),
                                    value: serde_json::Value::Bool(false),
                                }],
                                cx,
                            );
                        });
                    }),
                )
            })
    }

    /// 队列分区头右侧「+」：打开队列管理窗口。
    fn queue_add_button(&self, cx: &Context<Self>) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        let open_queue_manager = self.host.open_queue_manager.clone();
        div()
            .id("download-queue-add")
            .flex()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded(tokens.radius.sm)
            .cursor_pointer()
            .text_color(tokens.colors.muted_foreground)
            .hover(|style| style.bg(tokens.colors.muted))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                if let Some(open) = open_queue_manager.clone() {
                    open(window, cx);
                }
            })
            .child(Icon::new(IconName::Plus).size(px(11.)))
            .into_any_element()
    }

    fn nav_item(
        &self,
        id: impl Into<gpui::ElementId>,
        selection: SidebarSelection,
        label: SharedString,
        icon: IconName,
        trailing: (SharedString, Option<gpui::Hsla>),
        cx: &mut Context<Self>,
    ) -> fluxdown_ui_components::Button {
        let (count, dot) = trailing;
        let tokens = active_theme(cx).tokens();
        let selected = self.selected_item == selection;
        let count_color = if selected {
            tokens.colors.accent_foreground
        } else {
            tokens.colors.muted_foreground.opacity(0.65)
        };
        let trailing = h_flex()
            .flex_none()
            .items_center()
            .gap(tokens.spacing.xs)
            .when_some(dot, |this, color| {
                this.child(div().size(px(5.)).rounded_full().bg(color))
            })
            .child(
                div()
                    .min_w(px(12.))
                    .text_right()
                    .text_size(px(11.))
                    .text_color(count_color)
                    .child(count),
            );

        sidebar_navigation_button(
            id,
            label,
            Icon::new(icon).size(px(14.)),
            trailing,
            selected,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_sidebar_item(selection.clone(), cx);
        }))
    }

    fn filter_count(&self, filter: &DownloadFilter, cx: &Context<Self>) -> SharedString {
        SharedString::from(
            self.table_state
                .read(cx)
                .delegate()
                .count_matching(filter)
                .to_string(),
        )
    }

    fn queue_count(&self, queue_id: &str, cx: &Context<Self>) -> SharedString {
        SharedString::from(
            self.table_state
                .read(cx)
                .delegate()
                .count_in_queue(queue_id)
                .to_string(),
        )
    }

    /// 设备计数：本机计所有本地任务；其余按远程任务来源设备 id / 指纹精确匹配。
    fn device_count(&self, device_id: &str, cx: &Context<Self>) -> SharedString {
        let count = if device_id == SidebarSelection::LOCAL_DEVICE {
            self.table_state
                .read(cx)
                .delegate()
                .count_where(|task| task.source == TaskSource::Local)
        } else {
            let device_id = device_id.to_owned();
            self.table_state
                .read(cx)
                .delegate()
                .count_where(move |task| {
                    task.source == TaskSource::Remote && task.from_device == device_id
                })
        };
        SharedString::from(count.to_string())
    }

    fn folder_item(
        &self,
        id: &'static str,
        status: DownloadStatusFilter,
        label: SharedString,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> fluxdown_ui_components::Button {
        let tokens = active_theme(cx).tokens();
        let filter = DownloadFilter::status(status);
        let selection = SidebarSelection::Download(filter.clone());
        let selected = self.selected_item == selection;
        let foreground = if selected {
            tokens.colors.accent_foreground
        } else {
            tokens.colors.muted_foreground
        };
        let trailing = h_flex()
            .flex_none()
            .items_center()
            .gap(tokens.spacing.xs)
            .text_color(foreground)
            .child(
                div()
                    .min_w(px(12.))
                    .text_right()
                    .text_size(px(11.))
                    .child(self.filter_count(&filter, cx)),
            )
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.)),
            );

        sidebar_navigation_button(
            id,
            label,
            Icon::new(if expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            })
            .size(px(14.)),
            trailing,
            selected,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.retarget_status_folder(status, cx);
            this.select_sidebar_item(selection.clone(), cx);
            cx.notify();
        }))
    }

    /// 状态文件夹下的分类子项（内置 `all` 之外的可见分类）；分类的编辑 / 新建走这里的右键菜单。
    fn category_entries(&self) -> Vec<CustomCategoryDto> {
        self.controller
            .categories()
            .visible()
            .filter(|rule| rule.dto.builtin_type.as_deref() != Some("all"))
            .map(|rule| rule.dto.clone())
            .collect()
    }

    fn render_categories(&self, status: DownloadStatusFilter, cx: &mut Context<Self>) -> Div {
        let entries = self.category_entries();
        let mut items = Vec::with_capacity(entries.len());
        for dto in entries {
            let filter = DownloadFilter::with_category(status, &dto.id);
            let count = self.filter_count(&filter, cx);
            let menu = self.category_item_context_menu(&dto, cx);
            let item = self
                .nav_item(
                    format!("download-nav-{}-{}", folder_id(status), dto.id),
                    SidebarSelection::Download(filter),
                    self.strings.category_label(&dto),
                    category_icon(&dto.icon),
                    (count, None),
                    cx,
                )
                .context_menu(menu);
            items.push(div().w_full().pl(px(18.)).child(item));
        }
        v_flex().w_full().children(items)
    }

    fn retarget_status_folder(&mut self, status: DownloadStatusFilter, cx: &App) {
        let next = DownloadStatusFilter::exclusive_toggle(self.expanded_status, status);
        if next == self.expanded_status {
            return;
        }
        if cx.reduce_motion() {
            self.folder_motion = StatusFolderMotion::settled(next);
            self.folder_motion_started_at = None;
        } else {
            self.folder_motion = self.folder_motion.retarget(
                ease_in_out_cubic(self.folder_motion_linear_progress()),
                next,
            );
            self.folder_motion_started_at = Some(std::time::Instant::now());
        }
        self.expanded_status = next;
    }

    fn sidebar_motion_linear_progress(started_at: Option<std::time::Instant>) -> f32 {
        let Some(started_at) = started_at else {
            return 1.;
        };
        (started_at.elapsed().as_secs_f32()
            / crate::pages::downloads::SIDEBAR_MOTION_DURATION.as_secs_f32())
        .clamp(0., 1.)
    }

    fn folder_motion_linear_progress(&self) -> f32 {
        Self::sidebar_motion_linear_progress(self.folder_motion_started_at)
    }

    pub(crate) fn section_is_expanded(&self, section: SidebarSection) -> bool {
        self.section_expanded.get(&section).copied().unwrap_or(true)
    }

    pub(crate) fn toggle_section(&mut self, section: SidebarSection, cx: &App) {
        let expanded = self.section_is_expanded(section);
        let target = if expanded { 1. } else { 0. };
        let from = self
            .section_motion_from
            .get(&section)
            .copied()
            .unwrap_or(target);
        let progress = ease_in_out_cubic(Self::sidebar_motion_linear_progress(
            self.section_motion_started_at.get(&section).copied(),
        ));
        let current = from + (target - from) * progress;
        let next = !expanded;
        if cx.reduce_motion() {
            self.section_motion_from
                .insert(section, if next { 1. } else { 0. });
            self.section_motion_started_at.remove(&section);
        } else {
            self.section_motion_from.insert(section, current);
            self.section_motion_started_at
                .insert(section, std::time::Instant::now());
        }
        self.section_expanded.insert(section, next);
    }

    pub(crate) fn section_open_amount(
        &self,
        section: SidebarSection,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        let target = if self.section_is_expanded(section) {
            1.
        } else {
            0.
        };
        if cx.reduce_motion() {
            return target;
        }
        let linear = Self::sidebar_motion_linear_progress(
            self.section_motion_started_at.get(&section).copied(),
        );
        if linear < 1. {
            window.request_animation_frame();
        }
        let from = self
            .section_motion_from
            .get(&section)
            .copied()
            .unwrap_or(target);
        from + (target - from) * ease_in_out_cubic(linear)
    }

    fn folder_open_amount(
        &self,
        status: DownloadStatusFilter,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        if cx.reduce_motion() {
            return if self.expanded_status == Some(status) {
                1.
            } else {
                0.
            };
        }
        let linear = self.folder_motion_linear_progress();
        if linear < 1. {
            window.request_animation_frame();
        }
        self.folder_motion.amount(status, ease_in_out_cubic(linear))
    }

    /// 状态文件夹的显示文案：与 Flutter 桌面端 `widgets/sidebar.dart::_statusLabel` 同源。
    fn folder_label(&self, status: DownloadStatusFilter) -> SharedString {
        match status {
            DownloadStatusFilter::All => self.strings.status_all.clone(),
            DownloadStatusFilter::Incomplete => self.strings.status_incomplete.clone(),
            DownloadStatusFilter::Completed => self.strings.status_completed.clone(),
            DownloadStatusFilter::Failed => self.strings.tab_failed.clone(),
            DownloadStatusFilter::Paused => self.strings.status_paused.clone(),
        }
    }

    fn render_filter_branch(
        &self,
        id: &'static str,
        status: DownloadStatusFilter,
        label: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let expanded = self.expanded_status == Some(status);
        let open_amount = self.folder_open_amount(status, window, cx);
        let category_count = self.category_entries().len() as f32;
        v_flex()
            .w_full()
            .child(self.folder_item(id, status, label, expanded, cx))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(NAV_ITEM_HEIGHT * category_count * open_amount))
                    .child(self.render_categories(status, cx)),
            )
    }

    /// 分区容器：与 Flutter 桌面端一致的扁平分组（无卡片边框），
    /// 分区之间只靠留白与小标题区分，避免侧栏出现多层套框。
    fn section_card(&self, cx: &Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .px(tokens.spacing.xs)
            .pt(tokens.spacing.sm)
            .pb(tokens.spacing.xxs)
    }

    /// 状态区：全部 / 下载中 / 已完成 / 失败 / 暂停 五个文件夹，各自可展开显示分类子项。
    fn render_status_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let open_amount = self.section_open_amount(SidebarSection::Status, window, cx);
        let category_count = self.category_entries().len() as f32;
        let folder_open_sum: f32 = DownloadStatusFilter::ALL
            .iter()
            .map(|status| self.folder_open_amount(*status, window, cx))
            .sum();
        let content_height = NAV_ITEM_HEIGHT * DownloadStatusFilter::ALL.len() as f32
            + NAV_ITEM_HEIGHT * category_count * folder_open_sum;

        let mut body = v_flex().w_full();
        for status in DownloadStatusFilter::ALL {
            body = body.child(self.render_filter_branch(
                folder_id(status),
                status,
                self.folder_label(status),
                window,
                cx,
            ));
        }

        self.section_card(cx)
            .child(self.section_header(
                "download-status-toggle",
                self.strings.sidebar_status.clone(),
                SidebarSection::Status,
                open_amount,
                None,
                cx,
            ))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(content_height * open_amount))
                    .child(body),
            )
    }

    /// 分类子项右键菜单：编辑（内置项不显示）、新建。
    fn category_item_context_menu(
        &self,
        dto: &CustomCategoryDto,
        _cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let open_editor = self.host.open_category_editor.clone();
        let edit_label = self.strings.edit_category.clone();
        let add_label = self.strings.add_category.clone();
        let is_builtin = dto.is_builtin;
        let category_id = dto.id.clone();

        move |menu, _window, _cx| {
            let menu = menu.when_some(
                open_editor.clone().filter(|_| !is_builtin),
                |menu, open_editor| {
                    let category_id = category_id.clone();
                    menu.item(PopupMenuItem::new(edit_label.clone()).on_click(
                        move |_, window, cx| {
                            open_editor(Some(category_id.clone()), window, cx);
                        },
                    ))
                },
            );
            menu.when_some(open_editor.clone(), |menu, open_editor| {
                menu.item(
                    PopupMenuItem::new(add_label.clone()).on_click(move |_, window, cx| {
                        open_editor(None, window, cx);
                    }),
                )
            })
        }
    }

    /// 队列右键菜单：启动/停止、管理队列、删除（主队列不显示，删除前确认）。
    fn queue_item_context_menu(
        &self,
        queue: &QueueDto,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let this = cx.weak_entity();
        let start_label = self.strings.start_queue_action.clone();
        let stop_label = self.strings.stop_queue_action.clone();
        let manage_label = self.strings.manage_queue_action.clone();
        let delete_label = self.strings.delete_queue_action.clone();
        let delete_ok_label = self.strings.delete.clone();
        let cancel_label = self.strings.cancel.clone();
        let description = SharedString::from(self.strings.queue_delete_confirm_desc.replace(
            "{name}",
            if queue.name.is_empty() {
                &queue.queue_id
            } else {
                &queue.name
            },
        ));
        let open_queue_manager = self.host.open_queue_manager.clone();
        let queue_id = queue.queue_id.clone();
        let is_running = queue.is_running;
        let is_main = queue.queue_id == fluxdown_protocol::MAIN_QUEUE_ID;

        move |menu, _window, _cx| {
            let menu = menu.item({
                let this = this.clone();
                let queue_id = queue_id.clone();
                if is_running {
                    PopupMenuItem::new(stop_label.clone()).on_click(move |_, _window, cx| {
                        let queue_id = queue_id.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.execute_commands(
                                vec![DownloadsCommand::QueueStop { queue_id }],
                                cx,
                            );
                        });
                    })
                } else {
                    PopupMenuItem::new(start_label.clone()).on_click(move |_, _window, cx| {
                        let queue_id = queue_id.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.execute_commands(
                                vec![DownloadsCommand::QueueStart { queue_id }],
                                cx,
                            );
                        });
                    })
                }
            });
            let menu = menu.item({
                let open_queue_manager = open_queue_manager.clone();
                PopupMenuItem::new(manage_label.clone()).on_click(move |_, window, cx| {
                    if let Some(open) = open_queue_manager.clone() {
                        open(window, cx);
                    }
                })
            });
            menu.when(!is_main, |menu| {
                let this = this.clone();
                let queue_id = queue_id.clone();
                let delete_label = delete_label.clone();
                let title = delete_label.clone();
                let description = description.clone();
                let ok_label = delete_ok_label.clone();
                let cancel_label = cancel_label.clone();
                menu.separator()
                    .item(
                        PopupMenuItem::new(delete_label).on_click(move |_, window, cx| {
                            let this = this.clone();
                            let queue_id = queue_id.clone();
                            let title = title.clone();
                            let description = description.clone();
                            let ok_label = ok_label.clone();
                            let cancel_label = cancel_label.clone();
                            window.open_alert_dialog(cx, move |dialog, _, _| {
                                let this = this.clone();
                                let queue_id = queue_id.clone();
                                dialog
                                    .title(title.clone())
                                    .description(description.clone())
                                    .button_props(
                                        gpui_component::dialog::DialogButtonProps::default()
                                            .ok_text(ok_label.clone())
                                            .ok_variant(
                                                gpui_component::button::ButtonVariant::Danger,
                                            )
                                            .cancel_text(cancel_label.clone()),
                                    )
                                    .on_ok(move |_, _, cx| {
                                        let _ = this.update(cx, |this, cx| {
                                            this.execute_commands(
                                                vec![DownloadsCommand::QueueDelete {
                                                    queue_id: queue_id.clone(),
                                                }],
                                                cx,
                                            );
                                        });
                                        true
                                    })
                            });
                        }),
                    )
            })
        }
    }

    fn render_queue_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let open_amount = self.section_open_amount(SidebarSection::Queues, window, cx);
        let queues: Vec<QueueDto> = self.controller.queues().to_vec();
        let success = cx.theme().success;
        let muted = active_theme(cx).tokens().colors.muted_foreground;
        let count = queues.len() as f32;
        let mut items = Vec::with_capacity(queues.len());
        for queue in queues {
            let id = queue.queue_id.clone();
            let label = match id.as_str() {
                fluxdown_protocol::MAIN_QUEUE_ID => self.strings.main_queue.clone(),
                fluxdown_protocol::LATER_QUEUE_ID => self.strings.later_queue.clone(),
                _ => SharedString::from(queue.name.clone()),
            };
            let running = queue.is_running;
            let count_label = self.queue_count(&id, cx);
            let menu = self.queue_item_context_menu(&queue, cx);
            items.push(
                self.nav_item(
                    format!("download-nav-queue-{id}"),
                    SidebarSelection::Queue(id),
                    label,
                    IconName::GalleryVerticalEnd,
                    (count_label, Some(if running { success } else { muted })),
                    cx,
                )
                .context_menu(menu),
            );
        }
        self.section_card(cx)
            .child(self.section_header(
                "download-queue-toggle",
                self.strings.sidebar_queues.clone(),
                SidebarSection::Queues,
                open_amount,
                Some(self.queue_add_button(cx)),
                cx,
            ))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(NAV_ITEM_HEIGHT * count * open_amount))
                    .child(v_flex().w_full().opacity(open_amount).children(items)),
            )
    }

    /// RSS 订阅右键菜单：立即抓取、管理订阅。
    fn rss_item_context_menu(
        &self,
        source_id: String,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let this = cx.weak_entity();
        let refresh_label = self.strings.rss_refresh_action.clone();
        let manage_label = self.strings.rss_manage_action.clone();
        let navigate_rss = self.host.navigate_rss.clone();

        move |menu, _window, _cx| {
            let menu = menu.item({
                let this = this.clone();
                let source_id = source_id.clone();
                PopupMenuItem::new(refresh_label.clone()).on_click(move |_, _window, cx| {
                    let source_id = source_id.clone();
                    let _ = this.update(cx, |this, cx| {
                        this.execute_commands(vec![DownloadsCommand::RssRefresh { source_id }], cx);
                    });
                })
            });
            menu.item({
                let navigate_rss = navigate_rss.clone();
                PopupMenuItem::new(manage_label.clone()).on_click(move |_, window, cx| {
                    if let Some(navigate) = navigate_rss.clone() {
                        navigate(window, cx);
                    }
                })
            })
        }
    }

    fn render_rss_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let open_amount = self.section_open_amount(SidebarSection::Rss, window, cx);
        let sources: Vec<RssSourceDto> = self.controller.rss_sources().to_vec();
        let count = sources.len() as f32;
        let mut items = Vec::with_capacity(sources.len());
        for source in sources {
            let id = source.source_id.clone();
            let label = if source.name.is_empty() {
                SharedString::from(source.url.clone())
            } else {
                SharedString::from(source.name.clone())
            };
            let count_label = {
                let id = id.clone();
                SharedString::from(
                    self.table_state
                        .read(cx)
                        .delegate()
                        .count_where(move |task| task.rss_source_id == id)
                        .to_string(),
                )
            };
            let menu = self.rss_item_context_menu(id.clone(), cx);
            items.push(
                self.nav_item(
                    format!("download-nav-rss-{id}"),
                    SidebarSelection::RssSource(id),
                    label,
                    IconName::Globe,
                    (count_label, None),
                    cx,
                )
                .context_menu(menu),
            );
        }
        self.section_card(cx)
            .child(self.section_header(
                "download-rss-toggle",
                self.strings.sidebar_rss.clone(),
                SidebarSection::Rss,
                open_amount,
                None,
                cx,
            ))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(NAV_ITEM_HEIGHT * count * open_amount))
                    .child(v_flex().w_full().opacity(open_amount).children(items)),
            )
    }

    /// 设备区：本机 + 云设备 + 已配对设备；远程任务按来源设备 id / 指纹计数。
    fn render_devices_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let open_amount = self.section_open_amount(SidebarSection::Devices, window, cx);
        let mut entries: Vec<(String, SharedString)> = vec![(
            SidebarSelection::LOCAL_DEVICE.to_owned(),
            self.strings.this_device.clone(),
        )];
        entries.extend(self.controller.cloud_devices().iter().map(|device| {
            let name = if device.name.is_empty() {
                device.device_id.clone()
            } else {
                device.name.clone()
            };
            (device.device_id.clone(), SharedString::from(name))
        }));
        entries.extend(self.controller.linked_devices().iter().map(|device| {
            let name = if device.name.is_empty() {
                device.fingerprint.clone()
            } else {
                device.name.clone()
            };
            (device.fingerprint.clone(), SharedString::from(name))
        }));
        let count = entries.len() as f32;
        let mut items = Vec::with_capacity(entries.len());
        for (id, label) in entries {
            let icon = if id == SidebarSelection::LOCAL_DEVICE {
                IconName::Cpu
            } else {
                IconName::Network
            };
            let count_label = self.device_count(&id, cx);
            items.push(self.nav_item(
                format!("download-nav-device-{id}"),
                SidebarSelection::Device(id),
                label,
                icon,
                (count_label, None),
                cx,
            ));
        }
        self.section_card(cx)
            .child(self.section_header(
                "download-devices-toggle",
                self.strings.sidebar_devices.clone(),
                SidebarSection::Devices,
                open_amount,
                None,
                cx,
            ))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(NAV_ITEM_HEIGHT * count * open_amount))
                    .child(v_flex().w_full().opacity(open_amount).children(items)),
            )
    }

    pub(crate) fn render_sidebar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = active_theme(cx).tokens().colors.surface;
        let mut root = v_flex()
            .size_full()
            .min_w_0()
            .overflow_y_scrollbar()
            .bg(surface);
        for section in SidebarSection::ALL {
            if !self
                .controller
                .preference_bool(section.visibility_pref(), true)
            {
                continue;
            }
            let content: Div = match section {
                SidebarSection::Status => self.render_status_section(window, cx),
                SidebarSection::Queues => self.render_queue_section(window, cx),
                SidebarSection::Rss => self.render_rss_section(window, cx),
                SidebarSection::Devices => self.render_devices_section(window, cx),
            };
            root = root.child(content);
        }
        root
    }
}
