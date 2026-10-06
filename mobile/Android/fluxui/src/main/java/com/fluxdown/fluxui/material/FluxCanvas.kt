package com.fluxdown.fluxui.material

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.fluxdown.fluxui.theme.FluxTheme

/**
 * 页面背景（§5.6 z 序）：画布色 0 → 氛围光 1 → 颗粒 2 → [content]（舞台 / 页面内容，z 3）。
 *
 * - [activity]：0..1 吞吐活跃度（见 [auraActivity]），在绘制阶段读取，不触发重组。
 * - [static] = true：推入页 / 下层页——不跑着色器，只画静态双径向渐变（[FluxAuraStatic] `pushed = true`）。
 * - 根页应放在 `Modifier.fluxBackdropSource(...)` 的根内，使玻璃面能取到画布 + 氛围光 + 颗粒作为背后内容。
 * - [auraMaxFps]：根页氛围光重绘上限；Sheet / 菜单 / 对话框打开期间传 `perf.auraMaxFps / 2`。
 */
@Composable
fun FluxCanvas(
    activity: () -> Float,
    modifier: Modifier = Modifier,
    static: Boolean = false,
    auraMaxFps: Int = FluxTheme.perf.auraMaxFps,
    content: @Composable BoxScope.() -> Unit = {},
) {
    Box(modifier.background(FluxTheme.colors.canvas)) {
        if (static) {
            FluxAuraStatic(activity, Modifier.fillMaxSize(), pushed = true)
        } else {
            FluxAura(activity, Modifier.fillMaxSize(), auraMaxFps)
        }
        Box(Modifier.fillMaxSize().fluxGrain())
        content()
    }
}
