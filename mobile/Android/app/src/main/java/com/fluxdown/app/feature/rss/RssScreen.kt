package com.fluxdown.app.feature.rss

import android.content.Context
import android.net.Uri
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.State
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshots.SnapshotStateSet
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import com.fluxdown.app.R
import com.fluxdown.app.actions.LocalTaskActions
import com.fluxdown.app.actions.TaskActions
import com.fluxdown.app.feature.devices.DockMiniEffect
import com.fluxdown.app.feature.devices.GlyphTile
import com.fluxdown.app.feature.devices.localizedName
import com.fluxdown.app.i18n.str
import com.fluxdown.app.nav.LocalNavigator
import com.fluxdown.app.nav.SheetRoute
import com.fluxdown.app.shell.LocalAppContainer
import com.fluxdown.app.shell.hostState
import com.fluxdown.core.host.HostException
import com.fluxdown.core.host.HostSession
import com.fluxdown.core.model.RssSource
import com.fluxdown.core.store.Connection
import com.fluxdown.core.store.HostStore
import com.fluxdown.fluxui.chrome.FluxGlassIconButton
import com.fluxdown.fluxui.chrome.FluxHeader
import com.fluxdown.fluxui.chrome.FluxHostPill
import com.fluxdown.fluxui.controls.BadgeTone
import com.fluxdown.fluxui.controls.FluxBadge
import com.fluxdown.fluxui.controls.FluxDivider
import com.fluxdown.fluxui.controls.FluxStatRow
import com.fluxdown.fluxui.controls.FluxTag
import com.fluxdown.fluxui.controls.StatItem
import com.fluxdown.fluxui.controls.Tone
import com.fluxdown.fluxui.data.MetaTone
import com.fluxdown.fluxui.data.buildTaskMeta
import com.fluxdown.fluxui.feedback.FluxEmpty
import com.fluxdown.fluxui.feedback.FluxGlyph
import com.fluxdown.fluxui.icons.FluxIcon
import com.fluxdown.fluxui.icons.FluxIcons
import com.fluxdown.fluxui.material.FluxGlass
import com.fluxdown.fluxui.material.FluxGlassKind
import com.fluxdown.fluxui.material.fluxFlowIn
import com.fluxdown.fluxui.material.fluxGlass
import com.fluxdown.fluxui.material.rememberFlowInGate
import com.fluxdown.fluxui.overlay.FluxBanner
import com.fluxdown.fluxui.overlay.FluxBannerKind
import com.fluxdown.fluxui.overlay.FluxMenuItem
import com.fluxdown.fluxui.overlay.FluxOverlayState
import com.fluxdown.fluxui.overlay.FluxSwipeAction
import com.fluxdown.fluxui.overlay.FluxSwipeTone
import com.fluxdown.fluxui.overlay.FluxToastKind
import com.fluxdown.fluxui.overlay.LocalFluxOverlays
import com.fluxdown.fluxui.overlay.LocalSwipeRevealCoordinator
import com.fluxdown.fluxui.overlay.SwipeReveal
import com.fluxdown.fluxui.theme.FluxHaptics
import com.fluxdown.fluxui.theme.FluxText
import com.fluxdown.fluxui.theme.FluxTheme
import com.fluxdown.fluxui.theme.fluxPressable
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin

/** 同时抓取的订阅数上限（R1：下拉“全部抓取” ≤ 4 并发）。 */
private const val REFRESH_PARALLELISM = 4

/** 相对时间重算周期：仅前台，纯本地（无轮询请求）。 */
private const val CLOCK_TICK_MS = 30_000L

/**
 * R1 订阅源页（Dock 目的地）：汇总（未读 / 订阅 / 失败）+ 订阅列表。
 * 条目列表（R2）与编辑器（R3）依赖的接口不在 [HostSession] 端口内，本页不提供对应入口。
 */
