package com.fluxdown.app.feature.downloads

import com.fluxdown.app.R
import com.fluxdown.app.data.GroupBy
import com.fluxdown.app.data.SortKey
import com.fluxdown.app.data.ViewPrefs
import com.fluxdown.app.ui.TaskVisualState
import com.fluxdown.app.ui.visualState
import com.fluxdown.core.format.Format
import com.fluxdown.core.model.Category
import com.fluxdown.core.model.CategoryIndex
import com.fluxdown.core.model.Queue
import com.fluxdown.core.model.Segment
import com.fluxdown.core.model.Task
import com.fluxdown.core.model.TaskRuntime
import com.fluxdown.core.model.TaskStatus
import com.fluxdown.core.store.HostState
import com.fluxdown.fluxui.data.FlowSegmentUi
import com.fluxdown.fluxui.data.FlowStripState
import java.time.Instant
import java.time.ZoneId
import java.util.Objects
import kotlin.math.max

/*
 * 下载列表的纯派生逻辑：筛选 → 计数 → 排序 → 分区 / 分组 → 扁平条目。
 * 全程在后台调度器（单协程顺序）执行；[DeriveCache] 只在该协程内访问。
 */

private const val REORDER_MIN_MS = 2_000L
private const val REORDER_IDLE_MS = 1_500L

internal class DeriveInput(
    val state: HostState,
    val prefs: ViewPrefs,
    val filter: DownloadsFilter,
    val collapsed: Set<String>,
)

internal class DeriveResult(val list: DownloadsList, val facets: Facets, val retryAtMs: Long)

internal class DeriveCache {
    class Entry(val item: TaskItem, val runtime: TaskRuntime?)

    val items = HashMap<String, Entry>()
    var categoriesRef: List<Category>? = null
    var index: CategoryIndex = CategoryIndex(Category.BUILTIN)
    val categoryByName = HashMap<String, Category?>()
    var sortSig = 0
    var lastReorderMs = 0L
    var lastPos: Map<String, Int> = emptyMap()
}

private class Group(val key: String, val label: GroupLabel, val items: List<TaskItem>, val extra: String? = null)

private class StatusBucket(val key: String, val res: Int, val accepts: (TaskStatus) -> Boolean)

