import FluxDomain
import FluxUI
import Foundation

// 下载列表的派生逻辑（同 Android `DownloadsDerive.kt`）：筛选 → 计数 → 排序（含智能档位与进度 / 速度键限频重排）→
// 分区 / 分组 → 行视图模型。`DownloadsDeriver` 持有跨次派生的缓存，只在主 actor 上调用。

struct DeriveInput {
    let state: HostState
    let order: ViewOrder
    let filter: DownloadsFilter
    /// 已折叠的分组键（仅在分组视图生效）。
    let collapsed: Set<String>
}

struct DeriveResult {
    let list: DownloadsList
    let facets: Facets
    /// 重排被延后时，建议的重新派生时刻（Unix 毫秒）；0 = 无需重试。
    let retryAtMs: Int64
}

struct DownloadsDeriver {
    /// 动态排序键（进度 / 速度）两次重排的最短间隔；手指 / 指针活动后的静默期（§3.7 数据绑定）。
    static let reorderMinMs: Int64 = 2_000
    static let reorderIdleMs: Int64 = 1_500

    private struct Entry {
        let task: DownloadTask
        let runtime: TaskRuntime?
        let down: Int64
        let up: Int64
        let queuePosition: Int
        let boosted: Bool
        let category: TaskCategory?
        let queue: TaskQueue?
        let item: TaskItem
    }

    private var entries: [String: Entry] = [:]
    private var categories: [TaskCategory] = []
    private var index = CategoryIndex(TaskCategory.builtin)
    private var categoryByName: [String: TaskCategory?] = [:]
    private var sortSignature = 0
    private var lastReorderMs: Int64 = 0
    private var lastPositions: [String: Int] = [:]

    init() {}

