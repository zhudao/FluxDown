import FluxDomain
import FluxUI
import Foundation

// 下载列表的纯派生模型（同 Android `DownloadsModel.kt`）：筛选 → 计数 → 排序 → 分区 / 分组 → 行视图模型。
// 全部是值类型，不依赖 SwiftUI，可直接单测（见 `FluxDownTests/DownloadsDeriveTests.swift`）。

/// 状态文件夹（范围条）；顺序即显示顺序（02-downloads §3.2）。
nonisolated enum StatusFolder: CaseIterable, Hashable {
    case all, active, completed, failed, paused

    func accepts(_ status: TaskStatus) -> Bool {
        switch self {
        case .all: true
        case .active: status == .pending || status == .downloading || status == .preparing
        case .completed: status == .completed
        case .failed: status == .failed
        case .paused: status == .paused
        }
    }
}

/// 列表筛选：状态文件夹 · 分类 · 队列范围 · 搜索词。
nonisolated struct DownloadsFilter: Hashable {
    var folder: StatusFolder = .all
    var categoryId: String?
    /// 已规范化的队列 id（主队列 = `TaskQueue.main`）；nil = 全部队列。
    var queueId: String?
    var query: String = ""
    /// 仅某任务组的成员（组详情页内部使用；下载列表本身不设置）。
    var groupId: String?
}

/// 主队列的任务 `queueId` 可能是 ""（隐式主队列）或 `main`，视为同一个。
func normalizedQueueId(_ id: String) -> String { id.isEmpty ? TaskQueue.main : id }

/// 派生显示状态（§1.3：颜色 + 图形冗余）。
nonisolated enum TaskVisual: Hashable {
    case downloading, queued, pending, preparing, verifying, paused, failed, seeding, missing, completed

    init(_ task: DownloadTask, queuePosition: Int) {
        switch task.status {
        case .downloading: self = .downloading
        case .pending: self = queuePosition > 0 ? .queued : .pending
        case .preparing: self = task.totalBytes > 0 ? .verifying : .preparing
        case .paused: self = .paused
        case .failed: self = .failed
        case .completed:
            if task.fileMissing {
                self = .missing
            } else if task.seedingStatus == .seeding {
                self = .seeding
            } else {
                self = .completed
            }
        case .unknown: self = .pending
        }
    }
}

/// 分组 / 排序 / 过滤之外决定列表顺序的偏好（不含密度与卡片字段：它们只影响行渲染）。
nonisolated struct ViewOrder: Hashable {
    var groupBy: GroupBy
    var sortKey: SortKey
    var ascending: Bool
}

/// 行内进度条的输入：有分段 → 按真实字节区间；否则回退总进度（`SegmentMapView` 的两种形态）。
nonisolated struct ProgressBarModel: Equatable {
    var spans: [SegmentSpan]
    var totalBytes: Int64
    /// 总进度；总大小未知 + 下载中 + 无分段 → 不定进度条。
    var progress: Double?
    var tone: SegmentTone
}

/// 一行任务的全部展示输入（派生时算好并按输入复用实例：未变化的任务得到相等的值，行视图据此跳过重算）。
nonisolated struct TaskItem: Identifiable, Equatable {
    let task: DownloadTask
    let speedDown: Int64
    let speedUp: Int64
    /// >0 → 「排队 #n」。
    let queuePosition: Int
    let boosted: Bool
    let category: TaskCategory?
    let queue: TaskQueue?
    /// 来源站点（去 `www.` 与端口）；BT / eD2K 哨兵无 host → 空串。
    let site: String
    let visual: TaskVisual
    let kind: FileKind
    let progressBar: ProgressBarModel?
    let ring: RingGlyph?
    let ringProgress: Double?
    /// 仅下载中且速度 / 总大小已知。
    let eta: Int64?
    let activeTransfers: Int?
    let connectedPeers: Int?

    var id: String { task.taskId }
}

/// 分区 / 分组标题（视图层解析为文案；与 Android `GroupLabel` 同构）。
nonisolated enum SectionTitle: Equatable {
    case inFlight
    /// 选中 已完成 / 失败 / 已暂停 文件夹时换成该文件夹名。
    case history(StatusFolder?)
    /// i18n 键（状态 / 日期 / 「其他」/「未分组」…）。
    case key(String)
    case text(String)
    case category(TaskCategory)
    case queue(TaskQueue)
}

nonisolated struct DoneOfTotal: Equatable {
    let done: Int
    let total: Int
}

nonisolated struct DownloadsSection: Identifiable, Equatable {
    nonisolated enum Kind: Equatable {
        /// 「传输中」：右侧实时汇总下行速度 + 任务数。
        case inFlight
        case history
        /// 可折叠的分组。
        case group
    }

    let id: String
    let kind: Kind
    let title: SectionTitle
    /// 该分组 / 分区的成员数（折叠时仍是完整成员数）。
    let count: Int
    /// 仅「任务组」分组：完成数 / 总数。
    let doneOfTotal: DoneOfTotal?
    /// 仅 `.inFlight`：汇总下行速度。
    let downSpeed: Int64
    let collapsed: Bool
    /// 折叠时为空。
    let items: [TaskItem]
}

/// 派生出的列表：分区 + 可见任务 id（全选范围）+ 总任务数（区分「无任务」与「筛选后为空」）。
nonisolated struct DownloadsList: Equatable {
    let sections: [DownloadsSection]
    let visibleIds: [String]
    let taskTotal: Int
    let loaded: Bool

    static let initial = DownloadsList(sections: [], visibleIds: [], taskTotal: 0, loaded: false)

    var isEmpty: Bool { sections.allSatisfy { $0.count == 0 } }
}

nonisolated struct CategoryPill: Equatable, Identifiable {
    let category: TaskCategory
    let count: Int
    var id: String { category.id }
}

nonisolated struct QueueFacet: Equatable, Identifiable {
    let queue: TaskQueue
    let count: Int
    var id: String { queue.queueId }
}

/// 分面：文件夹计数（队列 / 搜索范围内）· 当前文件夹内的分类计数 · 各队列任务数。
nonisolated struct Facets: Equatable {
    /// 按 `StatusFolder.allCases` 顺序。
    let folderCounts: [Int]
    let categories: [CategoryPill]
    let queues: [QueueFacet]
    /// 当前筛选结果行数。
    let matching: Int

    static let initial = Facets(
        folderCounts: Array(repeating: 0, count: StatusFolder.allCases.count),
        categories: [],
        queues: [],
        matching: 0
    )

    func count(_ folder: StatusFolder) -> Int {
        guard let index = StatusFolder.allCases.firstIndex(of: folder), folderCounts.indices.contains(index) else { return 0 }
        return folderCounts[index]
    }
}
