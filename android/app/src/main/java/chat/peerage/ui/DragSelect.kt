package chat.peerage.ui

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.unit.dp
import chat.peerage.AppModel
import chat.peerage.DragRange
import chat.peerage.Message
import chat.peerage.MsgRef
import kotlinx.coroutines.Job
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Выбор протяжкой в ленте (app-ui.md → «Жесты»). Долгий тап по пузырю зовёт [start]; пока
 * палец не отпущен, [dragSelect] на ленте ведёт выбор за ним ([DragRange]), у края ленты
 * прокручивает её. Ряды сообщают свои границы в [rows] — по ним ищется ряд под пальцем.
 */
internal class DragSelect {
    /** Границы выбираемых рядов в окне, по сообщению. */
    val rows = HashMap<MsgRef, Rect>()

    /** Выбираемые ряды ленты в хронологии (альбом — один ряд) — задаёт лента. */
    var order: () -> List<List<MsgRef>> = { emptyList() }

    /** Прокрутить ленту на [px] (положительное — к старым), задаёт лента. */
    var scrollBy: suspend (Float) -> Float = { 0f }

    /** Идёт протяжка: палец держит выбор. Читается обработчиком жеста, не композицией. */
    @Volatile var active = false
        private set

    private var model: AppModel? = null
    private var base = emptySet<MsgRef>()
    private var anchor: MsgRef? = null
    private var last: MsgRef? = null
    private var adding = true

    /** Долгий тап по ряду сообщения [m]: ряд выбирается (или снимается, если был выбран целиком), дальше — протяжка. */
    fun start(model: AppModel, m: Message) {
        val ref = MsgRef(m.id, m.sender)
        this.model = model
        base = model.selection
        adding = !DragRange.rowSelected(base, DragRange.rowOf(order(), ref))
        anchor = ref
        last = null
        active = true
        extendTo(ref)
    }

    fun extendTo(ref: MsgRef) {
        val a = anchor ?: return
        if (ref == last) return
        last = ref
        model?.let { it.selection = DragRange.apply(base, order(), a, ref, adding) }
    }

    /** Ряд под точкой окна на высоте [y]. */
    fun rowAt(y: Float): MsgRef? = rows.entries.firstOrNull { y >= it.value.top && y < it.value.bottom }?.key

    fun end() {
        active = false
        anchor = null
        model = null
    }
}

internal val LocalDragSelect = staticCompositionLocalOf<DragSelect?> { null }

/**
 * Протяжка на ленте: после долгого тапа движение пальца забирается у ленты (иначе она бы
 * прокручивалась и свайп-ответ срабатывал) и выбирает ряды под ним. У верхнего и нижнего края
 * лента едет сама — так выбирается и то, что за экраном.
 */
internal fun Modifier.dragSelect(ctl: DragSelect): Modifier {
    var frame: LayoutCoordinates? = null
    return onGloballyPositioned { frame = it }.pointerInput(ctl) {
        val edge = 56.dp.toPx()
        val maxStep = 18.dp.toPx()
        coroutineScope {
            awaitEachGesture {
                awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                var y = 0f
                var auto: Job? = null
                val follow = { frame?.takeIf { it.isAttached }?.let { f -> ctl.rowAt(f.localToRoot(Offset(0f, y)).y)?.let(ctl::extendTo) } }
                while (true) {
                    val change = awaitPointerEvent(PointerEventPass.Initial).changes.firstOrNull() ?: break
                    if (!change.pressed) break
                    if (!ctl.active) continue
                    change.consume()
                    y = change.position.y
                    follow()
                    if (auto == null) {
                        auto = launch {
                            while (isActive && ctl.active) {
                                val step = when {
                                    y < edge -> (edge - y) / edge * maxStep // вверх — к старым
                                    y > size.height - edge -> -(y - (size.height - edge)) / edge * maxStep
                                    else -> 0f
                                }
                                if (step != 0f) {
                                    ctl.scrollBy(step)
                                    follow()
                                }
                                delay(16)
                            }
                        }
                    }
                }
                auto?.cancel()
                ctl.end()
            }
        }
    }
}
