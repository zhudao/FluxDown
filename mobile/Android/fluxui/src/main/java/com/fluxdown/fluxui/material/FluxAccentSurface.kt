package com.fluxdown.fluxui.material

import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.fluxdown.fluxui.theme.FluxColors

/**
 * 强调色实心面：主按钮、分裂按钮、新建球、对话框主按钮、侧栏“新建”、滑动强调动作共用的唯一实现（§5.5）。
 *
 * 自下而上：
 * 1. [lift] > 0 时同色系环境投影：向下偏移且内缩，只落在形状下方，不向四周晕成光斑；
 * 2. 1dp 贴地唇边（无模糊）；
 * 3. accentFillA → accentFillB 竖向渐变，底端只取 60% 落差，避免下半部发闷；
 * 4. 上半部柔光；内侧 1dp 顶沿高光与底沿暗边（均渐隐）；
 * 5. 0.5dp 定边描边：浅色压暗（在浅底上边缘清晰），深色提亮。
 *
 * `onAccent` 对渐变两端 ≥ 4.5:1 的护栏（[com.fluxdown.fluxui.theme.resolveAccent]）对两端之间的中间色同样成立；
 * 柔光在标签所在的中线处已降到 0。按压反馈由调用方负责（缩放或叠 `onAccent` 状态层）。
 */
internal fun Modifier.fluxAccentSurface(colors: FluxColors, shape: Shape, lift: Dp = 0.dp): Modifier {
    val surface = Modifier.drawWithCache {
        val outline = shape.createOutline(size, layoutDirection, this)
        val clip = Path().apply { addOutline(outline) }
        val dark = colors.dark
        val fill = Brush.verticalGradient(
            listOf(colors.accentFillA, lerp(colors.accentFillA, colors.accentFillB, 0.6f)),
        )
        val sheen = Brush.verticalGradient(
            0f to Color.White.copy(alpha = if (dark) 0.08f else 0.12f),
            0.5f to Color.Transparent,
        )
        val rim = Brush.verticalGradient(
            0f to Color.White.copy(alpha = if (dark) 0.30f else 0.42f),
            0.4f to Color.Transparent,
        )
        val base = Brush.verticalGradient(
            0.6f to Color.Transparent,
            1f to Color.Black.copy(alpha = if (dark) 0.18f else 0.10f),
        )
        val shade = lerp(colors.accentFillB, Color.Black, 0.35f)
        val lip = if (dark) Color.Black.copy(alpha = 0.35f) else shade.copy(alpha = 0.22f)
        val edge = if (dark) Color.White.copy(alpha = 0.14f) else shade.copy(alpha = 0.38f)
        // 描边居中于轮廓：裁到形状内后 2dp 只剩内侧 1dp。
        val inner = Stroke(2.dp.toPx())
        val hairline = Stroke(0.5.dp.toPx())
        val lipDy = 1.dp.toPx()
        onDrawBehind {
            translate(top = lipDy) { drawOutline(outline, lip) }
            drawOutline(outline, fill)
            drawOutline(outline, sheen)
            clipPath(clip) {
                drawOutline(outline, rim, style = inner)
                drawOutline(outline, base, style = inner)
            }
            drawOutline(outline, edge, style = hairline)
        }
    }
    val shadowed = if (lift > 0.dp) {
        fluxGlow(colors.accentAmbient(), lift * 0.9f, shape, spread = lift * -0.5f, dy = lift * 0.8f)
    } else {
        this
    }
    return shadowed.then(surface)
}

/** 环境投影色：浅色 = 压暗的强调色低 α（有色阴影而非光晕），深色 = 强调色低 α（保留克制的发光感）。 */
private fun FluxColors.accentAmbient(): Color =
    if (dark) accent.copy(alpha = 0.30f) else lerp(accentFillB, Color.Black, 0.2f).copy(alpha = 0.30f)
