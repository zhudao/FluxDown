package com.fluxdown.core.protocol

/*
 * `daemon.rss.*` 订阅编辑所需的 wire 类型（镜像 `native/protocol/src/daemon.rs` 的 `RssSourceDto` /
 * `RssValidate*`，同 iOS `FluxDomain/Protocol/Rss.swift`）。快照里的 `RssSource` 只是 UI 子集；
 * `daemon.rss.updateSource` 需要**完整**订阅，因此编辑前先 `listSources` 取最新副本、只改表单字段再整体写回。
 */

/** 完整订阅（`RssSourceDto`）；未列出的键（含只读运行态）原样保留在 [extra] 中写回。 */
data class RssSourceDetail(
    val sourceId: String = "",
    val url: String,
    val name: String = "",
    val enabled: Boolean = true,
    val autoDownload: Boolean = true,
    val startPaused: Boolean = false,
    /** 空 = 内置主队列。 */
    val queueId: String = "",
    /** 空 = 队列目录 → 全局目录。 */
    val saveDir: String = "",
    /** 0 = 引擎默认 30。 */
    val intervalMinutes: Int = 0,
    val includePattern: String = "",
    val excludePattern: String = "",
    val useRegex: Boolean = false,
    val smartEpisode: Boolean = false,
    /** 字节，0 = 不限。 */
    val sizeMinBytes: Long = 0,
    val sizeMaxBytes: Long = 0,
    val sendReferer: Boolean = true,
    val notifyOnDownload: Boolean = true,
    /** 1..100；0 = 引擎默认 20。 */
    val maxPerFetch: Int = 0,
    val cookies: String = "",
    val userAgent: String = "",
    val proxyUrl: String = "",
    /** 原始对象（providerId / providerConfig / 运行态等），写回时作为底稿。 */
    val extra: Map<String, JsonValue> = emptyMap(),
) {
    val effectiveIntervalMinutes: Int get() = if (intervalMinutes > 0) intervalMinutes else DEFAULT_INTERVAL_MINUTES
    val effectiveMaxPerFetch: Int get() = if (maxPerFetch > 0) maxPerFetch else DEFAULT_MAX_PER_FETCH

    fun toJson(): JsonValue.Obj {
        val out = LinkedHashMap(extra)
        if (!out.containsKey("providerId")) out["providerId"] = JsonValue.Str(BUILTIN_PROVIDER_ID)
        if (!out.containsKey("providerConfig")) out["providerConfig"] = JsonValue.Str("")
        out["sourceId"] = JsonValue.Str(sourceId)
        out["url"] = JsonValue.Str(url)
        out["name"] = JsonValue.Str(name)
        out["enabled"] = JsonValue.of(enabled)
        out["autoDownload"] = JsonValue.of(autoDownload)
        out["startPaused"] = JsonValue.of(startPaused)
        out["queueId"] = JsonValue.Str(queueId)
        out["saveDir"] = JsonValue.Str(saveDir)
        out["intervalMinutes"] = JsonValue.of(intervalMinutes)
        out["includePattern"] = JsonValue.Str(includePattern)
        out["excludePattern"] = JsonValue.Str(excludePattern)
        out["useRegex"] = JsonValue.of(useRegex)
        out["smartEpisode"] = JsonValue.of(smartEpisode)
        out["sizeMinBytes"] = JsonValue.of(sizeMinBytes)
        out["sizeMaxBytes"] = JsonValue.of(sizeMaxBytes)
        out["sendReferer"] = JsonValue.of(sendReferer)
        out["notifyOnDownload"] = JsonValue.of(notifyOnDownload)
        out["maxPerFetch"] = JsonValue.of(maxPerFetch)
        out["cookies"] = JsonValue.Str(cookies)
        out["userAgent"] = JsonValue.Str(userAgent)
        out["proxyUrl"] = JsonValue.Str(proxyUrl)
        return JsonValue.Obj(out)
    }

    companion object {
        const val BUILTIN_PROVIDER_ID = "rss"
        const val DEFAULT_INTERVAL_MINUTES = 30
        const val DEFAULT_MAX_PER_FETCH = 20
        val MAX_PER_FETCH_RANGE = 1..100

        /** 全字段 `#[serde(default)]`（`url` 必填）：缺 `url` → null。 */
        fun fromJson(v: JsonValue?): RssSourceDetail? {
            val url = v.strOrNull("url") ?: return null
            return RssSourceDetail(
                sourceId = v.str("sourceId"),
                url = url,
                name = v.str("name"),
                enabled = v.bool("enabled", true),
                autoDownload = v.bool("autoDownload", true),
                startPaused = v.bool("startPaused", false),
                queueId = v.str("queueId"),
                saveDir = v.str("saveDir"),
                intervalMinutes = v.int("intervalMinutes"),
                includePattern = v.str("includePattern"),
                excludePattern = v.str("excludePattern"),
                useRegex = v.bool("useRegex", false),
                smartEpisode = v.bool("smartEpisode", false),
                sizeMinBytes = v.long("sizeMinBytes"),
                sizeMaxBytes = v.long("sizeMaxBytes"),
                sendReferer = v.bool("sendReferer", true),
                notifyOnDownload = v.bool("notifyOnDownload", true),
                maxPerFetch = v.int("maxPerFetch"),
                cookies = v.str("cookies"),
                userAgent = v.str("userAgent"),
                proxyUrl = v.str("proxyUrl"),
                extra = v.objOrNull.orEmpty(),
            )
        }

        fun listFromJson(v: JsonValue?): List<RssSourceDetail> = v.arrayOrNull.orEmpty().mapNotNull { fromJson(it) }
    }
}

