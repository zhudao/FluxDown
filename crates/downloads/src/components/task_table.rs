use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use fluxdown_ui_components::toolbar_action_button;
use fluxdown_ui_theme::{CONTROL_HEIGHT, active_theme};
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, Modifiers, MouseButton, ParentElement, Render,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{
    ActiveTheme as _, FocusableExt as _, Icon, IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
    spinner::Spinner,
    table::{Column, ColumnSort, DataTable, TableDelegate, TableState},
    tooltip::Tooltip,
    v_flex,
};

use crate::{
    controller::DownloadsCommand,
    model::{
        CategoryIndex, DownloadFilter, DownloadTaskView, RowId, RowKey, SidebarSelection, TaskKind,
        TaskSource, TaskState, TaskStore, format_bytes,
        view_prefs::{DateBucket, SortDir, ViewGroupBy, ViewPrefs, ViewSortKey, state_group_key},
    },
    pages::downloads::DownloadView,
    strings::DownloadStrings,
};

const SELECTION_COLUMN_WIDTH: f32 = 36.;
const TABLE_HEADER_CROP: f32 = 2.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownloadColumnKind {
    FileName,
    Progress,
    Size,
    Speed,
    Eta,
    Status,
    Created,
    Protocol,
    Source,
    Queue,
}

impl DownloadColumnKind {
    pub(crate) const ALL: [Self; 10] = [
        Self::FileName,
        Self::Progress,
        Self::Size,
        Self::Speed,
        Self::Eta,
        Self::Status,
        Self::Created,
        Self::Protocol,
        Self::Source,
        Self::Queue,
    ];

    /// 宽度不足时的裁列顺序（先裁前者），`FileName` 永不裁。
    const FIT_DROP_ORDER: [Self; 6] = [
        Self::Created,
        Self::Source,
        Self::Protocol,
        Self::Queue,
        Self::Eta,
        Self::Speed,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::FileName => "file_name",
            Self::Progress => "progress",
            Self::Size => "size",
            Self::Speed => "speed",
            Self::Eta => "eta",
            Self::Status => "status",
            Self::Created => "created",
            Self::Protocol => "protocol",
            Self::Source => "source",
            Self::Queue => "queue",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    fn default_visible(self) -> bool {
        !matches!(self, Self::Protocol | Self::Source | Self::Queue)
    }

    fn default_width(self) -> f32 {
        match self {
            Self::FileName => 240.,
            Self::Progress => 150.,
            Self::Size => 90.,
            Self::Speed => 100.,
            Self::Eta => 100.,
            Self::Status => 110.,
            Self::Created => 110.,
            Self::Protocol => 64.,
            Self::Source => 148.,
            Self::Queue => 88.,
        }
    }

    fn min_width(self) -> f32 {
        match self {
            Self::FileName => 160.,
            Self::Progress => 110.,
            Self::Size => 72.,
            Self::Speed => 82.,
            Self::Eta => 82.,
            Self::Status => 84.,
            Self::Created => 90.,
            Self::Protocol => 56.,
            Self::Source => 96.,
            Self::Queue => 72.,
        }
    }

    fn sort_key(self) -> Option<ViewSortKey> {
        match self {
            Self::FileName => Some(ViewSortKey::Name),
            Self::Progress => Some(ViewSortKey::Progress),
            Self::Size => Some(ViewSortKey::Size),
            Self::Speed => Some(ViewSortKey::Speed),
            Self::Created => Some(ViewSortKey::Created),
            _ => None,
        }
    }

    fn label(self, strings: &DownloadStrings) -> SharedString {
        match self {
            Self::FileName => strings.col_file_name.clone(),
            Self::Progress => strings.col_progress.clone(),
            Self::Size => strings.col_size.clone(),
            Self::Speed => strings.col_speed.clone(),
            Self::Eta => strings.col_eta.clone(),
            Self::Status => strings.col_status.clone(),
            Self::Created => strings.col_created.clone(),
            Self::Protocol => strings.col_protocol.clone(),
            Self::Source => strings.col_source.clone(),
            Self::Queue => strings.col_queue.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct DownloadColumn {
    pub(crate) kind: DownloadColumnKind,
    pub(crate) width: f32,
    /// 用户开关。
    pub(crate) visible: bool,
    /// 宽度不足自动裁掉（用户开关保持）。
    auto_hidden: bool,
}

impl DownloadColumn {
    fn new(kind: DownloadColumnKind) -> Self {
        Self {
            kind,
            width: kind.default_width(),
            visible: kind.default_visible(),
            auto_hidden: false,
        }
    }

    fn shown(&self) -> bool {
        self.visible && !self.auto_hidden
    }
}

#[derive(Clone)]
struct DraggedColumnMenuItem {
    kind: DownloadColumnKind,
    label: SharedString,
}

impl Render for DraggedColumnMenuItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        h_flex()
            .px(tokens.spacing.md)
            .py(tokens.spacing.xs)
            .gap(tokens.spacing.sm)
            .rounded(tokens.radius.sm)
            .border_1()
            .border_color(tokens.colors.border)
            .bg(tokens.colors.surface)
            .shadow_sm()
            .text_color(tokens.colors.surface_foreground)
            .child(Icon::new(IconName::Menu).size(px(13.)))
            .child(self.label.clone())
    }
}

/// 表格可见行：任务行或分组头。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VisibleRow {
    Task(RowId),
    GroupHeader {
        key: String,
        label: SharedString,
        count: usize,
        collapsed: bool,
    },
}

/// 分组计算结果。
struct GroupBucket {
    key: String,
    label: SharedString,
    order: i64,
}

/// 侧栏选中项到表格筛选的投影。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TableFilter {
    Download(DownloadFilter),
    Queue(String),
    RssSource(String),
    Device(String),
    Group(String),
}

impl From<&SidebarSelection> for TableFilter {
    fn from(selection: &SidebarSelection) -> Self {
        match selection {
            SidebarSelection::Download(filter) => Self::Download(filter.clone()),
            SidebarSelection::Queue(id) => Self::Queue(id.clone()),
            SidebarSelection::RssSource(id) => Self::RssSource(id.clone()),
            SidebarSelection::Device(id) => Self::Device(id.clone()),
        }
    }
}

/// 右键菜单单任务事实：足以判定菜单项可见性的最小状态快照，与
/// [`DownloadTaskView`] 解耦以便纯函数测试。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskMenuFacts {
    pub(crate) is_local: bool,
    pub(crate) state: TaskState,
    pub(crate) boosted: bool,
    /// `url` 是否为 BT 哨兵值 `torrent-file://…`（不可重新下载）。
    pub(crate) is_torrent_sentinel: bool,
    /// 失败态且错误消息带插件重试前缀（与
    /// `lib/src/widgets/task_list_item.dart` 的 `_pluginErrorPrefix` 同源）。
    pub(crate) is_plugin_retry_error: bool,
}

/// 引擎/daemon 插件系统失败任务的错误消息前缀。
pub(crate) const PLUGIN_ERROR_PREFIX: &str = "[插件]";

/// 任务右键菜单条目；数组顺序即渲染顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuEntry {
    Resume,
    Pause,
    IgnorePluginRetry,
    Boost,
    CancelBoost,
    OpenFile,
    OpenFolder,
    Rename,
    Redownload,
    CopyUrl,
    MoveToQueue,
    Separator,
    Delete,
    DeleteWithFiles,
    OpenInWindow,
}

/// 纯函数：按选中任务集合的事实计算应显示的右键菜单项。
///
/// 多选时每一项都要求对**全部**选中任务成立（交集语义），resume/pause 例外
/// ——只要选中集合中**存在**一个可恢复/可暂停的任务就显示（与工具栏按钮行为
/// 一致：工具栏无条件对每个选中项下发命令，无效状态的任务被服务端忽略）。
pub(crate) fn context_menu_items(selection: &[TaskMenuFacts]) -> Vec<MenuEntry> {
    if selection.is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    if selection
        .iter()
        .any(|task| !matches!(task.state, TaskState::Completed | TaskState::Downloading))
    {
        items.push(MenuEntry::Resume);
    }
    if selection
        .iter()
        .any(|task| matches!(task.state, TaskState::Downloading | TaskState::Pending))
    {
        items.push(MenuEntry::Pause);
    }
    if let [only] = selection {
        if only.state == TaskState::Failed && only.is_plugin_retry_error {
            items.push(MenuEntry::IgnorePluginRetry);
        }
        if only.is_local {
            items.push(if only.boosted {
                MenuEntry::CancelBoost
            } else {
                MenuEntry::Boost
            });
        }
    }
    if selection
        .iter()
        .all(|task| task.is_local && task.state == TaskState::Completed)
    {
        items.push(MenuEntry::OpenFile);
    }
    if selection.iter().all(|task| task.is_local) {
        items.push(MenuEntry::OpenFolder);
    }
    if let [only] = selection
        && only.is_local
    {
        items.push(MenuEntry::Rename);
    }
    if selection.iter().all(|task| {
        task.is_local
            && matches!(task.state, TaskState::Completed | TaskState::Failed)
            && !task.is_torrent_sentinel
    }) {
        items.push(MenuEntry::Redownload);
    }
    items.push(MenuEntry::CopyUrl);
    if selection.iter().all(|task| task.is_local) {
        items.push(MenuEntry::MoveToQueue);
    }
    items.push(MenuEntry::Separator);
    items.push(MenuEntry::Delete);
    items.push(MenuEntry::DeleteWithFiles);
    if selection.iter().any(|task| task.is_local) {
        items.push(MenuEntry::OpenInWindow);
    }
    items
}

