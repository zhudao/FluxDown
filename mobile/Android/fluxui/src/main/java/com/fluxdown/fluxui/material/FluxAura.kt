package com.fluxdown.fluxui.material

import android.graphics.RuntimeShader
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableFloatState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.toArgb
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.currentStateAsState
import com.fluxdown.fluxui.theme.FluxTheme
import com.fluxdown.fluxui.theme.LocalAuraGain
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest

// ───────────────────────── §6.2 吞吐 → 亮度 / 流速映射（与 core.js P.tick 一致） ─────────────────────────

/** 46 MB/s 视为满亮（core.js: `46 * 1024 * 1024`）。 */
const val AURA_FULL_BPS: Long = 46L * 1024 * 1024

/** 总下载吞吐（B/s）→ 氛围光活跃度 `a ∈ [0,1]`。 */
fun auraActivity(downBps: Long): Float = (downBps / AURA_FULL_BPS.toFloat()).coerceIn(0f, 1f)

/** 每 1 s 一次的一阶指数平滑：`aura += (target − aura) * 0.4`。UI 侧在 1 Hz 数据到达之间另有弹簧补间。 */
fun auraStep(cur: Float, target: Float): Float = cur + (target - cur) * 0.4f

/** `Math.round(a*100)/100`。 */
fun auraRounded(a: Float): Float = (a * 100).roundToInt() / 100f

/** 相位倍速：b1 周期 52→14 s ⇒ 1.0 → 3.71。 */
fun auraPhaseSpeed(a: Float): Float = 52f / (52f - 38f * a)

/** `a < 0.03` 或 Reduce motion ⇒ 静止。 */
fun auraIdle(a: Float, reduceMotion: Boolean): Boolean = a < AURA_IDLE || reduceMotion

private const val AURA_IDLE = 0.03f

/** 三团光的周期 52 / 64 / 44 s 的最小公倍数 ≈ 9152 s：相位按此回绕，Float 精度不随运行时长劣化，且无接缝。 */
private const val PHASE_WRAP = 9152f

// ───────────────────────────────────────── §6.1/§6.3 氛围光 ─────────────────────────────────────────

/**
 * 页面氛围光（z 序：画布之上、颗粒之下，§5.6）。三团径向光，色相 = 强调色（b2 偏紫），
 * 亮度与漂移速度由 [activity]（0..1，见 [auraActivity]）驱动；空闲时静止暗淡。
 *
 * - 增益取自 [LocalAuraGain]（= 用户强度/60 × 明暗增益）；`≤ 0` 时不绘制任何内容。
 * - API 33+：AGSL `RuntimeShader`（§6.3）；API 31–32：静态双径向渐变（§6.4）。
 * - [activity] 在**绘制/快照流**中读取（不触发重组）；1 Hz 数据之间以 `soft` 弹簧补间，Reduce motion 时 `snap`。
 * - 相位推进：仅在生命周期 ≥ STARTED、非 Reduce motion、活跃度 ≥ 0.03、[maxFps] > 0 时运行；
 *   重绘上限 [maxFps]（Sheet / 菜单 / 对话框打开期间由外壳传入 `perf.auraMaxFps / 2` 即 15 fps）。
 *   [maxFps] = 0（省电 / 过热）或 Reduce motion ⇒ 冻结相位，仅画静态着色帧（亮度仍随 activity 变化）。
 */
@Composable
fun FluxAura(
    activity: () -> Float,
    modifier: Modifier = Modifier,
    maxFps: Int = FluxTheme.perf.auraMaxFps,
) {
    val gain = LocalAuraGain.current
    if (gain <= 0f) return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        FluxAuraShader(activity, gain, maxFps, modifier)
    } else {
        FluxAuraStatic(activity, modifier)
    }
}

@Composable
private fun rememberSmoothedActivity(activity: () -> Float): Animatable<Float, AnimationVector1D> {
    val motion = FluxTheme.motion
    val a = remember { Animatable(Snapshot.withoutReadObservation { activity() }.coerceIn(0f, 1f)) }
    LaunchedEffect(motion) {
        snapshotFlow { activity().coerceIn(0f, 1f) }.collectLatest { a.animateTo(it, motion.of(motion.soft)) }
    }
    return a
}

