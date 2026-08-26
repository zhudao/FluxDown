use std::time::{Duration, Instant};

use fluxdown_ui_i18n::Translator;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, KeyBinding, ParentElement, Render, Styled,
    Window, actions, div, px,
};
use gpui_component::{
    ResizableState, h_resizable, resizable_panel,
    table::{TableEvent, TableState},
};

use crate::{
    components::task_table::DownloadTableDelegate,
    model::{
        DownloadFilter, DownloadStatusFilter, SidebarSelection, StatusFolderMotion, preview_tasks,
    },
    strings::DownloadStrings,
};

actions!(downloads, [SelectAllTasks]);

pub(crate) const TASK_ROW_HEIGHT: f32 = 38.;
pub(crate) const SIDEBAR_MOTION_DURATION: Duration = Duration::from_millis(200);

/// 下载能力的顶层页面。
pub struct DownloadView {
    pub(crate) strings: DownloadStrings,
    pub(crate) selected_item: SidebarSelection,
    pub(crate) expanded_status: Option<DownloadStatusFilter>,
    pub(crate) folder_motion: StatusFolderMotion,
    pub(crate) folder_motion_started_at: Option<Instant>,
    pub(crate) queues_expanded: bool,
    pub(crate) queue_motion_from: f32,
    pub(crate) queue_motion_started_at: Option<Instant>,
    pub(crate) table_state: Entity<TableState<DownloadTableDelegate>>,
    resizable_state: Entity<ResizableState>,
    resizable_state_initialized: bool,
}

impl DownloadView {
    /// 创建下载页面，并订阅共享翻译状态。
    pub fn new(
        translator: Entity<Translator>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.bind_keys([KeyBinding::new("ctrl-a", SelectAllTasks, Some("DataTable"))]);
        let strings = DownloadStrings::from_translator(translator.read(cx));
        let table_state = cx.new(|cx| {
            TableState::new(
                DownloadTableDelegate::new(strings.clone(), preview_tasks()),
                window,
                cx,
            )
            .row_selectable(false)
            .col_selectable(false)
        });

        cx.observe(&translator, |this, translator, cx| {
            this.set_strings(DownloadStrings::from_translator(translator.read(cx)), cx);
        })
        .detach();
        cx.subscribe_in(&table_state, window, Self::handle_table_event)
            .detach();

        Self {
            strings,
            selected_item: SidebarSelection::Download(DownloadFilter::ALL),
            expanded_status: Some(DownloadStatusFilter::All),
            folder_motion: StatusFolderMotion::settled(Some(DownloadStatusFilter::All)),
            folder_motion_started_at: None,
            queues_expanded: true,
            queue_motion_from: 1.,
            queue_motion_started_at: None,
            table_state,
            resizable_state: cx.new(|_| ResizableState::default()),
            resizable_state_initialized: false,
        }
    }

    fn set_strings(&mut self, strings: DownloadStrings, cx: &mut Context<Self>) {
        self.strings = strings.clone();
        self.table_state.update(cx, |table, cx| {
            table.delegate_mut().set_strings(strings);
            table.refresh(cx);
        });
        cx.notify();
    }

    fn handle_table_event(
        &mut self,
        table_state: &Entity<TableState<DownloadTableDelegate>>,
        event: &TableEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let TableEvent::RightClickedRow(Some(row_ix)) = event else {
            return;
        };
        table_state.update(cx, |table, cx| {
            if let Some(task_id) = table.delegate().task_id_at(*row_ix) {
                table.delegate_mut().select_task_for_context_menu(task_id);
            }
            table.set_right_clicked_row(None, cx);
        });
    }
}

impl Render for DownloadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_panel_measured = self
            .resizable_state
            .read(cx)
            .sizes()
            .get(1)
            .is_some_and(|size| *size > px(1.));
        if !self.resizable_state_initialized && main_panel_measured {
            self.resizable_state
                .update(cx, |state, cx| state.reset_panel(1, cx));
            self.resizable_state_initialized = true;
        }

        div().size_full().min_w_0().min_h_0().child(
            h_resizable("downloads-content")
                .with_state(&self.resizable_state)
                .on_resize(|state, _, cx| {
                    state.update(cx, |state, cx| state.reset_panel(1, cx));
                })
                .child(
                    resizable_panel()
                        .size(px(160.))
                        .flex_none()
                        .size_range(px(148.)..px(280.))
                        .child(self.render_sidebar(window, cx)),
                )
                .child(resizable_panel().child(self.render_main(cx))),
        )
    }
}