/// 构造一个分组头右键菜单项：点击时把 `group_id` 回调进 [`DownloadView`] 的方法
/// （无需 `Window`，用于批量命令类操作）。
fn group_menu_item(
    label: SharedString,
    icon: IconName,
    host: &WeakEntity<DownloadView>,
    group_id: &str,
    action: fn(&mut DownloadView, String, &mut Context<DownloadView>),
) -> PopupMenuItem {
    let host = host.clone();
    let group_id = group_id.to_owned();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, _, cx| {
            let group_id = group_id.clone();
            let _ = host.update(cx, |view, cx| action(view, group_id, cx));
        })
}

/// 同 [`group_menu_item`]，但回调需要 `Window`（打开确认对话框 / 独立窗口）。
fn group_menu_item_windowed(
    label: SharedString,
    icon: IconName,
    host: &WeakEntity<DownloadView>,
    group_id: &str,
    action: fn(&mut DownloadView, String, &mut Window, &mut Context<DownloadView>),
) -> PopupMenuItem {
    let host = host.clone();
    let group_id = group_id.to_owned();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, window, cx| {
            let group_id = group_id.clone();
            let _ = host.update(cx, |view, cx| action(view, group_id, window, cx));
        })
}

pub(crate) struct DownloadTableDelegate {
    strings: DownloadStrings,
    columns: Vec<DownloadColumn>,
    store: Rc<TaskStore>,
    categories: Rc<CategoryIndex>,
    visible: Vec<VisibleRow>,
    seen_generation: u64,
    view_dirty: bool,
    selected_tasks: HashSet<RowKey>,
    selection_anchor: Option<RowKey>,
    filter: TableFilter,
    query: String,
    prefs: ViewPrefs,
    /// queue_id → 名称（列 / 分组标签）。
    queue_names: Vec<(String, String)>,
    /// group_id → 名称。
    group_names: HashMap<String, String>,
    /// 远程任务来源设备匹配用：设备 id / 指纹 → 别名集合。
    device_aliases: HashMap<String, Vec<String>>,
    /// 排序在未改变的前提下是否在偏好中被列头触发（用于持久化）。
    sort_changed: bool,
    /// 上级页面弱引用：右键菜单需要参数的命令（移动到队列 / 忽略插件重试 /
    /// 任务组操作）经它回调 `DownloadView` 的方法执行；`None` 时这些项不渲染。
    host: Option<WeakEntity<DownloadView>>,
    /// 右键菜单动作分派目标；`None` 时菜单项不带 action_context（仍可点击，
    /// 只是动作从菜单自身的焦点路径冒泡）。
    action_context: Option<FocusHandle>,
}

impl DownloadTableDelegate {
    pub(crate) fn new(strings: DownloadStrings, store: Rc<TaskStore>) -> Self {
        Self {
            strings,
            columns: DownloadColumnKind::ALL
                .into_iter()
                .map(DownloadColumn::new)
                .collect(),
            store,
            categories: Rc::new(CategoryIndex::default()),
            visible: Vec::new(),
            seen_generation: u64::MAX,
            view_dirty: true,
            selected_tasks: HashSet::new(),
            selection_anchor: None,
            filter: TableFilter::Download(DownloadFilter::ALL),
            query: String::new(),
            prefs: ViewPrefs::default(),
            queue_names: Vec::new(),
            group_names: HashMap::new(),
            device_aliases: HashMap::new(),
            sort_changed: false,
            host: None,
            action_context: None,
        }
    }

    pub(crate) fn set_strings(&mut self, strings: DownloadStrings) {
        self.strings = strings;
        self.view_dirty = true;
    }

    /// 注入宿主弱引用；右键菜单中需要参数的命令经它回调
    /// [`DownloadView`] 的方法执行。
    pub(crate) fn set_host(&mut self, host: WeakEntity<DownloadView>) {
        self.host = Some(host);
    }

    /// 注入右键菜单动作分派目标（根元素的 focus handle）。
    pub(crate) fn set_action_context(&mut self, handle: FocusHandle) {
        self.action_context = Some(handle);
    }

    pub(crate) fn set_categories(&mut self, categories: Rc<CategoryIndex>) {
        if !Rc::ptr_eq(&self.categories, &categories) {
            self.categories = categories;
            self.view_dirty = true;
        }
    }

    pub(crate) fn set_queue_names(&mut self, queues: Vec<(String, String)>) {
        if self.queue_names != queues {
            self.queue_names = queues;
            self.view_dirty = true;
        }
    }

    pub(crate) fn set_group_names(&mut self, groups: HashMap<String, String>) {
        if self.group_names != groups {
            self.group_names = groups;
            self.view_dirty = true;
        }
    }

    pub(crate) fn set_device_aliases(&mut self, aliases: HashMap<String, Vec<String>>) {
        if self.device_aliases != aliases {
            self.device_aliases = aliases;
            self.view_dirty = true;
        }
    }

    pub(crate) fn set_filter(&mut self, filter: TableFilter) {
        if self.filter != filter {
            self.filter = filter;
            self.view_dirty = true;
        }
    }

    pub(crate) fn set_query(&mut self, query: &str) {
        let query = query.trim().to_lowercase();
        if self.query != query {
            self.query = query;
            self.view_dirty = true;
        }
    }

    pub(crate) fn prefs(&self) -> &ViewPrefs {
        &self.prefs
    }

    /// 替换视图偏好（密度 / 分组 / 排序 / 列 / 折叠）。
    pub(crate) fn set_prefs(&mut self, prefs: ViewPrefs) {
        if self.prefs == prefs {
            return;
        }
        self.apply_column_prefs(&prefs.columns);
        self.prefs = prefs;
        self.view_dirty = true;
    }

    pub(crate) fn prefs_mut(&mut self) -> &mut ViewPrefs {
        self.view_dirty = true;
        &mut self.prefs
    }

    /// 列头排序是否改动了偏好（读取后清零）。
    pub(crate) fn take_sort_changed(&mut self) -> bool {
        std::mem::take(&mut self.sort_changed)
    }

    fn apply_column_prefs(&mut self, prefs: &[crate::model::view_prefs::ColumnPref]) {
        if prefs.is_empty() {
            return;
        }
        let mut columns: Vec<DownloadColumn> = prefs
            .iter()
            .filter_map(|pref| {
                let kind = DownloadColumnKind::from_key(&pref.key)?;
                let mut column = DownloadColumn::new(kind);
                column.visible = pref.visible;
                if pref.width >= kind.min_width() {
                    column.width = pref.width;
                }
                Some(column)
            })
            .collect();
        for kind in DownloadColumnKind::ALL {
            if !columns.iter().any(|column| column.kind == kind) {
                columns.push(DownloadColumn::new(kind));
            }
        }
        if !columns.iter().any(|column| column.visible) {
            columns[0].visible = true;
        }
        self.columns = columns;
    }

    /// 当前列配置 → 偏好条目。
    pub(crate) fn column_prefs(&self) -> Vec<crate::model::view_prefs::ColumnPref> {
        self.columns
            .iter()
            .map(|column| crate::model::view_prefs::ColumnPref {
                key: column.kind.key().to_owned(),
                visible: column.visible,
                width: column.width,
            })
            .collect()
    }

    /// 宽度预算：可用宽度不足时按 [`DownloadColumnKind::FIT_DROP_ORDER`] 自动裁列。
    pub(crate) fn fit_columns_to_width(&mut self, available: f32) -> bool {
        let mut changed = false;
        for column in &mut self.columns {
            if column.auto_hidden {
                column.auto_hidden = false;
                changed = true;
            }
        }
        let width_sum = |columns: &[DownloadColumn]| {
            SELECTION_COLUMN_WIDTH
                + columns
                    .iter()
                    .filter(|column| column.shown())
                    .map(|column| column.width)
                    .sum::<f32>()
        };
        for kind in DownloadColumnKind::FIT_DROP_ORDER {
            if width_sum(&self.columns) <= available {
                break;
            }
            if let Some(column) = self
                .columns
                .iter_mut()
                .find(|column| column.kind == kind && column.shown())
            {
                column.auto_hidden = true;
                changed = true;
            }
        }
        changed
    }

