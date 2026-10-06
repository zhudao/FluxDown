package com.fluxdown.app.feature.settings

import androidx.annotation.StringRes
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.foundation.layout.Column
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.platform.LocalContext
import com.fluxdown.fluxui.controls.FluxFieldAction
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.animation.animateContentSize
import com.fluxdown.app.R
import com.fluxdown.app.actions.LocalTaskActions
import com.fluxdown.app.i18n.fill
import com.fluxdown.app.i18n.str
import com.fluxdown.app.nav.LocalNavigator
import com.fluxdown.app.shell.LocalAppContainer
import com.fluxdown.app.shell.hostState
import com.fluxdown.app.ui.label
import com.fluxdown.fluxui.chrome.FluxPill
import com.fluxdown.fluxui.controls.FluxField
import com.fluxdown.fluxui.controls.FluxFieldRow
import com.fluxdown.fluxui.controls.FluxListRow
import com.fluxdown.fluxui.controls.FluxNumberField
import com.fluxdown.fluxui.controls.FluxSelect
import com.fluxdown.fluxui.controls.FluxStepperRow
import com.fluxdown.fluxui.controls.FluxSwitchRow
import com.fluxdown.fluxui.controls.GlassSection
import com.fluxdown.fluxui.controls.GlassSectionScope
import com.fluxdown.fluxui.controls.SelectOption
import com.fluxdown.fluxui.icons.FluxIcons
import com.fluxdown.fluxui.material.rememberFlowInGate
import com.fluxdown.fluxui.overlay.FluxBanner
import com.fluxdown.fluxui.overlay.FluxBannerKind
import com.fluxdown.fluxui.overlay.FluxDialogButton
import com.fluxdown.fluxui.overlay.FluxDialogButtonStyle
import com.fluxdown.fluxui.overlay.FluxDialogSpec
import com.fluxdown.fluxui.overlay.FluxPortal
import com.fluxdown.fluxui.overlay.FluxSheet
import com.fluxdown.fluxui.overlay.FluxSheetHeader
import com.fluxdown.fluxui.overlay.LocalFluxOverlays
import com.fluxdown.fluxui.controls.FluxIconButton
import com.fluxdown.fluxui.controls.IconButtonSize
import com.fluxdown.fluxui.theme.FluxText
import com.fluxdown.fluxui.theme.FluxTheme
import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToLong

/** 全局 User-Agent 预设（与 PC 端同一份取值）。 */
private class UaPreset(val id: String, @StringRes val label: Int, val ua: String)

private val UaPresets = listOf(
    UaPreset(
        "chrome", R.string.userAgentPresetChrome,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
    ),
    UaPreset(
        "firefox", R.string.userAgentPresetFirefox,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:147.0) Gecko/20100101 Firefox/147.0",
    ),
    UaPreset(
        "edge", R.string.userAgentPresetEdge,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.3800.70",
    ),
    UaPreset(
        "safari", R.string.userAgentPresetSafari,
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15",
    ),
)

private const val UA_DEFAULT = "default"
private const val UA_CUSTOM = "custom"

/** 一次组合内的表单视图：显示值 = 乐观值 ?: 主机值。 */
private class Form(
    val editor: ConfigEditor,
    val config: Map<String, String>,
    val readOnly: Boolean,
    val uaForceCustom: Boolean,
    val setUaForceCustom: (Boolean) -> Unit,
    val openPicker: (PickerSpec) -> Unit,
) {
    fun has(key: String) = config.containsKey(key)
    fun value(key: String): String? = editor.optimistic[key] ?: config[key]
    fun bool(key: String): Boolean = value(key).let { it == "true" || it == "1" }
    fun int(key: String, default: Int): Int = value(key)?.trim()?.toIntOrNull() ?: default
    fun long(key: String, default: Long): Long = value(key)?.trim()?.toLongOrNull() ?: default
    fun any(vararg keys: String) = keys.any { has(it) }
}

/**
 * S5 · 下载设置。字段绑定主机 `config`（仅展示 config 中存在的键）；
 * 写入：本地乐观 → 250ms 防抖 → `patchConfig`；Conflict 重试一次；失败回滚 + toast。
 * 只读（断线）时全部控件置灰，点按给出 REJECT 触感。
 */
