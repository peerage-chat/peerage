package chat.peerage.ui

import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.SeekableTransitionState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.unit.dp
import chat.peerage.NavState
import chat.peerage.Screen
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlin.math.abs

/**
 * Свайп вправо — назад (п. 40, app-ui.md → «Жесты»). Палец ведёт тот же переход, что и
 * предиктивный «назад» в [ScreenTransitions]: экран уезжает вправо, из-под него выходит
 * прежний. Отпустил дальше [COMMIT] ширины или броском — назад; иначе экран возвращается.
 *
 * Все вызовы — с главного потока (обработчик касаний), по одному жесту за раз.
 */
internal class SwipeBack(
    private val scope: CoroutineScope,
    private val seek: SeekableTransitionState<Screen>,
    private val nav: NavState,
    private val enabled: () -> Boolean,
    private val onBack: () -> Unit,
) {
    private var progress: Channel<Float>? = null
    private var commit = false

    /** Начать жест; `false` — назад сейчас некуда (корень, звонок, уже идёт переход). */
    fun start(): Boolean {
        if (progress != null || !enabled()) return false
        val prev = nav.previous ?: return false
        val ch = Channel<Float>(Channel.CONFLATED)
        progress = ch
        commit = false
        scope.launch {
            for (p in ch) seek.seekTo(p, prev)
            progress = null
            if (commit) onBack() else seek.animateTo(seek.currentState)
        }
        return true
    }

    /** Доля ширины, на которую сдвинут экран. Переход идёт по кривой [FastOutSlowInEasing]
     *  (как в [ScreenTransitions]): доля пальца переводится в долю времени, иначе экран
     *  отставал бы от пальца или обгонял его. */
    fun drag(fraction: Float) {
        progress?.trySend(FastOutSlowInEasing.inverse(fraction.coerceIn(0f, 1f)))
    }

    fun end(commit: Boolean) {
        this.commit = commit
        progress?.close()
    }

    companion object {
        /** Дальше этой доли ширины отпускание — назад. */
        const val COMMIT = 0.35f
    }
}

/** Время, на котором кривая доходит до [y] (кривые переходов монотонны): бисекция. */
private fun Easing.inverse(y: Float): Float {
    var lo = 0f
    var hi = 1f
    repeat(20) {
        val mid = (lo + hi) / 2
        if (transform(mid) < y) lo = mid else hi = mid
    }
    return (lo + hi) / 2
}

/** Свайп назад текущего экрана; `null` — экран не в стеке переходов. */
internal val LocalSwipeBack = staticCompositionLocalOf<SwipeBack?> { null }

/**
 * Свайп вправо по области — назад через [ctl]. Решает, чей жест, в проходе `Initial`, до
 * детей: протяжка влево остаётся ответу свайпом, вертикаль — прокрутке, а горизонталь вправо
 * забирается целиком (ряды и лента её уже не видят). Пока [busy] (выбор протяжкой) — молчит.
 * Край экрана слева — системный «назад», туда касания не доходят.
 */
internal fun Modifier.swipeBack(ctl: SwipeBack?, busy: () -> Boolean): Modifier = if (ctl == null) {
    this
} else {
    // Координаты — в окне, не в области: область уезжает вместе с пальцем, и в своих
    // координатах палец почти стоит (медленный свайп не дотягивал до порога).
    var frame: LayoutCoordinates? = null
    onGloballyPositioned { frame = it }.pointerInput(ctl) {
        val slop = viewConfiguration.touchSlop
        val fling = FLING.toPx()
        val inWindow = { p: androidx.compose.ui.geometry.Offset -> frame?.takeIf { it.isAttached }?.localToRoot(p) ?: p }
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            val start = inWindow(down.position)
            val tracker = VelocityTracker().apply { addPosition(down.uptimeMillis, start) }
            var dx = 0f
            var dy = 0f
            var claimed = false
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                val ch = event.changes.firstOrNull { it.id == down.id }
                if (ch == null || event.changes.size > 1) {
                    if (claimed) ctl.end(false)
                    break
                }
                val at = inWindow(ch.position)
                val fraction = (at.x - start.x - slop) / size.width
                if (!ch.pressed) {
                    if (claimed) ctl.end(fraction > SwipeBack.COMMIT || (tracker.calculateVelocity().x > fling && fraction > 0f))
                    break
                }
                tracker.addPosition(ch.uptimeMillis, at)
                if (!claimed) {
                    if (busy() || ch.isConsumed) break
                    dx += ch.positionChange().x
                    dy += ch.positionChange().y
                    // Вертикаль — прокрутка, влево — ответ свайпом: не наш жест.
                    if (abs(dy) > slop || dx < -slop) break
                    if (dx > slop && dx > 2 * abs(dy)) {
                        claimed = ctl.start()
                        if (!claimed) break
                    }
                }
                if (claimed) {
                    ch.consume()
                    ctl.drag(fraction)
                }
            }
        }
    }
}

/** Бросок быстрее этого (в секунду) — назад, даже если экран сдвинут немного. */
private val FLING = 800.dp
