package com.fluxdown.app.feature.settings

import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateMapOf
import com.fluxdown.app.AppContainer
import com.fluxdown.core.host.HostErrorCode
import com.fluxdown.core.host.HostException
import com.fluxdown.fluxui.overlay.FluxOverlayState
import com.fluxdown.fluxui.overlay.FluxToastKind
import com.fluxdown.fluxui.theme.FluxHaptics
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull

/**
 * 主机配置（`daemon.config.patch`）的乐观写入器。
 *
 * - [set]：本地先行（[optimistic]）→ 同键 250ms 防抖 → `patchConfig(当前 revision, {key: value})`。
 * - Conflict：等待 store 收到更新的 revision 后重试一次；仍失败或其它错误 → 回滚乐观值 + toast。
 * - 写入在 `appScope` 中执行，离开页面不会丢失尚在防抖中的修改。
 * - 提交之间串行（[lock]），并等到 store 反映新值后才释放，使下一次提交总能拿到最新 revision。
 */
@Stable
internal class ConfigEditor(
    private val container: AppContainer,
    private val overlays: FluxOverlayState,
    private val haptics: FluxHaptics,
    private val errorText: (HostException) -> String,
    private val disconnectedText: String,
) {
    /** 尚未被主机确认的本地值：界面读取 `optimistic[key] ?: host[key]`。 */
    val optimistic = mutableStateMapOf<String, String>()

    private val timers = HashMap<String, Job>()
    private val lock = Mutex()

    /** 防抖写入单个键。 */
    fun set(key: String, value: String) {
        val state = container.store.state.value
        if (state.isReadOnly) {
            reject(disconnectedText)
            return
        }
        if (value == state.config[key] && timers[key] == null && !optimistic.containsKey(key)) return
        optimistic[key] = value
        timers.remove(key)?.cancel()
        timers[key] = container.appScope.launch {
            delay(DEBOUNCE_MS)
            timers.remove(key)
            commit(mapOf(key to value))
        }
    }

    /** 立即写入多个键（对话框确认等一次性动作）。 */
    fun setNow(values: Map<String, String>) {
        if (container.store.state.value.isReadOnly) {
            reject(disconnectedText)
            return
        }
        for ((k, v) in values) {
            timers.remove(k)?.cancel()
            optimistic[k] = v
        }
        container.appScope.launch { commit(values) }
    }

    private suspend fun commit(values: Map<String, String>) = lock.withLock {
        try {
            patchWithRetry(values)
            // 等 store 反映新值，避免回落到旧值闪一帧；最多 2s。
            withTimeoutOrNull(CONFIRM_TIMEOUT_MS) {
                container.store.state.first { s -> values.all { (k, v) -> s.config[k] == v } }
            }
        } catch (e: HostException) {
            reject(errorText(e))
        } finally {
            for ((k, v) in values) {
                // 期间用户又改了同键（新的防抖在跑）则保留其乐观值
                if (optimistic[k] == v && timers[k] == null) optimistic.remove(k)
            }
        }
    }

    private suspend fun patchWithRetry(values: Map<String, String>) {
        var retried = false
        while (true) {
            val stale = container.store.state.value
            try {
                container.session.patchConfig(stale.configRevision, values)
                return
            } catch (e: HostException) {
                if (e.code != HostErrorCode.Conflict || retried) throw e
                retried = true
                withTimeoutOrNull(CONFLICT_WAIT_MS) {
                    container.store.state.first { it.configRevision != stale.configRevision }
                }
            }
        }
    }

    private fun reject(text: String) {
        haptics.reject()
        overlays.toast(text, FluxToastKind.Error)
    }

    private companion object {
        const val DEBOUNCE_MS = 250L
        const val CONFIRM_TIMEOUT_MS = 2_000L
        const val CONFLICT_WAIT_MS = 1_000L
    }
}