    /// 存储变化或视图参数变化时重算可见行。返回是否重算。
    pub(crate) fn refresh_view(&mut self) -> bool {
        let generation = self.store.generation();
        if !self.view_dirty && generation == self.seen_generation {
            return false;
        }
        self.seen_generation = generation;
        self.view_dirty = false;
        self.visible = self.compute_visible();
        self.selected_tasks
            .retain(|key| self.store.row_id(key).is_some());
        self.selection_anchor = self
            .selection_anchor
            .take()
            .filter(|key| self.selected_tasks.contains(key));
        true
    }

    fn matches_query(&self, task: &DownloadTaskView) -> bool {
        if self.query.is_empty() {
            return true;
        }
        task.name.to_lowercase().contains(&self.query)
            || task.url.to_lowercase().contains(&self.query)
            || task.site.to_lowercase().contains(&self.query)
    }

    fn matches_filter(&self, task: &DownloadTaskView) -> bool {
        match &self.filter {
            TableFilter::Download(filter) => filter.matches(task, &self.categories),
            TableFilter::Queue(queue_id) => {
                task.source == TaskSource::Local && task.queue_id == *queue_id
            }
            TableFilter::RssSource(source_id) => task.rss_source_id == *source_id,
            TableFilter::Device(device) => {
                if device == SidebarSelection::LOCAL_DEVICE {
                    task.source == TaskSource::Local
                } else {
                    task.source == TaskSource::Remote
                        && (task.from_device == *device
                            || self
                                .device_aliases
                                .get(device)
                                .is_some_and(|aliases| aliases.contains(&task.from_device)))
                }
            }
            TableFilter::Group(group_id) => task.group_id == *group_id,
        }
    }