internal fun derive(input: DeriveInput, cache: DeriveCache, nowMs: Long, interactionMs: Long): DeriveResult {
    val s = input.state
    val f = input.filter
    val p = input.prefs

    if (cache.categoriesRef !== s.categories) {
        cache.categoriesRef = s.categories
        cache.index = CategoryIndex(s.categories)
        cache.categoryByName.clear()
        cache.items.clear()
    }
    val index = cache.index
    val queuesById = HashMap<String, Queue>(s.queues.size * 2)
    for (q in s.queues) queuesById[normQueue(q.queueId)] = q

    // 1. 行模型（未变化的复用同一实例）
    val all = ArrayList<TaskItem>(s.tasks.size)
    for (t in s.tasks) all += itemFor(t, s, cache, queuesById)
    if (cache.items.size > all.size) {
        val ids = HashSet<String>(all.size * 2)
        for (it in all) ids += it.id
        cache.items.keys.retainAll(ids)
    }

    // 2. 范围（队列 / 搜索）→ 文件夹计数
    val query = f.query.trim().lowercase()
    val scoped = ArrayList<TaskItem>(all.size)
    for (it in all) {
        if (f.queueId != null && normQueue(it.task.queueId) != f.queueId) continue
        if (query.isNotEmpty() && !matchesQuery(it, query)) continue
        scoped += it
    }
    val folderCounts = IntArray(StatusFolder.entries.size)
    for (it in scoped) for (fo in StatusFolder.entries) if (fo.accepts(it.task.status)) folderCounts[fo.ordinal]++

    // 3. 文件夹内 → 分类计数 → 分类筛选
    val inFolder = scoped.filter { f.folder.accepts(it.task.status) }
    val catCounts = HashMap<String, Int>()
    for (it in inFolder) it.category?.let { c -> catCounts[c.id] = (catCounts[c.id] ?: 0) + 1 }
    val selectedCat = f.categoryId?.takeIf { id -> index.ordered.any { it.id == id && !it.isAll } }
    val pills = ArrayList<CategoryPill>()
    for (c in index.ordered) {
        if (c.isAll || !c.visible) continue
        val n = catCounts[c.id] ?: 0
        if (n > 0 || c.id == selectedCat) pills += CategoryPill(c, n)
    }
    val rows = if (selectedCat == null) inFolder else inFolder.filter { it.category?.id == selectedCat }

    // 4. 排序（进度 / 速度键节流重排）
    var retryAt = 0L
    var ordered = rows.sortedWith(comparatorFor(p))
    val dynamic = p.sortKey == SortKey.Progress || p.sortKey == SortKey.Speed
    val sig = Objects.hash(f, p.sortKey, p.ascending, p.groupBy)
    if (dynamic && cache.sortSig == sig && cache.lastPos.isNotEmpty()) {
        val sinceReorder = nowMs - cache.lastReorderMs
        val sinceTouch = nowMs - interactionMs
        if (sinceReorder < REORDER_MIN_MS || sinceTouch < REORDER_IDLE_MS) {
            val pos = cache.lastPos
            ordered = ordered.sortedBy { pos[it.id] ?: Int.MAX_VALUE }
            retryAt = max(cache.lastReorderMs + REORDER_MIN_MS, interactionMs + REORDER_IDLE_MS)
        } else {
            cache.lastReorderMs = nowMs
        }
    } else {
        cache.lastReorderMs = nowMs
    }
    cache.sortSig = sig
    cache.lastPos = HashMap<String, Int>(ordered.size * 2).also { m -> ordered.forEachIndexed { i, it -> m[it.id] = i } }

    // 5. 分区 / 分组 → 扁平条目
    val entries = ArrayList<ListEntry>(ordered.size + 8)
    val visible = ArrayList<String>(ordered.size)
    if (p.groupBy == GroupBy.None) {
        sectioned(ordered, f.folder, entries, visible)
    } else {
        for (g in groupsFor(ordered, p.groupBy, s, index, nowMs)) {
            val closed = g.key in input.collapsed
            entries += GroupHeaderEntry(g.key, g.label, g.items.size, g.extra, closed)
            if (!closed) {
                g.items.forEachIndexed { i, it ->
                    entries += RowEntry(it, RowZone.Card, first = i == 0, last = i == g.items.lastIndex)
                    visible += it.id
                }
            }
        }
    }

    // 6. 队列分面
    val queueCounts = HashMap<String, Int>()
    for (it in all) {
        val k = normQueue(it.task.queueId)
        queueCounts[k] = (queueCounts[k] ?: 0) + 1
    }
    val queueFacets = s.queues.sortedBy { it.position }.map { QueueFacet(it, queueCounts[normQueue(it.queueId)] ?: 0) }

    return DeriveResult(
        list = DownloadsList(entries, visible, s.tasks.size, loaded = true),
        facets = Facets(folderCounts.asList(), pills, queueFacets, ordered.size),
        retryAtMs = retryAt,
    )
}

// ── 行模型 ────────────────────────────────────────────────────────────────

