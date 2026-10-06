package com.fluxdown.app.feature.settings

import androidx.compose.runtime.Composable
import com.fluxdown.app.nav.SettingsPage

/** 推入式设置子页（页头返回 → `nav.pop()`）。 */
@Composable
fun SettingsPageScreen(page: SettingsPage) {
    when (page) {
        SettingsPage.Appearance -> AppearancePage()
        SettingsPage.Download -> DownloadPage()
        SettingsPage.About -> AboutPage()
    }
}
