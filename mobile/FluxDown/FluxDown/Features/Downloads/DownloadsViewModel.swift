import FluxDomain
import Foundation
import Observation

/// 与列表派生同步发布的「壳层」信息：只在值变化时才发布，页面 body 不随 10 Hz 的主机状态重算。
nonisolated struct DownloadsChrome: Equatable {
    nonisolated enum Link: Equatable { case connecting, live, stale, failed }

    var link: Link = .connecting
    var diskFree: UInt64?
    /// 第一个待处理的选择请求（含被用户划走的）：横幅可重新打开。
    var pendingSelection: PendingSelection?

    /// 断连宽限后只读（stale / failed）；首次连接中不弹横幅但也不可写。
    var isReadOnly: Bool { link != .live }
    var showsOfflineBanner: Bool { link == .stale || link == .failed }
}

nonisolated struct PendingSelection: Equatable {
    let requestId: String
    /// 标题 i18n 键（BT 文件 / HLS 画质 / 插件规格）。
    let titleKey: String
    let taskName: String
    let deadlineUnixMs: Int64
}

/// 多选的批量能力：继续 / 暂停 / 移动 各自的可用性（Resume / Pause 取「存在」）。
nonisolated struct SelectionCaps: Equatable {
    var canResume = false
    var canPause = false
    var canMove = false
    var resumeIds: [String] = []
    var pauseIds: [String] = []
    var moveIds: [String] = []
}

/// 下载页的视图状态：筛选 / 折叠 / 多选 + 派生列表与分面（同 Android `DownloadsView`）。
///
/// 通过 `withObservationTracking` 观察主机状态与排序偏好：任一变化 → 在主 actor 上重新派生并只在结果不同时发布。
/// 进度 / 速度排序键的重排节流（2s）与指针活动静默期（1.5s）由 `DownloadsDeriver` 处理，到期自动补一次派生。
@MainActor
@Observable
final class DownloadsModel {
    private enum Key {
        static let collapsed = "ui.downloads.collapsed_groups"
    }

    var filter = DownloadsFilter()
    /// regular 分栏右栏正在显示的任务组详情（D6）；非 nil 时优先于 `router.selectedTaskId`。
    var detailGroupId: String?
    private(set) var list = DownloadsList.initial
    private(set) var facets = Facets.initial
    private(set) var chrome = DownloadsChrome()
    private(set) var collapsed: Set<String>
    private(set) var selectionCaps = SelectionCaps()

    /// 多选模式；退出时清空选择。
    var isSelecting = false {
        didSet { if !isSelecting { selection = [] } }
    }

    var selection: Set<String> = [] {
        didSet { updateCaps() }
    }