    mutating func derive(
        _ input: DeriveInput,
        nowMs: Int64,
        interactionMs: Int64,
        calendar: Calendar = .current
    ) -> DeriveResult {
        let s = input.state
        let f = input.filter
        let order = input.order

        if categories != s.categories {
            categories = s.categories
            index = CategoryIndex(s.categories)
            categoryByName.removeAll()
            entries.removeAll()
        }
        var queuesById: [String: TaskQueue] = [:]
        queuesById.reserveCapacity(s.queues.count)
        for queue in s.queues { queuesById[normalizedQueueId(queue.queueId)] = queue }

        // 1. 行模型（输入未变化的复用同一值）
        var all: [TaskItem] = []
        all.reserveCapacity(s.tasks.count)
        for task in s.tasks { all.append(item(for: task, state: s, queuesById: queuesById)) }
        if entries.count > all.count {
            let ids = Set(all.map(\.id))
            entries = entries.filter { ids.contains($0.key) }
        }

        // 2. 范围（队列 / 搜索）→ 文件夹计数
        let query = f.query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        var scoped: [TaskItem] = []
        scoped.reserveCapacity(all.count)
        for it in all {
            if let groupId = f.groupId, it.task.groupId != groupId { continue }
            if let queueId = f.queueId, normalizedQueueId(it.task.queueId) != queueId { continue }
            if !query.isEmpty, !Self.matches(it, query: query) { continue }
            scoped.append(it)
        }
        var folderCounts = Array(repeating: 0, count: StatusFolder.allCases.count)
        for it in scoped {
            for (i, folder) in StatusFolder.allCases.enumerated() where folder.accepts(it.task.status) {
                folderCounts[i] += 1
            }
        }

        // 3. 文件夹内 → 分类计数 → 分类筛选
        let inFolder = scoped.filter { f.folder.accepts($0.task.status) }
        var categoryCounts: [String: Int] = [:]
        for it in inFolder {
            if let category = it.category { categoryCounts[category.id, default: 0] += 1 }
        }
        let selectedCategory = f.categoryId.flatMap { id in index.ordered.first { $0.id == id && !$0.isAll }?.id }
        var pills: [CategoryPill] = []
        for category in index.ordered where !category.isAll && category.visible {
            let n = categoryCounts[category.id] ?? 0
            if n > 0 || category.id == selectedCategory { pills.append(CategoryPill(category: category, count: n)) }
        }
        let rows = selectedCategory.map { id in inFolder.filter { $0.category?.id == id } } ?? inFolder

        // 4. 排序（进度 / 速度键节流重排）
        var retryAt: Int64 = 0
        var ordered = rows.sorted { Self.compare($0, $1, order: order) < 0 }
        let dynamic = order.sortKey == .progress || order.sortKey == .speed
        var hasher = Hasher()
        hasher.combine(f)
        hasher.combine(order)
        let signature = hasher.finalize()
        if dynamic, sortSignature == signature, !lastPositions.isEmpty {
            let sinceReorder = nowMs - lastReorderMs
            let sinceTouch = nowMs - interactionMs
            if sinceReorder < Self.reorderMinMs || sinceTouch < Self.reorderIdleMs {
                let positions = lastPositions
                ordered = ordered.enumerated()
                    .sorted { lhs, rhs in
                        let l = positions[lhs.element.id] ?? .max
                        let r = positions[rhs.element.id] ?? .max
                        return l != r ? l < r : lhs.offset < rhs.offset
                    }
                    .map(\.element)
                retryAt = max(lastReorderMs + Self.reorderMinMs, interactionMs + Self.reorderIdleMs)
            } else {
                lastReorderMs = nowMs
            }
        } else {
            lastReorderMs = nowMs
        }
        sortSignature = signature
        var positions: [String: Int] = [:]
        positions.reserveCapacity(ordered.count)
        for (i, it) in ordered.enumerated() { positions[it.id] = i }
        lastPositions = positions

        // 5. 分区 / 分组
        var sections: [DownloadsSection] = []
        var visible: [String] = []
        visible.reserveCapacity(ordered.count)
        if order.groupBy == .none {
            Self.sectioned(ordered, folder: f.folder, into: &sections, visible: &visible)
        } else {
            for group in groups(for: ordered, by: order.groupBy, state: s, now: nowMs, calendar: calendar) {
                let closed = input.collapsed.contains(group.key)
                sections.append(DownloadsSection(
                    id: group.key,
                    kind: .group,
                    title: group.title,
                    count: group.items.count,
                    doneOfTotal: group.doneOfTotal,
                    downSpeed: 0,
                    collapsed: closed,
                    items: closed ? [] : group.items
                ))
                if !closed { visible.append(contentsOf: group.items.map(\.id)) }
            }
        }

        // 6. 队列分面
        var queueCounts: [String: Int] = [:]
        for it in all { queueCounts[normalizedQueueId(it.task.queueId), default: 0] += 1 }
        let queueFacets = s.queues
            .sorted { $0.position < $1.position }
            .map { QueueFacet(queue: $0, count: queueCounts[normalizedQueueId($0.queueId)] ?? 0) }

        return DeriveResult(
            list: DownloadsList(sections: sections, visibleIds: visible, taskTotal: s.tasks.count, loaded: true),
            facets: Facets(folderCounts: folderCounts, categories: pills, queues: queueFacets, matching: ordered.count),
            retryAtMs: retryAt
        )
    }

    // MARK: 行模型

    private mutating func item(for task: DownloadTask, state s: HostState, queuesById: [String: TaskQueue]) -> TaskItem {
        let speed = s.speeds[task.taskId]
        let down = speed?.down ?? 0
        let up = speed?.up ?? 0
        let runtime = s.runtime[task.taskId]
        let queuePosition = Int(s.queuePositions[task.taskId] ?? 0)
        let boosted = s.priorityTaskId == task.taskId
        let category: TaskCategory?
        if let cached = categoryByName[task.fileName] {
            category = cached
        } else {
            category = index.categoryOf(task.fileName)
            categoryByName[task.fileName] = .some(category)
        }
        let queue = queuesById[normalizedQueueId(task.queueId)]

        if let previous = entries[task.taskId],
           previous.task == task, previous.runtime == runtime, previous.down == down, previous.up == up,
           previous.queuePosition == queuePosition, previous.boosted == boosted,
           previous.category == category, previous.queue == queue {
            return previous.item
        }

        let visual = TaskVisual(task, queuePosition: queuePosition)
        var kind = FileKind.from(fileName: task.fileName)
        if kind == .other, task.protocol == .bt { kind = .torrent }
        let built = TaskItem(
            task: task,
            speedDown: down,
            speedUp: up,
            queuePosition: queuePosition,
            boosted: boosted,
            category: category,
            queue: queue,
            site: Self.site(of: task),
            visual: visual,
            kind: kind,
            progressBar: Self.progressBar(task: task, visual: visual, runtime: runtime),
            ring: Self.ring(visual: visual, status: task.status),
            ringProgress: Self.ringProgress(task: task, visual: visual),
            eta: visual == .downloading ? Format.etaSeconds(downloaded: task.downloadedBytes, total: task.totalBytes, speed: down) : nil,
            activeTransfers: runtime?.activeTransfers.map { Int($0) },
            connectedPeers: runtime?.connectedPeers.map { Int($0) }
        )
        entries[task.taskId] = Entry(
            task: task, runtime: runtime, down: down, up: up, queuePosition: queuePosition,
            boosted: boosted, category: category, queue: queue, item: built
        )
        return built
    }