@Composable
internal fun DownloadPage() {
    val nav = LocalNavigator.current
    val container = LocalAppContainer.current
    val overlays = LocalFluxOverlays.current
    val actions = LocalTaskActions.current
    val haptics = FluxTheme.haptics
    val hostState = hostState()
    val config by remember { derivedStateOf { hostState.value.config } }
    val readOnly by remember { derivedStateOf { hostState.value.isReadOnly } }
    val queues by remember { derivedStateOf { hostState.value.queues } }
    val disconnected = str(R.string.localServiceDisconnected)
    val editor = remember(container, overlays, haptics, actions, disconnected) {
        ConfigEditor(container, overlays, haptics, actions::errorText, disconnected)
    }
    var picker by remember { mutableStateOf<PickerSpec?>(null) }
    var showNicHelp by remember { mutableStateOf(false) }
    val gate = rememberFlowInGate()
    val motion = FluxTheme.motion
    var uaForceCustom by rememberSaveable { mutableStateOf(false) }
    val form = Form(editor, config, readOnly, uaForceCustom, { uaForceCustom = it }) { picker = it }
    val animateSize = Modifier.animateContentSize(motion.of(motion.fluid))

    Box(Modifier.fillMaxSize()) {
        SettingsPageFrame(
            title = str(R.string.settingsCatDownload),
            onBack = { nav.pop() },
            modifier = Modifier.pointerInput(readOnly) {
                if (readOnly) detectTapGestures(onTap = { haptics.reject() })
            },
        ) {
            var index = 0
            if (readOnly) {
                flowItem(index++, gate, "banner") {
                    FluxBanner(str(R.string.localServiceDisconnected), kind = FluxBannerKind.Warn, slim = true)
                }
            }
            if (form.any("default_save_dir", "download.remember_last_save_dir")) {
                flowItem(index++, gate, "save") {
                    GlassSection(animateSize, title = str(R.string.settingsGroupSaveLocation)) {
                        saveLocationRows(form)
                    }
                }
            }
            if (form.any(
                    "download.notify_on_complete", "download.silent_download", "use_server_time",
                    "file_exists_behavior", "file_missing_action", "idle_file_scan", "default_queue_id", "dedup_same_url",
                )
            ) {
                flowItem(index++, gate, "behavior") {
                    GlassSection(animateSize, title = str(R.string.settingsGroupBehavior)) {
                        behaviorRows(form, queues)
                    }
                }
            }
            if (form.any(
                    "default_segments", "cdn_multi_enabled", "multi_nic_enabled",
                    "max_concurrent_tasks", "speed_limit_bytes", "upload_limit_bytes",
                )
            ) {
                flowItem(index++, gate, "connection") {
                    GlassSection(animateSize, title = str(R.string.settingsGroupConnection)) {
                        connectionRows(form, onNicHelp = { showNicHelp = true })
                    }
                }
            }
            if (form.any("max_auto_retries", "auto_retry_delay_secs", "auto_resume_on_start")) {
                flowItem(index++, gate, "retry") {
                    GlassSection(animateSize, title = str(R.string.settingsGroupRetry)) {
                        retryRows(form)
                    }
                }
            }
            if (form.has("global_user_agent")) {
                flowItem(index++, gate, "advanced") {
                    GlassSection(animateSize, title = str(R.string.settingsGroupAdvanced)) {
                        advancedRows(form)
                    }
                }
            }
        }
        PickerSheetHost(picker = picker, onDismiss = { picker = null })
        NicHelpSheet(visible = showNicHelp, onDismiss = { showNicHelp = false })
    }
}

// ───────────────────────────── 分组 ─────────────────────────────

private fun GlassSectionScope.saveLocationRows(f: Form) {
    if (f.has("default_save_dir")) {
        row {
            val container = LocalAppContainer.current
            val hostRef by container.host.collectAsStateWithLifecycle()
            FluxFieldRow(
                title = str(R.string.defaultSaveDir),
                subtitle = str(R.string.defaultSaveDirDesc),
            ) {
                if (hostRef is com.fluxdown.core.model.HostRef.Local) {
                    LocalSaveDirPicker(f)
                } else {
                    // 远端主机：路径在服务器上，只能手填（手机无法浏览服务器目录）
                    CommitTextField(
                        value = f.value("default_save_dir").orEmpty(),
                        onCommit = { f.editor.set("default_save_dir", it.trim()) },
                        enabled = !f.readOnly,
                        placeholder = str(R.string.mobileSaveDirUnset),
                    )
                }
            }
        }
    }
    switchRow(f, "download.remember_last_save_dir", R.string.rememberLastSaveDir, R.string.rememberLastSaveDirDesc)
}