    @ObservationIgnored private var store: HostStore?
    @ObservationIgnored private var prefs: ViewPrefsStore?
    @ObservationIgnored private var deriver = DownloadsDeriver()
    @ObservationIgnored private var lastInteractionMs: Int64 = 0
    @ObservationIgnored private var retryTask: Task<Void, Never>?
    @ObservationIgnored private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        collapsed = Set(defaults.stringArray(forKey: Key.collapsed) ?? [])
    }

    // MARK: 绑定与派生

    /// 绑定数据源并开始观察（幂等）。
    func bind(store: HostStore, prefs: ViewPrefsStore) {
        guard self.store == nil else { return }
        self.store = store
        self.prefs = prefs
        recompute()
        track()
    }

    private func track() {
        guard let store, let prefs else { return }
        withObservationTracking {
            _ = store.state
            _ = prefs.groupBy
            _ = prefs.sortKey
            _ = prefs.ascending
            _ = filter
            _ = collapsed
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.recompute()
                self.track()
            }
        }
    }

    private static func nowMs() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }

    func recompute() {
        guard let store, let prefs else { return }
        let state = store.state

        // 选中的队列被删除 → 回退「全部」（对齐 PC）。
        if let queueId = filter.queueId, !state.queues.isEmpty,
           !state.queues.contains(where: { normalizedQueueId($0.queueId) == queueId }) {
            filter.queueId = nil
        }

        let now = Self.nowMs()
        let result = deriver.derive(
            DeriveInput(
                state: state,
                order: ViewOrder(groupBy: prefs.groupBy, sortKey: prefs.sortKey, ascending: prefs.ascending),
                filter: filter,
                collapsed: collapsed
            ),
            nowMs: now,
            interactionMs: lastInteractionMs
        )
        if result.list != list { list = result.list }
        if result.facets != facets { facets = result.facets }

        let nextChrome = Self.chrome(for: state)
        if nextChrome != chrome { chrome = nextChrome }

        if !selection.isEmpty {
            let kept = selection.intersection(state.tasks.lazy.map(\.taskId))
            if kept != selection { selection = kept } else { updateCaps() }
        }

        retryTask?.cancel()
        retryTask = nil
        if result.retryAtMs > 0 {
            let delay = max(result.retryAtMs - now, 50)
            retryTask = Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(delay))
                guard !Task.isCancelled else { return }
                self?.recompute()
            }
        }
    }

    private static func chrome(for state: HostState) -> DownloadsChrome {
        let link: DownloadsChrome.Link = switch state.connection {
        case .connecting: .connecting
        case .live: .live
        case .stale: .stale
        case .failed: .failed
        }
        var pending: PendingSelection?
        if let request = state.selections.first {
            let titleKey = switch request.kind {
            case .bt: "btFileSelectTitle"
            case .hls: "hlsQualityTitle"
            case .variant: "resolveVariantTitle"
            }
            pending = PendingSelection(
                requestId: request.requestId,
                titleKey: titleKey,
                taskName: state.task(request.taskId)?.fileName ?? "",
                deadlineUnixMs: request.deadlineUnixMs
            )
        }
        return DownloadsChrome(link: link, diskFree: state.stats.diskFreeBytes, pendingSelection: pending)
    }

    /// 手指 / 指针活动：进度 / 速度排序键延后重排（§3.7）。
    func touch() {
        lastInteractionMs = Self.nowMs()
    }

    // MARK: 筛选

    func setFolder(_ folder: StatusFolder) {
        guard filter.folder != folder || filter.categoryId != nil else { return }
        filter.folder = folder
        filter.categoryId = nil
    }

    func setCategory(_ id: String?) {
        filter.categoryId = id
    }

    /// nil 清除队列范围。
    func setQueue(_ id: String?) {
        filter.queueId = id.map(normalizedQueueId)
    }

    func clearFilters() {
        filter = DownloadsFilter()
    }

    func toggleGroup(_ key: String) {
        if collapsed.contains(key) { collapsed.remove(key) } else { collapsed.insert(key) }
        defaults.set(collapsed.sorted(), forKey: Key.collapsed)
    }

    // MARK: 多选

    func beginSelecting(with id: String? = nil) {
        isSelecting = true
        if let id { selection = [id] }
    }

    var allVisibleSelected: Bool {
        !list.visibleIds.isEmpty && list.visibleIds.allSatisfy { selection.contains($0) }
    }

    /// 全选 ↔ 清空（范围 = 可见任务；全选时再点 = 清空）。
    func toggleSelectAll() {
        selection = allVisibleSelected ? [] : Set(list.visibleIds)
    }

    func selectedTasks() -> [DownloadTask] {
        guard let store, !selection.isEmpty else { return [] }
        return store.state.tasks.filter { selection.contains($0.taskId) }
    }

    private func updateCaps() {
        guard let store, !selection.isEmpty else {
            if selectionCaps != SelectionCaps() { selectionCaps = SelectionCaps() }
            return
        }
        var caps = SelectionCaps()
        for task in store.state.tasks where selection.contains(task.taskId) {
            switch task.status {
            case .paused, .failed: caps.resumeIds.append(task.taskId)
            case .downloading, .pending, .preparing: caps.pauseIds.append(task.taskId)
            case .completed, .unknown: break
            }
            if task.status != .completed, task.status != .unknown { caps.moveIds.append(task.taskId) }
        }
        caps.canResume = !caps.resumeIds.isEmpty
        caps.canPause = !caps.pauseIds.isEmpty
        caps.canMove = !caps.moveIds.isEmpty
        if caps != selectionCaps { selectionCaps = caps }
    }
}
