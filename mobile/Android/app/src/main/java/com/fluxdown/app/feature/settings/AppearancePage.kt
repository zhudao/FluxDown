package com.fluxdown.app.feature.settings

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import com.fluxdown.fluxui.controls.FluxDivider
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.fluxdown.app.R
import com.fluxdown.app.data.AppearanceState
import com.fluxdown.app.data.ThemeMode
import com.fluxdown.app.i18n.str
import com.fluxdown.app.nav.LocalNavigator
import com.fluxdown.app.shell.LocalAppContainer
import com.fluxdown.fluxui.controls.ButtonSize
import com.fluxdown.fluxui.controls.ButtonVariant
import com.fluxdown.fluxui.controls.FluxButton
import com.fluxdown.fluxui.controls.FluxListRow
import com.fluxdown.fluxui.controls.FluxSlider
import com.fluxdown.fluxui.controls.FluxSwitchRow
import com.fluxdown.fluxui.controls.GlassSection
import com.fluxdown.fluxui.icons.FluxIcon
import com.fluxdown.fluxui.icons.FluxIcons
import com.fluxdown.fluxui.material.rememberFlowInGate
import com.fluxdown.fluxui.overlay.FluxToastKind
import com.fluxdown.fluxui.overlay.LocalFluxOverlays
import com.fluxdown.fluxui.theme.FluxText
import com.fluxdown.fluxui.theme.FluxTheme
import com.fluxdown.fluxui.theme.wallpaperAccent
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import java.io.IOException
import kotlin.math.roundToInt

/**
 * S4 · 外观（设备本地，断线不影响）：预览、语言、主题模式、主题色（含自定义取色器）、
 * 跟随壁纸取色、氛围光强度、系统文字与显示大小。所有修改经 [com.fluxdown.app.data.AppearanceRepo] 持久化，
 * FluxTheme 随 DataStore 流实时重绘。
 */