    /// 来源站点：originUrl ‖ url 的 host（去 `www.` 与端口）；BT / eD2K 哨兵无 host → 空串。
    static func site(of task: DownloadTask) -> String {
        let raw = task.originUrl.isEmpty ? task.url : task.originUrl
        if raw.hasPrefix("magnet:") || raw.hasPrefix("torrent-file://") || raw.hasPrefix("ed2k://") { return "" }
        guard let scheme = raw.range(of: "://") else { return "" }
        var rest = raw[scheme.upperBound...].prefix { $0 != "/" }
        rest = rest.prefix { $0 != "?" }
        if let at = rest.lastIndex(of: "@") { rest = rest[rest.index(after: at)...] }
        var host = String(rest.prefix { $0 != ":" })
        if host.hasPrefix("www.") { host.removeFirst(4) }
        return host
    }

    private static func matches(_ item: TaskItem, query: String) -> Bool {
        let task = item.task
        return task.fileName.lowercased().contains(query)
            || task.url.lowercased().contains(query)
            || task.originUrl.lowercased().contains(query)
            || item.site.lowercased().contains(query)
    }

    /// 进度条：按真实字节区间投影；无分段退化为单条总进度。完成 / 做种 / 文件已删除行不显示（§3.5）。
    static func progressBar(task: DownloadTask, visual: TaskVisual, runtime: TaskRuntime?) -> ProgressBarModel? {
        let tone: SegmentTone
        switch visual {
        case .completed, .missing, .seeding:
            return nil
        case .downloading, .preparing, .verifying:
            tone = .downloading
        case .queued, .pending:
            guard task.totalBytes > 0 || task.downloadedBytes > 0 else { return nil }
            tone = .queued
        case .paused:
            guard task.totalBytes > 0 || task.downloadedBytes > 0 else { return nil }
            tone = .paused
        case .failed:
            guard task.totalBytes > 0 || task.downloadedBytes > 0 else { return nil }
            tone = .failed
        }
        // 准备中（总大小未知）= 不定进度条（`SegmentMapView` 在 progress == nil + .downloading 时横扫）。
        let progress: Double? = visual == .preparing ? nil : task.progress
        let usesSegments = task.totalBytes > 0 && (visual == .downloading || visual == .paused || visual == .failed)
        var spans: [SegmentSpan] = []
        if usesSegments, let segments = runtime?.segments, !segments.isEmpty {
            let live = visual == .downloading
            spans = segments.compactMap { segment in
                guard segment.endByte >= segment.startByte else { return nil }
                // 协议里 `endByte` 是闭区间终点；`SegmentSpan.endByte` 不含。
                return SegmentSpan(
                    startByte: segment.startByte,
                    endByte: segment.endByte + 1,
                    downloadedBytes: segment.downloadedBytes,
                    active: live ? segment.active : false
                )
            }
        }
        return ProgressBarModel(spans: spans, totalBytes: spans.isEmpty ? 0 : task.totalBytes, progress: progress, tone: tone)
    }

    /// 行尾圆环字形（§1.3）；未知状态不可控，无圆环。
    static func ring(visual: TaskVisual, status: TaskStatus) -> RingGlyph? {
        guard status != .unknown else { return nil }
        switch visual {
        case .downloading, .pending, .queued: return .pause
        case .preparing, .verifying: return .preparing
        case .paused: return .resume
        case .failed: return .retry
        case .missing: return .redownload
        case .seeding, .completed: return .open
        }
    }

    static func ringProgress(task: DownloadTask, visual: TaskVisual) -> Double? {
        switch visual {
        case .completed, .missing, .seeding, .preparing, .verifying: nil
        default: task.progress
        }
    }

    // MARK: 排序

