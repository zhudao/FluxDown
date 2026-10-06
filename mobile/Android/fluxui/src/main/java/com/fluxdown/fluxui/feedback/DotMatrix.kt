package com.fluxdown.fluxui.feedback

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import com.fluxdown.fluxui.theme.FluxText
import com.fluxdown.fluxui.theme.FluxTheme

/** 9×9 点阵字形（原型 `GLYPH`）。`'#'` 亮 · `'o'` 强调色 · `'+'` 半亮 · `'.'` 灭。 */
enum class FluxGlyph(val rows: List<String>) {
    File(listOf("..#####..", "..#...##.", "..#...#.#", "..#...###", "..#.....#", "..#.o.o.#", "..#.....#", "..#.....#", "..#######")),
    Inbox(listOf(".........", ".#######.", ".#.....#.", ".#.....#.", ".#.....#.", ".##...##.", ".#.###.#.", ".#.....#.", ".#######.")),
    Check(listOf(".........", ".......o.", "......oo.", ".....oo..", ".o..oo...", ".oooo....", "..oo.....", ".........", ".........")),
    Search(listOf("..#####..", ".#.....#.", "#.......#", "#.......#", "#.......#", ".#.....#.", "..#####..", "......##.", ".......##")),
    Wifi(listOf(".........", "..#####..", ".#.....#.", "#..###..#", "..#...#..", "....#....", "....o....", ".........", ".........")),
    Rss(listOf("#####....", "....##...", "......#..", "..##..#..", "...#...#.", "#..#...#.", "##.#...#.", ".........", "o........")),
    Device(listOf("..#####..", "..#...#..", "..#...#..", "..#...#..", "..#...#..", "..#...#..", "..#...#..", "..#####..", "....o....")),
    Download(listOf("....#....", "....#....", "....#....", "..#.#.#..", "...###...", "....#....", ".#######.", ".#.....#.", ".#######.")),
    Plug(listOf(".#.....#.", ".#.....#.", ".#######.", ".#.....#.", "..#...#..", "...###...", "....#....", "....#....", "....#...."));
}

/** 字形集合与数字点阵（3×5，`.` `:` 带强调点）。 */
object DotGlyphs {
    val File: List<String> = FluxGlyph.File.rows
    val Inbox: List<String> = FluxGlyph.Inbox.rows
    val Check: List<String> = FluxGlyph.Check.rows
    val Search: List<String> = FluxGlyph.Search.rows
    val Wifi: List<String> = FluxGlyph.Wifi.rows
    val Rss: List<String> = FluxGlyph.Rss.rows
    val Device: List<String> = FluxGlyph.Device.rows
    val Download: List<String> = FluxGlyph.Download.rows
    val Plug: List<String> = FluxGlyph.Plug.rows

    private val DIGITS: Map<Char, List<String>> = mapOf(
        '0' to listOf("###", "#.#", "#.#", "#.#", "###"),
        '1' to listOf(".#.", "##.", ".#.", ".#.", "###"),
        '2' to listOf("###", "..#", "###", "#..", "###"),
        '3' to listOf("###", "..#", "###", "..#", "###"),
        '4' to listOf("#.#", "#.#", "###", "..#", "..#"),
        '5' to listOf("###", "#..", "###", "..#", "###"),
        '6' to listOf("###", "#..", "###", "#.#", "###"),
        '7' to listOf("###", "..#", ".#.", ".#.", ".#."),
        '8' to listOf("###", "#.#", "###", "#.#", "###"),
        '9' to listOf("###", "#.#", "###", "..#", "###"),
        '.' to listOf(".", ".", ".", ".", "o"),
        ':' to listOf(".", "o", ".", "o", "."),
        '-' to listOf("...", "...", "###", "...", "..."),
        ' ' to listOf(".", ".", ".", ".", "."),
    )

    /** 文本（数字 / `.` `:` `-` / 空格；其余视作空格）→ 5 行点阵，字间 1 列空。 */
    fun text(s: String): List<String> {
        val rows = Array(5) { StringBuilder() }
        s.forEachIndexed { i, ch ->
            val g = DIGITS[ch] ?: DIGITS.getValue(' ')
            for (r in 0 until 5) {
                if (i > 0) rows[r].append('.')
                rows[r].append(g[r])
            }
        }
        return rows.map { it.toString() }
    }
}

private const val DM_BLINK_MS = 3200f
private const val DM_BLINK_STAGGER_MS = 90f
private const val DM_BLINK_LOW = 0.45f

private const val K_OFF = 0
private const val K_ON = 1
private const val K_MID = 2
private const val K_HI = 3

/**
 * 点阵图形（§12.32，原型 `ui.dotMatrix`）。圆点 [dot] / 间距 [gap]；颜色 `ink`：灭 α .09 · 亮 .92 · 半亮 .40 ·
 * 强调点 `accentHi` α 1 + 8dp `accentGlow` 辉光。亮点 / 强调点以 3.2 s 周期闪烁（→ α .45），相位错开 `90 ms × (k % 17)`；
 * Reduce motion 或 [animate] = false 时静止。单 Canvas 逐点绘制，时钟在绘制阶段读取（不触发重组）。
 *
 * [label] 非空时 `Role.Image` + contentDescription；为空则视为装饰，清除语义。
 */