@Composable
fun RssScreen() {
    val container = LocalAppContainer.current
    val nav = LocalNavigator.current
    val overlays = LocalFluxOverlays.current
    val actions = LocalTaskActions.current
    val haptics = FluxTheme.haptics
    val appContext = LocalContext.current.applicationContext
    val space = FluxTheme.space
    val host = hostState()
    val hostRef by container.host.collectAsStateWithLifecycle()
    val sources by remember { derivedStateOf { host.value.rssSources } }
    val online by remember { derivedStateOf { host.value.connection == Connection.Live } }
    val unread by remember { derivedStateOf { sources.sumOf { it.unreadCount } } }
    val failing by remember { derivedStateOf { sources.count { it.failCount > 0 } } }

    val controller = remember(container, overlays, haptics, actions, appContext) {
        RssController(container.appScope, { container.session }, container.store, overlays, haptics, appContext, actions)
    }
    val nowSec by rememberNowSeconds()
    val nowMin = nowSec / 60

    val listState = rememberLazyListState()
    DockMiniEffect(listState)
    val swipe = LocalSwipeRevealCoordinator.current
    val gate = rememberFlowInGate()
    val margin = space.screenMargin
    val hostName = hostRef.localizedName()
    val topInset = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()

    LazyColumn(
        state = listState,
        modifier = Modifier
            .fillMaxSize()
            .then(if (swipe != null) Modifier.nestedScroll(swipe.nestedScrollConnection) else Modifier),
        contentPadding = PaddingValues(top = topInset, bottom = space.dockClearance),
    ) {
        item(key = "header") {
            FluxHeader(
                title = str(R.string.rssSubscriptions),
                modifier = Modifier.padding(horizontal = margin).fluxFlowIn(0, gate, "header"),
                hostPill = {
                    FluxHostPill(
                        name = hostName,
                        online = online,
                        onClick = { nav.openSheet(SheetRoute.HostSwitch) },
                        description = str(R.string.mobileRssSwitchHostDesc, "name" to hostName),
                    )
                },
                actions = {
                    if (sources.isNotEmpty()) {
                        FluxGlassIconButton(
                            icon = FluxIcons.RefreshCw,
                            contentDescription = str(R.string.mobileRssRefreshAll),
                            onClick = { controller.refreshAll(sources) },
                            enabled = controller.busy.isEmpty(),
                        )
                    }
                },
            )
        }

        if (failing > 0) {
            item(key = "failed-banner") {
                FluxBanner(
                    text = str(R.string.mobileRssFailedBanner, "n" to failing),
                    modifier = Modifier.padding(horizontal = margin, vertical = 6.dp).fluxFlowIn(1, gate, "banner"),
                    kind = FluxBannerKind.Error,
                    icon = FluxIcons.CircleAlert,
                )
            }
        }

        if (sources.isEmpty()) {
            item(key = "empty") {
                Box(Modifier.fillMaxWidth().padding(top = 56.dp), contentAlignment = Alignment.Center) {
                    FluxEmpty(
                        glyph = FluxGlyph.Rss,
                        title = str(R.string.mobileRssEmptyTitle),
                        subtitle = str(R.string.mobileRssEmptyHint),
                    )
                }
            }
        } else {
            item(key = "summary") {
                SummaryCard(
                    unread = unread,
                    feeds = sources.size,
                    failing = failing,
                    modifier = Modifier.padding(horizontal = margin, vertical = 8.dp).fluxFlowIn(2, gate, "summary"),
                )
            }
            itemsIndexed(sources, key = { _, s -> s.sourceId }) { index, source ->
                FeedRow(
                    source = source,
                    refreshing = source.sourceId in controller.busy,
                    nowMin = nowMin,
                    last = index == sources.lastIndex,
                    onRefresh = { controller.refresh(source) },
                    onToggle = { controller.toggle(source) },
                    modifier = Modifier.fluxFlowIn(index + 3, gate, source.sourceId),
                )
            }
        }
    }
}

// ── 汇总 ────────────────────────────────────────────────────────────────