    fn compute_visible(&self) -> Vec<VisibleRow> {
        let local = self.store.local();
        let remote = self.store.remote();
        let mut rows: Vec<(RowId, &DownloadTaskView)> = local
            .iter()
            .enumerate()
            .map(|(ix, task)| (RowId::Local(ix), task))
            .chain(
                remote
                    .iter()
                    .enumerate()
                    .map(|(ix, task)| (RowId::Remote(ix), task)),
            )
            .filter(|(_, task)| self.matches_filter(task) && self.matches_query(task))
            .collect();
        rows.sort_by(|(_, left), (_, right)| self.prefs.compare(left, right));

        if self.prefs.group_by == ViewGroupBy::None {
            return rows
                .into_iter()
                .map(|(id, _)| VisibleRow::Task(id))
                .collect();
        }

        let mut buckets: Vec<(GroupBucket, Vec<RowId>)> = Vec::new();
        for (id, task) in rows {
            let bucket = self.group_bucket(task);
            match buckets
                .iter_mut()
                .find(|(existing, _)| existing.key == bucket.key)
            {
                Some((_, ids)) => ids.push(id),
                None => buckets.push((bucket, vec![id])),
            }
        }
        buckets.sort_by(|(left, _), (right, _)| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.label.cmp(&right.label))
        });
        let mut visible = Vec::new();
        for (bucket, ids) in buckets {
            let collapsed = self.prefs.is_group_collapsed(&bucket.key);
            visible.push(VisibleRow::GroupHeader {
                key: bucket.key,
                label: bucket.label,
                count: ids.len(),
                collapsed,
            });
            if !collapsed {
                visible.extend(ids.into_iter().map(VisibleRow::Task));
            }
        }
        visible
    }

    fn group_bucket(&self, task: &DownloadTaskView) -> GroupBucket {
        let strings = &self.strings;
        match self.prefs.group_by {
            ViewGroupBy::None => GroupBucket {
                key: String::new(),
                label: SharedString::default(),
                order: 0,
            },
            ViewGroupBy::Status => GroupBucket {
                key: format!("status:{}", state_group_key(task.state)),
                label: strings.state_label(task.state),
                order: i64::from(task.state.smart_rank()),
            },
            ViewGroupBy::Date => {
                let bucket = DateBucket::of(task.created_at_secs);
                GroupBucket {
                    key: format!("date:{}", bucket.key()),
                    label: strings.date_bucket_label(bucket),
                    order: bucket as i64,
                }
            }
            ViewGroupBy::Type => {
                let id = self.categories.category_of(task);
                let (label, order) = self
                    .categories
                    .rules()
                    .iter()
                    .enumerate()
                    .find(|(_, rule)| rule.dto.id == id)
                    .map_or_else(
                        || (strings.category_other.clone(), i64::MAX),
                        |(ix, rule)| (strings.category_label(&rule.dto), ix as i64),
                    );
                GroupBucket {
                    key: format!("type:{id}"),
                    label,
                    order,
                }
            }
            ViewGroupBy::Queue => {
                if task.source == TaskSource::Remote {
                    return GroupBucket {
                        key: "queue:remote".to_owned(),
                        label: strings.remote_tasks.clone(),
                        order: i64::MAX,
                    };
                }
                let (label, order) = self
                    .queue_names
                    .iter()
                    .enumerate()
                    .find(|(_, (id, _))| *id == task.queue_id)
                    .map_or_else(
                        || (SharedString::from(task.queue_id.clone()), i64::MAX - 1),
                        |(ix, (_, name))| (SharedString::from(name.clone()), ix as i64),
                    );
                GroupBucket {
                    key: format!("queue:{}", task.queue_id),
                    label,
                    order,
                }
            }
            ViewGroupBy::Site => {
                let site = task.source_site();
                let label = if site.is_empty() {
                    strings.site_unknown(task)
                } else {
                    SharedString::from(site.to_owned())
                };
                GroupBucket {
                    key: format!("site:{}", label),
                    label,
                    order: 0,
                }
            }
            ViewGroupBy::Group => {
                if task.group_id.is_empty() {
                    return GroupBucket {
                        key: "group:".to_owned(),
                        label: strings.ungrouped.clone(),
                        order: i64::MAX,
                    };
                }
                let label = self
                    .group_names
                    .get(&task.group_id)
                    .cloned()
                    .unwrap_or_else(|| task.group_id.clone());
                GroupBucket {
                    key: format!("group:{}", task.group_id),
                    label: SharedString::from(label),
                    order: 0,
                }
            }
        }
    }

    fn visible_task_ids(&self) -> impl Iterator<Item = RowId> + '_ {
        self.visible.iter().filter_map(|row| match row {
            VisibleRow::Task(id) => Some(*id),
            VisibleRow::GroupHeader { .. } => None,
        })
    }

    fn visible_task_keys(&self) -> Vec<RowKey> {
        self.visible_task_ids()
            .filter_map(|id| self.store.row(id).map(|row| row.key.clone()))
            .collect()
    }

    pub(crate) fn count_matching(&self, filter: &DownloadFilter) -> usize {
        let local = self.store.local();
        let remote = self.store.remote();
        local
            .iter()
            .chain(remote.iter())
            .filter(|task| filter.matches(task, &self.categories))
            .count()
    }

    pub(crate) fn count_where(&self, predicate: impl Fn(&DownloadTaskView) -> bool) -> usize {
        let local = self.store.local();
        let remote = self.store.remote();
        local
            .iter()
            .chain(remote.iter())
            .filter(|t| predicate(t))
            .count()
    }

    pub(crate) fn count_in_queue(&self, queue_id: &str) -> usize {
        self.store
            .local()
            .iter()
            .filter(|task| task.queue_id == queue_id)
            .count()
    }

    fn shown_columns_count(&self) -> usize {
        self.columns.iter().filter(|column| column.shown()).count()
    }

    fn shown_column(&self, col_ix: usize) -> Option<&DownloadColumn> {
        if col_ix == 0 {
            return None;
        }
        self.columns
            .iter()
            .filter(|column| column.shown())
            .nth(col_ix - 1)
    }

    fn move_shown_column(&mut self, from_ix: usize, to_ix: usize) {
        if from_ix == 0 || to_ix == 0 {
            return;
        }
        let positions: Vec<_> = self
            .columns
            .iter()
            .enumerate()
            .filter_map(|(ix, column)| column.shown().then_some(ix))
            .collect();
        let Some(&from_position) = positions.get(from_ix - 1) else {
            return;
        };
        let Some(&to_position) = positions.get(to_ix - 1) else {
            return;
        };
        let column = self.columns.remove(from_position);
        self.columns.insert(to_position, column);
    }

    fn move_column_kind(&mut self, from: DownloadColumnKind, to: DownloadColumnKind) {
        let Some(from_ix) = self.columns.iter().position(|column| column.kind == from) else {
            return;
        };
        let Some(to_ix) = self.columns.iter().position(|column| column.kind == to) else {
            return;
        };
        if from_ix == to_ix {
            return;
        }
        let column = self.columns.remove(from_ix);
        self.columns.insert(to_ix, column);
    }

    fn set_column_visible(&mut self, kind: DownloadColumnKind, visible: bool) -> bool {
        if !visible && self.columns.iter().filter(|column| column.visible).count() <= 1 {
            return false;
        }
        let Some(column) = self.columns.iter_mut().find(|column| column.kind == kind) else {
            return false;
        };
        if column.visible == visible {
            return false;
        }
        column.visible = visible;
        true
    }

    fn reset_columns(&mut self) {
        self.columns = DownloadColumnKind::ALL
            .into_iter()
            .map(DownloadColumn::new)
            .collect();
    }

    pub(crate) fn select_all_tasks(&mut self) {
        self.selected_tasks.clear();
        self.selected_tasks.extend(self.visible_task_keys());
        self.selection_anchor = None;
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected_tasks.clear();
        self.selection_anchor = None;
    }

    fn select_task(&mut self, key: RowKey, modifiers: Modifiers) {
        if modifiers.shift
            && let Some(anchor) = self.selection_anchor.clone()
        {
            let keys = self.visible_task_keys();
            if let Some(anchor_index) = keys.iter().position(|item| *item == anchor)
                && let Some(task_index) = keys.iter().position(|item| *item == key)
            {
                let (start, end) = if anchor_index <= task_index {
                    (anchor_index, task_index)
                } else {
                    (task_index, anchor_index)
                };
                self.selected_tasks.clear();
                self.selected_tasks
                    .extend(keys[start..=end].iter().cloned());
                self.selection_anchor = Some(key);
                return;
            }
        }

        if modifiers.secondary() {
            if !self.selected_tasks.remove(&key) {
                self.selected_tasks.insert(key.clone());
            }
        } else {
            self.selected_tasks.clear();
            self.selected_tasks.insert(key.clone());
        }
        self.selection_anchor = Some(key);
    }

    pub(crate) fn row_key_at(&self, row_ix: usize) -> Option<RowKey> {
        match self.visible.get(row_ix)? {
            VisibleRow::Task(id) => self.store.row(*id).map(|row| row.key.clone()),
            VisibleRow::GroupHeader { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn group_key_at(&self, row_ix: usize) -> Option<&str> {
        match self.visible.get(row_ix)? {
            VisibleRow::GroupHeader { key, .. } => Some(key),
            VisibleRow::Task(_) => None,
        }
    }

    pub(crate) fn select_task_for_context_menu(&mut self, key: RowKey) {
        if !self.selected_tasks.contains(&key) {
            self.selected_tasks.clear();
            self.selected_tasks.insert(key.clone());
        }
        self.selection_anchor = Some(key);
    }

    pub(crate) fn selected_keys(&self) -> Vec<RowKey> {
        let mut keys: Vec<RowKey> = self.selected_tasks.iter().cloned().collect();
        keys.sort();
        keys
    }

    pub(crate) fn toggle_group_collapsed(&mut self, key: &str) {
        self.prefs.toggle_group_collapsed(key);
        self.view_dirty = true;
    }

    fn task_visuals(
        &self,
        task: &DownloadTaskView,
        cx: &App,
    ) -> (SharedString, gpui::Hsla, IconName, gpui::Hsla, SharedString) {
        let tokens = active_theme(cx).tokens();
        let (status, status_color) = match task.state {
            TaskState::Completed => (self.strings.status_completed.clone(), cx.theme().success),
            TaskState::Paused => (self.strings.status_paused.clone(), cx.theme().warning),
            TaskState::Failed => (self.strings.status_error.clone(), cx.theme().danger),
            TaskState::Downloading => (
                self.strings.status_downloading.clone(),
                tokens.colors.primary,
            ),
            TaskState::Pending => (
                self.strings.status_pending.clone(),
                tokens.colors.muted_foreground,
            ),
        };
        let (file_icon, file_icon_color, category) = match task.kind {
            TaskKind::Application => (
                IconName::HardDrive,
                tokens.colors.muted_foreground,
                self.strings.category_program.clone(),
            ),
            TaskKind::Mobile => (
                IconName::MemoryStick,
                tokens.colors.muted_foreground,
                self.strings.category_program.clone(),
            ),
            TaskKind::DiskImage | TaskKind::Archive => (
                IconName::Inbox,
                cx.theme().warning,
                self.strings.category_archive.clone(),
            ),
            TaskKind::Video => (
                IconName::Play,
                tokens.colors.muted_foreground,
                self.strings.category_video.clone(),
            ),
            TaskKind::Audio => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_audio.clone(),
            ),
            TaskKind::Document => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_document.clone(),
            ),
            TaskKind::Image => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_image.clone(),
            ),
            TaskKind::Other => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_other.clone(),
            ),
        };
        (status, status_color, file_icon, file_icon_color, category)
    }

    fn render_file_cell(&self, task: &DownloadTaskView, cx: &App) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        let compact = self.prefs.density == crate::model::view_prefs::ViewDensity::Compact;
        let (_, _, file_icon, file_icon_color, category) = self.task_visuals(task, cx);
        let name = if task.metadata_pending {
            self.strings.metadata_loading.clone()
        } else {
            SharedString::from(task.name.clone())
        };
        let icon = if task.metadata_pending {
            Spinner::new()
                .with_size(px(14.))
                .icon(IconName::LoaderCircle)
                .color(tokens.colors.muted_foreground)
                .into_any_element()
        } else {
            Icon::new(file_icon)
                .size(px(14.))
                .text_color(file_icon_color)
                .into_any_element()
        };
        h_flex()
            .size_full()
            .min_w_0()
            .items_center()
            .gap(tokens.spacing.sm)
            .child(div().flex_none().child(icon))
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(relative(1.))
                            .font_weight(tokens.typography.sm.weight)
                            .child(name),
                    )
                    .when(!task.metadata_pending && !compact, |this| {
                        this.child(
                            div()
                                .text_size(px(8.))
                                .line_height(relative(1.))
                                .text_color(tokens.colors.muted_foreground)
                                .child(category),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_progress_cell(&self, task: &DownloadTaskView, cx: &App) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        let (_, status_color, _, _, _) = self.task_visuals(task, cx);
        h_flex()
            .size_full()
            .items_center()
            .gap(tokens.spacing.xs)
            .child(
                div()
                    .relative()
                    .h(px(5.))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .rounded_full()
                    .bg(tokens.colors.muted)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .w(relative(task.progress))
                            .bg(status_color),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(44.))
                    .text_right()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(task.progress_label.clone()),
            )
            .into_any_element()
    }

    fn text_cell(&self, text: impl Into<SharedString>, cx: &App) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        h_flex()
            .size_full()
            .min_w_0()
            .items_center()
            .text_size(tokens.typography.xs.size)
            .text_color(tokens.colors.muted_foreground)
            .child(div().min_w_0().truncate().child(text.into()))
            .into_any_element()
    }

    fn render_group_header(
        &self,
        row_ix: usize,
        key: &str,
        label: &SharedString,
        count: usize,
        collapsed: bool,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        let group_progress =
            key.strip_prefix("group:")
                .filter(|id| !id.is_empty())
                .map(|group_id| {
                    self.store
                        .local()
                        .iter()
                        .filter(|task| task.group_id == group_id)
                        .fold((0usize, 0usize), |(completed, total), task| {
                            let completed = if task.state == TaskState::Completed {
                                completed + 1
                            } else {
                                completed
                            };
                            (completed, total + 1)
                        })
                });
        let key = key.to_owned();
        h_flex()
            .id(("download-group-header", row_ix))
            .size_full()
            .items_center()
            .gap(tokens.spacing.xs)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |table, _: &ClickEvent, _, cx| {
                table.delegate_mut().toggle_group_collapsed(&key);
                table.delegate_mut().refresh_view();
                table.refresh(cx);
                cx.notify();
            }))
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(px(12.))
                .text_color(tokens.colors.muted_foreground),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens.typography.sm.size)
                    .font_weight(tokens.typography.sm.weight)
                    .child(label.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(format!("· {count}")),
            )
            .when_some(group_progress, |this, (completed, total)| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(tokens.typography.xs.size)
                        .text_color(tokens.colors.muted_foreground)
                        .child(format!("· {completed}/{total}")),
                )
            })
            .into_any_element()
    }

    /// 选中集合 → 任务事实；选择滞后于存储事件时任一键缺失即返回 `None`。
    fn selected_menu_facts(&self) -> Option<(Vec<RowKey>, Vec<TaskMenuFacts>)> {
        let selected = self.selected_keys();
        if selected.is_empty() {
            return None;
        }
        let facts = selected
            .iter()
            .map(|key| self.task_menu_facts(key))
            .collect::<Option<Vec<_>>>()?;
        Some((selected, facts))
    }

    fn task_menu_facts(&self, key: &RowKey) -> Option<TaskMenuFacts> {
        let row = self.store.get(key)?;
        Some(TaskMenuFacts {
            is_local: key.is_local(),
            state: row.state,
            boosted: row.boosted,
            is_torrent_sentinel: row.url.starts_with("torrent-file://"),
            is_plugin_retry_error: row.state == TaskState::Failed
                && row.error_message.starts_with(PLUGIN_ERROR_PREFIX),
        })
    }

    /// 任务行右键菜单：按 [`context_menu_items`] 的纯函数结果逐项渲染。
    fn task_context_menu(&self, menu: PopupMenu) -> PopupMenu {
        let Some((selected, facts)) = self.selected_menu_facts() else {
            return menu;
        };
        context_menu_items(&facts)
            .into_iter()
            .fold(menu, |menu, entry| {
                self.apply_menu_entry(menu, entry, &selected)
            })
    }

    fn apply_menu_entry(
        &self,
        menu: PopupMenu,
        entry: MenuEntry,
        selected: &[RowKey],
    ) -> PopupMenu {
        match entry {
            MenuEntry::Resume => menu.menu_with_icon(
                self.strings.resume.clone(),
                IconName::Play,
                Box::new(crate::actions::ResumeSelected),
            ),
            MenuEntry::Pause => menu.menu_with_icon(
                self.strings.pause.clone(),
                IconName::Pause,
                Box::new(crate::actions::PauseSelected),
            ),
            MenuEntry::IgnorePluginRetry => {
                let (Some(host), Some(task_id)) = (
                    self.host.clone(),
                    selected.first().map(|key| key.task_id().to_owned()),
                ) else {
                    return menu;
                };
                menu.item(
                    PopupMenuItem::new(self.strings.ignore_plugin_retry.clone())
                        .icon(IconName::TriangleAlert)
                        .on_click(move |_, window, cx| {
                            let task_id = task_id.clone();
                            let _ = host.update(cx, |view, cx| {
                                view.confirm_ignore_plugin_retry(task_id, window, cx);
                            });
                        }),
                )
            }
            MenuEntry::Boost | MenuEntry::CancelBoost => {
                let cancel = matches!(entry, MenuEntry::CancelBoost);
                menu.menu_with_icon(
                    if cancel {
                        self.strings.cancel_boost.clone()
                    } else {
                        self.strings.boost_download.clone()
                    },
                    if cancel {
                        IconName::StarOff
                    } else {
                        IconName::Star
                    },
                    Box::new(crate::actions::ToggleBoostSelected),
                )
            }
            MenuEntry::OpenFile => menu.menu_with_icon(
                self.strings.open_file.clone(),
                IconName::File,
                Box::new(crate::actions::OpenSelected),
            ),
            MenuEntry::OpenFolder => menu.menu_with_icon(
                self.strings.open_folder.clone(),
                IconName::FolderOpen,
                Box::new(crate::actions::RevealSelected),
            ),
            MenuEntry::Rename => menu.menu_with_icon(
                self.strings.rename_task.clone(),
                IconName::Replace,
                Box::new(crate::actions::RenameSelected),
            ),
            MenuEntry::Redownload => menu.menu_with_icon(
                self.strings.redownload_task.clone(),
                IconName::Redo2,
                Box::new(crate::actions::RedownloadSelected),
            ),
            MenuEntry::CopyUrl => menu.menu_with_icon(
                self.strings.copy_url.clone(),
                IconName::Copy,
                Box::new(crate::actions::CopySelectedUrl),
            ),
            MenuEntry::MoveToQueue => self.append_move_to_queue(menu),
            MenuEntry::Separator => menu.separator(),
            MenuEntry::Delete => menu.menu_with_icon(
                self.strings.delete.clone(),
                IconName::Delete,
                Box::new(crate::actions::DeleteSelected),
            ),
            MenuEntry::DeleteWithFiles => menu.menu_with_icon(
                self.strings.delete_task_and_file.clone(),
                IconName::Delete,
                Box::new(crate::actions::DeleteSelectedWithFiles),
            ),
            MenuEntry::OpenInWindow => menu.menu_with_icon(
                self.strings.open_in_window.clone(),
                IconName::Maximize,
                Box::new(crate::actions::OpenSelectedInWindow),
            ),
        }
    }

    /// 「移动到队列」：固定版 gpui-component 的 `PopupMenu::submenu` 需要
    /// `Context<PopupMenu>`，而 `TableDelegate::context_menu` 只提供
    /// `Context<TableState<Self>>`（两者是不同实体的上下文，无法互转），因此
    /// 这里无法构造真正的嵌套子菜单，改为「小节标题 + 缩进平铺项」。
    fn append_move_to_queue(&self, menu: PopupMenu) -> PopupMenu {
        let Some(host) = self.host.clone() else {
            return menu;
        };
        if self.queue_names.is_empty() {
            return menu;
        }
        let mut menu = menu.label(self.strings.move_to_queue.clone());
        for (queue_id, name) in &self.queue_names {
            let host = host.clone();
            let queue_id = queue_id.clone();
            menu = menu.item(
                PopupMenuItem::new(SharedString::from(format!("    {name}"))).on_click(
                    move |_, _, cx| {
                        let queue_id = queue_id.clone();
                        let _ =
                            host.update(cx, |view, cx| view.move_selected_to_queue(queue_id, cx));
                    },
                ),
            );
        }
        menu
    }

    /// 分组头行右键菜单（P1.8）；`group_id` 为空（未分组桶）不显示菜单。
    fn group_context_menu(&self, group_id: &str, menu: PopupMenu) -> PopupMenu {
        if group_id.is_empty() {
            return menu;
        }
        let Some(host) = self.host.as_ref() else {
            return menu;
        };
        let has_failed_member = self
            .store
            .local()
            .iter()
            .any(|task| task.group_id == group_id && task.state == TaskState::Failed);

        let mut menu = menu.item(group_menu_item(
            self.strings.group_pause_all.clone(),
            IconName::Pause,
            host,
            group_id,
            DownloadView::group_pause_all,
        ));
        menu = menu.item(group_menu_item(
            self.strings.group_resume_all.clone(),
            IconName::Play,
            host,
            group_id,
            DownloadView::group_resume_all,
        ));
        if has_failed_member {
            menu = menu.item(group_menu_item(
                self.strings.group_retry_failed.clone(),
                IconName::Redo2,
                host,
                group_id,
                DownloadView::group_retry_failed,
            ));
        }
        menu = menu.item(group_menu_item(
            self.strings.group_open_folder.clone(),
            IconName::FolderOpen,
            host,
            group_id,
            DownloadView::group_open_folder,
        ));
        menu = menu.item(group_menu_item(
            self.strings.group_copy_source_link.clone(),
            IconName::Copy,
            host,
            group_id,
            DownloadView::group_copy_source_link,
        ));
        menu = menu.separator();
        menu = menu.item(group_menu_item(
            self.strings.group_delete.clone(),
            IconName::Delete,
            host,
            group_id,
            DownloadView::group_delete,
        ));
        menu = menu.item(group_menu_item_windowed(
            self.strings.group_delete_with_files.clone(),
            IconName::Delete,
            host,
            group_id,
            DownloadView::confirm_group_delete_with_files,
        ));
        menu.item(group_menu_item_windowed(
            self.strings.open_group_in_window.clone(),
            IconName::Maximize,
            host,
            group_id,
            DownloadView::open_group_in_window,
        ))
    }
}