@RequiresApi(Build.VERSION_CODES.TIRAMISU)
@Composable
private fun FluxAuraShader(activity: () -> Float, gain: Float, maxFps: Int, modifier: Modifier) {
    val colors = FluxTheme.colors
    val motion = FluxTheme.motion
    val a = rememberSmoothedActivity(activity)
    val phase = remember { mutableFloatStateOf(0f) }

    val lifeState by LocalLifecycleOwner.current.lifecycle.currentStateAsState()
    val animated = lifeState.isAtLeast(Lifecycle.State.STARTED) && !motion.reduce && maxFps > 0
    LaunchedEffect(animated, maxFps) {
        if (!animated) return@LaunchedEffect
        snapshotFlow { a.value >= AURA_IDLE }.collectLatest { active ->
            if (active) runPhase(a, phase, maxFps)
        }
    }

    val shader = remember { RuntimeShader(FLUX_AURA_AGSL) }
    val brush = remember(shader) { ShaderBrush(shader) }
    val accentArgb = colors.accent.toArgb()
    val accent2Argb = lerp(colors.violet, colors.accent, 0.60f).toArgb()
    Spacer(
        modifier.fillMaxSize().drawBehind {
            shader.setFloatUniform("resolution", size.width, size.height)
            shader.setFloatUniform("unit", density)
            shader.setFloatUniform("time", phase.floatValue)
            shader.setFloatUniform("intensity", a.value.coerceIn(0f, 1f))
            shader.setFloatUniform("gain", gain)
            shader.setFloatUniform("drift", 0f, 0f)
            shader.setColorUniform("accent", accentArgb)
            shader.setColorUniform("accent2", accent2Argb)
            drawRect(brush)
        },
    )
}

/** 相位积分：以 delay 控制节拍（不逐帧唤醒），再对齐到下一帧取时间戳。 */
private suspend fun runPhase(a: Animatable<Float, AnimationVector1D>, phase: MutableFloatState, maxFps: Int) {
    // 目标间隔 1000/maxFps ms，扣除约一帧的对齐等待
    val waitMs = (1000L / maxFps - 10L).coerceAtLeast(1L)
    var last = withFrameNanos { it }
    while (true) {
        delay(waitMs)
        val now = withFrameNanos { it }
        val dt = (now - last) / 1e9f
        last = now
        var t = phase.floatValue + dt * auraPhaseSpeed(a.value.coerceIn(0f, 1f))
        if (t > PHASE_WRAP) t -= PHASE_WRAP
        phase.floatValue = t
    }
}

// ───────────────────────────────────── §6.4 静态渐变（API 31–32 / 推入页） ─────────────────────────────────────

/**
 * 静态氛围光：两层椭圆径向渐变，**不漂移，只有亮度随 activity 变化**；不跑着色器、不推进相位。
 * - [pushed] = false：根页 API 31–32 回退（`aura-fill`）；true：推入页 `layer::before`（所有 API）。
 * - 同一时刻只有最上层的根页 / 页面绘制氛围光；下层页（under）只保留静态渐变。
 * 增益取自 [LocalAuraGain]（`≤ 0` 不绘制）。
 */
@Composable
fun FluxAuraStatic(
    activity: () -> Float,
    modifier: Modifier = Modifier,
    pushed: Boolean = false,
) {
    val k = LocalAuraGain.current
    if (k <= 0f) return
    val colors = FluxTheme.colors
    val a = rememberSmoothedActivity(activity)
    val c1 = colors.accent
    val c2 = lerp(colors.violet, colors.accent, 0.62f)
    Spacer(
        modifier.fillMaxSize().drawWithCache {
            val w = size.width
            val h = size.height
            val g1 = EllipseGlow(
                center = if (pushed) Offset(w * 0.85f, 0f) else Offset(w * 0.82f, h * 0.02f),
                rx = w * 0.60f,
                ry = h * (if (pushed) 0.34f else 0.38f),
                brush = glowBrush(c1, 0.70f, w * 0.60f),
            )
            val g2 = EllipseGlow(
                center = if (pushed) Offset(w * 0.06f, h) else Offset(w * 0.08f, h * 0.96f),
                rx = w * 0.70f,
                ry = h * (if (pushed) 0.36f else 0.40f),
                brush = glowBrush(c2, 0.72f, w * 0.70f),
            )
            onDrawBehind {
                val av = a.value.coerceIn(0f, 1f)
                val a1 = if (pushed) 0.40f * av * k + 0.08f else 0.58f * av * k + 0.09f
                val a2 = if (pushed) 0.30f * av * k + 0.05f else 0.46f * av * k + 0.06f
                drawGlow(g1, a1.coerceAtMost(1f))
                drawGlow(g2, a2.coerceAtMost(1f))
            }
        },
    )
}

