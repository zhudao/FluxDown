package com.fluxdown.app.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import com.fluxdown.fluxui.theme.FluxAccent
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

/** 明暗模式：`appearance.theme_mode`。 */
enum class ThemeMode(val wire: String) {
    System("system"), Light("light"), Dark("dark");

    companion object {
        fun of(v: String?): ThemeMode = entries.firstOrNull { it.wire == v } ?: System
    }
}

/** 外观设置快照（docs 01-foundations §11.3）。 */
data class AppearanceState(
    val mode: ThemeMode = ThemeMode.System,
    /** `appearance.color_scheme`：blue / green / violet / rose / custom。 */
    val scheme: String = "blue",
    val customColor: Int = FluxAccent.DEFAULT_CUSTOM,
    /** 设备本地：跟随壁纸取色（只替换强调色槽位）。 */
    val dynamicColor: Boolean = false,
    /** 设备本地：氛围光强度 0..100，60 = 1.0×，0 = 关闭。 */
    val auraIntensity: Int = 60,
) {
    /** 优先级：壁纸 > 自定义 > 预设。 */
    val accent: FluxAccent
        get() = when {
            dynamicColor -> FluxAccent.Wallpaper
            scheme == "custom" -> FluxAccent.Custom(customColor)
            else -> FluxAccent.Preset(scheme)
        }
}

/**
 * 外观偏好。`theme_mode / color_scheme / custom_color` 属于云同步键（接入 agent 后经
 * `agent.preferences.patch` 同步）；`dynamic_color / aura_intensity` 为设备本地新增键。
 */
class AppearanceRepo(private val ds: DataStore<Preferences>) {
    private object K {
        val mode = stringPreferencesKey("appearance.theme_mode")
        val scheme = stringPreferencesKey("appearance.color_scheme")
        val custom = intPreferencesKey("appearance.custom_color")
        val dynamic = booleanPreferencesKey("appearance.dynamic_color")
        val aura = intPreferencesKey("appearance.aura_intensity")
    }

    val state: Flow<AppearanceState> = ds.data.map { p ->
        AppearanceState(
            mode = ThemeMode.of(p[K.mode]),
            scheme = p[K.scheme] ?: "blue",
            customColor = p[K.custom] ?: FluxAccent.DEFAULT_CUSTOM,
            dynamicColor = p[K.dynamic] ?: false,
            auraIntensity = (p[K.aura] ?: 60).coerceIn(0, 100),
        )
    }

    suspend fun setMode(mode: ThemeMode) = ds.edit { it[K.mode] = mode.wire }
    suspend fun setScheme(scheme: String) = ds.edit { it[K.scheme] = scheme }
    suspend fun setCustomColor(argb: Int) = ds.edit {
        it[K.custom] = argb
        it[K.scheme] = "custom"
    }
    suspend fun setDynamicColor(enabled: Boolean) = ds.edit { it[K.dynamic] = enabled }
    suspend fun setAuraIntensity(v: Int) = ds.edit { it[K.aura] = v.coerceIn(0, 100) }
}