impl TableDelegate for DownloadTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.shown_columns_count() + 1
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.visible.len()
    }

    /// 空态：图标 + 标题 + 引导（与 Flutter 桌面端同文案）。
    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .child(
                Icon::new(IconName::Inbox)
                    .size(px(44.))
                    .text_color(colors.muted_foreground.opacity(0.35)),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.muted_foreground)
                    .child(self.strings.empty_title.clone()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground.opacity(0.75))
                    .child(self.strings.empty_subtitle.clone()),
            )
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if col_ix == 0 {
            return Column::new("selection", "")
                .width(px(SELECTION_COLUMN_WIDTH))
                .min_width(px(SELECTION_COLUMN_WIDTH))
                .max_width(px(SELECTION_COLUMN_WIDTH))
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false)
                .p_0();
        }

        let Some(column) = self.shown_column(col_ix) else {
            return Column::new("missing", "").resizable(false).movable(false);
        };
        let base = Column::new(column.kind.key(), column.kind.label(&self.strings))
            .width(px(column.width))
            .min_width(px(column.kind.min_width()))
            .max_width(px(480.));
        match column.kind.sort_key() {
            Some(key) if key == self.prefs.sort_key => base.sort(match self.prefs.sort_dir {
                SortDir::Asc => ColumnSort::Ascending,
                SortDir::Desc => ColumnSort::Descending,
            }),
            Some(_) => base.sortable(),
            None => base,
        }
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if col_ix == 0 {
            let keys = self.visible_task_keys();
            let all_tasks_selected =
                !keys.is_empty() && keys.iter().all(|key| self.selected_tasks.contains(key));
            return h_flex()
                .size_full()
                .justify_center()
                .relative()
                .left(px(4.0))
                .top(px(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Checkbox::new("select-all-download-tasks")
                        .with_size(Size::XSmall)
                        .checked(all_tasks_selected)
                        .focus_ring(false)
                        .on_click(cx.listener(|table, checked: &bool, _, cx| {
                            let delegate = table.delegate_mut();
                            if *checked {
                                delegate.select_all_tasks();
                            } else {
                                delegate.clear_selection();
                            }
                            cx.notify();
                        })),
                );
        }

        let tokens = active_theme(cx).tokens();
        let label = self
            .shown_column(col_ix)
            .map(|column| column.kind.label(&self.strings))
            .unwrap_or_default();
        h_flex()
            .size_full()
            .items_center()
            .relative()
            .top(px(1.))
            // 表头比正文小半档并加中等字重：与 Flutter 桌面端表头层级一致。
            .text_size(px(12.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(tokens.colors.muted_foreground)
            .child(label)
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let Some(VisibleRow::Task(id)) = self.visible.get(row_ix).cloned() else {
            return div().id(("download-group-row", row_ix));
        };
        let Some(key) = self.store.row(id).map(|row| row.key.clone()) else {
            return div().id(("download-task-row", row_ix));
        };
        let selected = self.selected_tasks.contains(&key);
        let selected_background = active_theme(cx).tokens().colors.accent;

        div()
            .id(("download-task-row", row_ix))
            .relative()
            .when(selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(active_theme(cx).tokens().radius.sm)
                        .bg(selected_background),
                )
            })
            .on_click(cx.listener(move |table, event: &ClickEvent, _, cx| {
                table
                    .delegate_mut()
                    .select_task(key.clone(), event.modifiers());
                cx.notify();
            }))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let id = match self.visible.get(row_ix) {
            Some(VisibleRow::Task(id)) => *id,
            Some(VisibleRow::GroupHeader {
                key,
                label,
                count,
                collapsed,
            }) => {
                if col_ix == 1 {
                    let (key, label, count, collapsed) =
                        (key.clone(), label.clone(), *count, *collapsed);
                    return self.render_group_header(row_ix, &key, &label, count, collapsed, cx);
                }
                return div().into_any_element();
            }
            None => return div().into_any_element(),
        };
        let Some(task) = self.store.row(id) else {
            return div().into_any_element();
        };
        let task: &DownloadTaskView = &task;
        let tokens = active_theme(cx).tokens();

        if col_ix == 0 {
            let key = task.key.clone();
            let selected = self.selected_tasks.contains(&key);
            return h_flex()
                .size_full()
                .justify_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Checkbox::new(("download-task-multi-select", row_ix))
                        .with_size(Size::XSmall)
                        .checked(selected)
                        .focus_ring(false)
                        .on_click(cx.listener(move |table, checked: &bool, _, cx| {
                            let delegate = table.delegate_mut();
                            if *checked {
                                delegate.selected_tasks.insert(key.clone());
                            } else {
                                delegate.selected_tasks.remove(&key);
                            }
                            delegate.selection_anchor = Some(key.clone());
                            cx.notify();
                        })),
                )
                .into_any_element();
        }

        let Some(kind) = self.shown_column(col_ix).map(|column| column.kind) else {
            return div().into_any_element();
        };
        match kind {
            DownloadColumnKind::FileName => self.render_file_cell(task, cx),
            DownloadColumnKind::Progress => self.render_progress_cell(task, cx),
            DownloadColumnKind::Size => self.text_cell(task.size.clone(), cx),
            DownloadColumnKind::Status => {
                let (status, status_color, _, _, _) = self.task_visuals(task, cx);
                h_flex()
                    .size_full()
                    .items_center()
                    .text_size(tokens.typography.xs.size)
                    .text_color(status_color)
                    .child(status)
                    .into_any_element()
            }
            DownloadColumnKind::Speed => {
                let speed = task
                    .speed_bytes_per_second
                    .filter(|speed| *speed > 0)
                    .map(|speed| format!("{}/s", format_bytes(speed)))
                    .unwrap_or_else(|| "—".to_owned());
                self.text_cell(speed, cx)
            }
            DownloadColumnKind::Eta => {
                let eta = task
                    .eta_seconds
                    .filter(|seconds| *seconds <= 86_400)
                    .map_or_else(
                        || SharedString::from("—"),
                        |seconds| self.strings.format_eta(seconds),
                    );
                self.text_cell(eta, cx)
            }
            DownloadColumnKind::Created => {
                self.text_cell(self.strings.format_created(task.created_at_secs), cx)
            }
            DownloadColumnKind::Protocol => self.text_cell(task.protocol.label(), cx),
            DownloadColumnKind::Source => self.text_cell(task.source_site().to_owned(), cx),
            DownloadColumnKind::Queue => {
                let name = self
                    .queue_names
                    .iter()
                    .find(|(id, _)| *id == task.queue_id)
                    .map_or_else(|| task.queue_id.clone(), |(_, name)| name.clone());
                self.text_cell(name, cx)
            }
        }
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        self.move_shown_column(col_ix, to_ix);
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        let Some(kind) = self.shown_column(col_ix).map(|column| column.kind) else {
            return;
        };
        let Some(key) = kind.sort_key() else {
            return;
        };
        match sort {
            ColumnSort::Default => {
                self.prefs.sort_key = ViewSortKey::Smart;
            }
            ColumnSort::Ascending => {
                self.prefs.sort_key = key;
                self.prefs.sort_dir = SortDir::Asc;
            }
            ColumnSort::Descending => {
                self.prefs.sort_key = key;
                self.prefs.sort_dir = SortDir::Desc;
            }
        }
        self.sort_changed = true;
        self.view_dirty = true;
        self.refresh_view();
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let menu = match self.action_context.clone() {
            Some(handle) => menu.action_context(handle),
            None => menu,
        };
        match self.visible.get(row_ix).cloned() {
            Some(VisibleRow::GroupHeader { key, .. }) => self.group_context_menu(&key, menu),
            Some(VisibleRow::Task(_)) => self.task_context_menu(menu),
            None => menu,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ToolbarCommand {
    Resume,
    Pause,
    PauseAll,
    ResumeAll,
    Delete,
    Open,
    Reveal,
}

impl DownloadView {
    #[allow(
        clippy::too_many_arguments,
        reason = "toolbar helper keeps per-button visual and command inputs explicit"
    )]
    fn toolbar_icon_action(
        &self,
        tooltip_id: &'static str,
        button_id: &'static str,
        label: SharedString,
        icon: IconName,
        destructive: bool,
        action: ToolbarCommand,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tooltip_label = label.clone();
        div()
            .id(tooltip_id)
            .size(px(30.))
            .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
            .child(
                toolbar_action_button(
                    button_id,
                    label,
                    Icon::new(icon).size(px(15.)),
                    destructive,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.execute_toolbar(action, cx);
                })),
            )
    }

    /// 选中集合 → 命令列表（远程任务走 `agent.remote.command`）。
    pub(crate) fn commands_for_selection(
        &self,
        action: ToolbarCommand,
        cx: &Context<Self>,
    ) -> Vec<DownloadsCommand> {
        let selected = self.table_state.read(cx).delegate().selected_keys();
        match action {
            ToolbarCommand::PauseAll => vec![DownloadsCommand::PauseAll],
            ToolbarCommand::ResumeAll => vec![DownloadsCommand::ResumeAll],
            ToolbarCommand::Open | ToolbarCommand::Reveal => selected
                .into_iter()
                .filter(RowKey::is_local)
                .map(|key| {
                    let task_id = key.task_id().to_owned();
                    match action {
                        ToolbarCommand::Open => DownloadsCommand::OpenTask { task_id },
                        _ => DownloadsCommand::RevealTask { task_id },
                    }
                })
                .collect(),
            ToolbarCommand::Resume | ToolbarCommand::Pause | ToolbarCommand::Delete => selected
                .into_iter()
                .map(|key| {
                    let task_id = key.task_id().to_owned();
                    match (key.is_local(), action) {
                        (false, _) => DownloadsCommand::RemoteCommand(serde_json::json!({
                            "taskId": task_id,
                            "action": match action {
                                ToolbarCommand::Resume => "resume",
                                ToolbarCommand::Pause => "pause",
                                _ => "delete",
                            }
                        })),
                        (true, ToolbarCommand::Resume) => DownloadsCommand::Resume { task_id },
                        (true, ToolbarCommand::Pause) => DownloadsCommand::Pause { task_id },
                        (true, _) => DownloadsCommand::Delete {
                            task_id,
                            delete_files: false,
                        },
                    }
                })
                .collect(),
        }
    }

    pub(crate) fn execute_toolbar(&mut self, action: ToolbarCommand, cx: &mut Context<Self>) {
        let commands = self.commands_for_selection(action, cx);
        self.execute_commands(commands, cx);
    }

    /// 逐条执行；任一失败在页面横幅提示。
    pub(crate) fn execute_commands(
        &mut self,
        commands: Vec<DownloadsCommand>,
        cx: &mut Context<Self>,
    ) {
        for command in commands {
            let future = self.controller.execute(command);
            cx.spawn(async move |this, cx| {
                let failed = future.await.is_err();
                let _ = this.update(cx, |this, cx| {
                    this.last_error = failed.then(|| this.strings.action_failed.clone());
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn render_columns_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let table_state = self.table_state.clone();
        let menu_title = self.strings.view_columns_menu_title.clone();
        let reset_label = self.strings.view_columns_reset_action.clone();
        let at_least_one_label = self.strings.view_columns_at_least_one.clone();
        let trigger_label = menu_title.clone();
        let on_columns_changed = self.persist_prefs_handle(cx);

        Popover::new("download-columns-popover")
            .p_0()
            .w(px(230.))
            .trigger(
                Button::new("download-columns")
                    .ghost()
                    .xsmall()
                    .compact()
                    .icon(IconName::Settings2)
                    .tooltip(trigger_label),
            )
            .content(move |_, _, cx| {
                let tokens = active_theme(cx).tokens().clone();
                let columns = table_state.read(cx).delegate().columns.clone();
                let visible_count = columns.iter().filter(|column| column.visible).count();
                let table_for_reset = table_state.clone();
                let persist_reset = on_columns_changed.clone();

                v_flex()
                    .w_full()
                    .py(tokens.spacing.sm)
                    .child(
                        h_flex()
                            .px(tokens.spacing.md)
                            .pb(tokens.spacing.sm)
                            .gap(tokens.spacing.sm)
                            .text_size(tokens.typography.xs.size)
                            .font_weight(tokens.typography.sm.weight)
                            .child(Icon::new(IconName::Settings2).size(px(13.)))
                            .child(menu_title.clone()),
                    )
                    .children(columns.into_iter().map(|column| {
                        let kind = column.kind;
                        let label = kind.label(&table_state.read(cx).delegate().strings);
                        let drag = DraggedColumnMenuItem {
                            kind,
                            label: label.clone(),
                        };
                        let table_for_checkbox = table_state.clone();
                        let table_for_drop = table_state.clone();
                        let persist_checkbox = on_columns_changed.clone();
                        let persist_drop = on_columns_changed.clone();
                        h_flex()
                            .id(format!("download-column-menu-item-{}", kind.key()))
                            .h(px(32.))
                            .px(tokens.spacing.md)
                            .gap(tokens.spacing.sm)
                            .cursor_pointer()
                            .hover(|style| style.bg(tokens.colors.muted.opacity(0.55)))
                            .drag_over::<DraggedColumnMenuItem>(move |style, _, _, _| {
                                style.bg(tokens.colors.accent)
                            })
                            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                            .on_drop(move |drag: &DraggedColumnMenuItem, _, cx| {
                                table_for_drop.update(cx, |table, cx| {
                                    table.delegate_mut().move_column_kind(drag.kind, kind);
                                    table.refresh(cx);
                                    cx.notify();
                                });
                                persist_drop(cx);
                            })
                            .child(
                                Icon::new(IconName::Menu)
                                    .size(px(13.))
                                    .text_color(tokens.colors.muted_foreground),
                            )
                            .child(
                                Checkbox::new(format!("download-column-visible-{}", kind.key()))
                                    .with_size(Size::XSmall)
                                    .checked(column.visible)
                                    .focus_ring(false)
                                    .on_click(move |checked: &bool, _, cx| {
                                        let changed = table_for_checkbox.update(cx, |table, cx| {
                                            let changed = table
                                                .delegate_mut()
                                                .set_column_visible(kind, *checked);
                                            if changed {
                                                table.refresh(cx);
                                                cx.notify();
                                            }
                                            changed
                                        });
                                        if changed {
                                            persist_checkbox(cx);
                                        }
                                    }),
                            )
                            .child(div().min_w_0().flex_1().truncate().child(label))
                    }))
                    .when(visible_count == 1, |this| {
                        this.child(
                            div()
                                .px(tokens.spacing.md)
                                .py(tokens.spacing.xs)
                                .text_size(px(10.))
                                .text_color(tokens.colors.muted_foreground)
                                .child(at_least_one_label.clone()),
                        )
                    })
                    .child(
                        div()
                            .h(px(1.))
                            .my(tokens.spacing.xs)
                            .bg(tokens.colors.border),
                    )
                    .child(
                        Button::new("download-columns-reset")
                            .ghost()
                            .w_full()
                            .justify_start()
                            .label(reset_label.clone())
                            .on_click(move |_, _, cx| {
                                table_for_reset.update(cx, |table, cx| {
                                    table.delegate_mut().reset_columns();
                                    table.refresh(cx);
                                    cx.notify();
                                });
                                persist_reset(cx);
                            }),
                    )
            })
    }

    pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let separator = || {
            div()
                .h(px(24.))
                .w(px(1.))
                .mx(tokens.spacing.xs)
                .bg(tokens.colors.border)
        };

        h_flex()
            .h(px(40.))
            .w_full()
            .flex_none()
            .items_center()
            .px(px(4.))
            .mb(px(4.))
            .gap(tokens.spacing.xxs)
            .child(
                Button::new("download-create")
                    .primary()
                    .small()
                    .h(CONTROL_HEIGHT)
                    .icon(IconName::Plus)
                    .label(self.strings.new_download.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_new_download(window, cx);
                    })),
            )
            .child(separator())
            .child(self.toolbar_icon_action(
                "download-resume-tooltip",
                "download-resume",
                self.strings.resume.clone(),
                IconName::Play,
                false,
                ToolbarCommand::Resume,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-pause-tooltip",
                "download-pause",
                self.strings.pause.clone(),
                IconName::Pause,
                false,
                ToolbarCommand::Pause,
                cx,
            ))
            .child(separator())
            .child(self.toolbar_icon_action(
                "download-stop-all-tooltip",
                "download-stop-all",
                self.strings.stop_all.clone(),
                IconName::CircleX,
                false,
                ToolbarCommand::PauseAll,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-resume-all-tooltip",
                "download-resume-all",
                self.strings.resume_all.clone(),
                IconName::Play,
                false,
                ToolbarCommand::ResumeAll,
                cx,
            ))
            .child(separator())
            .child(self.toolbar_icon_action(
                "download-delete-tooltip",
                "download-delete",
                self.strings.delete.clone(),
                IconName::Delete,
                true,
                ToolbarCommand::Delete,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-open-tooltip",
                "download-open",
                self.strings.open_file.clone(),
                IconName::File,
                false,
                ToolbarCommand::Open,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-reveal-tooltip",
                "download-reveal",
                self.strings.open_folder.clone(),
                IconName::FolderOpen,
                false,
                ToolbarCommand::Reveal,
                cx,
            ))
            .child(div().flex_1())
            .children(self.render_toolbar_trailing(cx))
            .child(self.render_columns_menu(cx))
    }

    pub(crate) fn render_table(
        &self,
        available_width: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row_height = self
            .table_state
            .read(cx)
            .delegate()
            .prefs
            .density
            .row_height();
        let table_state = self.table_state.clone();
        self.table_state.update(cx, |table, cx| {
            if table.delegate_mut().fit_columns_to_width(available_width) {
                table.refresh(cx);
            }
        });
        div()
            .id("download-task-table-container")
            .relative()
            .mx(px(4.))
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top(px(-TABLE_HEADER_CROP))
                    .right_0()
                    .bottom_0()
                    .left_0()
                    .child(
                        DataTable::new(&table_state)
                            .with_size(Size::Size(px(row_height)))
                            .stripe(false)
                            .bordered(false)
                            .scrollbar_visible(true, true),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, rc::Rc};

    use fluxdown_ui_i18n::{I18nCatalog, I18nError};
    use gpui::Modifiers;

    use super::{DownloadColumnKind, DownloadTableDelegate, VisibleRow};
    use crate::{
        model::{
            DownloadTaskView, RowKey, TaskStore,
            view_prefs::{ViewGroupBy, ViewPrefs},
        },
        strings::DownloadStrings,
    };

    fn delegate(statuses: &[i32]) -> Result<DownloadTableDelegate, I18nError> {
        let catalog = std::sync::Arc::new(I18nCatalog::load_embedded()?);
        let strings = DownloadStrings::from_translator(&catalog.translator("en"));
        let store = Rc::new(TaskStore::default());
        let rows = statuses
            .iter()
            .enumerate()
            .map(|(index, status)| {
                let task =
                    serde_json::from_value::<fluxdown_protocol::TaskDto>(serde_json::json!({
                        "taskId": format!("t{index}"),
                        "url": "https://example.com/file",
                        "fileName": format!("f{index}.bin"),
                        "saveDir": "/tmp",
                        "status": status,
                        "downloadedBytes": 0,
                        "totalBytes": 1,
                        "errorMessage": "",
                        "createdAt": format!("{}", 100 - index),
                        "proxyUrl": "",
                        "queueId": "main",
                        "checksum": ""
                    }))
                    .expect("task");
                DownloadTaskView::local(&task, None, false)
            })
            .collect();
        store.replace_local(rows);
        let mut delegate = DownloadTableDelegate::new(strings, store);
        delegate.refresh_view();
        Ok(delegate)
    }

    #[test]
    fn context_menu_selection_preserves_selected_rows_and_replaces_unselected_rows()
    -> Result<(), I18nError> {
        let mut delegate = delegate(&[2, 2, 2])?;
        delegate
            .selected_tasks
            .extend([RowKey::Local("t0".into()), RowKey::Local("t1".into())]);

        delegate.select_task_for_context_menu(RowKey::Local("t1".into()));
        assert_eq!(
            delegate.selected_tasks,
            HashSet::from([RowKey::Local("t0".into()), RowKey::Local("t1".into())])
        );

        delegate.select_task_for_context_menu(RowKey::Local("t2".into()));
        assert_eq!(
            delegate.selected_tasks,
            HashSet::from([RowKey::Local("t2".into())])
        );
        Ok(())
    }

    #[test]
    fn grouping_inserts_headers_and_collapsing_hides_members() -> Result<(), I18nError> {
        let mut delegate = delegate(&[1, 3, 1, 3])?;
        let mut prefs = ViewPrefs::default();
        prefs.group_by = ViewGroupBy::Status;
        delegate.set_prefs(prefs);
        delegate.refresh_view();
        assert_eq!(delegate.visible.len(), 6);
        assert!(matches!(
            delegate.visible[0],
            VisibleRow::GroupHeader {
                count: 2,
                collapsed: false,
                ..
            }
        ));

        let key = delegate
            .group_key_at(0)
            .map(str::to_owned)
            .expect("group key");
        delegate.toggle_group_collapsed(&key);
        delegate.refresh_view();
        assert_eq!(delegate.visible.len(), 4);
        assert!(matches!(
            delegate.visible[0],
            VisibleRow::GroupHeader {
                collapsed: true,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn shift_range_selection_skips_group_headers() -> Result<(), I18nError> {
        let mut delegate = delegate(&[1, 3, 1, 3])?;
        let mut prefs = ViewPrefs::default();
        prefs.group_by = ViewGroupBy::Status;
        delegate.set_prefs(prefs);
        delegate.refresh_view();
        let first = delegate.row_key_at(1).expect("first task");
        let last = delegate.row_key_at(5).expect("last task");
        delegate.select_task(first, Modifiers::default());
        delegate.select_task(
            last,
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
        assert_eq!(delegate.selected_tasks.len(), 4);
        Ok(())
    }

    #[test]
    fn fit_columns_drops_lowest_priority_first() -> Result<(), I18nError> {
        let mut delegate = delegate(&[1])?;
        for column in &mut delegate.columns {
            column.visible = true;
        }
        delegate.fit_columns_to_width(750.);
        let hidden: std::collections::HashSet<&str> = delegate
            .columns
            .iter()
            .filter(|column| column.auto_hidden)
            .map(|column| column.kind.key())
            .collect();
        assert_eq!(
            hidden,
            std::collections::HashSet::from(["created", "source", "protocol", "queue", "eta"])
        );
        assert!(
            delegate
                .columns
                .iter()
                .any(|column| column.kind == DownloadColumnKind::Speed && column.shown())
        );
        assert!(
            delegate
                .columns
                .iter()
                .any(|column| column.kind == DownloadColumnKind::FileName && column.shown())
        );
        delegate.fit_columns_to_width(10_000.);
        assert!(delegate.columns.iter().all(|column| !column.auto_hidden));
        Ok(())
    }
}

#[cfg(test)]
mod context_menu_tests {
    use super::{MenuEntry, PLUGIN_ERROR_PREFIX, TaskMenuFacts, context_menu_items};
    use crate::model::TaskState;

    fn task(is_local: bool, state: TaskState) -> TaskMenuFacts {
        TaskMenuFacts {
            is_local,
            state,
            boosted: false,
            is_torrent_sentinel: false,
            is_plugin_retry_error: false,
        }
    }

    #[test]
    fn empty_selection_has_no_menu_items() {
        assert!(context_menu_items(&[]).is_empty());
    }

    #[test]
    fn single_local_completed_task_shows_full_menu() {
        let items = context_menu_items(&[task(true, TaskState::Completed)]);
        assert_eq!(
            items,
            vec![
                MenuEntry::Boost,
                MenuEntry::OpenFile,
                MenuEntry::OpenFolder,
                MenuEntry::Rename,
                MenuEntry::Redownload,
                MenuEntry::CopyUrl,
                MenuEntry::MoveToQueue,
                MenuEntry::Separator,
                MenuEntry::Delete,
                MenuEntry::DeleteWithFiles,
                MenuEntry::OpenInWindow,
            ]
        );
    }

    #[test]
    fn multi_select_only_keeps_items_valid_for_every_selected_task() {
        // 一个本地已完成 + 一个远程下载中：单选专属项（重命名/加速/忽略插件重试）
        // 消失；要求全体本地的项（打开文件/目录/移动队列/重下载）因远程任务
        // 不满足而消失；resume 因两者都不满足「非完成非下载中」而消失；pause
        // 因远程任务处于下载中而显示（存在语义）；复制链接/删除类始终显示；
        // 「独立窗口打开」因存在本地任务而显示。
        let selection = [
            task(true, TaskState::Completed),
            task(false, TaskState::Downloading),
        ];
        let items = context_menu_items(&selection);
        assert_eq!(
            items,
            vec![
                MenuEntry::Pause,
                MenuEntry::CopyUrl,
                MenuEntry::Separator,
                MenuEntry::Delete,
                MenuEntry::DeleteWithFiles,
                MenuEntry::OpenInWindow,
            ]
        );
    }

    #[test]
    fn multi_select_paused_and_failed_shows_resume_but_not_pause() {
        let selection = [task(true, TaskState::Paused), task(true, TaskState::Failed)];
        let items = context_menu_items(&selection);
        assert!(items.contains(&MenuEntry::Resume));
        assert!(!items.contains(&MenuEntry::Pause));
    }

    #[test]
    fn ignore_plugin_retry_only_for_single_failed_plugin_task() {
        let mut plugin_failure = task(true, TaskState::Failed);
        plugin_failure.is_plugin_retry_error = true;
        assert!(context_menu_items(&[plugin_failure]).contains(&MenuEntry::IgnorePluginRetry));

        // 双选即使都满足条件也不显示（单选专属）。
        assert!(
            !context_menu_items(&[plugin_failure, plugin_failure])
                .contains(&MenuEntry::IgnorePluginRetry)
        );

        // 失败但错误消息不带插件前缀不显示。
        let plain_failure = task(true, TaskState::Failed);
        assert!(!context_menu_items(&[plain_failure]).contains(&MenuEntry::IgnorePluginRetry));
        assert_eq!(PLUGIN_ERROR_PREFIX, "[插件]");
    }

    #[test]
    fn boost_toggles_label_by_boosted_state() {
        let mut boosted = task(true, TaskState::Downloading);
        boosted.boosted = true;
        assert!(context_menu_items(&[boosted]).contains(&MenuEntry::CancelBoost));

        let not_boosted = task(true, TaskState::Downloading);
        assert!(context_menu_items(&[not_boosted]).contains(&MenuEntry::Boost));
    }

    #[test]
    fn torrent_sentinel_and_remote_tasks_cannot_redownload() {
        let mut sentinel = task(true, TaskState::Completed);
        sentinel.is_torrent_sentinel = true;
        assert!(!context_menu_items(&[sentinel]).contains(&MenuEntry::Redownload));

        let remote = task(false, TaskState::Completed);
        assert!(!context_menu_items(&[remote]).contains(&MenuEntry::Redownload));
    }
}