private class EllipseGlow(val center: Offset, val rx: Float, val ry: Float, val brush: Brush)

/** 圆心在原点、半径 [r] 的径向渐变：`color @0 → transparent @[stop]`（CSS `radial-gradient(… C, transparent 70%)`）。 */
private fun glowBrush(color: Color, stop: Float, r: Float): Brush {
    val clear = color.copy(alpha = 0f)
    return Brush.radialGradient(
        colorStops = arrayOf(0f to color, stop to clear, 1f to clear),
        center = Offset.Zero,
        radius = r,
    )
}

/** Compose 无椭圆渐变：圆形渐变 + Y 向缩放（`ry/rx`）；只填椭圆外接矩形。 */
private fun DrawScope.drawGlow(g: EllipseGlow, alpha: Float) {
    translate(g.center.x, g.center.y) {
        scale(1f, g.ry / g.rx, pivot = Offset.Zero) {
            drawRect(g.brush, topLeft = Offset(-g.rx, -g.rx), size = Size(g.rx * 2f, g.rx * 2f), alpha = alpha)
        }
    }
}

// ───────────────────────────────────────── §6.3 AGSL 源码 ─────────────────────────────────────────

private const val FLUX_AURA_AGSL = """
uniform float2 resolution;
uniform float  unit;
uniform float  time;
uniform float  intensity;
uniform float  gain;
uniform float2 drift;
layout(color) uniform half4 accent;
layout(color) uniform half4 accent2;

const float TAU = 6.2831853;

float falloff(float2 p, float2 c, float r) {
    return clamp(1.0 - length(p - c) / (0.933 * r), 0.0, 1.0);
}

float4 over(float4 dst, float3 rgb, float a) {
    return float4(rgb * a + dst.rgb * (1.0 - a), a + dst.a * (1.0 - a));
}

half4 main(float2 fc) {
    float u = unit;
    float2 W = resolution;
    float2 par = drift * 6.0 * u;

    float t1 = TAU * time / 52.0;
    float2 c1 = float2(W.x - 70.0 * u, 50.0 * u)
              + u * (float2(-34.0, 46.0) * sin(t1) + float2(14.0, -12.0) * sin(2.0 * t1 + 0.6)) + par;
    float r1 = 260.0 * u * (1.0 + 0.07 * sin(t1 + 1.1));
    float o1 = (0.10 + 0.62 * intensity) * gain;

    float t2 = TAU * time / 64.0;
    float2 c2 = float2(40.0 * u, W.y - 100.0 * u)
              + u * (float2(40.0, -34.0) * sin(t2 + 0.9) + float2(-10.0, 12.0) * sin(2.0 * t2)) - par * 0.6;
    float r2 = 300.0 * u * (1.0 + 0.06 * sin(t2 + 2.0));
    float o2 = (0.07 + 0.46 * intensity) * gain;

    float t3 = TAU * time / 44.0;
    float2 c3 = float2(0.24 * W.x + 170.0 * u, 0.36 * W.y + 170.0 * u)
              + u * (float2(-28.0, -22.0) * sin(t3 + 1.7) + float2(10.0, 8.0) * sin(2.0 * t3 + 0.3)) + par * 0.3;
    float r3 = 170.0 * u * (1.0 + 0.10 * sin(t3));
    float o3 = (0.22 * intensity) * gain;

    float4 c = float4(0.0);
    c = over(c, accent.rgb,  clamp(0.80 * o1, 0.0, 1.0) * falloff(fc, c1, r1));
    c = over(c, accent2.rgb, clamp(0.70 * o2, 0.0, 1.0) * falloff(fc, c2, r2));
    c = over(c, accent.rgb,  clamp(0.40 * o3, 0.0, 1.0) * falloff(fc, c3, r3));

    float n = fract(sin(dot(fc, float2(12.9898, 78.233))) * 43758.5453) - 0.5;
    c.rgb += float3(n / 255.0) * c.a;
    return half4(c);
}
"""