/** `daemon.rss.validate` 参数（只读、不落库；慢方法）。 */
data class RssValidateRequest(val url: String, val cookies: String = "", val userAgent: String = "", val proxyUrl: String = "") {
    fun toJson(): JsonValue = jsonObject("url" to url, "cookies" to cookies, "userAgent" to userAgent, "proxyUrl" to proxyUrl)
}

/** `daemon.rss.validate` 结果：[error] 非空即验证失败（诊断载荷，不是传输错误）。 */
data class RssValidateResponse(val feedTitle: String, val itemCount: Int, val error: String) {
    companion object {
        fun fromJson(v: JsonValue?) = RssValidateResponse(v.str("feedTitle"), v.list("items").size, v.str("error"))
    }
}

/**
 * `200M` / `2G` / `1.5 GB` / `1024`（1024 进制，可带小数与 `B` 后缀）↔ 字节数，同 iOS `RssSizeLiteral` /
 * Web `filter.ts::parseSize/formatSize`。过滤规则本身由主机引擎求值。
 */
object RssSizeLiteral {
    private val scales = mapOf('k' to 1024.0, 'm' to 1024.0 * 1024, 'g' to 1024.0 * 1024 * 1024, 't' to 1024.0 * 1024 * 1024 * 1024)
    private val decimal = Regex("[0-9]+(\\.[0-9]+)?")

    /** 空串 / 无法解析 / 越界 → null。 */
    fun parse(input: String): Long? {
        var text = input.trim().lowercase()
        if (text.isEmpty()) return null
        if (text.last() == 'b') text = text.dropLast(1)
        var multiplier = 1.0
        text.lastOrNull()?.let { last -> scales[last]?.let { multiplier = it; text = text.dropLast(1) } }
        val number = text.trim()
        if (!decimal.matches(number)) return null
        val bytes = (number.toDoubleOrNull() ?: return null) * multiplier
        val limit = 9_223_372_036_854_775_808.0
        if (!bytes.isFinite() || bytes < 0 || bytes > limit) return null
        return if (bytes >= limit) Long.MAX_VALUE else bytes.toLong()
    }

    /** 字节数 → 字面量（≤ 0 → 空串 = 不限）；与 [parse] 往返。 */
    fun format(bytes: Long): String {
        if (bytes <= 0) return ""
        for ((scale, suffix) in listOf((1L shl 40) to "T", (1L shl 30) to "G", (1L shl 20) to "M", (1L shl 10) to "K")) {
            if (bytes % scale == 0L) return "${bytes / scale}$suffix"
        }
        return bytes.toString()
    }

    /** 编辑器输入：空 = 不限（0）；非法为 null。 */
    fun field(text: String): Long? = if (text.isBlank()) 0 else parse(text)
}

/** 每轮上限输入：1..100 的整数，否则 null。 */
fun parseRssMaxPerFetch(text: String): Int? {
    val t = text.trim()
    if (t.isEmpty() || !t.all { it in '0'..'9' }) return null
    return t.toIntOrNull()?.takeIf { it in RssSourceDetail.MAX_PER_FETCH_RANGE }
}
