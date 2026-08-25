#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownloadStatusFilter {
    All,
    Completed,
    Incomplete,
}

impl DownloadStatusFilter {
    pub(crate) fn exclusive_toggle(current: Option<Self>, clicked: Self) -> Option<Self> {
        if current == Some(clicked) {
            None
        } else {
            Some(clicked)
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::All => 0,
            Self::Completed => 1,
            Self::Incomplete => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StatusFolderMotion {
    from: [f32; 3],
    to: [f32; 3],
}

impl StatusFolderMotion {
    pub(crate) fn settled(open: Option<DownloadStatusFilter>) -> Self {
        let mut amounts = [0.; 3];
        if let Some(status) = open {
            amounts[status.slot()] = 1.;
        }
        Self {
            from: amounts,
            to: amounts,
        }
    }

    pub(crate) fn retarget(self, progress: f32, next: Option<DownloadStatusFilter>) -> Self {
        let progress = progress.clamp(0., 1.);
        let mut from = [0.; 3];
        for (slot, amount) in from.iter_mut().enumerate() {
            *amount = self.from[slot] + (self.to[slot] - self.from[slot]) * progress;
        }
        let mut to = [0.; 3];
        if let Some(status) = next {
            to[status.slot()] = 1.;
        }
        Self { from, to }
    }

    pub(crate) fn amount(self, status: DownloadStatusFilter, progress: f32) -> f32 {
        let progress = progress.clamp(0., 1.);
        let slot = status.slot();
        self.from[slot] + (self.to[slot] - self.from[slot]) * progress
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownloadCategory {
    All,
    Video,
    Audio,
    Document,
    Image,
    Program,
    Archive,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DownloadFilter {
    pub(crate) status: DownloadStatusFilter,
    pub(crate) category: DownloadCategory,
}

impl DownloadFilter {
    pub(crate) const ALL: Self = Self {
        status: DownloadStatusFilter::All,
        category: DownloadCategory::All,
    };

    pub(crate) const fn new(status: DownloadStatusFilter, category: DownloadCategory) -> Self {
        Self { status, category }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SidebarSection {
    Queues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarSelection {
    Download(DownloadFilter),
    MainQueue,
    LaterQueue,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TaskState {
    Completed,
    Paused,
}

#[derive(Clone, Copy)]
pub(crate) enum TaskKind {
    Application,
    DiskImage,
    Mobile,
}

#[derive(Clone, Copy)]
pub(crate) struct TaskPreview {
    pub(crate) id: usize,
    pub(crate) name: &'static str,
    pub(crate) size: &'static str,
    pub(crate) size_bytes: u64,
    pub(crate) speed_bytes_per_second: Option<u64>,
    pub(crate) eta_seconds: Option<u64>,
    pub(crate) created_order: u32,
    pub(crate) kind: TaskKind,
    pub(crate) progress: f32,
    pub(crate) progress_label: &'static str,
    pub(crate) state: TaskState,
}

impl DownloadFilter {
    pub(crate) fn matches(self, task: &TaskPreview) -> bool {
        let status_matches = match self.status {
            DownloadStatusFilter::All => true,
            DownloadStatusFilter::Completed => task.state == TaskState::Completed,
            DownloadStatusFilter::Incomplete => task.state != TaskState::Completed,
        };
        let category_matches = match self.category {
            DownloadCategory::All => true,
            DownloadCategory::Program => {
                matches!(task.kind, TaskKind::Application | TaskKind::Mobile)
            }
            DownloadCategory::Archive => matches!(task.kind, TaskKind::DiskImage),
            DownloadCategory::Video
            | DownloadCategory::Audio
            | DownloadCategory::Document
            | DownloadCategory::Image
            | DownloadCategory::Other => false,
        };
        status_matches && category_matches
    }
}

pub(crate) fn preview_tasks() -> Vec<TaskPreview> {
    vec![
        TaskPreview {
            id: 0,
            name: "rufus-4.15.exe",
            size: "1.9 MB",
            size_bytes: 1_900_000,
            speed_bytes_per_second: None,
            eta_seconds: None,
            created_order: 0,
            kind: TaskKind::Application,
            progress: 1.,
            progress_label: "100.0%",
            state: TaskState::Completed,
        },
        TaskPreview {
            id: 1,
            name: "cachyos-desktop-linux-260809.iso",
            size: "3.0 GB",
            size_bytes: 3_000_000_000,
            speed_bytes_per_second: None,
            eta_seconds: None,
            created_order: 1,
            progress: 1.,
            kind: TaskKind::DiskImage,
            progress_label: "100.0%",
            state: TaskState::Completed,
        },
        TaskPreview {
            id: 2,
            name: "Gopeed-v1.9.3-android-x86_64.apk",
            size: "25.4 MB",
            size_bytes: 25_400_000,
            speed_bytes_per_second: None,
            eta_seconds: None,
            created_order: 2,
            progress: 1.,
            progress_label: "100.0%",
            kind: TaskKind::Mobile,
            state: TaskState::Completed,
        },
        TaskPreview {
            id: 3,
            name: "Gopeed-v1.9.3-macos-amd64.dmg",
            size: "39.0 MB",
            size_bytes: 39_000_000,
            speed_bytes_per_second: None,
            eta_seconds: None,
            created_order: 3,
            progress: 0.666,
            progress_label: "66.6%",
            state: TaskState::Paused,
            kind: TaskKind::DiskImage,
        },
        TaskPreview {
            id: 4,
            name: "Gopeed-v1.9.3-windows-amd64.exe",
            size: "25.2 MB",
            size_bytes: 25_200_000,
            speed_bytes_per_second: None,
            eta_seconds: None,
            created_order: 4,
            progress: 0.718,
            progress_label: "71.8%",
            state: TaskState::Paused,
            kind: TaskKind::Application,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadCategory, DownloadFilter, DownloadStatusFilter, StatusFolderMotion, TaskPreview,
        preview_tasks,
    };

    fn matching_ids(filter: DownloadFilter) -> Vec<usize> {
        preview_tasks()
            .iter()
            .filter(|task: &&TaskPreview| filter.matches(task))
            .map(|task| task.id)
            .collect()
    }

    #[test]
    fn download_filter_intersects_status_and_category() {
        assert_eq!(
            matching_ids(DownloadFilter::new(
                DownloadStatusFilter::Completed,
                DownloadCategory::Program,
            )),
            [0, 2],
        );
        assert_eq!(
            matching_ids(DownloadFilter::new(
                DownloadStatusFilter::Incomplete,
                DownloadCategory::Archive,
            )),
            [3],
        );
    }

    #[test]
    fn toggling_status_folder_keeps_only_one_open() {
        assert_eq!(
            DownloadStatusFilter::exclusive_toggle(None, DownloadStatusFilter::All),
            Some(DownloadStatusFilter::All),
        );
        assert_eq!(
            DownloadStatusFilter::exclusive_toggle(
                Some(DownloadStatusFilter::All),
                DownloadStatusFilter::Completed,
            ),
            Some(DownloadStatusFilter::Completed),
        );
        assert_eq!(
            DownloadStatusFilter::exclusive_toggle(
                Some(DownloadStatusFilter::Completed),
                DownloadStatusFilter::Completed,
            ),
            None,
        );
    }

    #[test]
    fn switching_status_folders_keeps_total_open_amount() {
        let motion = StatusFolderMotion::settled(Some(DownloadStatusFilter::All))
            .retarget(1., Some(DownloadStatusFilter::Completed));
        let halfway = 0.5;
        let all = motion.amount(DownloadStatusFilter::All, halfway);
        let completed = motion.amount(DownloadStatusFilter::Completed, halfway);
        let incomplete = motion.amount(DownloadStatusFilter::Incomplete, halfway);
        assert!((all - 0.5).abs() < f32::EPSILON);
        assert!((completed - 0.5).abs() < f32::EPSILON);
        assert!((incomplete).abs() < f32::EPSILON);
        assert!((all + completed + incomplete - 1.).abs() < f32::EPSILON);
    }
}