/**
 * 本机主机的保存目录：只读展示 + 系统目录选择器（SAF，映射为绝对路径）+ 快捷“系统 Download/FluxDown”
 * + 偏离默认时一键恢复。不提供手填：手填路径极易写成不可写目录（引擎下载时才报 EPERM）。
 *
 * 能否写入只看系统真实结果（[isLocalDirWritable]）；共享存储里需要“所有文件访问”的目录，
 * 不可写时引导授权，而不是在应用层另设白名单。系统选择器本身禁止选 Download 根 / Android/data
 * 等目录（Android 11+ 隐私限制），可在其下新建子文件夹选择，或直接用快捷项。
 */
@Composable
private fun LocalSaveDirPicker(f: Form) {
    val context = LocalContext.current
    val overlays = LocalFluxOverlays.current
    val haptics = com.fluxdown.fluxui.theme.FluxTheme.haptics
    val current = f.value("default_save_dir").orEmpty()
    val defaultDir = remember(context) { com.fluxdown.app.ui.defaultLocalSaveDir(context).path }
    val publicDir = remember { com.fluxdown.app.ui.publicDownloadSaveDir().path }
    val notWritable = str(R.string.mobileSaveDirNotWritable)
    val unmappable = str(R.string.mobilePickDirUnmappable)
    val grantTitle = str(R.string.mobileAllFilesTitle)
    val grantDesc = str(R.string.mobileAllFilesDescNative)
    val grantAction = str(R.string.mobileGoGrant)
    val cancel = str(R.string.cancel)

    fun apply(dir: String) {
        when {
            com.fluxdown.app.ui.isLocalDirWritable(dir) -> if (dir != current) {
                haptics.tick()
                f.editor.set("default_save_dir", dir)
            }
            com.fluxdown.app.ui.canRequestAllFilesAccess(dir) -> {
                haptics.reject()
                overlays.showDialog(
                    com.fluxdown.fluxui.overlay.FluxDialogSpec(
                        title = grantTitle,
                        message = grantDesc,
                        icon = FluxIcons.FolderOpen,
                        buttons = listOf(
                            com.fluxdown.fluxui.overlay.FluxDialogButton(cancel),
                            com.fluxdown.fluxui.overlay.FluxDialogButton(grantAction, com.fluxdown.fluxui.overlay.FluxDialogButtonStyle.Primary) {
                                context.startActivity(com.fluxdown.app.ui.allFilesAccessIntent(context))
                            },
                        ),
                    ),
                )
            }
            else -> {
                haptics.reject()
                overlays.toast(notWritable.fill("dir" to dir), com.fluxdown.fluxui.overlay.FluxToastKind.Error)
            }
        }
    }

    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        val path = com.fluxdown.app.ui.treeUriToPath(uri)
        if (path == null) {
            haptics.reject()
            overlays.toast(unmappable, com.fluxdown.fluxui.overlay.FluxToastKind.Warn)
        } else {
            apply(path)
        }
    }
    val shown = current.ifEmpty { defaultDir }.trimEnd('/')
    val modified = shown != defaultDir.trimEnd('/')

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FluxField(
            value = shown,
            onValueChange = {},
            readOnly = true,
            mono = true,
            singleLine = false,
            enabled = !f.readOnly,
            trailing = {
                Row {
                    if (modified) {
                        FluxFieldAction(FluxIcons.RotateCcw, str(R.string.restoreDefaultPath), onClick = { apply(defaultDir) })
                    }
                    FluxFieldAction(FluxIcons.FolderOpen, str(R.string.browse), onClick = { launcher.launch(null) })
                }
            },
        )
        if (shown != publicDir && !f.readOnly) {
            com.fluxdown.fluxui.chrome.FluxPill(
                text = str(R.string.mobileUsePublicDownloadDir),
                selected = false,
                onClick = { apply(publicDir) },
                icon = FluxIcons.Download,
                small = true,
                toggleable = false,
            )
        }
    }
}