@Composable
private fun SummaryCard(unread: Int, feeds: Int, failing: Int, modifier: Modifier = Modifier) {
    val stats = listOf(
        StatItem(unread.toString(), str(R.string.mobileRssStatUnread), tone = if (unread > 0) Tone.Accent else Tone.Neutral),
        StatItem(feeds.toString(), str(R.string.mobileRssStatFeeds)),
        StatItem(failing.toString(), str(R.string.mobileRssStatFailing), tone = if (failing > 0) Tone.Coral else Tone.Neutral),
    )
    FluxStatRow(
        stats,
        modifier
            .fillMaxWidth()
            .fluxGlass(FluxGlass.G3, FluxTheme.shapes.card, kind = FluxGlassKind.Flat),
    )
}

// ── 订阅行 ──────────────────────────────────────────────────────────────

@Stable
private class RectBox {
    var rect: Rect = Rect.Zero
}

@Composable
private fun FeedRow(
    source: RssSource,
    refreshing: Boolean,
    nowMin: Long,
    last: Boolean,
    onRefresh: () -> Unit,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val c = FluxTheme.colors
    val t = FluxTheme.type
    val overlays = LocalFluxOverlays.current
    val margin = FluxTheme.space.screenMargin
    val roomy = t.fontScale >= 1.5f

    val failed = source.failCount > 0
    val title = source.name.ifBlank { source.url }
    val host = remember(source.url) { Uri.parse(source.url).host ?: source.url }

    val interval = if (source.intervalMinutes >= 60 && source.intervalMinutes % 60 == 0) {
        str(R.string.rssEveryHours, "n" to source.intervalMinutes / 60)
    } else {
        str(R.string.rssEveryMinutes, "n" to source.intervalMinutes)
    }
    val statusText = when {
        refreshing -> str(R.string.rssRefreshing)
        failed -> str(R.string.rssFailedTimes, "n" to source.failCount)
        source.lastSuccessAt > 0 -> str(R.string.rssLastFetch, "when" to relativeAgo(nowMin * 60 - source.lastSuccessAt))
        else -> str(R.string.rssNeverFetched)
    }
    val modeText = str(if (source.autoDownload) R.string.rssAutoDownloadOn else R.string.rssCollectMode)
    val statusTone = when {
        refreshing -> MetaTone.Accent
        failed -> MetaTone.Coral
        else -> MetaTone.Muted
    }
    val meta = remember(statusText, interval, modeText, statusTone, c) {
        c.buildTaskMeta {
            part(statusText, statusTone)
            trailing(interval, modeText)
        }
    }
    val unreadText = if (source.unreadCount > 0) str(R.string.mobileUnreadCount, "n" to source.unreadCount) else null
    val description = listOfNotNull(title, host, meta.text, source.lastError.takeIf { failed && it.isNotBlank() }, unreadText)
        .joinToString(", ")

    val menuItems = listOf(
        FluxMenuItem.Action(
            label = str(R.string.rssRefreshNow),
            onClick = onRefresh,
            icon = FluxIcons.RefreshCw,
            enabled = !refreshing,
        ),
        FluxMenuItem.Action(
            label = str(if (source.enabled) R.string.mobileRssDisable else R.string.mobileRssEnable),
            onClick = onToggle,
            icon = if (source.enabled) FluxIcons.PowerOff else FluxIcons.Power,
        ),
    )
    val rectBox = remember { RectBox() }
    val openMenu = { overlays.showMenu(rectBox.rect, menuItems, header = title) }

    val startActions = listOf(
        FluxSwipeAction(str(R.string.rssRefreshNow), FluxIcons.RefreshCw, FluxSwipeTone.Accent, onRefresh),
    )
    val endActions = listOf(
        FluxSwipeAction(
            label = str(if (source.enabled) R.string.mobileRssDisable else R.string.mobileRssEnable),
            icon = if (source.enabled) FluxIcons.PowerOff else FluxIcons.Power,
            tone = if (source.enabled) FluxSwipeTone.Warn else FluxSwipeTone.Neutral,
            onClick = onToggle,
        ),
    )

    val tileTint = when {
        failed -> c.coralText
        !source.enabled -> c.inkFaint
        source.unreadCount > 0 -> c.accentHi
        else -> c.ink
    }
    val titleStyle = remember(t) { t.weight(t.body, 450) }
    val metaStyle = remember(t, c) { t.mono.copy(color = c.inkMuted) }

    Column(modifier) {
        SwipeReveal(startActions = startActions, endActions = endActions) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .alpha(if (source.enabled) 1f else 0.45f)
                    .onGloballyPositioned { rectBox.rect = it.boundsInRoot() }
                    .fluxPressable(onClick = openMenu, onLongClick = openMenu, role = Role.Button)
                    .semantics(mergeDescendants = true) { contentDescription = description }
                    .heightIn(min = 72.dp)
                    .padding(horizontal = margin, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                GlyphTile(
                    icon = FluxIcons.Rss,
                    tint = tileTint,
                    ring = if (failed) c.coral.copy(alpha = 0.55f) else null,
                    overlay = if (refreshing) ({ FeedOrbit(c.accentHi) }) else null,
                )
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FluxText(
                            title,
                            modifier = Modifier.weight(1f, fill = false),
                            style = titleStyle,
                            color = c.ink,
                            maxLines = if (roomy) 2 else 1,
                            overflow = TextOverflow.MiddleEllipsis,
                        )
                        if (!source.enabled) FluxTag(str(R.string.mobileRssDisabled))
                    }
                    FluxText(host, style = t.sm, color = c.inkMuted, maxLines = 1)
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        if (failed) FluxIcon(FluxIcons.CircleAlert, null, size = 14.dp, tint = c.coralText)
                        BasicText(
                            meta,
                            modifier = Modifier.weight(1f, fill = false),
                            style = metaStyle,
                            maxLines = if (roomy) 2 else 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    if (failed && source.lastError.isNotBlank()) {
                        FluxText(source.lastError, style = t.monoS, color = c.coralText, maxLines = 2)
                    }
                }
                if (source.unreadCount > 0) FluxBadge(source.unreadCount, tone = BadgeTone.Accent)
            }
        }
        if (!last) FluxDivider(startInset = margin + 40.dp + 14.dp)
    }
}

