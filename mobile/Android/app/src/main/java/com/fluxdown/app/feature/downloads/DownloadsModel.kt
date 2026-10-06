package com.fluxdown.app.feature.downloads

import androidx.compose.runtime.Immutable
import com.fluxdown.app.ui.TaskVisualState
import com.fluxdown.core.model.Category
import com.fluxdown.core.model.Queue
import com.fluxdown.core.model.Task
import com.fluxdown.core.model.TaskStatus
import com.fluxdown.fluxui.data.FlowSegmentUi
import com.fluxdown.fluxui.data.FlowStripState

/** 状态文件夹（ScopeTabs / Rail）；顺序即显示顺序。 */
enum class StatusFolder {
    All, Active, Completed, Failed, Paused;

    fun accepts(status: TaskStatus): Boolean = when (this) {
        All -> true
        Active -> status == TaskStatus.Pending || status == TaskStatus.Downloading || status == TaskStatus.Preparing
        Completed -> status == TaskStatus.Completed
        Failed -> status == TaskStatus.Failed
        Paused -> status == TaskStatus.Paused
    }
}

/** 列表筛选：状态文件夹 · 分类 · 队列范围 · 搜索词。 */
@Immutable
data class DownloadsFilter(
    val folder: StatusFolder = StatusFolder.All,
    val categoryId: String? = null,
    /** 已规范化的队列 id（主队列 = [Queue.MAIN]）；null = 全部队列。 */
    val queueId: String? = null,
    val query: String = "",
) {
    val hasScope: Boolean get() = queueId != null || query.isNotBlank()
    val isDefault: Boolean get() = this == DownloadsFilter()
}

internal fun normQueue(id: String): String = if (id.isEmpty()) Queue.MAIN else id

/**
 * 一行任务的全部展示输入（在后台线程派生并复用实例：未变化的任务得到同一个对象，
 * 行组合项据此直接跳过重组）。流带 / ETA / 视觉状态都在这里算好，行只负责格式化文案。
 */
@Immutable
data class TaskItem(
    val task: Task,
    val speedDown: Long,
    val speedUp: Long,
    val queuePosition: Int,
    val boosted: Boolean,
    val category: Category?,
    val queue: Queue?,
    val site: String,
    val visual: TaskVisualState,
    val flow: List<FlowSegmentUi>?,
    val flowState: FlowStripState,
    val eta: Long?,
) {
    val id: String get() = task.taskId
}

/** 行所在的容器：传输中 / 分组卡片（圆角玻璃分区）与历史（出血安静列表）。 */
enum class RowZone { Flow, Card, History }

/** 分组标题：i18n 资源 / 原文 / 分类 / 队列（组合期解析为文案）。 */
@Immutable
data class GroupLabel(
    val res: Int = 0,
    val text: String = "",
    val category: Category? = null,
    val queue: Queue? = null,
)

/** 列表条目（LazyColumn 的扁平化模型）。 */
@Immutable
sealed interface ListEntry {
    val key: String
    val contentType: Int
}

internal object ContentType {
    const val Header = 0
    const val Banner = 1
    const val Hero = 2
    const val Slot = 3
    const val FlowHeader = 4
    const val HistoryHeader = 5
    const val GroupHeader = 6
    const val RowFlow = 7
    const val RowCard = 8
    const val RowHistory = 9
    const val Empty = 10
    const val Skeleton = 11
    const val Scope = 12
}

/** 「传输中」分区头：右侧实时汇总下行速度 + 任务数。 */
@Immutable
data class FlowHeaderEntry(val count: Int, val downSpeed: Long) : ListEntry {
    override val key: String get() = "h:flow"
    override val contentType: Int get() = ContentType.FlowHeader
}

/** 「历史」小标题；选了已完成 / 失败 / 已暂停文件夹时换成该文件夹名（[folder] 非 null）。 */
@Immutable
data class HistoryHeaderEntry(val count: Int, val folder: StatusFolder?) : ListEntry {
    override val key: String get() = "h:history"
    override val contentType: Int get() = ContentType.HistoryHeader
}

@Immutable
data class GroupHeaderEntry(
    val groupKey: String,
    val label: GroupLabel,
    val count: Int,
    val extra: String?,
    val closed: Boolean,
) : ListEntry {
    override val key: String get() = "g:$groupKey"
    override val contentType: Int get() = ContentType.GroupHeader
}

@Immutable
data class RowEntry(
    val item: TaskItem,
    val zone: RowZone,
    val first: Boolean,
    val last: Boolean,
) : ListEntry {
    override val key: String get() = "t:${item.id}"
    override val contentType: Int
        get() = when (zone) {
            RowZone.Flow -> ContentType.RowFlow
            RowZone.Card -> ContentType.RowCard
            RowZone.History -> ContentType.RowHistory
        }
}

/** 派生出的列表：条目 + 可见任务 id（全选范围）+ 总任务数（区分「无任务」与「筛选后为空」）。 */
@Immutable
data class DownloadsList(
    val entries: List<ListEntry>,
    val visibleIds: List<String>,
    val taskTotal: Int,
    val loaded: Boolean,
) {
    companion object {
        val Initial = DownloadsList(emptyList(), emptyList(), 0, loaded = false)
    }
}

@Immutable
data class CategoryPill(val category: Category, val count: Int)

@Immutable
data class QueueFacet(val queue: Queue, val count: Int)

/** 分面：文件夹计数（范围内）· 当前文件夹内的分类计数 · 各队列任务数。只在数值变化时才发布。 */
@Immutable
data class Facets(
    /** 按 [StatusFolder.ordinal] 排列。 */
    val folderCounts: List<Int>,
    val categories: List<CategoryPill>,
    val queues: List<QueueFacet>,
    /** 当前筛选结果行数。 */
    val matching: Int,
) {
    fun count(folder: StatusFolder): Int = folderCounts.getOrElse(folder.ordinal) { 0 }

    companion object {
        val Initial = Facets(List(StatusFolder.entries.size) { 0 }, emptyList(), emptyList(), 0)
    }
}