private fun GlassSectionScope.behaviorRows(f: Form, queues: List<com.fluxdown.core.model.Queue>) {
    switchRow(f, "download.notify_on_complete", R.string.notifyOnComplete, R.string.notifyOnCompleteDesc)
    switchRow(f, "download.silent_download", R.string.silentDownload, R.string.silentDownloadDesc)
    if (f.bool("download.silent_download")) {
        switchRow(f, "silent_skip_selection", R.string.silentSkipSelection, R.string.silentSkipSelectionDesc)
    }
    switchRow(f, "use_server_time", R.string.useServerTime, R.string.useServerTimeDesc)
    pickerRow(
        f, "file_exists_behavior", R.string.fileExistsBehavior, R.string.fileExistsBehaviorDesc, "rename",
        options = {
            listOf(
                SelectOption("rename", str(R.string.fileExistsRename)),
                SelectOption("overwrite", str(R.string.fileExistsOverwrite)),
                SelectOption("skip", str(R.string.fileExistsSkip)),
            )
        },
    )
    pickerRow(
        f, "file_missing_action", R.string.fileMissingAction, R.string.fileMissingActionDesc, "keep",
        options = {
            listOf(
                SelectOption("keep", str(R.string.fileMissingKeep)),
                SelectOption("delete", str(R.string.fileMissingDelete)),
            )
        },
    )
    switchRow(f, "idle_file_scan", R.string.idleFileScan, R.string.idleFileScanDesc)
    pickerRow(
        f, "default_queue_id", R.string.defaultQueueSetting, R.string.defaultQueueSettingDesc, "",
        options = {
            listOf(SelectOption("", str(R.string.defaultQueue))) +
                queues.map { SelectOption(it.queueId, it.label()) }
        },
    )
    switchRow(f, "dedup_same_url", R.string.mobileDedupSameUrl, R.string.mobileDedupSameUrlDesc)
}

private fun GlassSectionScope.connectionRows(f: Form, onNicHelp: () -> Unit) {
    val segments = f.int("default_segments", 0)
    stepperRow(f, "default_segments", R.string.defaultThreads, R.string.defaultThreadsDesc, 0..64, 0, zeroLabel = R.string.auto)
    if (f.has("auto_max_connections") && f.has("default_segments") && segments == 0) {
        stepperRow(f, "auto_max_connections", R.string.autoMaxConnections, R.string.autoMaxConnectionsDesc, 0..128, 16)
    }
    if (f.has("cdn_multi_enabled")) {
        row {
            val system = str(R.string.cdnMultiProxyConfirmDescSystem)
            val manual = str(R.string.cdnMultiProxyConfirmDescManual)
            val title = str(R.string.cdnMultiProxyConfirmTitle)
            val disable = str(R.string.cdnMultiProxyConfirmDisable)
            val cancel = str(R.string.cancel)
            val overlays = LocalFluxOverlays.current
            FluxSwitchRow(
                title = str(R.string.cdnMultiEnabled),
                subtitle = str(R.string.cdnMultiEnabledDesc),
                checked = f.bool("cdn_multi_enabled"),
                enabled = !f.readOnly,
                onCheckedChange = { on ->
                    val proxy = f.value("proxy_mode")
                    if (on && (proxy == "system" || proxy == "manual")) {
                        overlays.showDialog(
                            FluxDialogSpec(
                                title = title,
                                message = if (proxy == "system") system else manual,
                                icon = FluxIcons.TriangleAlert,
                                buttons = listOf(
                                    FluxDialogButton(cancel, FluxDialogButtonStyle.Secondary),
                                    FluxDialogButton(disable, FluxDialogButtonStyle.Primary) {
                                        f.editor.setNow(mapOf("cdn_multi_enabled" to "true", "proxy_mode" to "none"))
                                    },
                                ),
                            ),
                        )
                    } else {
                        f.editor.set("cdn_multi_enabled", on.toString())
                    }
                },
            )
        }
        if (f.bool("cdn_multi_enabled")) {
            stepperRow(f, "cdn_max_nodes", R.string.cdnMaxNodes, R.string.cdnMaxNodesDesc, 0..8, 0, zeroLabel = R.string.auto)
        }
    }
    if (f.has("multi_nic_enabled")) {
        switchRow(f, "multi_nic_enabled", R.string.multiNicEnabled, R.string.multiNicEnabledDesc)
        row(hasIcon = true) {
            FluxListRow(
                title = str(R.string.multiNicHelpHint),
                icon = FluxIcons.Info,
                chevron = true,
                onClick = onNicHelp,
            )
        }
    }
    if (f.has("max_concurrent_tasks")) {
        row {
            val template = stringResource(R.string.mobileAdjustedTo)
            FluxListRow(
                title = str(R.string.maxConcurrent),
                subtitle = str(R.string.maxConcurrentDesc),
                trailing = {
                    FluxNumberField(
                        value = f.long("max_concurrent_tasks", 5L),
                        onValueChange = { f.editor.set("max_concurrent_tasks", it.toString()) },
                        range = 1L..1024L,
                        modifier = Modifier.width(112.dp),
                        enabled = !f.readOnly,
                        adjustedHint = { template.fill("n" to it) },
                    )
                },
            )
        }
    }
    speedRow(f, "speed_limit_bytes", R.string.speedLimit, R.string.speedLimitDesc)
    speedRow(f, "upload_limit_bytes", R.string.uploadLimit, R.string.uploadLimitDesc)
}

