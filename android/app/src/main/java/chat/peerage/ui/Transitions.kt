package chat.peerage.ui

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedContentScope
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.SeekableTransitionState
import androidx.compose.animation.core.rememberTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import chat.peerage.NavState
import chat.peerage.Screen
import chat.peerage.Tokens
import kotlin.coroutines.cancellation.CancellationException

/**
 * Переходы между экранами (app-ui.md → «Движение»): вглубь — новый экран въезжает справа,
 * прежний отходит на четверть и гаснет; назад — зеркально; «Профиль» — с левой стороны. Предиктивный «назад» (Android 14+)
 * тянет тот же переход за пальцем: отпустил — довели, отменил — вернули.
 *
 * Оба экрана во время перехода смонтированы (~300 мс): эффекты уходящего (камера, плеер)
 * освобождаются на этот срок позже. Чат → чат — один ключ, без анимации: экран чата читает
 * общую модель, и два экземпляра показали бы одно и то же.
 *
 * Общих элементов между экранами нет. Аватар раньше перелетал в «Профиль» через
 * `SharedTransitionLayout`, но тот включает lookahead для ВСЕГО под ним: каждая лента
 * мерилась дважды за кадр прокрутки, а LazyColumn в lookahead ронял приложение
 * («Placement happened before lookahead», 0.2.0 на телефоне).
 */
@Composable
internal fun ScreenTransitions(
    nav: NavState,
    backEnabled: Boolean,
    onBack: () -> Unit,
    rootFade: Boolean = false,
    content: @Composable AnimatedContentScope.(Screen) -> Unit,
) {
    val target = nav.current
    val seek = remember { SeekableTransitionState(target) }
    val transition = rememberTransition(seek, label = "screen")

    LaunchedEffect(target) {
        if (seek.currentState != target || seek.targetState != target) seek.animateTo(target, tween(Tokens.Anim.screen, easing = FastOutSlowInEasing))
    }
    PredictiveBackHandler(enabled = backEnabled) { progress ->
        val prev = nav.previous ?: return@PredictiveBackHandler
        try {
            progress.collect { seek.seekTo(it.progress, prev) }
            onBack()
        } catch (e: CancellationException) {
            seek.animateTo(seek.currentState)
            throw e
        }
    }

    transition.AnimatedContent(
        transitionSpec = {
            val from = nav.indexOf(initialState)
            val to = nav.indexOf(targetState)
            val forward = if (from < 0) nav.lastForward else to > from
            // «Профиль» открывают своим аватаром слева сверху — он и выезжает слева,
            // и уходит туда же. Экраны глубже него — как все, справа.
            val side = if ((if (forward) targetState else initialState) == Screen.Me) -1 else 1
            val spec = tween<androidx.compose.ui.unit.IntOffset>(Tokens.Anim.screen, easing = FastOutSlowInEasing)
            val fade = tween<Float>(Tokens.Anim.screen)
            // Правая из двух панелей: экран, открытый с главной (она слева и никуда не
            // уходит), проявляется на месте, а не выезжает — сдвигать нечего. «Прежнего
            // нет в стеке, шаг вперёд на глубину 1» — это история, начатая заново.
            if (rootFade && (from == 0 || to == 0 || (from < 0 && nav.lastForward && to == 1))) {
                return@AnimatedContent (fadeIn(fade) togetherWith fadeOut(fade)).apply { targetContentZIndex = to.toFloat() }
            }
            val enter = slideInHorizontally(spec) { side * if (forward) it else -it / 4 } + fadeIn(fade, if (forward) 1f else 0.6f)
            val exit = slideOutHorizontally(spec) { side * if (forward) -it / 4 else it } + fadeOut(fade, if (forward) 0.6f else 1f)
            // Слой — место в истории: глубже лежит выше. Уходящий хранит слой, с которым
            // пришёл, — с «−1 на назад» цепочка «назад → назад» клала главную поверх.
            (enter togetherWith exit).apply { targetContentZIndex = to.toFloat() }
        },
        contentKey = { it },
        modifier = Modifier,
    ) { s -> content(s) }
}