@Composable
private fun relativeAgo(deltaSec: Long): String {
    val d = deltaSec.coerceAtLeast(0)
    return when {
        d < 60 -> str(R.string.rssJustNow)
        d < 3_600 -> str(R.string.rssMinutesAgo, "n" to d / 60)
        d < 86_400 -> str(R.string.rssHoursAgo, "n" to d / 3_600)
        else -> str(R.string.rssDaysAgo, "n" to d / 86_400)
    }
}

/** 抓取中的 16 点轨道（单 Canvas；相位在绘制阶段读取，不触发重组；Reduce motion 静止）。 */
@Composable
private fun FeedOrbit(color: Color, modifier: Modifier = Modifier) {
    val reduce = FluxTheme.motion.reduce
    val phase = if (reduce) {
        null
    } else {
        rememberInfiniteTransition(label = "feedOrbit").animateFloat(
            initialValue = 0f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(1_500, easing = LinearEasing), RepeatMode.Restart),
            label = "feedOrbitPhase",
        )
    }
    Canvas(modifier.size(22.dp).clearAndSetSemantics { }) {
        val n = 16
        val radius = size.minDimension / 2f - 1.6.dp.toPx()
        val dot = 1.5.dp.toPx()
        val head = (phase?.value ?: 0f) * n
        for (i in 0 until n) {
            val angle = (i.toFloat() / n) * 2f * PI.toFloat() - PI.toFloat() / 2f
            val behind = ((head - i) % n + n) % n
            val alpha = if (phase == null) {
                if (i % 4 == 0) 0.9f else 0.35f
            } else {
                val k = 1f - behind / n
                (k * k).coerceIn(0.12f, 1f)
            }
            drawCircle(
                color.copy(alpha = alpha),
                dot,
                Offset(center.x + cos(angle) * radius, center.y + sin(angle) * radius),
            )
        }
    }
}

