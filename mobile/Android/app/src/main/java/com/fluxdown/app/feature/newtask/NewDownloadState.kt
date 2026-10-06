package com.fluxdown.app.feature.newtask

import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.fluxdown.core.host.CreateTaskRequest
import com.fluxdown.core.model.Queue
import com.fluxdown.core.model.TaskProtocol
import com.fluxdown.core.store.HostState

internal enum class ThreadMode { Auto, Preset, Custom }

/** 高级面板里已改动的分区（用于 N1 入口副标题与提示点）。 */
internal enum class AdvancedItem { Auth, Proxy, UserAgent, Cookie, Referrer, Checksum, Headers, Tls }

/** 一行自定义请求头；稳定 id 作为列表 key，输入不会因重排丢焦点。 */
@Stable
internal class HeaderDraft(val id: Int) {
    var key by mutableStateOf("")
    var value by mutableStateOf("")
}

/** N2：高级选项草稿。 */
@Stable
internal class AdvancedState {
    var httpUser by mutableStateOf("")
    var httpPassword by mutableStateOf("")
    var saveSiteAuth by mutableStateOf(false)
    var proxyChoice by mutableStateOf(ProxyChoice.Follow)
    var proxyCustom by mutableStateOf("")
    var uaPreset by mutableStateOf(UaDefault)
    var userAgent by mutableStateOf("")
    var cookie by mutableStateOf("")
    var referrer by mutableStateOf("")
    var checksumAlgo by mutableStateOf(DefaultHashAlgorithm)
    var checksumHex by mutableStateOf("")
    val headers = mutableStateListOf<HeaderDraft>()
    var ignoreTls by mutableStateOf(false)
    private var nextHeaderId = 0

    fun addHeader() {
        headers += HeaderDraft(nextHeaderId++)
    }

    /** 按“当前会被提交”的口径列出已改动分区；[single] = 本次只有一条链接（单条专属项才计入）。 */
    fun modified(single: Boolean, singleHttp: Boolean): List<AdvancedItem> = buildList {
        if (singleHttp && (httpUser.isNotBlank() || httpPassword.isNotEmpty())) add(AdvancedItem.Auth)
        if (proxyChoice != ProxyChoice.Follow) add(AdvancedItem.Proxy)
        if (userAgent.isNotBlank()) add(AdvancedItem.UserAgent)
        if (cookie.isNotBlank()) add(AdvancedItem.Cookie)
        if (referrer.isNotBlank()) add(AdvancedItem.Referrer)
        if (single && checksumHex.isNotBlank()) add(AdvancedItem.Checksum)
        if (headers.any { it.key.isNotBlank() }) add(AdvancedItem.Headers)
        if (ignoreTls) add(AdvancedItem.Tls)
    }

    fun reset() {
        httpUser = ""
        httpPassword = ""
        saveSiteAuth = false
        proxyChoice = ProxyChoice.Follow
        proxyCustom = ""
        uaPreset = UaDefault
        userAgent = ""
        cookie = ""
        referrer = ""
        checksumAlgo = DefaultHashAlgorithm
        checksumHex = ""
        headers.clear()
        ignoreTls = false
    }

    fun headerMap(): Map<String, String> =
        headers.filter { it.key.isNotBlank() }.associate { it.key.trim() to it.value.trim() }
}