private fun GlassSectionScope.retryRows(f: Form) {
    stepperRow(
        f, "max_auto_retries", R.string.autoRetryCount, R.string.autoRetryCountDesc, -1..20, 3,
        zeroLabel = R.string.autoRetryOff, minusOneLabel = R.string.autoRetryUnlimited,
    )
    if (f.has("auto_retry_delay_secs")) {
        row {
            val template = stringResource(R.string.mobileAdjustedTo)
            FluxFieldRow(title = str(R.string.autoRetryDelay), subtitle = str(R.string.autoRetryDelayDesc)) {
                FluxNumberField(
                    value = f.long("auto_retry_delay_secs", 5L),
                    onValueChange = { f.editor.set("auto_retry_delay_secs", it.toString()) },
                    range = 0L..86_400L,
                    hint = str(R.string.autoRetryDelayUnit),
                    enabled = !f.readOnly,
                    adjustedHint = { template.fill("n" to it) },
                )
            }
        }
    }
    switchRow(f, "auto_resume_on_start", R.string.autoResumeOnStart, R.string.autoResumeOnStartDesc)
}

private fun GlassSectionScope.advancedRows(f: Form) {
    val key = "global_user_agent"
    val current = f.value(key).orEmpty()
    val matched = UaPresets.firstOrNull { it.ua == current }?.id
        ?: if (current.isEmpty()) UA_DEFAULT else UA_CUSTOM
    val effective = if (f.uaForceCustom) UA_CUSTOM else matched
    row {
        val title = str(R.string.userAgent)
        val options = buildList {
            add(SelectOption(UA_DEFAULT, str(R.string.userAgentPresetDefault)))
            UaPresets.forEach { add(SelectOption(it.id, str(it.label), hint = it.ua)) }
            add(SelectOption(UA_CUSTOM, str(R.string.userAgentPresetCustom)))
        }
        FluxListRow(
            title = title,
            subtitle = str(R.string.userAgentDesc),
            value = options.firstOrNull { it.value == effective }?.label,
            chevron = true,
            enabled = !f.readOnly,
            onClick = {
                f.openPicker(
                    PickerSpec(title, options, effective) { id ->
                        when (id) {
                            UA_DEFAULT -> {
                                f.setUaForceCustom(false)
                                f.editor.set(key, "")
                            }
                            UA_CUSTOM -> f.setUaForceCustom(true)
                            else -> {
                                f.setUaForceCustom(false)
                                UaPresets.firstOrNull { it.id == id }?.let { f.editor.set(key, it.ua) }
                            }
                        }
                    },
                )
            },
        )
    }
    if (effective == UA_CUSTOM) {
        row {
            FluxFieldRow(title = str(R.string.userAgentPresetCustom)) {
                CommitTextField(
                    value = current,
                    onCommit = { f.editor.set(key, it.trim()) },
                    enabled = !f.readOnly,
                    placeholder = str(R.string.userAgentPlaceholder),
                )
            }
        }
    }
}

// ───────────────────────────── 行构件 ─────────────────────────────