private fun itemFor(t: Task, s: HostState, cache: DeriveCache, queuesById: Map<String, Queue>): TaskItem {
    val speed = s.speeds[t.taskId]
    val sd = speed?.down ?: 0L
    val su = speed?.up ?: 0L
    val rt = s.runtime[t.taskId]
    val qp = s.queuePositions[t.taskId] ?: 0
    val boosted = s.priorityTaskId == t.taskId
    val cat = cache.categoryByName.getOrPut(t.fileName) { cache.index.categoryOf(t.fileName) }
    val queue = queuesById[normQueue(t.queueId)]

    val prev = cache.items[t.taskId]
    if (prev != null) {
        val it = prev.item
        if (it.task === t && prev.runtime === rt && it.speedDown == sd && it.speedUp == su &&
            it.queuePosition == qp && it.boosted == boosted && it.category === cat && it.queue === queue
        ) return it
    }

    val visual = t.visualState(qp)
    val (flow, flowState) = flowFor(t, visual, rt)
    val item = TaskItem(
        task = t,
        speedDown = sd,
        speedUp = su,
        queuePosition = qp,
        boosted = boosted,
        category = cat,
        queue = queue,
        site = siteOf(t),
        visual = visual,
        flow = flow,
        flowState = flowState,
        eta = if (visual == TaskVisualState.Downloading) Format.etaSeconds(t.downloadedBytes, t.totalBytes, sd) else null,
    )
    cache.items[t.taskId] = DeriveCache.Entry(item, rt)
    return item
}

/** 来源站点：originUrl ‖ url 的 host（去 www. 与端口）；BT / eD2K 哨兵无 host → 空串。 */
internal fun siteOf(t: Task): String {
    val raw = t.originUrl.ifEmpty { t.url }
    if (raw.startsWith("magnet:") || raw.startsWith("torrent-file://") || raw.startsWith("ed2k://")) return ""
    return raw.substringAfter("://", "")
        .substringBefore('/').substringBefore('?').substringAfterLast('@').substringBefore(':')
        .removePrefix("www.")
}

private fun matchesQuery(it: TaskItem, q: String): Boolean {
    val t = it.task
    return t.fileName.lowercase().contains(q) ||
        t.url.lowercase().contains(q) ||
        t.originUrl.lowercase().contains(q) ||
        it.site.lowercase().contains(q)
}

/** 流带：按真实字节区间等比；无分段退化为单段总进度（§3.7）。返回 (null, _) = 不显示流带。 */
private fun flowFor(t: Task, visual: TaskVisualState, rt: TaskRuntime?): Pair<List<FlowSegmentUi>?, FlowStripState> {
    val state = when (visual) {
        TaskVisualState.Downloading -> FlowStripState.Downloading
        TaskVisualState.Queued -> FlowStripState.Queued
        TaskVisualState.Pending -> FlowStripState.Pending
        TaskVisualState.Preparing, TaskVisualState.Verifying -> FlowStripState.Preparing
        TaskVisualState.Paused -> FlowStripState.Paused
        TaskVisualState.Failed -> FlowStripState.Failed
        TaskVisualState.Seeding -> FlowStripState.Seeding
        TaskVisualState.Missing -> FlowStripState.Missing
        TaskVisualState.Completed -> FlowStripState.Completed
    }
    val show = when (visual) {
        TaskVisualState.Completed, TaskVisualState.Missing -> false
        TaskVisualState.Preparing, TaskVisualState.Verifying, TaskVisualState.Downloading, TaskVisualState.Seeding -> true
        else -> t.totalBytes > 0 || t.downloadedBytes > 0
    }
    if (!show) return null to state
    if (visual == TaskVisualState.Seeding) return listOf(FlowSegmentUi(1f, 1f)) to state
    val segs = rt?.segments
    if (!segs.isNullOrEmpty() && t.totalBytes > 0 &&
        (visual == TaskVisualState.Downloading || visual == TaskVisualState.Paused || visual == TaskVisualState.Failed)
    ) {
        val live = visual == TaskVisualState.Downloading
        return segs.map { it.toUi(live) } to state
    }
    return listOf(FlowSegmentUi(1f, t.progress ?: 0f)) to state
}

private fun Segment.toUi(live: Boolean): FlowSegmentUi {
    val len = (endByte - startByte + 1).coerceAtLeast(1L)
    return FlowSegmentUi(
        fraction = len.toFloat(),
        filled = (downloadedBytes.toDouble() / len).toFloat().coerceIn(0f, 1f),
        active = live && active == true,
    )
}