/** N1 表单状态。用 [create] 以主机配置作默认值；每次打开 Sheet 新建一份。 */
@Stable
internal class NewDownloadState(
    prefill: String,
    defaultSaveDir: String,
    defaultQueueId: String,
    defaultSegments: Int,
) {
    var urlText by mutableStateOf(prefill)
    var saveDir by mutableStateOf(defaultSaveDir)
    var rename by mutableStateOf("")
    var queueId by mutableStateOf(defaultQueueId)
    var threadMode by mutableStateOf(
        when {
            defaultSegments <= 0 -> ThreadMode.Auto
            defaultSegments in ThreadPresets -> ThreadMode.Preset
            else -> ThreadMode.Custom
        },
    )
    var presetThreads by mutableIntStateOf(if (defaultSegments in ThreadPresets) defaultSegments else 8)
    var customThreads by mutableIntStateOf(defaultSegments.coerceIn(1, MaxThreads))
    var submitting by mutableStateOf(false)

    /** 点了提交但无有效链接：把空输入也标红。 */
    var showEmptyError by mutableStateOf(false)
    val advanced = AdvancedState()

    private val initialSaveDir = defaultSaveDir
    private val initialQueueId = defaultQueueId

    val entries: List<UrlEntry> by derivedStateOf { parseEntries(urlText).dedupe() }
    val single: Boolean by derivedStateOf { entries.size == 1 }
    val singleHttp: Boolean by derivedStateOf {
        entries.singleOrNull()?.let { protocolOf(it.url).let { p -> p == TaskProtocol.Http || p == TaskProtocol.Hls } } == true
    }

    /** 全部是磁力 / eD2K：线程数不适用。 */
    val threadsApplicable: Boolean by derivedStateOf {
        entries.isEmpty() || entries.any { protocolOf(it.url).let { p -> p != TaskProtocol.Bt && p != TaskProtocol.Ed2k } }
    }

    val segments: Int
        get() = when (threadMode) {
            ThreadMode.Auto -> 0
            ThreadMode.Preset -> presetThreads
            ThreadMode.Custom -> customThreads
        }

    val saveDirValid: Boolean get() = isValidSaveDir(saveDir)

    /** 有未提交内容：关闭前需要确认。 */
    val isDirty: Boolean
        get() = urlText.isNotBlank() || rename.isNotBlank() || saveDir != initialSaveDir || queueId != initialQueueId ||
            advanced.modified(single = true, singleHttp = true).isNotEmpty()

    /** 追加文本（粘贴 / 导入）；已有内容逐字保留。 */
    fun appendText(text: String) {
        val t = text.trim()
        if (t.isEmpty()) return
        urlText = if (urlText.isBlank()) t else urlText.trimEnd() + "\n" + t
    }

    /** 为每条链接构造请求。单条：重命名优先于 `out=`、面板校验值优先于 `checksum=`、附带 HTTP 认证。 */
    fun buildRequests(startPaused: Boolean, queue: String, manualProxy: String): List<CreateTaskRequest> {
        val list = entries
        val single = list.size == 1
        val adv = advanced
        val proxy = adv.proxyChoice.wire(manualProxy, adv.proxyCustom)
        val headers = adv.headerMap()
        val panelChecksum = if (single) checksumSpec(adv.checksumAlgo, adv.checksumHex) else ""
        return list.map { e ->
            val authOk = single && singleHttp
            CreateTaskRequest(
                url = e.url,
                fileName = if (single && rename.isNotBlank()) rename.trim() else e.fileName,
                saveDir = saveDir.trim(),
                segments = if (threadsApplicable) segments else 0,
                queueId = queue,
                startPaused = startPaused,
                cookies = adv.cookie.trim(),
                referrer = adv.referrer.trim(),
                userAgent = adv.userAgent.trim(),
                proxyUrl = proxy,
                checksum = panelChecksum.ifEmpty { e.checksum },
                ignoreTlsErrors = adv.ignoreTls,
                headers = headers,
                httpUser = if (authOk) adv.httpUser.trim() else "",
                httpPassword = if (authOk) adv.httpPassword else "",
                saveSiteAuth = authOk && adv.saveSiteAuth,
            )
        }
    }

    /** 已成功创建的链接从文本框移除（部分失败时保留失败项以便重试，避免重复创建）。 */
    fun removeUrls(done: Set<String>) {
        if (done.isEmpty()) return
        val remaining = entries.filter { it.url !in done }
        urlText = remaining.joinToString("\n") { it.toText() }
    }

    companion object {
        fun create(prefill: String, state: HostState): NewDownloadState {
            val cfg = state.config
            val queueId = cfg["default_queue_id"]?.takeIf { id -> state.queues.any { it.queueId == id } }
                ?: state.queues.firstOrNull { it.queueId == Queue.MAIN }?.queueId
                ?: state.queues.firstOrNull()?.queueId
                ?: ""
            return NewDownloadState(
                prefill = prefill.trim(),
                defaultSaveDir = cfg["default_save_dir"].orEmpty().trim(),
                defaultQueueId = queueId,
                defaultSegments = cfg["default_segments"]?.toIntOrNull() ?: 0,
            )
        }
    }
}