@Composable
internal fun AppearancePage() {
    val nav = LocalNavigator.current
    val container = LocalAppContainer.current
    val overlays = LocalFluxOverlays.current
    val context = LocalContext.current
    val haptics = FluxTheme.haptics
    val repo = container.appearance
    val appearance by repo.state.collectAsState(initial = AppearanceState())
    val gate = rememberFlowInGate()
    val motion = FluxTheme.motion
    val animateSize = Modifier.animateContentSize(motion.of(motion.fluid))
    val failText = str(R.string.localServiceActionFailed)
    val noApp = str(R.string.mobileNoSettingsApp)

    /** 持久化写入：失败时 toast（Error 吐司自带 REJECT 触感）。 */
    fun write(block: suspend () -> Unit) {
        container.appScope.launch {
            try {
                block()
            } catch (e: IOException) {
                overlays.toast(failText, FluxToastKind.Error)
            }
        }
    }

    // 氛围光：本地草稿即时驱动预览（读取下沉到各自的叶子里，拖动时不重组整页），200ms 防抖写入；离开页面时冲刷
    var auraDraft by remember { mutableStateOf<Int?>(null) }
    val auraState = remember { derivedStateOf { auraDraft ?: appearance.auraIntensity } }
    LaunchedEffect(Unit) {
        snapshotFlow { auraDraft }.collectLatest { d ->
            if (d != null) {
                delay(200)
                write { repo.setAuraIntensity(d) }
            }
        }
    }
    LaunchedEffect(Unit) {
        snapshotFlow { auraDraft != null && auraDraft == appearance.auraIntensity }.collect { settled ->
            if (settled) auraDraft = null
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            val d = auraDraft
            if (d != null && d != appearance.auraIntensity) {
                container.appScope.launch {
                    try {
                        repo.setAuraIntensity(d)
                    } catch (e: IOException) {
                        overlays.toast(failText, FluxToastKind.Error)
                    }
                }
            }
        }
    }

    // 自定义色：取色器每次变化 → 250ms 防抖写入（仍处于“自定义”时才写，避免覆盖随后点选的预设）
    var customDraft by remember { mutableIntStateOf(0) }
    var customDirty by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        snapshotFlow { if (customDirty) customDraft else null }.collectLatest { argb ->
            if (argb != null) {
                delay(250)
                if (appearance.scheme == "custom") write { repo.setCustomColor(argb) }
                customDirty = false
            }
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            if (customDirty && appearance.scheme == "custom") {
                val argb = customDraft
                container.appScope.launch {
                    try {
                        repo.setCustomColor(argb)
                    } catch (e: IOException) {
                        overlays.toast(failText, FluxToastKind.Error)
                    }
                }
            }
        }
    }

    SettingsPageFrame(title = str(R.string.settingsCatAppearance), onBack = { nav.pop() }) {
        flowItem(0, gate, "preview") {
            ThemePreview(auraIntensity = { auraState.value })
        }
        if (Build.VERSION.SDK_INT >= 33) {
            flowItem(1, gate, "language") {
                GlassSection {
                    row(hasIcon = true) {
                        FluxListRow(
                            title = str(R.string.language),
                            icon = FluxIcons.Globe,
                            value = str(R.string.languageNativeName),
                            chevron = true,
                            onClick = {
                                val intent = Intent(Settings.ACTION_APP_LOCALE_SETTINGS)
                                    .setData(Uri.fromParts("package", context.packageName, null))
                                startSettingsIntent(context, overlays, intent, noApp)
                            },
                        )
                    }
                }
            }
        }
        flowItem(2, gate, "theme") {
            GlassSection(animateSize, title = str(R.string.settingsGroupTheme)) {
                custom(padded = true) {
                    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(22.dp)) {
                        SubBlock(str(R.string.themeMode)) {
                            ModeTiles(
                                selected = appearance.mode,
                                onSelect = { mode: ThemeMode ->
                                    if (mode != appearance.mode) {
                                        haptics.tick()
                                        write { repo.setMode(mode) }
                                    }
                                },
                            )
                        }
                        SubBlock(str(R.string.themeColor)) {
                            Column(verticalArrangement = Arrangement.spacedBy(18.dp)) {
                                AccentDots(
                                    scheme = appearance.scheme,
                                    customColor = appearance.customColor,
                                    dimmed = appearance.dynamicColor,
                                    onPreset = { id ->
                                        if (id != appearance.scheme) {
                                            haptics.tick()
                                            write { repo.setScheme(id) }
                                        }
                                    },
                                    onCustom = {
                                        if (appearance.scheme != "custom") {
                                            haptics.tick()
                                            // 取色器以持久化的自定义色初始化
                                            write { repo.setScheme("custom") }
                                        }
                                    },
                                )
                                if (appearance.scheme == "custom" && !appearance.dynamicColor) {
                                    CustomColorPicker(
                                        initial = appearance.customColor,
                                        onChange = {
                                            customDraft = it
                                            customDirty = true
                                        },
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
        flowItem(3, gate, "ambience") {
            GlassSection(animateSize, title = str(R.string.mobileGroupAmbience)) {
                row(hasIcon = true) {
                    FluxSwitchRow(
                        title = str(R.string.mobileDynamicColor),
                        subtitle = str(R.string.mobileDynamicColorDesc),
                        icon = FluxIcons.Palette,
                        checked = appearance.dynamicColor,
                        onCheckedChange = { on -> write { repo.setDynamicColor(on) } },
                    )
                }
                if (appearance.dynamicColor) {
                    custom { FluxDivider(startInset = 16.dp) }
                    custom(padded = true) {
                        WallpaperCard(onUseOwn = { write { repo.setDynamicColor(false) } })
                    }
                }
                custom { FluxDivider(startInset = 16.dp) }
                custom(padded = true) {
                    AuraBlock(
                        value = auraState.value,
                        onValueChange = { auraDraft = it },
                        onFinished = {
                            val d = auraDraft
                            if (d != null) write { repo.setAuraIntensity(d) }
                        },
                    )
                }
            }
        }
        flowItem(4, gate, "interface") {
            GlassSection(title = str(R.string.settingsGroupInterface)) {
                row(hasIcon = true) {
                    FluxListRow(
                        title = str(R.string.mobileSystemTextSize),
                        subtitle = str(R.string.mobileSystemTextSizeDesc),
                        icon = FluxIcons.Type,
                        chevron = true,
                        onClick = {
                            startSettingsIntent(context, overlays, Intent(Settings.ACTION_DISPLAY_SETTINGS), noApp)
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun SubBlock(label: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        FluxText(
            label,
            style = FluxTheme.type.micro,
            color = FluxTheme.colors.inkMuted,
        )
        content()
    }
}

/** 跟随壁纸取色开启时的状态卡：壁纸色块 + 十六进制 + “改用自选色”。 */
@Composable
private fun WallpaperCard(onUseOwn: () -> Unit) {
    val context = LocalContext.current
    val c = FluxTheme.colors
    val t = FluxTheme.type
    val wallpaper = remember(context) { wallpaperAccent(context) ?: Color.Unspecified }
    val effective = if (wallpaper == Color.Unspecified) c.accent else wallpaper
    val hex = remember(effective) { effective.toArgb().hexRgb() }
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Spacer(Modifier.size(40.dp).background(effective, CircleShape))
        FluxText(
            str(R.string.mobileDynamicColorActive, "hex" to hex),
            style = t.sm,
            color = c.inkMuted,
            modifier = Modifier.weight(1f),
        )
        FluxButton(
            text = str(R.string.mobileDynamicColorOff),
            onClick = onUseOwn,
            variant = ButtonVariant.Ghost,
            size = ButtonSize.Sm,
        )
    }
}

/** 氛围光强度：标题 + 读数、滑杆（气泡净空 36dp）、预览条。0 显示“关闭”。 */
@Composable
private fun AuraBlock(value: Int, onValueChange: (Int) -> Unit, onFinished: () -> Unit) {
    val c = FluxTheme.colors
    val t = FluxTheme.type
    val offLabel = str(R.string.mobileAuraOff)
    val title = str(R.string.mobileAuraIntensity)
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                FluxIcon(FluxIcons.Sparkles, null, size = 18.dp, tint = c.inkMuted)
                FluxText(title, style = t.weight(t.body, 450), color = c.ink)
            }
            FluxText(
                if (value == 0) offLabel else "$value%",
                style = t.weight(t.sm, 500, mono = true),
                color = if (value == 0) c.inkFaint else c.ink,
            )
        }
        FluxText(str(R.string.mobileAuraIntensityDesc), style = t.sm, color = c.inkMuted)
        Spacer(Modifier.height(28.dp))
        FluxSlider(
            value = value.toFloat(),
            onValueChange = { onValueChange(it.roundToInt()) },
            range = 0f..100f,
            format = { v -> if (v.roundToInt() == 0) offLabel else "${v.roundToInt()}%" },
            onValueChangeFinished = onFinished,
            label = title,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(4.dp))
        AuraPreview(intensity = value)
    }
}