// ── 排序 ──────────────────────────────────────────────────────────────────

/** 智能排序档（§3.9）：优先下载 → 活跃 → 排队 → 失败 → 暂停 → 完成。 */
private fun tier(it: TaskItem): Int {
    val st = it.task.status
    return when {
        it.boosted && (st == TaskStatus.Downloading || st == TaskStatus.Pending) -> 0
        st == TaskStatus.Downloading || st == TaskStatus.Preparing -> 1
        st == TaskStatus.Pending -> 2
        st == TaskStatus.Failed -> 3
        st == TaskStatus.Paused -> 4
        else -> 5
    }
}

private fun comparatorFor(p: ViewPrefs): Comparator<TaskItem> {
    val byId = Comparator<TaskItem> { a, b -> a.id.compareTo(b.id) }
    if (p.sortKey == SortKey.Smart) {
        return Comparator<TaskItem> { a, b ->
            val ta = tier(a)
            val tb = tier(b)
            if (ta != tb) {
                ta - tb
            } else {
                val c = a.task.createdAt.compareTo(b.task.createdAt)
                if (ta <= 2) c else -c
            }
        }.then(byId)
    }
    val key: Comparator<TaskItem> = when (p.sortKey) {
        SortKey.Created -> Comparator<TaskItem> { a, b -> a.task.createdAt.compareTo(b.task.createdAt) }
        SortKey.Name -> Comparator<TaskItem> { a, b -> String.CASE_INSENSITIVE_ORDER.compare(a.task.fileName, b.task.fileName) }
        SortKey.Size -> Comparator<TaskItem> { a, b -> a.task.totalBytes.compareTo(b.task.totalBytes) }
        SortKey.Progress -> Comparator<TaskItem> { a, b -> (a.task.progress ?: -1f).compareTo(b.task.progress ?: -1f) }
        SortKey.Speed -> Comparator<TaskItem> { a, b -> a.speedDown.compareTo(b.speedDown) }
        else -> Comparator<TaskItem> { a, b -> tier(a) - tier(b) }
    }
    val directed = if (p.ascending) key else key.reversed()
    return directed
        .then(Comparator<TaskItem> { a, b -> b.task.createdAt.compareTo(a.task.createdAt) })
        .then(byId)
}

// ── 分区 / 分组 ───────────────────────────────────────────────────────────

private fun isInflight(st: TaskStatus) = st == TaskStatus.Pending || st == TaskStatus.Downloading || st == TaskStatus.Preparing

private fun sectioned(rows: List<TaskItem>, folder: StatusFolder, out: MutableList<ListEntry>, visible: MutableList<String>) {
    val inflight = rows.filter { isInflight(it.task.status) }
    val history = rows.filter { !isInflight(it.task.status) }
    if (inflight.isNotEmpty()) {
        out += FlowHeaderEntry(inflight.size, inflight.sumOf { it.speedDown })
        inflight.forEachIndexed { i, it ->
            out += RowEntry(it, RowZone.Flow, first = false, last = i == inflight.lastIndex)
            visible += it.id
        }
    }
    if (history.isNotEmpty()) {
        val named = folder.takeIf { it == StatusFolder.Completed || it == StatusFolder.Failed || it == StatusFolder.Paused }
        out += HistoryHeaderEntry(history.size, named)
        history.forEachIndexed { i, it ->
            out += RowEntry(it, RowZone.History, first = i == 0, last = i == history.lastIndex)
            visible += it.id
        }
    }
}

