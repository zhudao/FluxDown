package com.fluxdown.app.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.fluxdown.app.R
import com.fluxdown.app.data.AppearanceState
import com.fluxdown.app.data.ThemeMode
import com.fluxdown.app.feature.devices.localizedName
import com.fluxdown.app.feature.devices.localizedSubtitle
import com.fluxdown.app.i18n.str
import com.fluxdown.app.nav.LocalNavigator
import com.fluxdown.app.nav.Route
import com.fluxdown.app.nav.SettingsPage
import com.fluxdown.app.nav.SheetRoute
import com.fluxdown.app.shell.LocalAppContainer
import com.fluxdown.app.shell.hostState
import com.fluxdown.core.format.Format
import com.fluxdown.core.model.HostRef
import com.fluxdown.core.store.Connection
import com.fluxdown.fluxui.chrome.FluxHeader
import com.fluxdown.fluxui.chrome.FluxHostPill
import com.fluxdown.fluxui.controls.FluxListRow
import com.fluxdown.fluxui.controls.FluxPresenceDot
import com.fluxdown.fluxui.controls.GlassSection
import com.fluxdown.fluxui.controls.Tone
import com.fluxdown.fluxui.icons.FluxIcons
import com.fluxdown.fluxui.material.rememberFlowInGate
import com.fluxdown.fluxui.overlay.FluxBanner
import com.fluxdown.fluxui.overlay.FluxBannerKind
import com.fluxdown.fluxui.theme.FluxText
import com.fluxdown.fluxui.theme.FluxTheme
import java.util.Locale

/**
 * S1 · 设置首页（Tab 根页）。只列出已实现的分类：外观 / 下载 / 关于。
 * 顶部为当前主机卡（点按打开主机切换器）；离线时「引擎」分组降为 40% 并出现只读横幅。
 */
@Composable
fun SettingsScreen() {
    val nav = LocalNavigator.current
    val container = LocalAppContainer.current
    val hostState = hostState()
    val readOnly by remember { derivedStateOf { hostState.value.isReadOnly } }
    val host by container.host.collectAsState()
    val appearance by container.appearance.state.collectAsState(initial = AppearanceState())
    val listState = rememberLazyListState()
    val gate = rememberFlowInGate()
    val statusTop = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    DockMiniOnScroll(listState, nav)
    val context = LocalContext.current
    val version = remember(context) { context.appVersionName() }

    val hostTitle = host.localizedName()
    val switchHostDescription = str(R.string.mobileSettingsSwitchHost, "name" to hostTitle)

    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        LazyColumn(
            state = listState,
            modifier = Modifier.widthIn(max = PageMaxWidth).fillMaxWidth().fillMaxSize(),
            contentPadding = PaddingValues(
                start = FluxTheme.space.screenMargin,
                end = FluxTheme.space.screenMargin,
                top = statusTop,
                bottom = FluxTheme.space.dockClearance,
            ),
            verticalArrangement = Arrangement.spacedBy(SectionGap),
        ) {
            flowItem(0, gate, "header") {
                FluxHeader(title = str(R.string.settings))
            }
            if (readOnly) {
                flowItem(1, gate, "banner") {
                    FluxBanner(text = str(R.string.localServiceDisconnected), kind = FluxBannerKind.Warn, slim = true)
                }
            }
            flowItem(2, gate, "host") {
                GlassSection {
                    row(hasIcon = true) {
                        FluxListRow(
                            title = hostTitle,
                            subtitle = host.localizedSubtitle(),
                            icon = if (host is HostRef.Remote) FluxIcons.Server else FluxIcons.Smartphone,
                            trailing = { ConnectionBadge() },
                            chevron = true,
                            onClick = { nav.openSheet(SheetRoute.HostSwitch) },
                        )
                    }
                }
            }
            flowItem(3, gate, "personal") {
                GlassSection(title = str(R.string.settingsGroupPersonal)) {
                    row(hasIcon = true) {
                        FluxListRow(
                            title = str(R.string.settingsCatAppearance),
                            subtitle = appearanceReadout(appearance),
                            icon = FluxIcons.Palette,
                            chevron = true,
                            onClick = { nav.push(Route.Settings(SettingsPage.Appearance)) },
                        )
                    }
                }
            }
            flowItem(4, gate, "engine") {
                Box(Modifier.alpha(if (readOnly) 0.4f else 1f)) {
                    GlassSection(
                        title = str(R.string.settingsGroupEngine),
                        action = {
                            FluxHostPill(
                                name = hostTitle,
                                online = !readOnly,
                                onClick = { nav.openSheet(SheetRoute.HostSwitch) },
                                description = switchHostDescription,
                            )
                        },
                    ) {
                        row(hasIcon = true) {
                            FluxListRow(
                                title = str(R.string.settingsCatDownload),
                                subtitle = downloadReadout(),
                                icon = FluxIcons.Download,
                                chevron = true,
                                onClick = { nav.push(Route.Settings(SettingsPage.Download)) },
                            )
                        }
                    }
                }
            }
            flowItem(5, gate, "maintenance") {
                GlassSection(title = str(R.string.settingsGroupMaintenance)) {
                    row(hasIcon = true) {
                        FluxListRow(
                            title = str(R.string.settingsCatAbout),
                            subtitle = "v$version",
                            icon = FluxIcons.Info,
                            chevron = true,
                            onClick = { nav.push(Route.Settings(SettingsPage.About)) },
                        )
                    }
                }
            }
            flowItem(6, gate, "footer") {
                FluxText(
                    text = str(R.string.mobileFooter),
                    modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                    style = FluxTheme.type.sm.copy(textAlign = TextAlign.Center),
                    color = FluxTheme.colors.inkFaint,
                )
            }
        }
    }
}

