package chat.peerage.ui

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedContentScope
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SharedTransitionLayout
import androidx.compose.animation.SharedTransitionScope
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
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import chat.peerage.NavState
import chat.peerage.Screen
import chat.peerage.Tokens
import kotlin.coroutines.cancellation.CancellationException

/** Общие элементы между экранами (аватар → профиль); null — переходов нет (превью, тесты). */
@OptIn(ExperimentalSharedTransitionApi::class)
internal val LocalSharedScope = compositionLocalOf<SharedTransitionScope?> { null }

/** Видимость экрана, внутри которого рисуется элемент: для общих элементов. */
internal val LocalScreenScope = compositionLocalOf<AnimatedVisibilityScope?> { null }

/**
 * Переходы между экранами (app-ui.md → «Движение»): вглубь — новый экран въезжает справа,
 * прежний отходит на четверть и гаснет; назад — зеркально. Предиктивный «назад» (Android 14+)
 * тянет тот же переход за пальцем: отпустил — довели, отменил — вернули.
 *
 * Оба экрана во время перехода смонтированы (~300 мс): эффекты уходящего (камера, плеер)
 * освобождаются на этот срок позже. Чат → чат — один ключ, без анимации: экран чата читает
 * общую модель, и два экземпляра показали бы одно и то же.
 */
@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
internal fun ScreenTransitions(
    nav: NavState,
    backEnabled: Boolean,
    onBack: () -> Unit,
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

    SharedTransitionLayout {
        CompositionLocalProvider(LocalSharedScope provides this) {
            transition.AnimatedContent(
                transitionSpec = {
                    val from = nav.indexOf(initialState)
                    val to = nav.indexOf(targetState)
                    val forward = if (from < 0) nav.lastForward else to > from
                    val spec = tween<androidx.compose.ui.unit.IntOffset>(Tokens.Anim.screen, easing = FastOutSlowInEasing)
                    val fade = tween<Float>(Tokens.Anim.screen)
                    val enter = slideInHorizontally(spec) { if (forward) it else -it / 4 } + fadeIn(fade, if (forward) 1f else 0.6f)
                    val exit = slideOutHorizontally(spec) { if (forward) -it / 4 else it } + fadeOut(fade, if (forward) 0.6f else 1f)
                    (enter togetherWith exit).apply { targetContentZIndex = if (forward) 1f else -1f }
                },
                contentKey = { it },
                modifier = Modifier,
            ) { s ->
                CompositionLocalProvider(LocalScreenScope provides this) { content(s) }
            }
        }
    }
}

/** Ключ своего аватара: шапка главной ↔ карточка «Профиля». */
internal const val SELF_AVATAR = "self-avatar"

/** Общий элемент по ключу [key]: без переходов (превью) — обычный модификатор. */
@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
internal fun Modifier.sharedElement(key: String): Modifier {
    val shared = LocalSharedScope.current ?: return this
    val screen = LocalScreenScope.current ?: return this
    return with(shared) { this@sharedElement.sharedElement(rememberSharedContentState(key), screen) }
}
