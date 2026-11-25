package chat.peerage.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import chat.peerage.FastScroll
import chat.peerage.Tokens
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** Меньше стольких снимков ползунок не нужен: галерея и так листается за пару движений. */
private const val SCROLLER_MIN = 60

/** Столько ползунок держится на экране после прокрутки — успеть за него взяться. */
private const val SCROLLER_LINGER_MS = 1500L

/**
 * Быстрая прокрутка галереи (п. 70, как в Telegram): справа, пока галерею листают, —
 * ползунок; взялся за него — рядом месяц снимка под ним («Ноябрь 2026»), ведёшь — галерея
 * едет следом, вся, а не только загруженная часть ([total]; снимок дальше загруженного
 * догружает [onWant]). Скрытый ползунок касания не перехватывает: правый ряд плиток
 * нажимается как обычно.
 *
 * [offset] — сколько элементов сетки до первого снимка (плитка «Камера»).
 */
@Composable
internal fun BoxScope.GalleryScroller(
    grid: LazyGridState,
    total: Int,
    dateAt: (Int) -> Long?,
    ready: (Int) -> Boolean,
    onWant: (Int) -> Unit,
    offset: Int,
) {
    if (total < SCROLLER_MIN) return
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    val thumbPx = with(density) { Tokens.Size.fastThumbH.toPx() }
    var trackPx by remember { mutableFloatStateOf(1f) }
    var dragging by remember { mutableStateOf(false) }
    var dragFrac by remember { mutableFloatStateOf(0f) }
    var job by remember { mutableStateOf<Job?>(null) }

    // Виден, пока листают, и ещё немного после — иначе за него не взяться.
    var lastScroll by remember { mutableLongStateOf(0L) }
    var lingering by remember { mutableStateOf(false) }
    LaunchedEffect(grid) {
        snapshotFlow { grid.isScrollInProgress }.collect { moving ->
            if (moving) {
                lingering = true
                lastScroll = System.nanoTime()
            } else {
                val mark = lastScroll
                delay(SCROLLER_LINGER_MS)
                if (mark == lastScroll && !grid.isScrollInProgress && !dragging) lingering = false
            }
        }
    }
    val shown = dragging || lingering || grid.isScrollInProgress
    val alpha by animateFloatAsState(if (shown) 1f else 0f, tween(if (shown) 120 else 300), label = "fastScroll")
    if (alpha == 0f && !shown) return

    val first = (grid.firstVisibleItemIndex - offset).coerceAtLeast(0)
    val frac = if (dragging) dragFrac else FastScroll.fractionOf(first, total)
    val thumbTop = frac * trackPx

    fun jump(f: Float) {
        dragFrac = f.coerceIn(0f, 1f)
        val target = FastScroll.indexAt(dragFrac, total)
        onWant(target)
        job?.cancel()
        job = scope.launch {
            snapshotFlow { ready(target) }.first { it }
            grid.scrollToItem(target + offset)
        }
    }

    Box(
        Modifier.align(Alignment.TopEnd).fillMaxHeight().width(Tokens.Size.touch).alpha(alpha)
            .onSizeChanged { trackPx = (it.height - thumbPx).coerceAtLeast(1f) }
            .pointerInput(total) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    // За ползунок — держим там, где взяли; мимо — ползунок встаёт под палец.
                    // Положение — заново из сетки: блок жеста живёт дольше одной композиции.
                    val now = FastScroll.fractionOf((grid.firstVisibleItemIndex - offset).coerceAtLeast(0), total)
                    val top = now * trackPx
                    val onThumb = down.position.y in top..(top + thumbPx)
                    val grab = if (onThumb) down.position.y - top else thumbPx / 2
                    dragging = true
                    dragFrac = now
                    if (!onThumb) jump((down.position.y - grab) / trackPx)
                    down.consume()
                    while (true) {
                        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        jump((change.position.y - grab) / trackPx)
                        change.consume()
                    }
                    dragging = false
                    lastScroll = System.nanoTime()
                    lingering = true
                }
            },
    ) {
        Box(
            Modifier.align(Alignment.TopEnd)
                .offset { IntOffset(0, thumbTop.roundToInt()) }
                .padding(end = Tokens.Space.xxs)
                .size(Tokens.Size.fastThumbW, Tokens.Size.fastThumbH)
                .clip(RoundedCornerShape(percent = 50))
                .background(if (dragging) Tokens.Colors.accent else Tokens.Colors.secondary),
        )
    }
    // Дата — слева от ползунка, пока его держат.
    if (dragging) {
        val date = dateAt(FastScroll.indexAt(dragFrac, total))
        if (date != null && date > 0) {
            Row(
                Modifier.align(Alignment.TopEnd)
                    .offset { IntOffset(-Tokens.Size.touch.roundToPx(), thumbTop.roundToInt()) }
                    .clip(RoundedCornerShape(Tokens.Radius.chip))
                    .background(Tokens.Colors.raised)
                    .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
            ) {
                Text(monthOf(date), color = Color.White, fontSize = Tokens.Font.option)
            }
        }
    }
}

/** «ноябрь 2026» → «Ноябрь 2026» на языке интерфейса. */
@Composable
private fun monthOf(sec: Long): String {
    val locale = LocalConfiguration.current.locales[0]
    return remember(sec / 86_400, locale) {
        java.text.SimpleDateFormat("LLLL yyyy", locale).format(java.util.Date(sec * 1000))
            .replaceFirstChar { it.titlecase(locale) }
    }
}