    /// 智能排序档（§1.2）：优先下载 → 活跃 → 排队 → 失败 → 暂停 → 完成。
    static func tier(_ item: TaskItem) -> Int {
        let status = item.task.status
        if item.boosted, status == .downloading || status == .pending { return 0 }
        if status == .downloading || status == .preparing { return 1 }
        if status == .pending { return 2 }
        if status == .failed { return 3 }
        if status == .paused { return 4 }
        return 5
    }

    private static func three<T: Comparable>(_ a: T, _ b: T) -> Int { a < b ? -1 : (a > b ? 1 : 0) }

    /// 三态比较：<0 → a 在前。
    static func compare(_ a: TaskItem, _ b: TaskItem, order: ViewOrder) -> Int {
        if order.sortKey == .smart {
            let ta = tier(a)
            let tb = tier(b)
            if ta != tb { return three(ta, tb) }
            let byCreated = three(a.task.createdAt, b.task.createdAt)
            // 活跃 / 排队档按添加顺序正序，历史档新 → 旧。
            let directed = ta <= 2 ? byCreated : -byCreated
            return directed != 0 ? directed : three(a.id, b.id)
        }
        var key: Int
        switch order.sortKey {
        case .created, .smart:
            key = three(a.task.createdAt, b.task.createdAt)
        case .name:
            switch a.task.fileName.caseInsensitiveCompare(b.task.fileName) {
            case .orderedAscending: key = -1
            case .orderedDescending: key = 1
            case .orderedSame: key = 0
            }
        case .size:
            key = three(a.task.totalBytes, b.task.totalBytes)
        case .progress:
            key = three(a.task.progress ?? -1, b.task.progress ?? -1)
        case .speed:
            key = three(a.speedDown, b.speedDown)
        case .status:
            key = three(tier(a), tier(b))
        }
        if !order.ascending { key = -key }
        if key != 0 { return key }
        let newestFirst = three(b.task.createdAt, a.task.createdAt)
        return newestFirst != 0 ? newestFirst : three(a.id, b.id)
    }

    // MARK: 分区 / 分组

    private static func isInflight(_ status: TaskStatus) -> Bool {
        status == .pending || status == .downloading || status == .preparing
    }

    private static func sectioned(
        _ rows: [TaskItem],
        folder: StatusFolder,
        into sections: inout [DownloadsSection],
        visible: inout [String]
    ) {
        let inflight = rows.filter { isInflight($0.task.status) }
        let history = rows.filter { !isInflight($0.task.status) }
        if !inflight.isEmpty {
            sections.append(DownloadsSection(
                id: "flow", kind: .inFlight, title: .inFlight, count: inflight.count, doneOfTotal: nil,
                downSpeed: inflight.reduce(0) { $0 + $1.speedDown }, collapsed: false, items: inflight
            ))
            visible.append(contentsOf: inflight.map(\.id))
        }
        if !history.isEmpty {
            let named: StatusFolder? = (folder == .completed || folder == .failed || folder == .paused) ? folder : nil
            sections.append(DownloadsSection(
                id: "history", kind: .history, title: .history(named), count: history.count, doneOfTotal: nil,
                downSpeed: 0, collapsed: false, items: history
            ))
            visible.append(contentsOf: history.map(\.id))
        }
    }

    private struct Group {
        let key: String
        let title: SectionTitle
        let items: [TaskItem]
        var doneOfTotal: DoneOfTotal?

        init(key: String, title: SectionTitle, items: [TaskItem], doneOfTotal: DoneOfTotal? = nil) {
            self.key = key
            self.title = title
            self.items = items
            self.doneOfTotal = doneOfTotal
        }
    }

    /// 按首次出现顺序分桶。
    private static func bucketed(_ rows: [TaskItem], key: (TaskItem) -> String) -> [(key: String, items: [TaskItem])] {
        var order: [String] = []
        var buckets: [String: [TaskItem]] = [:]
        for row in rows {
            let k = key(row)
            if buckets[k] == nil { order.append(k) }
            buckets[k, default: []].append(row)
        }
        return order.map { ($0, buckets[$0] ?? []) }
    }