/** 前台时每 30s 更新一次“现在”（只改本地时钟，用于相对时间；后台不运行）。 */
@Composable
private fun rememberNowSeconds(): State<Long> {
    val owner = LocalLifecycleOwner.current
    val now = remember { mutableLongStateOf(System.currentTimeMillis() / 1_000) }
    LaunchedEffect(owner) {
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                now.longValue = System.currentTimeMillis() / 1_000
                delay(CLOCK_TICK_MS)
            }
        }
    }
    return now
}

// ── 动作 ────────────────────────────────────────────────────────────────

/**
 * 订阅动作：只读拦截 + 触感 + toast。命令在应用级作用域里执行（离开本页不取消，结果仍有回执）。
 * [busy] = 正在抓取的订阅（行内显示点阵轨道，慢请求的反馈）。
 */
@Stable
private class RssController(
    private val scope: CoroutineScope,
    private val session: () -> HostSession,
    private val store: HostStore,
    private val overlays: FluxOverlayState,
    private val haptics: FluxHaptics,
    private val context: Context,
    private val actions: TaskActions,
) {
    val busy = SnapshotStateSet<String>()

    private fun s(id: Int, vararg args: Pair<String, Any?>) = context.str(id, *args)

    private fun guard(): Boolean {
        if (!store.state.value.isReadOnly) return true
        haptics.reject()
        overlays.toast(s(R.string.localServiceDisconnected), FluxToastKind.Error, FluxIcons.WifiOff)
        return false
    }

    /** @return true = 抓取成功。 */
    private suspend fun fetch(source: RssSource): Boolean {
        busy.add(source.sourceId)
        return try {
            session().refreshRssSource(source.sourceId)
            true
        } catch (e: HostException) {
            haptics.reject()
            overlays.toast(actions.errorText(e), FluxToastKind.Error)
            false
        } finally {
            busy.remove(source.sourceId)
        }
    }

    fun refresh(source: RssSource) {
        if (!guard() || source.sourceId in busy) return
        scope.launch {
            if (fetch(source)) {
                overlays.toast(s(R.string.mobileRssRefreshed, "name" to source.name.ifBlank { source.url }), FluxToastKind.Success)
            }
        }
    }

    fun refreshAll(sources: List<RssSource>) {
        if (!guard()) return
        val targets = sources.filter { it.enabled && it.sourceId !in busy }
        if (targets.isEmpty()) {
            overlays.toast(s(R.string.mobileRssNothingToRefresh), FluxToastKind.Info)
            return
        }
        scope.launch {
            val gate = Semaphore(REFRESH_PARALLELISM)
            val results = coroutineScope { targets.map { src -> async { gate.withPermit { fetch(src) } } }.awaitAll() }
            val ok = results.count { it }
            val failed = results.size - ok
            haptics.confirm()
            overlays.toast(
                s(R.string.mobileRssRefreshSummary, "ok" to ok, "failed" to failed),
                if (failed == 0) FluxToastKind.Success else FluxToastKind.Warn,
            )
        }
    }

    fun toggle(source: RssSource) {
        if (!guard()) return
        val enable = !source.enabled
        scope.launch {
            try {
                session().setRssSourceEnabled(source.sourceId, enable)
                haptics.confirm()
                overlays.toast(
                    s(if (enable) R.string.mobileRssEnabledToast else R.string.mobileRssDisabledToast),
                    FluxToastKind.Info,
                )
            } catch (e: HostException) {
                haptics.reject()
                overlays.toast(actions.errorText(e), FluxToastKind.Error)
            }
        }
    }
}
