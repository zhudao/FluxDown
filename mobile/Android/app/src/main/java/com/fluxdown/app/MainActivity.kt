package com.fluxdown.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.fluxdown.app.data.AppearanceState
import com.fluxdown.app.data.ThemeMode
import com.fluxdown.app.nav.AppNavigator
import com.fluxdown.app.nav.LocalNavigator
import com.fluxdown.app.nav.SheetRoute
import com.fluxdown.app.shell.AppShell
import com.fluxdown.app.shell.LocalAppContainer
import com.fluxdown.fluxui.theme.FluxTheme

class MainActivity : ComponentActivity() {
    private val navigator = AppNavigator()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val container = (application as FluxApplication).container
        if (savedInstanceState == null) handleIntent(intent)
        setContent {
            val appearance by container.appearance.state.collectAsStateWithLifecycle(AppearanceState())
            val dark = when (appearance.mode) {
                ThemeMode.System -> isSystemInDarkTheme()
                ThemeMode.Dark -> true
                ThemeMode.Light -> false
            }
            FluxTheme(dark = dark, accent = appearance.accent, auraIntensity = appearance.auraIntensity) {
                CompositionLocalProvider(
                    LocalAppContainer provides container,
                    LocalNavigator provides navigator,
                ) {
                    AppShell()
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    /** N4：分享文本 / magnet: / ed2k:// 唤起 → 预填“新建下载”。 */
    private fun handleIntent(intent: Intent?) {
        val text = when (intent?.action) {
            Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)
            Intent.ACTION_VIEW -> intent.dataString
            else -> null
        }?.trim()
        if (!text.isNullOrEmpty()) navigator.openSheet(SheetRoute.NewDownload(prefill = text))
    }
}