    private func groups(
        for rows: [TaskItem],
        by groupBy: GroupBy,
        state s: HostState,
        now nowMs: Int64,
        calendar: Calendar
    ) -> [Group] {
        switch groupBy {
        case .none:
            return []
        case .status:
            let buckets: [(key: String, title: String, accepts: (TaskStatus) -> Bool)] = [
                ("status:1", "statusDownloading", { $0 == .downloading }),
                ("status:0", "statusPending", { $0 == .pending || $0 == .preparing }),
                ("status:4", "statusError", { $0 == .failed }),
                ("status:2", "statusPaused", { $0 == .paused }),
                ("status:3", "statusCompleted", { $0 == .completed || $0 == .unknown }),
            ]
            return buckets.compactMap { bucket in
                let items = rows.filter { bucket.accepts($0.task.status) }
                return items.isEmpty ? nil : Group(key: bucket.key, title: .key(bucket.title), items: items)
            }
        case .date:
            let now = Date(timeIntervalSince1970: TimeInterval(nowMs) / 1000)
            let today = calendar.startOfDay(for: now)
            func start(daysAgo: Int) -> Int64 {
                Int64(calendar.date(byAdding: .day, value: -daysAgo, to: today)?.timeIntervalSince1970 ?? 0)
            }
            let bounds = [Int64(today.timeIntervalSince1970), start(daysAgo: 1), start(daysAgo: 7), start(daysAgo: 30)]
            let labels = ["today", "yesterday", "thisWeek", "thisMonth", "older"]
            var buckets = Array(repeating: [TaskItem](), count: labels.count)
            for row in rows {
                let created = row.task.createdAt
                let b = bounds.firstIndex { created >= $0 } ?? labels.count - 1
                buckets[b].append(row)
            }
            return buckets.enumerated().compactMap { i, items in
                items.isEmpty ? nil : Group(key: "date:\(i)", title: .key(labels[i]), items: items)
            }
        case .type:
            let byCategory = Dictionary(grouping: rows) { $0.category?.id ?? "" }
            var out: [Group] = []
            for category in index.ordered where !category.isAll {
                guard let items = byCategory[category.id] else { continue }
                out.append(Group(key: "type:\(category.id)", title: .category(category), items: items))
            }
            if let none = byCategory[""] { out.append(Group(key: "type:none", title: .key("categoryOther"), items: none)) }
            return out
        case .queue:
            let byQueue = Self.bucketed(rows) { normalizedQueueId($0.task.queueId) }
            let lookup = Dictionary(uniqueKeysWithValues: byQueue.map { ($0.key, $0.items) })
            var used = Set<String>()
            var out: [Group] = []
            for queue in s.queues.sorted(by: { $0.position < $1.position }) {
                let key = normalizedQueueId(queue.queueId)
                guard let items = lookup[key] else { continue }
                used.insert(key)
                out.append(Group(key: "queue:\(key)", title: .queue(queue), items: items))
            }
            for bucket in byQueue where !used.contains(bucket.key) {
                out.append(Group(key: "queue:\(bucket.key)", title: .text(bucket.key), items: bucket.items))
            }
            return out
        case .site:
            let bySite = Dictionary(grouping: rows) { $0.site }
            var out: [Group] = []
            for site in bySite.keys.filter({ !$0.isEmpty }).sorted(by: { $0.caseInsensitiveCompare($1) == .orderedAscending }) {
                out.append(Group(key: "site:\(site)", title: .text(site), items: bySite[site] ?? []))
            }
            if let hostless = bySite[""] {
                // 无 host：磁力 / 种子归「BT · 磁力」，其余（eD2K 等）归「—」。
                let bt = hostless.filter { $0.task.protocol == .bt }
                let other = hostless.filter { $0.task.protocol != .bt }
                if !bt.isEmpty { out.append(Group(key: "site:", title: .key("viewSiteBt"), items: bt)) }
                if !other.isEmpty { out.append(Group(key: "site:-", title: .text(Format.dash), items: other)) }
            }
            return out
        case .group:
            let byGroup = Dictionary(grouping: rows) { $0.task.groupId }
            var out: [Group] = []
            for group in s.groups.sorted(by: { $0.name.lowercased() < $1.name.lowercased() }) {
                guard let items = byGroup[group.groupId] else { continue }
                let done = items.filter { $0.task.status == .completed }.count
                out.append(Group(
                    key: "group:\(group.groupId)", title: .text(group.name), items: items,
                    doneOfTotal: DoneOfTotal(done: done, total: items.count)
                ))
            }
            let known = Set(s.groups.map(\.groupId))
            let loose = rows.filter { $0.task.groupId.isEmpty || !known.contains($0.task.groupId) }
            if !loose.isEmpty { out.append(Group(key: "group:none", title: .key("ungroupedTasks"), items: loose)) }
            return out
        }
    }
}