private fun GlassSectionScope.switchRow(f: Form, key: String, @StringRes title: Int, @StringRes desc: Int) {
    if (!f.has(key)) return
    row {
        FluxSwitchRow(
            title = str(title),
            subtitle = str(desc),
            checked = f.bool(key),
            enabled = !f.readOnly,
            onCheckedChange = { f.editor.set(key, it.toString()) },
        )
    }
}

private fun GlassSectionScope.stepperRow(
    f: Form,
    key: String,
    @StringRes title: Int,
    @StringRes desc: Int,
    range: IntRange,
    default: Int,
    @StringRes zeroLabel: Int? = null,
    @StringRes minusOneLabel: Int? = null,
) {
    if (!f.has(key)) return
    row {
        val zero = zeroLabel?.let { str(it) }
        val minusOne = minusOneLabel?.let { str(it) }
        FluxStepperRow(
            title = str(title),
            subtitle = str(desc),
            value = f.int(key, default).coerceIn(range),
            onValueChange = { f.editor.set(key, it.toString()) },
            range = range,
            format = { v ->
                when {
                    v == 0 && zero != null -> zero
                    v == -1 && minusOne != null -> minusOne
                    else -> v.toString()
                }
            },
            editable = true,
            enabled = !f.readOnly,
        )
    }
}

private fun GlassSectionScope.pickerRow(
    f: Form,
    key: String,
    @StringRes title: Int,
    @StringRes desc: Int,
    default: String,
    options: @Composable () -> List<SelectOption<String>>,
) {
    if (!f.has(key)) return
    row {
        val items = options()
        val current = f.value(key) ?: default
        val name = str(title)
        FluxListRow(
            title = name,
            subtitle = str(desc),
            value = items.firstOrNull { it.value == current }?.label ?: current,
            chevron = true,
            enabled = !f.readOnly,
            onClick = { f.openPicker(PickerSpec(name, items, current) { f.editor.set(key, it) }) },
        )
    }
}

private fun GlassSectionScope.speedRow(f: Form, key: String, @StringRes title: Int, @StringRes desc: Int) {
    if (!f.has(key)) return
    row {
        val name = str(title)
        FluxFieldRow(title = name, subtitle = str(desc)) {
            SpeedLimitControl(
                bytes = f.long(key, 0L),
                onChange = { f.editor.set(key, it.toString()) },
                enabled = !f.readOnly,
                name = name,
                openPicker = f.openPicker,
            )
        }
    }
}

// ───────────────────────────── 文本 / 限速控件 ─────────────────────────────

/** 失焦 / 完成键 / 离开页面时提交的文本框（路径、UA 等不宜逐字写入主机的字段）。 */
@Composable
private fun CommitTextField(
    value: String,
    onCommit: (String) -> Unit,
    enabled: Boolean,
    placeholder: String?,
) {
    var draft by remember(value) { mutableStateOf(value) }
    val latestDraft by rememberUpdatedState(draft)
    val latestValue by rememberUpdatedState(value)
    val latestCommit by rememberUpdatedState(onCommit)
    val focusManager = LocalFocusManager.current

    DisposableEffect(Unit) {
        onDispose { if (latestDraft != latestValue) latestCommit(latestDraft) }
    }
    FluxField(
        value = draft,
        onValueChange = { draft = it },
        placeholder = placeholder,
        mono = true,
        enabled = enabled,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = {
            if (draft != value) onCommit(draft)
            focusManager.clearFocus()
        }),
        onFocusChange = { focused -> if (!focused && draft != value) onCommit(draft) },
    )
}

private enum class SpeedUnit(val label: String, val factor: Long) {
    KB("KB/s", 1024L),
    MB("MB/s", 1024L * 1024L),
    GB("GB/s", 1024L * 1024L * 1024L),
}

/** 快捷档位：0 = 不限制，其余 (数值, 单位)。 */
private val SpeedPresets: List<Pair<Int, SpeedUnit>> = listOf(
    0 to SpeedUnit.MB,
    512 to SpeedUnit.KB,
    1 to SpeedUnit.MB,
    2 to SpeedUnit.MB,
    5 to SpeedUnit.MB,
    10 to SpeedUnit.MB,
    20 to SpeedUnit.MB,
)

/** 在 [unit] 中最多 2 位小数即可精确表示。 */
private fun representable(bytes: Long, unit: SpeedUnit): Boolean {
    return speedBytes(speedText(bytes, unit), unit) == bytes
}