@Composable
fun DotMatrix(
    rows: List<String>,
    modifier: Modifier = Modifier,
    dot: Dp = 6.dp,
    gap: Dp = 3.dp,
    animate: Boolean = true,
    label: String? = null,
) {
    val cols = rows.maxOfOrNull { it.length } ?: 0
    val nRows = rows.size
    if (cols == 0 || nRows == 0) return
    val c = FluxTheme.colors
    val ink = c.ink
    val hiColor = c.accentHi
    val glow = c.accentGlow

    val hasLit = remember(rows) { rows.any { r -> r.any { it == '#' || it == 'o' } } }
    val clock: State<Float>? = if (animate && hasLit && !FluxTheme.motion.reduce) {
        rememberInfiniteTransition(label = "fluxDotMatrix").animateFloat(
            initialValue = 0f,
            targetValue = DM_BLINK_MS,
            animationSpec = infiniteRepeatable(tween(DM_BLINK_MS.toInt(), easing = LinearEasing), RepeatMode.Restart),
            label = "fluxDotMatrixClock",
        )
    } else {
        null
    }

    val sem = if (label != null) {
        Modifier.semantics {
            role = Role.Image
            contentDescription = label
        }
    } else {
        Modifier.clearAndSetSemantics { }
    }

    Spacer(
        modifier
            .size(dot * cols + gap * (cols - 1), dot * nRows + gap * (nRows - 1))
            .then(sem)
            .drawWithCache {
                val d = dot.toPx()
                val step = d + gap.toPx()
                val r = d / 2f
                val n = nRows * cols
                val kind = IntArray(n)
                val cx = FloatArray(n)
                val cy = FloatArray(n)
                val glowR = r + 8.dp.toPx()
                val glowBrush = arrayOfNulls<Brush>(n)
                for (row in 0 until nRows) {
                    val line = rows[row]
                    for (col in 0 until cols) {
                        val i = row * cols + col
                        kind[i] = when (line.getOrElse(col) { '.' }) {
                            '#' -> K_ON
                            'o' -> K_HI
                            '+' -> K_MID
                            else -> K_OFF
                        }
                        cx[i] = col * step + r
                        cy[i] = row * step + r
                        if (kind[i] == K_HI) {
                            glowBrush[i] = Brush.radialGradient(
                                colorStops = arrayOf(
                                    0f to glow,
                                    (r / glowR) to glow.copy(alpha = glow.alpha * 0.55f),
                                    1f to glow.copy(alpha = 0f),
                                ),
                                center = Offset(cx[i], cy[i]),
                                radius = glowR,
                            )
                        }
                    }
                }
                onDrawBehind {
                    val t = clock?.value ?: -1f
                    for (i in 0 until n) {
                        val k = kind[i]
                        val center = Offset(cx[i], cy[i])
                        when (k) {
                            K_OFF -> drawCircle(ink.copy(alpha = 0.09f), r, center)
                            K_MID -> drawCircle(ink.copy(alpha = 0.40f), r, center)
                            K_ON -> drawCircle(ink.copy(alpha = blinkAlpha(0.92f, t, i)), r, center)
                            else -> {
                                val a = blinkAlpha(1f, t, i)
                                glowBrush[i]?.let { drawCircle(it, glowR, center, alpha = a) }
                                drawCircle(hiColor.copy(alpha = a), r, center)
                            }
                        }
                    }
                }
            },
    )
}

/** 第 [i]（0 起）个点在时钟 [t]（ms）的 α：`base → .45 → base`，错相 `90 ms × ((i+1) % 17)`；t < 0 = 静止。 */
private fun blinkAlpha(base: Float, t: Float, i: Int): Float {
    if (t < 0f) return base
    val k = ((i + 1) % 17).toFloat()
    val x = (((t - k * DM_BLINK_STAGGER_MS) % DM_BLINK_MS) + DM_BLINK_MS) % DM_BLINK_MS / DM_BLINK_MS
    val tri = if (x < 0.5f) x * 2f else (1f - x) * 2f
    val s = tri * tri * (3f - 2f * tri) // ease-in-out
    return base + (DM_BLINK_LOW - base) * s
}

/** 预置字形版本：`DotMatrix(FluxGlyph.Inbox)`。 */
@Composable
fun DotMatrix(
    glyph: FluxGlyph,
    modifier: Modifier = Modifier,
    dot: Dp = 6.dp,
    gap: Dp = 3.dp,
    animate: Boolean = true,
    label: String? = null,
) = DotMatrix(glyph.rows, modifier, dot, gap, animate, label)

/**
 * 空态（§12.32，原型 `ui.empty`）：**一律用点阵图形**。居中列：点阵（d 7 / gap 4）→ 标题 `h2`（间距 24）→
 * 说明 `sm inkMuted`（max 270、行高 1.5）→ [action]（间距 24）；padding 40 / 28。
 * 整体合并语义，点阵为装饰。
 */
@Composable
fun FluxEmpty(
    glyph: List<String>,
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    action: (@Composable () -> Unit)? = null,
) {
    val c = FluxTheme.colors
    val t = FluxTheme.type
    Column(
        modifier.semantics(mergeDescendants = true) { }.padding(horizontal = 28.dp, vertical = 40.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Top,
    ) {
        DotMatrix(glyph, dot = 7.dp, gap = 4.dp)
        Spacer(Modifier.height(24.dp))
        FluxText(title, style = t.h2.copy(textAlign = TextAlign.Center))
        if (subtitle != null) {
            Spacer(Modifier.height(6.dp))
            FluxText(
                subtitle,
                modifier = Modifier.widthIn(max = 270.dp),
                style = t.sm.copy(textAlign = TextAlign.Center, lineHeight = 1.5.em),
                color = c.inkMuted,
            )
        }
        if (action != null) {
            Spacer(Modifier.height(24.dp))
            action()
        }
    }
}

/** 预置字形版本：`FluxEmpty(FluxGlyph.Search, "没有结果")`。 */
@Composable
fun FluxEmpty(
    glyph: FluxGlyph,
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    action: (@Composable () -> Unit)? = null,
) = FluxEmpty(glyph.rows, title, modifier, subtitle, action)