private fun groupsFor(rows: List<TaskItem>, by: GroupBy, s: HostState, index: CategoryIndex, nowMs: Long): List<Group> = when (by) {
    GroupBy.None -> emptyList()
    GroupBy.Status -> {
        val order = listOf(
            StatusBucket("status:1", R.string.statusDownloading) { it == TaskStatus.Downloading },
            StatusBucket("status:0", R.string.statusPending) { it == TaskStatus.Pending || it == TaskStatus.Preparing },
            StatusBucket("status:4", R.string.statusError) { it == TaskStatus.Failed },
            StatusBucket("status:2", R.string.statusPaused) { it == TaskStatus.Paused },
            StatusBucket("status:3", R.string.statusCompleted) { it == TaskStatus.Completed || it == TaskStatus.Unknown },
        )
        order.mapNotNull { b ->
            val items = rows.filter { b.accepts(it.task.status) }
            if (items.isEmpty()) null else Group(b.key, GroupLabel(res = b.res), items)
        }
    }
    GroupBy.Date -> {
        val zone = ZoneId.systemDefault()
        val today = Instant.ofEpochMilli(nowMs).atZone(zone).toLocalDate()
        val bounds = longArrayOf(
            today.atStartOfDay(zone).toEpochSecond(),
            today.minusDays(1).atStartOfDay(zone).toEpochSecond(),
            today.minusDays(7).atStartOfDay(zone).toEpochSecond(),
            today.minusDays(30).atStartOfDay(zone).toEpochSecond(),
        )
        val labels = intArrayOf(R.string.today, R.string.yesterday, R.string.thisWeek, R.string.thisMonth, R.string.older)
        val buckets = Array(5) { ArrayList<TaskItem>() }
        for (it in rows) {
            val sec = it.task.createdAt
            val b = when {
                sec >= bounds[0] -> 0
                sec >= bounds[1] -> 1
                sec >= bounds[2] -> 2
                sec >= bounds[3] -> 3
                else -> 4
            }
            buckets[b] += it
        }
        buckets.mapIndexedNotNull { i, items ->
            if (items.isEmpty()) null else Group("date:$i", GroupLabel(res = labels[i]), items)
        }
    }
    GroupBy.Type -> {
        val byCat = rows.groupBy { it.category?.id }
        val out = ArrayList<Group>()
        for (c in index.ordered) {
            if (c.isAll) continue
            val items = byCat[c.id] ?: continue
            out += Group("type:${c.id}", GroupLabel(category = c), items)
        }
        byCat[null]?.let { out += Group("type:none", GroupLabel(res = R.string.categoryOther), it) }
        out
    }
    GroupBy.Queue -> {
        val byQueue = rows.groupBy { normQueue(it.task.queueId) }
        val out = ArrayList<Group>()
        val used = HashSet<String>()
        for (q in s.queues.sortedBy { it.position }) {
            val k = normQueue(q.queueId)
            val items = byQueue[k] ?: continue
            used += k
            out += Group("queue:$k", GroupLabel(queue = q), items)
        }
        for ((k, items) in byQueue) if (k !in used) out += Group("queue:$k", GroupLabel(text = k), items)
        out
    }
    GroupBy.Site -> {
        val bySite = rows.groupBy { it.site }
        val named = bySite.filterKeys { it.isNotEmpty() }.toSortedMap(String.CASE_INSENSITIVE_ORDER)
        val out = ArrayList<Group>()
        for ((site, items) in named) out += Group("site:$site", GroupLabel(text = site), items)
        bySite[""]?.let { out += Group("site:", GroupLabel(res = R.string.viewSiteBt), it) }
        out
    }
    GroupBy.Group -> {
        val byGroup = rows.groupBy { it.task.groupId }
        val out = ArrayList<Group>()
        for (g in s.groups.sortedBy { it.name.lowercase() }) {
            val items = byGroup[g.groupId] ?: continue
            val done = items.count { it.task.status == TaskStatus.Completed }
            out += Group("group:${g.groupId}", GroupLabel(text = g.name), items, extra = "$done/${items.size}")
        }
        val known = s.groups.mapTo(HashSet()) { it.groupId }
        val loose = rows.filter { it.task.groupId.isEmpty() || it.task.groupId !in known }
        if (loose.isNotEmpty()) out += Group("group:none", GroupLabel(res = R.string.ungroupedTasks), loose)
        out
    }
}