/** 首选单位能精确表示就用它；否则取能精确表示的最大单位；都不行回落 KB/s。 */
private fun effectiveUnit(bytes: Long, preferred: SpeedUnit): SpeedUnit {
    if (bytes <= 0L || representable(bytes, preferred)) return preferred
    return listOf(SpeedUnit.GB, SpeedUnit.MB, SpeedUnit.KB).firstOrNull { representable(bytes, it) } ?: SpeedUnit.KB
}

private fun speedText(bytes: Long, unit: SpeedUnit): String {
    if (bytes <= 0L) return ""
    val s = String.format(Locale.ROOT, "%.2f", bytes.toDouble() / unit.factor).trimEnd('0').trimEnd('.')
    return s.ifEmpty { "0" }
}

private fun speedBytes(text: String, unit: SpeedUnit): Long {
    val v = text.trim().replace(',', '.').toDoubleOrNull() ?: return 0L
    val raw = v.coerceAtLeast(0.0) * unit.factor
    return if (raw >= Long.MAX_VALUE.toDouble()) Long.MAX_VALUE else raw.roundToLong()
}

/** 字段 + 单位选择 + 快捷档位；基数 1024，两位小数；0 = 不限制。单位仅在本页会话内记忆。 */
@Composable
private fun SpeedLimitControl(
    bytes: Long,
    onChange: (Long) -> Unit,
    enabled: Boolean,
    name: String,
    openPicker: (PickerSpec) -> Unit,
) {
    var preferred by rememberSaveable(name) { mutableStateOf(SpeedUnit.MB) }
    val unit = effectiveUnit(bytes, preferred)
    var draft by remember(bytes, unit) { mutableStateOf(speedText(bytes, unit)) }
    val focusManager = LocalFocusManager.current
    val unitTitle = str(R.string.mobileSpeedUnit)
    val unlimited = str(R.string.mobileSpeedUnlimited)

    fun commit(text: String, u: SpeedUnit) {
        val next = speedBytes(text, u)
        if (next != bytes) onChange(next)
    }

    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FluxField(
            value = draft,
            onValueChange = { s ->
                var dot = false
                draft = s.filter { ch ->
                    when {
                        ch.isDigit() -> true
                        (ch == '.' || ch == ',') && !dot -> { dot = true; true }
                        else -> false
                    }
                }.take(12)
            },
            modifier = Modifier.weight(1f),
            placeholder = unlimited,
            mono = true,
            enabled = enabled,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = {
                commit(draft, unit)
                focusManager.clearFocus()
            }),
            onFocusChange = { focused -> if (!focused) commit(draft, unit) },
        )
        FluxSelect(
            value = unit.label,
            onClick = {
                openPicker(
                    PickerSpec(
                        title = unitTitle,
                        options = SpeedUnit.entries.map { SelectOption(it.name, it.label) },
                        selected = unit.name,
                        onSelect = { id ->
                            val picked = SpeedUnit.valueOf(id)
                            preferred = picked
                            // 切换单位保留已输入的数字
                            commit(draft, picked)
                        },
                    ),
                )
            },
            modifier = Modifier.width(120.dp),
            placeholder = unit.label,
            enabled = enabled,
        )
    }
    Row(
        Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        for ((n, u) in SpeedPresets) {
            val value = n * u.factor
            FluxPill(
                text = if (n == 0) unlimited else "$n ${u.label}",
                selected = bytes == value,
                onClick = {
                    if (n != 0) preferred = u
                    if (value != bytes) onChange(value)
                },
                small = true,
                toggleable = false,
            )
        }
    }
}

/** 多网卡聚合说明（沿用 PC 文案）。 */
@Composable
private fun NicHelpSheet(visible: Boolean, onDismiss: () -> Unit) {
    val title = str(R.string.multiNicHelpTitle)
    val close = str(R.string.close)
    FluxPortal {
        FluxSheet(
            visible = visible,
            onDismissRequest = onDismiss,
            title = title,
            header = {
                FluxSheetHeader(
                    title = title,
                    actions = { FluxIconButton(FluxIcons.X, close, onDismiss, size = IconButtonSize.Sm) },
                )
            },
        ) {
            FluxText(
                text = str(R.string.multiNicHelp),
                style = FluxTheme.type.body,
                color = FluxTheme.colors.inkMuted,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
        }
    }
}