/** 连接状态读数：圆点 + 文字；只在 [Connection] 变化时重组。 */
@Composable
private fun ConnectionBadge() {
    val hostState = hostState()
    val connection by remember { derivedStateOf { hostState.value.connection } }
    val c = FluxTheme.colors
    val (tone, label) = when (connection) {
        Connection.Live -> Tone.Mint to str(R.string.mobileSettingsConnLive)
        Connection.Connecting -> Tone.Amber to str(R.string.mobileSettingsConnConnecting)
        Connection.Stale -> Tone.Amber to str(R.string.mobileSettingsConnStale)
        is Connection.Failed -> Tone.Coral to str(R.string.mobileSettingsConnFailed)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        FluxPresenceDot(tone = tone, size = 7.dp)
        FluxText(label, style = FluxTheme.type.sm, color = c.inkMuted, maxLines = 1)
    }
}

@Composable
private fun appearanceReadout(a: AppearanceState): String {
    val mode = when (a.mode) {
        ThemeMode.System -> str(R.string.themeModeSystem)
        ThemeMode.Light -> str(R.string.themeModeLight)
        ThemeMode.Dark -> str(R.string.themeModeDark)
    }
    val accent = when {
        a.dynamicColor -> str(R.string.mobileSettingsReadoutWallpaper)
        a.scheme == "green" -> str(R.string.colorGreen)
        a.scheme == "violet" -> str(R.string.colorViolet)
        a.scheme == "rose" -> str(R.string.colorRose)
        a.scheme == "custom" -> str(R.string.colorCustom) + " " + a.customColor.hexRgb()
        else -> str(R.string.colorBlue)
    }
    val parts = ArrayList<String>(3)
    parts += mode
    parts += accent
    if (a.auraIntensity != AURA_DEFAULT) parts += str(R.string.mobileSettingsReadoutAura, "n" to a.auraIntensity)
    return parts.joinToString(" · ")
}

/** 下载分类读数：默认目录末段 · 并发数 [· 限速]；全部来自 `config`，仅在配置变化时重组。 */
@Composable
private fun downloadReadout(): String {
    val hostState = hostState()
    val config by remember { derivedStateOf { hostState.value.config } }
    val dir = config["default_save_dir"].orEmpty().trimEnd('/', '\\').substringAfterLast('/').substringAfterLast('\\')
    val concurrent = config["max_concurrent_tasks"]
    val limit = config["speed_limit_bytes"]?.toLongOrNull() ?: 0L
    val parts = ArrayList<String>(3)
    when {
        dir.isNotEmpty() && concurrent != null ->
            parts += str(R.string.mobileSettingsReadoutDownload, "dir" to dir, "n" to concurrent)
        concurrent != null -> parts += str(R.string.mobileSettingsReadoutConcurrent, "n" to concurrent)
        dir.isNotEmpty() -> parts += dir
    }
    Format.speed(limit)?.let { parts += str(R.string.mobileSettingsReadoutLimit, "speed" to it.toString()) }
    return parts.joinToString(" · ")
}

internal const val AURA_DEFAULT = 60

/** `#RRGGBB`（忽略 alpha）。 */
internal fun Int.hexRgb(): String = String.format(Locale.ROOT, "#%06X", this and 0xFFFFFF)
